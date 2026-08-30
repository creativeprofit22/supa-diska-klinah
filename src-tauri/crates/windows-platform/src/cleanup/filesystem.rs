use cleanup_core::{
    DirectoryEntry, EntryKind, EntryMetadata, FileIdentity, FileSystem, FsError, FsErrorKind,
    PathSemantics, ReadDirControl,
};
use std::{
    ffi::OsStr,
    io, mem,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    ptr,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_NO_MORE_FILES, HANDLE, INVALID_HANDLE_VALUE},
    Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_ATTRIBUTE_DIRECTORY,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_ID_BOTH_DIR_INFO, FILE_LIST_DIRECTORY, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, FileIdBothDirectoryInfo, FileIdBothDirectoryRestartInfo,
        GetFileInformationByHandle, GetFileInformationByHandleEx, OPEN_EXISTING,
    },
};

#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsFileSystem;

impl FileSystem for WindowsFileSystem {
    fn semantics(&self) -> PathSemantics {
        PathSemantics::CaseInsensitive
    }

    fn metadata_no_follow(&self, path: &Path) -> Result<EntryMetadata, FsError> {
        metadata_from_information(OwnedHandle::open(path, 0)?.information()?)
    }

    fn canonicalize(&self, path: &Path) -> Result<PathBuf, FsError> {
        std::fs::canonicalize(path).map_err(FsError::from)
    }

    fn read_dir(
        &self,
        path: &Path,
        expected_identity: FileIdentity,
        visitor: &mut dyn FnMut(DirectoryEntry) -> ReadDirControl,
    ) -> Result<(), FsError> {
        let handle = OwnedHandle::open(path, FILE_LIST_DIRECTORY)?;
        let parent = metadata_from_information(handle.information()?)?;
        if parent.kind != EntryKind::Directory || parent.identity != Some(expected_identity) {
            return Err(FsError::new(
                FsErrorKind::Changed,
                "directory became link-like or changed identity before enumeration",
            ));
        }
        handle.read_dir(path, expected_identity.volume, visitor)
    }
}

fn metadata_from_information(
    information: BY_HANDLE_FILE_INFORMATION,
) -> Result<EntryMetadata, FsError> {
    let kind = kind_from_attributes(information.dwFileAttributes);
    Ok(EntryMetadata {
        kind,
        identity: (kind != EntryKind::LinkLike).then_some(FileIdentity {
            volume: u64::from(information.dwVolumeSerialNumber),
            file: u64::from(information.nFileIndexHigh) << 32
                | u64::from(information.nFileIndexLow),
        }),
        size: u64::from(information.nFileSizeHigh) << 32 | u64::from(information.nFileSizeLow),
        modified: file_time(
            u64::from(information.ftLastWriteTime.dwHighDateTime) << 32
                | u64::from(information.ftLastWriteTime.dwLowDateTime),
        ),
    })
}

fn kind_from_attributes(attributes: u32) -> EntryKind {
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        EntryKind::LinkLike
    } else if attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
        EntryKind::Directory
    } else {
        EntryKind::File
    }
}

struct OwnedHandle(HANDLE);
impl OwnedHandle {
    fn open(path: &Path, access: u32) -> Result<Self, FsError> {
        let path = wide(path.as_os_str())?;
        // SAFETY: path is NUL-terminated, pointers are valid/null, and this type owns the handle.
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                access,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
                ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            Err(FsError::from(io::Error::last_os_error()))
        } else {
            Ok(Self(handle))
        }
    }

    fn information(&self) -> Result<BY_HANDLE_FILE_INFORMATION, FsError> {
        let mut information = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: the handle is live and information points to writable initialized storage.
        if unsafe { GetFileInformationByHandle(self.0, &mut information) } == 0 {
            Err(FsError::from(io::Error::last_os_error()))
        } else {
            Ok(information)
        }
    }

    fn read_dir(
        &self,
        parent: &Path,
        volume: u64,
        visitor: &mut dyn FnMut(DirectoryEntry) -> ReadDirControl,
    ) -> Result<(), FsError> {
        const BUFFER_WORDS: usize = 8192;
        let mut buffer = [0_u64; BUFFER_WORDS];
        let mut class = FileIdBothDirectoryRestartInfo;
        loop {
            // SAFETY: the live directory handle and aligned writable buffer satisfy the API contract.
            let success = unsafe {
                GetFileInformationByHandleEx(
                    self.0,
                    class,
                    buffer.as_mut_ptr().cast(),
                    mem::size_of_val(&buffer) as u32,
                )
            };
            if success == 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
                    return Ok(());
                }
                return Err(FsError::from(error));
            }
            class = FileIdBothDirectoryInfo;
            let mut offset = 0_usize;
            loop {
                let remaining = mem::size_of_val(&buffer) - offset;
                if remaining < mem::size_of::<FILE_ID_BOTH_DIR_INFO>() {
                    return Err(FsError::new(
                        FsErrorKind::InvalidData,
                        "Windows returned a truncated directory entry",
                    ));
                }
                // SAFETY: offset is checked within an aligned buffer; Windows aligns each record.
                let entry = unsafe {
                    &*buffer
                        .as_ptr()
                        .cast::<u8>()
                        .add(offset)
                        .cast::<FILE_ID_BOTH_DIR_INFO>()
                };
                let name_bytes = entry.FileNameLength as usize;
                let name_offset = mem::offset_of!(FILE_ID_BOTH_DIR_INFO, FileName);
                if !name_bytes.is_multiple_of(2) || name_offset + name_bytes > remaining {
                    return Err(FsError::new(
                        FsErrorKind::InvalidData,
                        "Windows returned an invalid directory entry name",
                    ));
                }
                // SAFETY: the validated byte length covers the UTF-16 name stored in this record.
                let name =
                    unsafe { std::slice::from_raw_parts(entry.FileName.as_ptr(), name_bytes / 2) };
                if name != [b'.' as u16] && name != [b'.' as u16, b'.' as u16] {
                    let name = String::from_utf16_lossy(name);
                    let kind = kind_from_attributes(entry.FileAttributes);
                    let identity = (kind != EntryKind::LinkLike).then_some(FileIdentity {
                        volume,
                        file: entry.FileId as u64,
                    });
                    if visitor(DirectoryEntry {
                        path: parent.join(&name),
                        name,
                        kind,
                        identity,
                    }) == ReadDirControl::Stop
                    {
                        return Ok(());
                    }
                }
                if entry.NextEntryOffset == 0 {
                    break;
                }
                let next = entry.NextEntryOffset as usize;
                if next < name_offset + name_bytes || next > remaining {
                    return Err(FsError::new(
                        FsErrorKind::InvalidData,
                        "Windows returned an invalid directory entry offset",
                    ));
                }
                offset += next;
            }
        }
    }
}
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: this type owns one valid handle and drops it exactly once.
        unsafe { CloseHandle(self.0) };
    }
}

fn wide(value: &OsStr) -> Result<Vec<u16>, FsError> {
    let value: Vec<u16> = value.encode_wide().collect();
    if value.contains(&0) {
        return Err(FsError::new(
            FsErrorKind::InvalidData,
            "Windows paths cannot contain NUL characters",
        ));
    }
    Ok(value.into_iter().chain(Some(0)).collect())
}

fn file_time(ticks: u64) -> Option<SystemTime> {
    const WINDOWS_TO_UNIX_TICKS: u64 = 116_444_736_000_000_000;
    ticks
        .checked_sub(WINDOWS_TO_UNIX_TICKS)
        .and_then(|ticks| UNIX_EPOCH.checked_add(Duration::from_nanos(ticks.saturating_mul(100))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{env, os::windows::fs::symlink_file};

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = env::temp_dir().join(format!(
                "supa-diska-filesystem-{}-{}",
                std::process::id(),
                getrandom::u64().unwrap()
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn reports_stable_identity_and_classifies_every_reparse_point_as_link_like() {
        let temp = Temp::new();
        let file = temp.0.join("file");
        let link = temp.0.join("link");
        let target = temp.0.join("target");
        let junction = temp.0.join("junction");
        std::fs::write(&file, b"data").unwrap();
        std::fs::create_dir(&target).unwrap();
        let symlink_created = match symlink_file(&file, &link) {
            Ok(()) => true,
            Err(error) if error.raw_os_error() == Some(1314) => false,
            Err(error) => panic!("failed to create test symlink: {error}"),
        };
        junction::create(&target, &junction).unwrap();
        let adapter = WindowsFileSystem;
        let first = adapter.metadata_no_follow(&file).unwrap();
        let second = adapter.metadata_no_follow(&file).unwrap();
        assert_eq!(first.identity, second.identity);
        if symlink_created {
            assert_eq!(
                adapter.metadata_no_follow(&link).unwrap().kind,
                EntryKind::LinkLike
            );
        }
        assert_eq!(
            adapter.metadata_no_follow(&junction).unwrap().kind,
            EntryKind::LinkLike
        );
        let parent_identity = adapter
            .metadata_no_follow(&temp.0)
            .unwrap()
            .identity
            .unwrap();
        let mut entries = Vec::new();
        adapter
            .read_dir(&temp.0, parent_identity, &mut |entry| {
                entries.push(entry);
                ReadDirControl::Continue
            })
            .unwrap();
        if symlink_created {
            assert_eq!(
                entries
                    .iter()
                    .find(|entry| entry.path == link)
                    .unwrap()
                    .kind,
                EntryKind::LinkLike
            );
        }
        assert_eq!(
            entries
                .iter()
                .find(|entry| entry.path == junction)
                .unwrap()
                .kind,
            EntryKind::LinkLike
        );
    }

    #[test]
    fn rejects_directory_to_junction_swaps_and_disappearance() {
        let temp = Temp::new();
        let target = temp.0.join("target");
        let raced = temp.0.join("raced");
        std::fs::create_dir(&target).unwrap();
        std::fs::create_dir(&raced).unwrap();
        let adapter = WindowsFileSystem;
        let identity = adapter
            .metadata_no_follow(&raced)
            .unwrap()
            .identity
            .unwrap();
        std::fs::remove_dir(&raced).unwrap();
        junction::create(&target, &raced).unwrap();
        let error = adapter
            .read_dir(&raced, identity, &mut |_| ReadDirControl::Continue)
            .unwrap_err();
        assert_eq!(error.kind, FsErrorKind::Changed);
        junction::delete(&raced).unwrap();
        std::fs::remove_dir(&raced).unwrap();

        std::fs::create_dir(&raced).unwrap();
        let identity = adapter
            .metadata_no_follow(&raced)
            .unwrap()
            .identity
            .unwrap();
        std::fs::remove_dir(&raced).unwrap();
        if std::os::windows::fs::symlink_dir(&target, &raced).is_ok() {
            let error = adapter
                .read_dir(&raced, identity, &mut |_| ReadDirControl::Continue)
                .unwrap_err();
            assert_eq!(error.kind, FsErrorKind::Changed);
            std::fs::remove_dir(&raced).unwrap();
        }
        assert_eq!(
            adapter.metadata_no_follow(&raced).unwrap_err().kind,
            FsErrorKind::NotFound
        );
    }
}
