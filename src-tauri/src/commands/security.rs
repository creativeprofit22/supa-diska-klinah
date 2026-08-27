use serde::{Deserialize, Serialize};
use windows_platform::security::{
    CreateSystemRestorePointResult, RestorePointDescription, broker::BrokerError,
};

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

    fn helper_unavailable() -> Self {
        Self {
            code: "helperUnavailable",
            message: "The privileged helper is unavailable.",
        }
    }
}

impl From<BrokerError> for SecurityCommandError {
    fn from(error: BrokerError) -> Self {
        let (code, message) = match error {
            BrokerError::AuthorizationCancelled => (
                "authorizationCancelled",
                "Administrator authorization was cancelled or denied.",
            ),
            BrokerError::HelperUnavailable => {
                ("helperUnavailable", "The privileged helper is unavailable.")
            }
            BrokerError::Timeout => (
                "operationTimedOut",
                "The privileged restore-point operation timed out.",
            ),
            BrokerError::InvalidRequest => (
                "invalidRequest",
                "The privileged restore-point request was invalid or stale.",
            ),
            BrokerError::PrivilegeFailure => (
                "privilegeFailure",
                "The privileged helper did not receive administrator access.",
            ),
            BrokerError::SystemRestoreFailure => (
                "systemRestoreFailure",
                "Windows System Restore could not create the restore point.",
            ),
        };
        Self { code, message }
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
    .map_err(|_| SecurityCommandError::helper_unavailable())?
    .map_err(SecurityCommandError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broker_failures_map_to_stable_frontend_codes() {
        for (error, expected_code) in [
            (
                BrokerError::AuthorizationCancelled,
                "authorizationCancelled",
            ),
            (BrokerError::HelperUnavailable, "helperUnavailable"),
            (BrokerError::Timeout, "operationTimedOut"),
            (BrokerError::InvalidRequest, "invalidRequest"),
            (BrokerError::PrivilegeFailure, "privilegeFailure"),
            (BrokerError::SystemRestoreFailure, "systemRestoreFailure"),
        ] {
            let command_error = SecurityCommandError::from(error);
            assert_eq!(command_error.code, expected_code);
            assert!(!command_error.message.is_empty());
        }
    }
}
