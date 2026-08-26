use std::{
    ffi::OsStr,
    fmt, fs, io,
    os::windows::{ffi::OsStrExt, fs::MetadataExt},
    path::{Component, Path, PathBuf},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
    Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_ATTRIBUTE_DIRECTORY,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ,
        FILE_SHARE_READ, FILE_TYPE_DISK, GetFileInformationByHandle, GetFileType,
        GetFinalPathNameByHandleW, OPEN_EXISTING,
    },
};

#[derive(Debug)]
pub enum PathPolicyError {
    NotAbsolute,
    Traversal,
    NotDescendant,
    ReparsePoint,
    WrongType,
    Io(io::Error),
}

impl fmt::Display for PathPolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NotAbsolute => "root and candidate paths must be absolute",
            Self::Traversal => "lexical parent traversal is forbidden",
            Self::NotDescendant => "candidate is not a strict descendant of the root",
            Self::ReparsePoint => "reparse points are forbidden in contained paths",
            Self::WrongType => "contained path has the wrong filesystem type",
            Self::Io(_) => "contained path could not be validated",
        })
    }
}

impl std::error::Error for PathPolicyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for PathPolicyError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedPath(PathBuf);

impl ValidatedPath {
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

#[derive(Debug)]
pub struct ValidatedExecutable {
    path: PathBuf,
    handle: HANDLE,
}

impl ValidatedExecutable {
    pub fn as_path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ValidatedExecutable {
    fn drop(&mut self) {
        // SAFETY: the handle is uniquely owned until this value is dropped.
        unsafe { CloseHandle(self.handle) };
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileIdentity {
    volume: u32,
    index: u64,
}

pub fn validate_contained(root: &Path, candidate: &Path) -> Result<ValidatedPath, PathPolicyError> {
    if !root.is_absolute() || !candidate.is_absolute() {
        return Err(PathPolicyError::NotAbsolute);
    }
    if has_parent_component(root) || has_parent_component(candidate) {
        return Err(PathPolicyError::Traversal);
    }
    reject_reparse_components(root)?;
    reject_reparse_components(candidate)?;
    let root = fs::canonicalize(root)?;
    let candidate = fs::canonicalize(candidate)?;
    reject_reparse_components(&root)?;
    reject_reparse_components(&candidate)?;
    if candidate == root || !candidate.starts_with(&root) {
        return Err(PathPolicyError::NotDescendant);
    }
    Ok(ValidatedPath(candidate))
}

pub fn validate_executable(
    root: &Path,
    candidate: &Path,
) -> Result<ValidatedExecutable, PathPolicyError> {
    if !root.is_absolute() || !candidate.is_absolute() {
        return Err(PathPolicyError::NotAbsolute);
    }
    if has_parent_component(root) || has_parent_component(candidate) {
        return Err(PathPolicyError::Traversal);
    }

    let handle = open_executable(candidate)?;
    let result = validate_open_executable(root, handle);
    match result {
        Ok(path) => Ok(ValidatedExecutable { path, handle }),
        Err(error) => {
            // SAFETY: ownership was not transferred into ValidatedExecutable.
            unsafe { CloseHandle(handle) };
            Err(error)
        }
    }
}

fn open_executable(path: &Path) -> Result<HANDLE, PathPolicyError> {
    let path = wide(path.as_os_str());
    // SAFETY: path is NUL-terminated; the returned handle is checked and then owned.
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            FILE_GENERIC_READ,
            FILE_SHARE_READ,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        Err(io::Error::last_os_error().into())
    } else {
        Ok(handle)
    }
}

fn validate_open_executable(root: &Path, handle: HANDLE) -> Result<PathBuf, PathPolicyError> {
    if unsafe { GetFileType(handle) } != FILE_TYPE_DISK {
        return Err(PathPolicyError::WrongType);
    }
    let identity = file_identity(handle)?;
    let final_path = final_path(handle)?;
    let validated = validate_contained(root, &final_path)?;

    let reopened = open_executable(validated.as_path())?;
    let reopened_identity = file_identity(reopened);
    // SAFETY: reopened is owned locally and no longer used after closing.
    unsafe { CloseHandle(reopened) };
    if reopened_identity? != identity {
        return Err(PathPolicyError::Io(io::Error::other(
            "executable identity changed during validation",
        )));
    }
    Ok(validated.0)
}

fn file_identity(handle: HANDLE) -> Result<FileIdentity, PathPolicyError> {
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: handle is live and information points to writable storage.
    if unsafe { GetFileInformationByHandle(handle, &mut information) } == 0 {
        return Err(io::Error::last_os_error().into());
    }
    if information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(PathPolicyError::ReparsePoint);
    }
    if information.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
        return Err(PathPolicyError::WrongType);
    }
    Ok(FileIdentity {
        volume: information.dwVolumeSerialNumber,
        index: u64::from(information.nFileIndexHigh) << 32 | u64::from(information.nFileIndexLow),
    })
}

fn final_path(handle: HANDLE) -> Result<PathBuf, PathPolicyError> {
    let mut buffer = vec![0; 512];
    loop {
        // SAFETY: handle is live and buffer exposes its full writable capacity.
        let length = unsafe {
            GetFinalPathNameByHandleW(handle, buffer.as_mut_ptr(), buffer.len() as u32, 0)
        };
        if length == 0 {
            return Err(io::Error::last_os_error().into());
        }
        if length < buffer.len() as u32 {
            buffer.truncate(length as usize);
            return Ok(PathBuf::from(String::from_utf16(&buffer).map_err(
                |_| io::Error::new(io::ErrorKind::InvalidData, "invalid executable path"),
            )?));
        }
        buffer.resize(length as usize + 1, 0);
    }
}

fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}

fn has_parent_component(path: &Path) -> bool {
    path.components()
        .any(|component| component == Component::ParentDir)
}

fn reject_reparse_components(path: &Path) -> Result<(), PathPolicyError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        if matches!(component, Component::Prefix(_)) {
            continue;
        }
        let metadata = fs::symlink_metadata(&current)?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(PathPolicyError::ReparsePoint);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{env, fs, os::windows::fs::symlink_file};

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let directory = env::temp_dir().join(format!(
                "supa-diska-path-policy-{}-{}",
                std::process::id(),
                getrandom::u64().unwrap()
            ));
            fs::create_dir(&directory).unwrap();
            Self(directory)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn accepts_only_existing_strict_descendants() {
        let temp = TestDirectory::new();
        let root = temp.0.join("root");
        let candidate = root.join("child");
        fs::create_dir_all(&candidate).unwrap();
        assert_eq!(
            validate_contained(&root, &candidate).unwrap().as_path(),
            fs::canonicalize(candidate).unwrap()
        );
    }

    #[test]
    fn rejects_relative_traversal_equality_siblings_and_missing_paths() {
        let temp = TestDirectory::new();
        let root = temp.0.join("root");
        let child = root.join("child");
        let sibling = temp.0.join("root-other");
        fs::create_dir_all(&child).unwrap();
        fs::create_dir(&sibling).unwrap();
        assert!(matches!(
            validate_contained(Path::new("root"), Path::new("root/child")),
            Err(PathPolicyError::NotAbsolute)
        ));
        assert!(matches!(
            validate_contained(&root, &root.join("child").join("..").join("child")),
            Err(PathPolicyError::Traversal)
        ));
        assert!(matches!(
            validate_contained(&root, &root),
            Err(PathPolicyError::NotDescendant)
        ));
        assert!(matches!(
            validate_contained(&root, &sibling),
            Err(PathPolicyError::NotDescendant)
        ));
        assert!(matches!(
            validate_contained(&root, &root.join("missing")),
            Err(PathPolicyError::Io(_))
        ));
    }

    #[test]
    fn rejects_real_ntfs_junctions() {
        let temp = TestDirectory::new();
        let root = temp.0.join("root");
        let outside = temp.0.join("outside");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&outside).unwrap();
        let link = root.join("junction");
        junction::create(&outside, &link).unwrap();
        assert!(matches!(
            validate_contained(&root, &link),
            Err(PathPolicyError::ReparsePoint)
        ));
        junction::delete(link).unwrap();
    }

    #[test]
    fn rejects_file_symlinks_when_windows_allows_test_creation() {
        let temp = TestDirectory::new();
        let root = temp.0.join("root");
        let outside = temp.0.join("outside.txt");
        fs::create_dir(&root).unwrap();
        fs::write(&outside, b"outside").unwrap();
        let link = root.join("link.txt");
        if symlink_file(&outside, &link).is_err() {
            return;
        }
        assert!(matches!(
            validate_contained(&root, &link),
            Err(PathPolicyError::ReparsePoint)
        ));
    }

    #[test]
    fn executable_handle_blocks_replacement_until_launch_finishes() {
        let temp = TestDirectory::new();
        let root = temp.0.join("root");
        let helper = root.join("helper.exe");
        let replacement = root.join("replacement.exe");
        fs::create_dir(&root).unwrap();
        fs::write(&helper, b"validated helper").unwrap();
        fs::write(&replacement, b"replacement").unwrap();

        let validated = validate_executable(&root, &helper).unwrap();
        assert!(fs::remove_file(&helper).is_err());
        assert_eq!(fs::read(&helper).unwrap(), b"validated helper");

        drop(validated);
        fs::remove_file(&helper).unwrap();
        fs::rename(&replacement, &helper).unwrap();
        assert_eq!(fs::read(&helper).unwrap(), b"replacement");
    }
}
