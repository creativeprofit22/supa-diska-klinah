use serde::Serialize;
use windows_platform::restore::{RestoreError, RestorePointList, RestoreProtection};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RestoreCommandError {
    code: &'static str,
    message: &'static str,
}

impl RestoreCommandError {
    fn unavailable() -> Self {
        Self {
            code: "restoreUnavailable",
            message: "System Restore information is unavailable.",
        }
    }
}

impl From<RestoreError> for RestoreCommandError {
    fn from(error: RestoreError) -> Self {
        let (code, message) = match error {
            RestoreError::Wmi(_) => (
                "restoreQueryFailed",
                "Windows could not list System Restore points.",
            ),
            RestoreError::Registry => (
                "restoreStatusFailed",
                "Windows could not read the System Restore protection status.",
            ),
        };
        Self { code, message }
    }
}

#[tauri::command]
pub(crate) async fn list_restore_points() -> Result<RestorePointList, RestoreCommandError> {
    tauri::async_runtime::spawn_blocking(windows_platform::restore::list_restore_points)
        .await
        .map_err(|_| RestoreCommandError::unavailable())?
        .map_err(RestoreCommandError::from)
}

#[tauri::command]
pub(crate) async fn get_restore_protection() -> Result<RestoreProtection, RestoreCommandError> {
    tauri::async_runtime::spawn_blocking(windows_platform::restore::get_restore_protection)
        .await
        .map_err(|_| RestoreCommandError::unavailable())?
        .map_err(RestoreCommandError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_failures_map_to_stable_frontend_codes() {
        for (error, expected_code) in [
            (RestoreError::Wmi(-1), "restoreQueryFailed"),
            (RestoreError::Registry, "restoreStatusFailed"),
        ] {
            let command_error = RestoreCommandError::from(error);
            assert_eq!(command_error.code, expected_code);
            assert!(!command_error.message.is_empty());
        }
    }
}
