//! Native known-folder resolution and location classification for heuristics.
//! Environment variables are never consulted: they are user-controlled.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use protection_core::LocationClass;
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::UI::Shell::{
    FOLDERID_CommonStartup, FOLDERID_Downloads, FOLDERID_LocalAppData, FOLDERID_Profile,
    FOLDERID_ProgramFiles, FOLDERID_ProgramFilesX86, FOLDERID_RoamingAppData, FOLDERID_Startup,
    FOLDERID_Windows, SHGetKnownFolderPath,
};
use windows_sys::core::GUID;

pub(crate) fn known_folder(id: &GUID) -> Option<PathBuf> {
    let mut ptr = std::ptr::null_mut();
    // SAFETY: `id` is a valid GUID; the shell allocates `ptr`, freed below even on failure.
    let result = unsafe { SHGetKnownFolderPath(id, 0, std::ptr::null_mut(), &mut ptr) };
    struct Allocation(*mut u16);
    impl Drop for Allocation {
        fn drop(&mut self) {
            // SAFETY: CoTaskMemFree accepts null and shell-allocated pointers.
            unsafe { CoTaskMemFree(self.0.cast()) }
        }
    }
    let _allocation = Allocation(ptr);
    if result < 0 || ptr.is_null() {
        return None;
    }
    let mut len = 0;
    // SAFETY: the shell returns a NUL-terminated string; the scan is bounded.
    while len < 32_768 && unsafe { *ptr.add(len) } != 0 {
        len += 1;
    }
    if len >= 32_768 {
        return None;
    }
    // SAFETY: `len` characters were just read from the same allocation.
    let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
    Some(PathBuf::from(OsString::from_wide(slice)))
}

/// Resolved folders used for classification and the quick scan scope.
#[derive(Clone, Debug, Default)]
pub struct KnownLocations {
    pub windows: Option<PathBuf>,
    pub program_files: Vec<PathBuf>,
    pub autostart: Vec<PathBuf>,
    pub temp: Vec<PathBuf>,
    pub downloads: Option<PathBuf>,
    pub roaming: Option<PathBuf>,
    pub local: Option<PathBuf>,
    pub profile: Option<PathBuf>,
}

impl KnownLocations {
    pub fn native() -> Self {
        let windows = known_folder(&FOLDERID_Windows);
        let local = known_folder(&FOLDERID_LocalAppData);
        let mut temp = Vec::new();
        if let Some(local) = &local {
            temp.push(local.join("Temp"));
        }
        if let Some(windows) = &windows {
            temp.push(windows.join("Temp"));
        }
        Self {
            program_files: [&FOLDERID_ProgramFiles, &FOLDERID_ProgramFilesX86]
                .into_iter()
                .filter_map(known_folder)
                .collect(),
            autostart: [&FOLDERID_Startup, &FOLDERID_CommonStartup]
                .into_iter()
                .filter_map(known_folder)
                .collect(),
            downloads: known_folder(&FOLDERID_Downloads),
            roaming: known_folder(&FOLDERID_RoamingAppData),
            profile: known_folder(&FOLDERID_Profile),
            windows,
            local,
            temp,
        }
    }

    /// Most specific class wins (autostart lives under roaming AppData).
    pub fn classify(&self, path: &Path) -> LocationClass {
        let under = |root: &Path| starts_with_ci(path, root);
        if self.autostart.iter().any(|root| under(root)) {
            LocationClass::Autostart
        } else if self.temp.iter().any(|root| under(root)) {
            LocationClass::Temp
        } else if self.windows.as_deref().is_some_and(under) {
            LocationClass::SystemRoot
        } else if self.program_files.iter().any(|root| under(root)) {
            LocationClass::ProgramFiles
        } else if self.downloads.as_deref().is_some_and(under) {
            LocationClass::Downloads
        } else if self.roaming.as_deref().is_some_and(under) {
            LocationClass::RoamingAppData
        } else if self.local.as_deref().is_some_and(under)
            || self.profile.as_deref().is_some_and(under)
        {
            LocationClass::OtherUserWritable
        } else {
            LocationClass::Other
        }
    }
}

/// Case-insensitive, component-wise prefix test (Windows path semantics).
pub(crate) fn starts_with_ci(path: &Path, root: &Path) -> bool {
    let mut path_parts = path.components();
    for root_part in root.components() {
        match path_parts.next() {
            Some(part)
                if part.as_os_str().to_string_lossy().to_lowercase()
                    == root_part.as_os_str().to_string_lossy().to_lowercase() => {}
            _ => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> KnownLocations {
        KnownLocations {
            windows: Some(r"C:\Windows".into()),
            program_files: vec![r"C:\Program Files".into()],
            autostart: vec![
                r"C:\Users\a\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup".into(),
            ],
            temp: vec![
                r"C:\Users\a\AppData\Local\Temp".into(),
                r"C:\Windows\Temp".into(),
            ],
            downloads: Some(r"C:\Users\a\Downloads".into()),
            roaming: Some(r"C:\Users\a\AppData\Roaming".into()),
            local: Some(r"C:\Users\a\AppData\Local".into()),
            profile: Some(r"C:\Users\a".into()),
        }
    }

    #[test]
    fn classifies_most_specific_location_case_insensitively() {
        let known = fixture();
        let cases = [
            (
                r"c:\windows\system32\svchost.exe",
                LocationClass::SystemRoot,
            ),
            (r"C:\Windows\Temp\x.exe", LocationClass::Temp),
            (
                r"C:\Users\a\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup\x.lnk",
                LocationClass::Autostart,
            ),
            (
                r"C:\Users\a\AppData\Roaming\app\x.exe",
                LocationClass::RoamingAppData,
            ),
            (r"C:\Users\a\AppData\Local\Temp\x.exe", LocationClass::Temp),
            (
                r"C:\Users\a\AppData\Local\app\x.exe",
                LocationClass::OtherUserWritable,
            ),
            (r"C:\Users\a\Downloads\x.exe", LocationClass::Downloads),
            (r"C:\Program Files\App\x.exe", LocationClass::ProgramFiles),
            (r"D:\tools\x.exe", LocationClass::Other),
            (r"C:\WindowsApps\x.exe", LocationClass::Other),
        ];
        for (path, expected) in cases {
            assert_eq!(known.classify(Path::new(path)), expected, "{path}");
        }
    }

    #[test]
    fn native_locations_resolve_windows_folder() {
        let known = KnownLocations::native();
        assert!(known.windows.as_deref().is_some_and(Path::is_dir));
    }
}
