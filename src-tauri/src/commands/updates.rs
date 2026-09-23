use serde::Serialize;
use windows_platform::{
    system_change::AdapterError,
    updates::{self, UpdateStatus},
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdatesCommandError {
    code: &'static str,
    message: &'static str,
}

impl UpdatesCommandError {
    fn unavailable() -> Self {
        Self {
            code: "updatesUnavailable",
            message: "Windows Update status could not be read.",
        }
    }
}

impl From<AdapterError> for UpdatesCommandError {
    fn from(error: AdapterError) -> Self {
        let (code, message) = match error {
            AdapterError::Unsupported(_) => (
                "apiUnavailable",
                "The Windows Update Agent is not available on this device.",
            ),
            AdapterError::Denied => ("accessDenied", "Windows denied access to Windows Update."),
            AdapterError::Failed => (
                "detectionFailed",
                "Windows Update could not start checking for updates.",
            ),
        };
        Self { code, message }
    }
}

#[tauri::command]
pub(crate) async fn get_windows_update_status() -> Result<UpdateStatus, UpdatesCommandError> {
    tauri::async_runtime::spawn_blocking(updates::windows_update_status)
        .await
        .map_err(|_| UpdatesCommandError::unavailable())
}

#[tauri::command]
pub(crate) async fn detect_windows_updates() -> Result<(), UpdatesCommandError> {
    tauri::async_runtime::spawn_blocking(updates::trigger_detection)
        .await
        .map_err(|_| UpdatesCommandError::unavailable())?
        .map_err(UpdatesCommandError::from)
}
