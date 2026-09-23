use serde::Serialize;
use windows_platform::services::ServiceItem;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ServicesCommandError {
    code: &'static str,
    message: &'static str,
}

impl ServicesCommandError {
    fn unavailable() -> Self {
        Self {
            code: "servicesUnavailable",
            message: "The Windows service list could not be read.",
        }
    }
}

#[tauri::command]
pub(crate) async fn list_services() -> Result<Vec<ServiceItem>, ServicesCommandError> {
    tauri::async_runtime::spawn_blocking(windows_platform::services::list_services)
        .await
        .map_err(|_| ServicesCommandError::unavailable())
}
