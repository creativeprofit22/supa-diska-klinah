use serde::Serialize;
use std::sync::Arc;
use windows_platform::cleanup::{
    AutoCleanupPolicy, CleanupDisposition, CleanupExecutionSummary, CleanupPlanSummary,
    CleanupPreview, CleanupPreviewError, CleanupService, CleanupServiceError,
    ProjectArtifactDiscovery, discover_project_artifacts as discover_project_artifacts_adapter,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CleanupCommandError {
    code: &'static str,
    message: &'static str,
}

impl CleanupCommandError {
    fn task_unavailable() -> Self {
        Self {
            code: "taskUnavailable",
            message: "Cleanup operation could not be started.",
        }
    }
}

impl From<CleanupServiceError> for CleanupCommandError {
    fn from(error: CleanupServiceError) -> Self {
        let (code, message) = match error {
            CleanupServiceError::InvalidInput => {
                ("invalidInput", "The cleanup request was invalid.")
            }
            CleanupServiceError::NotFound => {
                ("notFound", "The cleanup selection is no longer available.")
            }
            CleanupServiceError::Conflict => {
                ("cleanupBusy", "Another cleanup operation is running.")
            }
            CleanupServiceError::ValidationFailed => (
                "validationFailed",
                "Cleanup stopped because an item changed.",
            ),
            CleanupServiceError::PersistenceFailed => {
                ("persistenceFailed", "Cleanup records could not be saved.")
            }
            CleanupServiceError::OperationFailed => {
                ("operationFailed", "Cleanup could not be completed.")
            }
        };
        Self { code, message }
    }
}

impl From<CleanupPreviewError> for CleanupCommandError {
    fn from(error: CleanupPreviewError) -> Self {
        match error {
            CleanupPreviewError::ProjectRootInvalid => Self {
                code: "invalidInput",
                message: "The project root was invalid.",
            },
            _ => Self {
                code: "discoveryFailed",
                message: "Project artifacts could not be scanned.",
            },
        }
    }
}

async fn run_blocking<T, E>(
    operation: impl FnOnce() -> Result<T, E> + Send + 'static,
) -> Result<T, CleanupCommandError>
where
    T: Send + 'static,
    E: Send + 'static,
    CleanupCommandError: From<E>,
{
    tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(|_| CleanupCommandError::task_unavailable())?
        .map_err(CleanupCommandError::from)
}

#[tauri::command]
pub(crate) async fn preview_cleanup(
    service: tauri::State<'_, Arc<CleanupService>>,
) -> Result<CleanupPreview, CleanupCommandError> {
    let service = Arc::clone(service.inner());
    run_blocking(move || service.preview()).await
}

#[tauri::command]
pub(crate) async fn discover_project_artifacts(
    root: String,
) -> Result<ProjectArtifactDiscovery, CleanupCommandError> {
    run_blocking(move || discover_project_artifacts_adapter(&root)).await
}

fn validate_manual_disposition(disposition: CleanupDisposition) -> Result<(), CleanupCommandError> {
    if disposition == CleanupDisposition::Quarantine {
        Err(CleanupCommandError::from(CleanupServiceError::InvalidInput))
    } else {
        Ok(())
    }
}

#[tauri::command]
pub(crate) async fn create_cleanup_plan(
    service: tauri::State<'_, Arc<CleanupService>>,
    scan_id: String,
    candidate_ids: Vec<String>,
    disposition: CleanupDisposition,
) -> Result<CleanupPlanSummary, CleanupCommandError> {
    validate_manual_disposition(disposition)?;
    let service = Arc::clone(service.inner());
    run_blocking(move || service.create_plan(&scan_id, &candidate_ids, disposition)).await
}

#[tauri::command]
pub(crate) async fn execute_cleanup_plan(
    service: tauri::State<'_, Arc<CleanupService>>,
    plan_id: String,
) -> Result<CleanupExecutionSummary, CleanupCommandError> {
    let service = Arc::clone(service.inner());
    run_blocking(move || service.execute(&plan_id)).await
}

#[tauri::command]
pub(crate) async fn execute_permanent_cleanup_plan(
    service: tauri::State<'_, Arc<CleanupService>>,
    plan_id: String,
) -> Result<CleanupExecutionSummary, CleanupCommandError> {
    let service = Arc::clone(service.inner());
    run_blocking(move || service.execute_permanent(&plan_id)).await
}

#[tauri::command]
pub(crate) async fn undo_cleanup(
    service: tauri::State<'_, Arc<CleanupService>>,
    execution_id: String,
) -> Result<CleanupExecutionSummary, CleanupCommandError> {
    let service = Arc::clone(service.inner());
    run_blocking(move || service.undo(&execution_id)).await
}

#[tauri::command]
pub(crate) async fn cleanup_history(
    service: tauri::State<'_, Arc<CleanupService>>,
) -> Result<Vec<CleanupExecutionSummary>, CleanupCommandError> {
    let service = Arc::clone(service.inner());
    run_blocking(move || service.history()).await
}

#[tauri::command]
pub(crate) async fn get_auto_cleanup_policy(
    service: tauri::State<'_, Arc<CleanupService>>,
) -> Result<AutoCleanupPolicy, CleanupCommandError> {
    let service = Arc::clone(service.inner());
    run_blocking(move || service.policy()).await
}

#[tauri::command]
pub(crate) async fn set_auto_cleanup_policy(
    service: tauri::State<'_, Arc<CleanupService>>,
    enabled: bool,
    grace_days: u16,
) -> Result<AutoCleanupPolicy, CleanupCommandError> {
    let service = Arc::clone(service.inner());
    run_blocking(move || service.set_policy(enabled, grace_days)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipc_rejects_automatic_quarantine_mode() {
        assert!(validate_manual_disposition(CleanupDisposition::RecycleBin).is_ok());
        assert!(validate_manual_disposition(CleanupDisposition::Permanent).is_ok());
        let error = validate_manual_disposition(CleanupDisposition::Quarantine).unwrap_err();
        assert_eq!(error.code, "invalidInput");
    }

    #[test]
    fn project_discovery_failures_map_to_stable_non_sensitive_errors() {
        let invalid = CleanupCommandError::from(CleanupPreviewError::ProjectRootInvalid);
        assert_eq!(invalid.code, "invalidInput");
        assert_eq!(invalid.message, "The project root was invalid.");

        for error in [
            CleanupPreviewError::TemporaryRootUnavailable,
            CleanupPreviewError::ProtectionUnavailable,
            CleanupPreviewError::ProtectionInvalid,
            CleanupPreviewError::CatalogInvalid,
            CleanupPreviewError::ScanFailed,
        ] {
            let command_error = CleanupCommandError::from(error);
            assert_eq!(command_error.code, "discoveryFailed");
            assert_eq!(
                command_error.message,
                "Project artifacts could not be scanned."
            );
            assert!(!command_error.message.contains('\\'));
            assert!(!command_error.message.contains("C:"));
        }
    }

    #[test]
    fn service_failures_map_to_stable_non_sensitive_errors() {
        for (error, expected_code, expected_message) in [
            (
                CleanupServiceError::InvalidInput,
                "invalidInput",
                "The cleanup request was invalid.",
            ),
            (
                CleanupServiceError::NotFound,
                "notFound",
                "The cleanup selection is no longer available.",
            ),
            (
                CleanupServiceError::Conflict,
                "cleanupBusy",
                "Another cleanup operation is running.",
            ),
            (
                CleanupServiceError::ValidationFailed,
                "validationFailed",
                "Cleanup stopped because an item changed.",
            ),
            (
                CleanupServiceError::PersistenceFailed,
                "persistenceFailed",
                "Cleanup records could not be saved.",
            ),
            (
                CleanupServiceError::OperationFailed,
                "operationFailed",
                "Cleanup could not be completed.",
            ),
        ] {
            let command_error = CleanupCommandError::from(error);
            assert_eq!(command_error.code, expected_code);
            assert_eq!(command_error.message, expected_message);
            assert!(!command_error.message.contains('\\'));
            assert!(!command_error.message.contains("C:"));
        }
    }
}
