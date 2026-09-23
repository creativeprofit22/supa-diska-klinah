use serde::Serialize;
use windows_platform::power::PowerStatus;
use windows_platform::system_change::AdapterError;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PowerCommandError {
    code: &'static str,
    message: &'static str,
}

impl PowerCommandError {
    fn worker_unavailable() -> Self {
        Self {
            code: "workerUnavailable",
            message: "The power status query could not be completed.",
        }
    }
}

impl From<AdapterError> for PowerCommandError {
    fn from(error: AdapterError) -> Self {
        let (code, message) = match error {
            AdapterError::Unsupported(_) => (
                "unsupported",
                "Power status is not supported on this system.",
            ),
            AdapterError::Denied => ("accessDenied", "Access to power settings was denied."),
            AdapterError::Failed => ("systemError", "Windows could not read power settings."),
        };
        Self { code, message }
    }
}

#[tauri::command]
pub(crate) async fn get_power_status() -> Result<PowerStatus, PowerCommandError> {
    tauri::async_runtime::spawn_blocking(windows_platform::power::power_status)
        .await
        .map_err(|_| PowerCommandError::worker_unavailable())?
        .map_err(PowerCommandError::from)
}
