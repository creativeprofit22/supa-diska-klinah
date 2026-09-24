//! Native dialogs the webview cannot answer: folder picker and confirmations.

use std::path::PathBuf;

use windows::core::w;

use super::service::ProtectionError;

/// Native folder picker for a folder scan. Must run on a blocking thread.
pub fn pick_scan_folder(owner: isize) -> Result<Option<PathBuf>, ProtectionError> {
    crate::storage::root_picker::pick_folder_titled(owner, w!("Choose a folder to scan"))
        .map_err(|_| ProtectionError::WindowUnavailable)
}

/// Native folder picker for a rule-pack import (folder holding pack.json + pack.sig).
pub fn pick_rule_pack_folder(owner: isize) -> Result<Option<PathBuf>, ProtectionError> {
    crate::storage::root_picker::pick_folder_titled(
        owner,
        w!("Choose the folder containing pack.json and pack.sig"),
    )
    .map_err(|_| ProtectionError::WindowUnavailable)
}

/// Yes/No confirmation owned by the app window, defaulting to No.
pub fn confirm(owner: isize, title: &str, message: &str) -> Result<(), ProtectionError> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        IDYES, IsWindow, MB_DEFBUTTON2, MB_ICONWARNING, MB_YESNO, MessageBoxW,
    };
    // SAFETY: IsWindow accepts any value and only reports validity.
    if owner == 0 || unsafe { IsWindow(owner as _) } == 0 {
        return Err(ProtectionError::WindowUnavailable);
    }
    let text: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
    let caption: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
    // SAFETY: both strings are NUL-terminated and outlive the modal call.
    let answer = unsafe {
        MessageBoxW(
            owner as _,
            text.as_ptr(),
            caption.as_ptr(),
            MB_YESNO | MB_DEFBUTTON2 | MB_ICONWARNING,
        )
    };
    if answer == IDYES {
        Ok(())
    } else {
        Err(ProtectionError::ConfirmationDeclined)
    }
}
