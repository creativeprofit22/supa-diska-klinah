use serde::Serialize;
use windows_platform::hosts::{HostsError, HostsReport};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HostsCommandError {
    code: &'static str,
    message: &'static str,
}

impl HostsCommandError {
    fn unavailable() -> Self {
        Self {
            code: "hostsUnavailable",
            message: "The hosts file could not be read.",
        }
    }
}

impl From<HostsError> for HostsCommandError {
    fn from(error: HostsError) -> Self {
        let (code, message) = match error {
            HostsError::SystemDirectory => (
                "systemDirectoryUnavailable",
                "The Windows system directory could not be resolved.",
            ),
            HostsError::NotFound => ("hostsNotFound", "The hosts file does not exist."),
            HostsError::Denied => ("accessDenied", "Access to the hosts file was denied."),
            HostsError::TooLarge => (
                "hostsTooLarge",
                "The hosts file is larger than 1 MiB and was not read.",
            ),
            HostsError::Io | HostsError::StateChanged => {
                ("hostsUnavailable", "The hosts file could not be read.")
            }
        };
        Self { code, message }
    }
}

#[tauri::command]
pub(crate) async fn get_hosts_report() -> Result<HostsReport, HostsCommandError> {
    tauri::async_runtime::spawn_blocking(windows_platform::hosts::hosts_report)
        .await
        .map_err(|_| HostsCommandError::unavailable())?
        .map_err(HostsCommandError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_failures_map_to_stable_frontend_codes() {
        for (error, expected_code) in [
            (HostsError::SystemDirectory, "systemDirectoryUnavailable"),
            (HostsError::NotFound, "hostsNotFound"),
            (HostsError::Denied, "accessDenied"),
            (HostsError::TooLarge, "hostsTooLarge"),
            (HostsError::Io, "hostsUnavailable"),
        ] {
            let command_error = HostsCommandError::from(error);
            assert_eq!(command_error.code, expected_code);
            assert!(!command_error.message.is_empty());
        }
    }
}
