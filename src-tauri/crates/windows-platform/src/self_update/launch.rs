//! The only process launch of the self-updater: `ShellExecuteW("open")` on the
//! verified installer, with no arguments and no elevation request. The
//! per-machine NSIS installer raises its own UAC prompt.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use super::InstallerLauncher;

/// Production launcher.
pub struct ShellLauncher;

impl InstallerLauncher for ShellLauncher {
    fn launch(&self, installer: &Path) -> Result<(), ()> {
        let file: Vec<u16> = installer.as_os_str().encode_wide().chain(Some(0)).collect();
        if file[..file.len() - 1].contains(&0) {
            return Err(());
        }
        let verb: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
        // SAFETY: both strings are NUL-terminated and outlive the call; null
        // parameters/directory are allowed. No owner window is needed.
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                file.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        // Values above 32 mean success.
        if result as isize > 32 {
            Ok(())
        } else {
            Err(())
        }
    }
}
