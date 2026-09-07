//! Native current-user bindings. Environment variables never grant cleanup authority.
use cleanup_core::storage::StorageError;
use cleanup_core::{EntryKind, FileSystem};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KnownFolder {
    Local,
    LocalLow,
    Roaming,
    Profile,
    ProgramFiles,
    ProgramFilesX86,
    ProgramData,
}
pub trait KnownFolderResolver {
    fn resolve(&self, folder: KnownFolder) -> Result<PathBuf, StorageError>;
}
pub struct NativeKnownFolders;
impl KnownFolderResolver for NativeKnownFolders {
    fn resolve(&self, folder: KnownFolder) -> Result<PathBuf, StorageError> {
        use std::os::windows::ffi::OsStringExt;
        use windows_sys::Win32::{System::Com::CoTaskMemFree, UI::Shell::*};
        let id = match folder {
            KnownFolder::Local => FOLDERID_LocalAppData,
            KnownFolder::LocalLow => FOLDERID_LocalAppDataLow,
            KnownFolder::Roaming => FOLDERID_RoamingAppData,
            KnownFolder::Profile => FOLDERID_Profile,
            KnownFolder::ProgramFiles => FOLDERID_ProgramFiles,
            KnownFolder::ProgramFilesX86 => FOLDERID_ProgramFilesX86,
            KnownFolder::ProgramData => FOLDERID_ProgramData,
        };
        let mut ptr = std::ptr::null_mut();
        // The shell owns allocation even when HRESULT reports failure.
        let result = unsafe { SHGetKnownFolderPath(&id, 0, std::ptr::null_mut(), &mut ptr) };
        struct ShellAllocation(*mut u16);
        impl Drop for ShellAllocation {
            fn drop(&mut self) {
                unsafe {
                    CoTaskMemFree(self.0.cast());
                }
            }
        }
        let _allocation = ShellAllocation(ptr);
        if result < 0 || ptr.is_null() {
            return Err(StorageError::UnsupportedScope);
        }
        let mut len = 0;
        while len < 32768 && unsafe { *ptr.add(len) } != 0 {
            len += 1;
        }
        let path = if len < 32768 {
            Some(PathBuf::from(std::ffi::OsString::from_wide(unsafe {
                std::slice::from_raw_parts(ptr, len)
            })))
        } else {
            None
        };
        let path = path.ok_or(StorageError::InvalidEvidence)?;
        validate_binding(path)
    }
}
pub(crate) fn validate_binding(path: PathBuf) -> Result<PathBuf, StorageError> {
    let fs = crate::WindowsFileSystem;
    if !cleanup_core::is_local_storage_path(&path) {
        return Err(StorageError::InvalidEvidence);
    }
    for ancestor in path.ancestors().filter(|p| !p.as_os_str().is_empty()) {
        let meta = fs
            .metadata_no_follow(ancestor)
            .map_err(|_| StorageError::InvalidEvidence)?;
        if meta.kind != EntryKind::Directory || meta.identity.is_none() {
            return Err(StorageError::InvalidEvidence);
        }
    }
    let canonical = fs
        .canonicalize(&path)
        .map_err(|_| StorageError::InvalidEvidence)?;
    if !fs.semantics().equivalent(&canonical, &path) {
        return Err(StorageError::InvalidEvidence);
    }
    Ok(canonical)
}
