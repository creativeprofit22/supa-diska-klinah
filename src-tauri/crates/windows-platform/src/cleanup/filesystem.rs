use cleanup_core::{
    DirectoryEntry, EntryKind, EntryMetadata, FileIdentity, FileSystem, FsError, FsErrorKind,
    PathSemantics, ReadDirControl,
};
use std::{
    ffi::OsStr,
    fs::OpenOptions,
    io::{self, Read},
    mem,
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
        FILE_SHARE_WRITE, FILE_STANDARD_INFO, FileIdBothDirectoryInfo,
        FileIdBothDirectoryRestartInfo, FileStandardInfo, GetDiskFreeSpaceExW,
        GetFileInformationByHandle, GetFileInformationByHandleEx, OPEN_EXISTING,
    },
};

#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsFileSystem;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CopyReport {
    pub entries: usize,
    pub bytes: u64,
}

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

    fn allocated_size(&self, path: &Path, _: &EntryMetadata) -> Result<u64, FsError> {
        let handle = OwnedHandle::open(path, 0)?;
        let mut information = FILE_STANDARD_INFO::default();
        // SAFETY: the handle is live and the output buffer has the requested structure size.
        if unsafe {
            GetFileInformationByHandleEx(
                handle.0,
                FileStandardInfo,
                (&raw mut information).cast(),
                mem::size_of::<FILE_STANDARD_INFO>() as u32,
            )
        } == 0
        {
            return Err(FsError::from(io::Error::last_os_error()));
        }
        u64::try_from(information.AllocationSize)
            .map_err(|_| FsError::new(FsErrorKind::InvalidData, "negative allocation size"))
    }

    fn ensure_inactive(&self, path: &Path) -> Result<(), FsError> {
        self.ensure_delete_available(path)
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

impl WindowsFileSystem {
    pub fn free_space(&self, path: &Path) -> Result<u64, FsError> {
        let path = wide(path.as_os_str())?;
        let mut available = 0_u64;
        // SAFETY: path is NUL-terminated and available points to writable storage.
        if unsafe {
            GetDiskFreeSpaceExW(
                path.as_ptr(),
                &raw mut available,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        } == 0
        {
            Err(FsError::from(io::Error::last_os_error()))
        } else {
            Ok(available)
        }
    }

    pub fn ensure_delete_available(&self, path: &Path) -> Result<(), FsError> {
        OwnedHandle::open_with_share(path, 0x0001_0000, FILE_SHARE_READ | FILE_SHARE_WRITE)
            .map(drop)
    }

    pub fn same_volume(&self, left: &Path, right: &Path) -> Result<bool, FsError> {
        let left = self
            .metadata_no_follow(left)?
            .identity
            .ok_or_else(|| FsError::new(FsErrorKind::Changed, "left identity unavailable"))?;
        let right = self
            .metadata_no_follow(right)?
            .identity
            .ok_or_else(|| FsError::new(FsErrorKind::Changed, "right identity unavailable"))?;
        Ok(left.volume == right.volume)
    }

    pub fn copy_tree_no_follow(
        &self,
        source: &Path,
        destination: &Path,
        max_entries: usize,
        max_bytes: u64,
    ) -> Result<CopyReport, FsError> {
        copy_tree(source, destination, max_entries, max_bytes)
    }

    pub fn remove_tree_no_follow(&self, path: &Path, max_entries: usize) -> Result<(), FsError> {
        remove_tree(path, max_entries)
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
        Self::open_with_share(
            path,
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        )
    }

    fn open_with_share(path: &Path, access: u32, share: u32) -> Result<Self, FsError> {
        let path = wide(path.as_os_str())?;
        // SAFETY: path is NUL-terminated, pointers are valid/null, and this type owns the handle.
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                access,
                share,
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

fn copy_tree(
    source: &Path,
    destination: &Path,
    max_entries: usize,
    max_bytes: u64,
) -> Result<CopyReport, FsError> {
    let source_metadata = std::fs::symlink_metadata(source).map_err(FsError::from)?;
    if source_metadata.file_type().is_symlink() {
        return Err(FsError::new(
            FsErrorKind::Changed,
            "refusing to copy a link-like entry",
        ));
    }
    if source_metadata.is_file() {
        let bytes = copy_file_bounded(source, destination, max_bytes)?;
        return Ok(CopyReport { entries: 1, bytes });
    }
    std::fs::create_dir(destination).map_err(FsError::from)?;
    let mut pending = vec![(source.to_path_buf(), destination.to_path_buf())];
    let mut report = CopyReport {
        entries: 0,
        bytes: 0,
    };
    while let Some((source_directory, destination_directory)) = pending.pop() {
        for entry in std::fs::read_dir(source_directory).map_err(FsError::from)? {
            report.entries = report.entries.saturating_add(1);
            if report.entries > max_entries {
                return Err(FsError::new(
                    FsErrorKind::InvalidData,
                    "copy entry limit exceeded",
                ));
            }
            let entry = entry.map_err(FsError::from)?;
            let file_type = entry.file_type().map_err(FsError::from)?;
            if file_type.is_symlink() {
                return Err(FsError::new(
                    FsErrorKind::Changed,
                    "refusing to copy a link-like entry",
                ));
            }
            let target = destination_directory.join(entry.file_name());
            if file_type.is_dir() {
                std::fs::create_dir(&target).map_err(FsError::from)?;
                pending.push((entry.path(), target));
            } else {
                let remaining = max_bytes.checked_sub(report.bytes).ok_or_else(|| {
                    FsError::new(FsErrorKind::InvalidData, "copy byte limit exceeded")
                })?;
                report.bytes = report
                    .bytes
                    .checked_add(copy_file_bounded(&entry.path(), &target, remaining)?)
                    .ok_or_else(|| FsError::new(FsErrorKind::InvalidData, "copy byte overflow"))?;
            }
        }
    }
    Ok(report)
}

fn copy_file_bounded(source: &Path, destination: &Path, max_bytes: u64) -> Result<u64, FsError> {
    let source = std::fs::File::open(source).map_err(FsError::from)?;
    let mut source = source.take(max_bytes.saturating_add(1));
    let mut destination = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(FsError::from)?;
    let copied = io::copy(&mut source, &mut destination).map_err(FsError::from)?;
    if copied > max_bytes {
        return Err(FsError::new(
            FsErrorKind::InvalidData,
            "copy byte limit exceeded",
        ));
    }
    destination.sync_all().map_err(FsError::from)?;
    Ok(copied)
}

fn remove_tree(path: &Path, max_entries: usize) -> Result<(), FsError> {
    let metadata = std::fs::symlink_metadata(path).map_err(FsError::from)?;
    if metadata.file_type().is_symlink() {
        return Err(FsError::new(
            FsErrorKind::Changed,
            "refusing to remove a link-like entry",
        ));
    }
    if metadata.is_file() {
        return std::fs::remove_file(path).map_err(FsError::from);
    }
    let mut directories = vec![path.to_path_buf()];
    let mut visited = Vec::new();
    let mut count = 0_usize;
    while let Some(directory) = directories.pop() {
        visited.push(directory.clone());
        for entry in std::fs::read_dir(&directory).map_err(FsError::from)? {
            count = count.saturating_add(1);
            if count > max_entries {
                return Err(FsError::new(
                    FsErrorKind::InvalidData,
                    "removal entry limit exceeded",
                ));
            }
            let entry = entry.map_err(FsError::from)?;
            let file_type = entry.file_type().map_err(FsError::from)?;
            if file_type.is_symlink() {
                return Err(FsError::new(
                    FsErrorKind::Changed,
                    "refusing to remove a link-like entry",
                ));
            }
            if file_type.is_dir() {
                directories.push(entry.path());
            } else {
                std::fs::remove_file(entry.path()).map_err(FsError::from)?;
            }
        }
    }
    for directory in visited.into_iter().rev() {
        std::fs::remove_dir(directory).map_err(FsError::from)?;
    }
    Ok(())
}

pub(crate) fn wide(value: &OsStr) -> Result<Vec<u16>, FsError> {
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
    fn measures_allocation_and_removes_only_bounded_no_follow_trees() {
        let temp = Temp::new();
        let tree = temp.0.join("tree");
        let file = tree.join("file");
        std::fs::create_dir(&tree).unwrap();
        std::fs::write(&file, vec![7_u8; 8193]).unwrap();
        let adapter = WindowsFileSystem;
        let metadata = adapter.metadata_no_follow(&file).unwrap();

        assert!(adapter.allocated_size(&file, &metadata).unwrap() >= metadata.size);
        assert!(adapter.free_space(&temp.0).unwrap() > 0);
        adapter.ensure_delete_available(&file).unwrap();
        let copied = temp.0.join("copied");
        assert_eq!(
            adapter
                .copy_tree_no_follow(&tree, &copied, 2, 8193)
                .unwrap(),
            CopyReport {
                entries: 1,
                bytes: 8193
            }
        );
        assert_eq!(std::fs::read(copied.join("file")).unwrap().len(), 8193);
        assert!(adapter.remove_tree_no_follow(&tree, 0).is_err());
        assert!(tree.exists());
        adapter.remove_tree_no_follow(&tree, 2).unwrap();
        assert!(!tree.exists());
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
