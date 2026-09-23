use super::storage::{StorageCommandError, decode, valid_id};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use windows_platform::{
    cleanup::CleanupService,
    history::{HistoryKind, HistoryPage, HistoryRequest},
    storage::{
        StorageLimits,
        scans::StorageService,
        uninstaller::ProgramQuery,
        vendor_uninstall::{VendorJob, VendorJobError},
    },
};

#[derive(Debug, Serialize)]
pub(crate) struct VendorCommandError {
    code: &'static str,
}
impl From<VendorJobError> for VendorCommandError {
    fn from(error: VendorJobError) -> Self {
        Self {
            code: match error {
                VendorJobError::Busy => "busy",
                VendorJobError::InvalidId | VendorJobError::InvalidHistoryRequest => {
                    "invalid_input"
                }
                VendorJobError::HistoryCountCapacity => "history_count_capacity",
                VendorJobError::HistorySizeCapacity => "history_size_capacity",
                VendorJobError::Expired => "expired",
                VendorJobError::RegistryChanged => "registry_changed",
                VendorJobError::UnsupportedCommand => "unsupported_command",
                VendorJobError::ExecutableChanged => "executable_changed",
                VendorJobError::Storage => "vendor_unavailable",
                VendorJobError::Limit => "limit_reached",
                VendorJobError::Conflict => "conflict",
            },
        }
    }
}
impl From<StorageCommandError> for VendorCommandError {
    fn from(_: StorageCommandError) -> Self {
        Self {
            code: "invalid_input",
        }
    }
}
async fn blocking<T: Send + 'static>(
    op: impl FnOnce() -> Result<T, VendorJobError> + Send + 'static,
) -> Result<T, VendorCommandError> {
    tauri::async_runtime::spawn_blocking(op)
        .await
        .map_err(|_| VendorCommandError {
            code: "vendor_unavailable",
        })?
        .map_err(Into::into)
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PrepareInput {
    snapshot_id: String,
    program_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct JobInput {
    job_id: String,
}
fn job_input(request: &tauri::ipc::Request<'_>) -> Result<JobInput, VendorCommandError> {
    let input: JobInput = decode(request)?;
    if !valid_id(&input.job_id) {
        return Err(VendorJobError::InvalidId.into());
    }
    Ok(input)
}
#[tauri::command]
pub(crate) async fn start_program_inventory(
    cleanup: tauri::State<'_, Arc<CleanupService>>,
    storage: tauri::State<'_, Arc<StorageService>>,
    request: tauri::ipc::Request<'_>,
) -> Result<String, StorageCommandError> {
    let query: ProgramQuery = decode(&request)?;
    let cleanup = Arc::clone(cleanup.inner());
    let storage = Arc::clone(storage.inner());
    tauri::async_runtime::spawn_blocking(move || {
        cleanup
            .vendor_jobs()
            .map_err(|_| StorageCommandError::unavailable())?
            .start_inventory(&storage, StorageLimits::default(), query)
            .map_err(Into::into)
    })
    .await
    .map_err(|_| StorageCommandError::unavailable())?
}
#[tauri::command]
pub(crate) async fn prepare_vendor_job(
    cleanup: tauri::State<'_, Arc<CleanupService>>,
    storage: tauri::State<'_, Arc<StorageService>>,
    request: tauri::ipc::Request<'_>,
) -> Result<VendorJob, VendorCommandError> {
    let input: PrepareInput = decode(&request)?;
    if !valid_id(&input.snapshot_id) || !valid_id(&input.program_id) {
        return Err(VendorJobError::InvalidId.into());
    }
    let cleanup = Arc::clone(cleanup.inner());
    let storage = Arc::clone(storage.inner());
    blocking(move || {
        cleanup
            .vendor_jobs()?
            .prepare(&storage, &input.snapshot_id, &input.program_id)
    })
    .await
}
#[tauri::command]
pub(crate) async fn confirm_vendor_job<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    cleanup: tauri::State<'_, Arc<CleanupService>>,
    request: tauri::ipc::Request<'_>,
) -> Result<VendorJob, VendorCommandError> {
    let input = job_input(&request)?;
    let owner = window
        .hwnd()
        .map_err(|_| VendorCommandError {
            code: "vendor_unavailable",
        })?
        .0 as isize;
    let cleanup = Arc::clone(cleanup.inner());
    blocking(move || cleanup.vendor_jobs()?.confirm_native(&input.job_id, owner)).await
}
#[tauri::command]
pub(crate) async fn vendor_job_status(
    cleanup: tauri::State<'_, Arc<CleanupService>>,
    request: tauri::ipc::Request<'_>,
) -> Result<VendorJob, VendorCommandError> {
    let input = job_input(&request)?;
    let cleanup = Arc::clone(cleanup.inner());
    blocking(move || cleanup.vendor_jobs()?.status(&input.job_id)).await
}
#[tauri::command]
pub(crate) async fn cancel_vendor_job(
    cleanup: tauri::State<'_, Arc<CleanupService>>,
    request: tauri::ipc::Request<'_>,
) -> Result<VendorJob, VendorCommandError> {
    let input = job_input(&request)?;
    let cleanup = Arc::clone(cleanup.inner());
    blocking(move || cleanup.vendor_jobs()?.cancel(&input.job_id)).await
}
#[tauri::command]
pub(crate) async fn release_vendor_job(
    cleanup: tauri::State<'_, Arc<CleanupService>>,
    request: tauri::ipc::Request<'_>,
) -> Result<(), VendorCommandError> {
    let input = job_input(&request)?;
    let cleanup = Arc::clone(cleanup.inner());
    blocking(move || cleanup.vendor_jobs()?.release(&input.job_id)).await
}
#[tauri::command]
pub(crate) async fn vendor_job_history(
    cleanup: tauri::State<'_, Arc<CleanupService>>,
    request: tauri::ipc::Request<'_>,
) -> Result<HistoryPage<VendorJob>, VendorCommandError> {
    let input: HistoryRequest = decode(&request)?;
    input
        .validate(HistoryKind::Vendor)
        .map_err(|_| VendorJobError::InvalidHistoryRequest)?;
    let cleanup = Arc::clone(cleanup.inner());
    blocking(move || cleanup.vendor_jobs()?.history_page(input)).await
}
