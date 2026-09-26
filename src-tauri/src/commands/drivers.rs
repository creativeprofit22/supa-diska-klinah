use serde::Serialize;
use windows_platform::{drivers::DriverPackage, system_change::AdapterError};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DriversCommandError {
    code: &'static str,
    message: &'static str,
}

impl DriversCommandError {
    fn unavailable() -> Self {
        Self {
            code: "driversUnavailable",
            message: "The driver store could not be read.",
        }
    }
}

impl From<AdapterError> for DriversCommandError {
    fn from(error: AdapterError) -> Self {
        match error {
            AdapterError::Denied => Self {
                code: "accessDenied",
                message: "Access to the driver store was denied.",
            },
            AdapterError::Unsupported(_) | AdapterError::Failed => Self::unavailable(),
        }
    }
}

/// Read-only list of third-party driver packages. Deletion goes through the
/// shared plan/confirm/execute commands.
#[tauri::command]
pub(crate) async fn list_driver_packages() -> Result<Vec<DriverPackage>, DriversCommandError> {
    tauri::async_runtime::spawn_blocking(windows_platform::drivers::list_driver_packages)
        .await
        .map_err(|_| DriversCommandError::unavailable())?
        .map_err(DriversCommandError::from)
}
