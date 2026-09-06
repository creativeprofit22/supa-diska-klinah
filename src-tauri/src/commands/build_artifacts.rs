use serde::Serialize;
use std::sync::Arc;
use windows_platform::cleanup::{
    ArtifactBudgetPolicy, ArtifactBudgetPreview, BuildArtifactError, BuildProfile, BuildRun,
    CleanupService, RegisterBuildProfileInput, SetArtifactBudgetPolicyResult,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BuildArtifactCommandError {
    code: &'static str,
    message: &'static str,
}

impl From<BuildArtifactError> for BuildArtifactCommandError {
    fn from(error: BuildArtifactError) -> Self {
        let (code, message) = match error {
            BuildArtifactError::InvalidInput => {
                ("invalidInput", "The build artifact request was invalid.")
            }
            BuildArtifactError::ApprovalDeclined => {
                ("approvalDeclined", "The native approval was declined.")
            }
            BuildArtifactError::NotFound => ("notFound", "The build item is no longer available."),
            BuildArtifactError::BuildBusy => ("buildBusy", "Another build is already running."),
            BuildArtifactError::ValidationFailed => (
                "validationFailed",
                "The operation stopped because registered build state changed.",
            ),
            BuildArtifactError::OperationFailed => (
                "operationFailed",
                "The build operation could not be completed.",
            ),
        };
        Self { code, message }
    }
}

async fn run_blocking<T>(
    operation: impl FnOnce() -> Result<T, BuildArtifactError> + Send + 'static,
) -> Result<T, BuildArtifactCommandError>
where
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(|_| BuildArtifactCommandError {
            code: "taskUnavailable",
            message: "The build task could not be started.",
        })?
        .map_err(BuildArtifactCommandError::from)
}

#[tauri::command]
pub(crate) async fn list_build_profiles(
    service: tauri::State<'_, Arc<CleanupService>>,
) -> Result<Vec<BuildProfile>, BuildArtifactCommandError> {
    let service = Arc::clone(service.inner());
    run_blocking(move || service.build_profiles()).await
}

#[tauri::command]
pub(crate) async fn register_build_profile(
    service: tauri::State<'_, Arc<CleanupService>>,
    input: RegisterBuildProfileInput,
) -> Result<BuildProfile, BuildArtifactCommandError> {
    let service = Arc::clone(service.inner());
    run_blocking(move || service.register_build_profile(input)).await
}

#[tauri::command]
pub(crate) async fn remove_build_profile(
    service: tauri::State<'_, Arc<CleanupService>>,
    profile_id: String,
) -> Result<(), BuildArtifactCommandError> {
    let service = Arc::clone(service.inner());
    run_blocking(move || service.remove_build_profile(&profile_id)).await
}

#[tauri::command]
pub(crate) async fn start_build_run(
    service: tauri::State<'_, Arc<CleanupService>>,
    profile_id: String,
) -> Result<BuildRun, BuildArtifactCommandError> {
    let service = Arc::clone(service.inner());
    run_blocking(move || service.start_build_run(&profile_id)).await
}

#[tauri::command]
pub(crate) async fn get_active_build_run(
    service: tauri::State<'_, Arc<CleanupService>>,
) -> Result<Option<BuildRun>, BuildArtifactCommandError> {
    let service = Arc::clone(service.inner());
    run_blocking(move || service.active_build_run()).await
}

#[tauri::command]
pub(crate) async fn get_build_run(
    service: tauri::State<'_, Arc<CleanupService>>,
    run_id: String,
) -> Result<BuildRun, BuildArtifactCommandError> {
    let service = Arc::clone(service.inner());
    run_blocking(move || service.build_run(&run_id)).await
}

#[tauri::command]
pub(crate) async fn cancel_build_run(
    service: tauri::State<'_, Arc<CleanupService>>,
    run_id: String,
) -> Result<BuildRun, BuildArtifactCommandError> {
    let service = Arc::clone(service.inner());
    run_blocking(move || service.cancel_build_run(&run_id)).await
}

#[tauri::command]
pub(crate) async fn get_artifact_budget_policy(
    service: tauri::State<'_, Arc<CleanupService>>,
) -> Result<ArtifactBudgetPolicy, BuildArtifactCommandError> {
    let service = Arc::clone(service.inner());
    run_blocking(move || service.artifact_budget_policy()).await
}

#[tauri::command]
pub(crate) async fn set_artifact_budget_policy(
    service: tauri::State<'_, Arc<CleanupService>>,
    policy: ArtifactBudgetPolicy,
) -> Result<SetArtifactBudgetPolicyResult, BuildArtifactCommandError> {
    let service = Arc::clone(service.inner());
    run_blocking(move || service.set_artifact_budget_policy(policy)).await
}

#[tauri::command]
pub(crate) async fn preview_artifact_budgets(
    service: tauri::State<'_, Arc<CleanupService>>,
) -> Result<ArtifactBudgetPreview, BuildArtifactCommandError> {
    let service = Arc::clone(service.inner());
    run_blocking(move || service.preview_artifact_budget()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_build_run_service_is_idle_after_recreation() {
        let root = std::env::temp_dir().join(
            [
                "supa-diska-active-run-command-",
                &std::process::id().to_string(),
                "-",
                &std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
                    .to_string(),
            ]
            .concat(),
        );
        for _ in 0..2 {
            let service = Arc::new(CleanupService::new(root.clone()).unwrap());
            let run: Option<BuildRun> =
                tauri::async_runtime::block_on(run_blocking(move || service.active_build_run()))
                    .unwrap();
            assert!(run.is_none());
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn active_build_run_command_boundary_preserves_snapshot_and_fixed_errors() {
        let run = tauri::async_runtime::block_on(run_blocking(|| {
            Ok(Some(BuildRun {
                run_id: "a".repeat(32),
                profile_id: "b".repeat(32),
                state: windows_platform::cleanup::BuildRunState::Running,
                started_at: Some(123),
                completed_at: None,
                exit_code: None,
            }))
        }))
        .unwrap()
        .unwrap();
        assert_eq!(run.run_id, "a".repeat(32));
        assert_eq!(run.profile_id, "b".repeat(32));
        assert_eq!(run.started_at, Some(123));
        assert!(run.completed_at.is_none());
        assert!(run.exit_code.is_none());
        let error = tauri::async_runtime::block_on(run_blocking::<Option<BuildRun>>(|| {
            Err(BuildArtifactError::OperationFailed)
        }))
        .unwrap_err();
        assert_eq!(error.code, "operationFailed");
        assert_eq!(error.message, "The build operation could not be completed.");
    }
}
