#![windows_subsystem = "windows"]

use windows_platform::{
    security::{helper, restore_point::WindowsRestorePointBackend},
    system_change::elevated::WindowsSystemChangeBackend,
};

fn main() {
    let code = match helper::run(
        std::env::args_os().skip(1),
        &WindowsRestorePointBackend,
        &WindowsSystemChangeBackend,
    ) {
        Ok(()) => 0,
        Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => 2,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => 3,
        Err(_) => 1,
    };
    std::process::exit(code);
}
