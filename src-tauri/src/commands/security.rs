use serde::{Deserialize, Serialize};
use windows_platform::security::{CreateSystemRestorePointResult, RestorePointDescription};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CreateRestorePointInput {
    description: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SecurityCommandError {
    code: &'static str,
    message: &'static str,
}

impl SecurityCommandError {
    fn invalid_input() -> Self {
        Self {
            code: "invalidInput",
            message: "The restore point description is invalid.",
        }
    }

    fn unavailable() -> Self {
        Self {
            code: "operationUnavailable",
            message: "Windows could not create the restore point.",
        }
    }
}

#[tauri::command]
pub(crate) async fn create_system_restore_point(
    input: CreateRestorePointInput,
) -> Result<CreateSystemRestorePointResult, SecurityCommandError> {
    let description = RestorePointDescription::parse(input.description)
        .map_err(|_| SecurityCommandError::invalid_input())?;
    tauri::async_runtime::spawn_blocking(move || {
        windows_platform::security::broker::create_system_restore_point(description)
    })
    .await
    .map_err(|_| SecurityCommandError::unavailable())?
    .map_err(|_| SecurityCommandError::unavailable())
}
