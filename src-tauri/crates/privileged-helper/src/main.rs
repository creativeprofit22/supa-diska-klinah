#![windows_subsystem = "windows"]

use windows_platform::{
    scheduler::ComTaskService,
    security::{helper, restore_point::WindowsRestorePointBackend},
    system_change::elevated::WindowsSystemChangeBackend,
};

/// Run only by the per-machine NSIS pre-uninstall hook (already elevated):
/// removes the app's `\SupaDiskaKlinah` scheduled tasks and, when empty, the
/// folder. Takes no other arguments and never contacts the app.
const REMOVE_SCHEDULED_TASKS: &str = "--remove-scheduled-tasks";

fn main() {
    let mut args = std::env::args_os().skip(1).peekable();
    if args.peek().is_some_and(|arg| arg == REMOVE_SCHEDULED_TASKS) {
        args.next();
        // Exactly the verb, nothing else.
        let code = if args.next().is_some() {
            2
        } else {
            match ComTaskService.remove_all_for_uninstall() {
                Ok(outcome) if outcome.failed == 0 => 0,
                Ok(_) | Err(_) => 1,
            }
        };
        std::process::exit(code);
    }

    let code = match helper::run(
        args,
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
