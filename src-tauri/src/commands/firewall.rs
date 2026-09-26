use serde::Serialize;
use windows_platform::{firewall::FirewallStatus, system_change::AdapterError};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FirewallCommandError {
    code: &'static str,
    message: &'static str,
}

impl FirewallCommandError {
    fn task_failed() -> Self {
        Self {
            code: "taskFailed",
            message: "The firewall status check stopped unexpectedly.",
        }
    }
}

impl From<AdapterError> for FirewallCommandError {
    fn from(error: AdapterError) -> Self {
        let (code, message) = match error {
            AdapterError::Unsupported(_) => (
                "unsupported",
                "Firewall status is not supported on this system.",
            ),
            AdapterError::Denied => ("accessDenied", "Access to the firewall policy was denied."),
            AdapterError::Failed => (
                "firewallReadFailed",
                "Windows Firewall status could not be read.",
            ),
        };
        Self { code, message }
    }
}

#[tauri::command]
pub(crate) async fn get_firewall_status() -> Result<FirewallStatus, FirewallCommandError> {
    tauri::async_runtime::spawn_blocking(windows_platform::firewall::get_firewall_status)
        .await
        .map_err(|_| FirewallCommandError::task_failed())?
        .map_err(FirewallCommandError::from)
}
