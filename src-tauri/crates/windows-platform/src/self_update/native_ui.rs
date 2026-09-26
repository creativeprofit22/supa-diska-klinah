//! Native install confirmation. The webview cannot answer it, so a scripted
//! page cannot start an installer on its own.

/// Yes/No question owned by the app window, defaulting to No. Returns `true`
/// only for an explicit Yes; an invalid owner window counts as No.
pub fn confirm_install(owner: isize, title: &str, body: &str) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        IDYES, IsWindow, MB_DEFBUTTON2, MB_ICONQUESTION, MB_YESNO, MessageBoxW,
    };
    // SAFETY: IsWindow accepts any value and only reports validity.
    if owner == 0 || unsafe { IsWindow(owner as _) } == 0 {
        return false;
    }
    let text: Vec<u16> = body.encode_utf16().chain(Some(0)).collect();
    let caption: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
    // SAFETY: both strings are NUL-terminated and outlive the modal call.
    let answer = unsafe {
        MessageBoxW(
            owner as _,
            text.as_ptr(),
            caption.as_ptr(),
            MB_YESNO | MB_DEFBUTTON2 | MB_ICONQUESTION,
        )
    };
    answer == IDYES
}
