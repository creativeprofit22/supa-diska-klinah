//! Helper-side hibernation observe/apply. Runs only inside the elevated
//! helper process. The single process launch in this module is
//! `<GetSystemDirectoryW>\powercfg.exe /hibernate on|off`.

use std::{
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use cleanup_core::system_change::{PriorState, UnsupportedReason};

use super::{PowerReader, hibernation_state, windows::WindowsPowerReader};
use crate::{security::system_changes::HelperChange, system_change::AdapterError};

pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;
pub const POWERCFG_TIMEOUT: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(100);
const POWERCFG_EXE: &str = "powercfg.exe";

/// Build the exact `powercfg` invocation: an absolute path directly inside
/// `system_dir` and the fixed two-argument argv.
pub fn powercfg_invocation(
    system_dir: &Path,
    enabled: bool,
) -> Result<(PathBuf, [&'static str; 2]), AdapterError> {
    if !system_dir.is_absolute() {
        return Err(AdapterError::Failed);
    }
    let program = system_dir.join(POWERCFG_EXE);
    if program.parent() != Some(system_dir) || !program.is_absolute() {
        return Err(AdapterError::Failed);
    }
    Ok((program, ["/hibernate", if enabled { "on" } else { "off" }]))
}

pub fn observe(change: &HelperChange) -> Result<PriorState, AdapterError> {
    match change {
        HelperChange::SetHibernation { .. } => {
            hibernation_state(&WindowsPowerReader.hibernation()?)
                .map(|enabled| PriorState::Enabled { enabled })
        }
        _ => Err(AdapterError::Failed),
    }
}

pub fn apply(change: &HelperChange) -> Result<(), AdapterError> {
    let HelperChange::SetHibernation { enabled } = change else {
        return Err(AdapterError::Failed);
    };
    // Re-resolve support inside the elevated process.
    let facts = WindowsPowerReader.hibernation()?;
    if !facts.supported() {
        return Err(AdapterError::Unsupported(UnsupportedReason::ApiUnavailable));
    }
    let (program, args) = powercfg_invocation(&super::windows::system_directory()?, *enabled)?;
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|_| AdapterError::Failed)?;
    let deadline = Instant::now() + POWERCFG_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(_)) => return Err(AdapterError::Failed),
            Ok(None) if Instant::now() < deadline => thread::sleep(POLL_INTERVAL),
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AdapterError::Failed);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cleanup_core::system_change::{CatalogId, ServiceStartType};

    #[test]
    fn argv_is_exactly_two_fixed_args_with_an_absolute_system_path() {
        let system = super::super::windows::system_directory().unwrap();
        for (enabled, word) in [(true, "on"), (false, "off")] {
            let (program, args) = powercfg_invocation(&system, enabled).unwrap();
            assert_eq!(args, ["/hibernate", word]);
            assert_eq!(args.len(), 2);
            assert!(program.is_absolute());
            assert_eq!(program.parent(), Some(system.as_path()));
            assert!(
                program
                    .file_name()
                    .unwrap()
                    .eq_ignore_ascii_case("powercfg.exe")
            );
        }
        assert_eq!(
            powercfg_invocation(Path::new(r"System32"), true),
            Err(AdapterError::Failed)
        );
    }

    #[test]
    fn other_helper_variants_fail() {
        let other = HelperChange::SetServiceStartType {
            catalog_id: CatalogId::parse("diagtrack").unwrap(),
            start_type: ServiceStartType::Disabled,
        };
        assert_eq!(observe(&other), Err(AdapterError::Failed));
        assert_eq!(apply(&other), Err(AdapterError::Failed));
    }

    #[test]
    fn live_observe_is_read_only() {
        let result = observe(&HelperChange::SetHibernation { enabled: true });
        assert!(matches!(
            result,
            Ok(PriorState::Enabled { .. })
                | Err(AdapterError::Unsupported(UnsupportedReason::ApiUnavailable))
        ));
    }
}
