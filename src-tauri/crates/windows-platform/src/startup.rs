use std::mem;
use windows_sys::Win32::{
    System::Threading::{GetStartupInfoW, STARTF_USESHOWWINDOW, STARTUPINFOW},
    UI::WindowsAndMessaging::{
        SW_FORCEMINIMIZE, SW_HIDE, SW_MINIMIZE, SW_SHOWMINIMIZED, SW_SHOWMINNOACTIVE,
    },
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartupWindowMode {
    Foreground,
    Background,
}

pub fn startup_window_mode() -> StartupWindowMode {
    // SAFETY: STARTUPINFOW is a plain Windows output structure initialized to its documented size.
    let mut startup: STARTUPINFOW = unsafe { mem::zeroed() };
    startup.cb = size_of::<STARTUPINFOW>() as u32;
    // SAFETY: `startup` points to writable memory for the complete STARTUPINFOW structure.
    unsafe { GetStartupInfoW(&mut startup) };
    mode_from_show_command(
        startup.dwFlags & STARTF_USESHOWWINDOW != 0,
        startup.wShowWindow as i32,
    )
}

fn mode_from_show_command(use_show_command: bool, show_command: i32) -> StartupWindowMode {
    if use_show_command
        && matches!(
            show_command,
            SW_HIDE | SW_SHOWMINIMIZED | SW_MINIMIZE | SW_SHOWMINNOACTIVE | SW_FORCEMINIMIZE
        )
    {
        StartupWindowMode::Background
    } else {
        StartupWindowMode::Foreground
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_and_minimized_startup_requests_stay_in_background() {
        for command in [
            SW_HIDE,
            SW_SHOWMINIMIZED,
            SW_MINIMIZE,
            SW_SHOWMINNOACTIVE,
            SW_FORCEMINIMIZE,
        ] {
            assert_eq!(
                mode_from_show_command(true, command),
                StartupWindowMode::Background
            );
        }
    }

    #[test]
    fn ordinary_or_unspecified_startup_requests_show_the_window() {
        assert_eq!(
            mode_from_show_command(false, SW_HIDE),
            StartupWindowMode::Foreground
        );
        assert_eq!(
            mode_from_show_command(true, 1),
            StartupWindowMode::Foreground
        );
    }
}
