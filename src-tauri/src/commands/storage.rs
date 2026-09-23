use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::sync::Arc;
use windows_platform::{
    cleanup::{CleanupDisposition, CleanupService, CleanupServiceError},
    storage::{
        self, PageRequest, StorageError, StorageModule, StoragePage, StorageSelection,
        StorageStatus,
        root_picker::{NativeScope, RootChoice, ScopeService},
        scans::{JobError, StorageService},
    },
};

#[derive(Debug, Serialize)]
pub(crate) struct StorageCommandError {
    code: &'static str,
}
impl StorageCommandError {
    pub(super) fn invalid() -> Self {
        Self {
            code: "invalid_input",
        }
    }
    pub(super) fn unavailable() -> Self {
        Self {
            code: "storage_unavailable",
        }
    }
}
impl From<JobError> for StorageCommandError {
    fn from(error: JobError) -> Self {
        Self {
            code: match error {
                JobError::Busy => "busy",
                JobError::Storage(StorageError::InvalidRequest) => "invalid_input",
                JobError::Storage(StorageError::InvalidCursor) => "invalid_cursor",
                JobError::Storage(StorageError::InvalidEvidence) => "invalid_evidence",
                JobError::Storage(StorageError::SnapshotUnavailable) => "snapshot_unavailable",
                JobError::Storage(StorageError::UnsupportedScope) => "scope_unavailable",
                JobError::Storage(StorageError::LimitReached) => "limit_reached",
                JobError::Storage(StorageError::Entropy)
                | JobError::Native(_)
                | JobError::WorkerFailed => "storage_unavailable",
            },
        }
    }
}
pub(super) fn decode<T: DeserializeOwned>(
    request: &tauri::ipc::Request<'_>,
) -> Result<T, StorageCommandError> {
    // Raw UTF-8 JSON only: enforce the byte cap before allocating nested DTOs.
    // The renderer supplies no paths, native handles, proofs, runners or limits.
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err(StorageCommandError::invalid());
    };
    if bytes.len() > storage::MAX_REQUEST_BYTES {
        return Err(JobError::Storage(StorageError::LimitReached).into());
    }
    // serde can deserialize named structs from positional JSON arrays. IPC uses
    // object-shaped DTOs only, including commands with an empty input object.
    if bytes.iter().find(|byte| !byte.is_ascii_whitespace()) != Some(&b'{') {
        return Err(StorageCommandError::invalid());
    }
    storage::decode_command(bytes).map_err(|error| JobError::Storage(error).into())
}
pub(super) fn valid_id(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ModuleInput {
    module: StorageModule,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ScopeInput {
    module: StorageModule,
    scope_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SnapshotInput {
    module: StorageModule,
    snapshot_id: String,
}
impl SnapshotInput {
    fn validate(&self) -> Result<(), StorageCommandError> {
        if valid_id(&self.snapshot_id) {
            Ok(())
        } else {
            Err(StorageCommandError::invalid())
        }
    }
    fn status(&self, service: &StorageService) -> Result<StorageStatus, StorageCommandError> {
        let (status, _) = service.status(&self.snapshot_id)?;
        if status.module != self.module {
            return Err(StorageCommandError {
                code: "invalid_evidence",
            });
        }
        // Internal failure strings may contain paths. Only the typed phase crosses IPC.
        Ok(status)
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PlanInput {
    selection: StorageSelection,
    disposition: CleanupDisposition,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StoragePlan {
    plan_id: String,
    disposition: CleanupDisposition,
    selected_count: usize,
    selected_bytes: u64,
}

#[tauri::command]
pub(crate) async fn choose_storage_root<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    service: tauri::State<'_, Arc<StorageService>>,
    scopes: tauri::State<'_, Arc<ScopeService>>,
    request: tauri::ipc::Request<'_>,
) -> Result<Option<RootChoice>, StorageCommandError> {
    let input: ModuleInput = decode(&request)?;
    if !storage::root_picker::personal_module(input.module) {
        return Err(StorageCommandError::invalid());
    }
    let owner = window
        .hwnd()
        .map_err(|_| StorageCommandError::unavailable())?
        .0 as isize;
    let service = Arc::clone(service.inner());
    let scopes = Arc::clone(scopes.inner());
    tauri::async_runtime::spawn_blocking(move || scopes.choose(&service, input.module, owner))
        .await
        .map_err(|_| StorageCommandError::unavailable())?
        .map_err(Into::into)
}
#[tauri::command]
pub(crate) async fn list_storage_scopes(
    scopes: tauri::State<'_, Arc<ScopeService>>,
    request: tauri::ipc::Request<'_>,
) -> Result<Vec<NativeScope>, StorageCommandError> {
    let input: ModuleInput = decode(&request)?;
    let scopes = Arc::clone(scopes.inner());
    tauri::async_runtime::spawn_blocking(move || scopes.list(input.module))
        .await
        .map_err(|_| StorageCommandError::unavailable())?
        .map_err(Into::into)
}
#[tauri::command]
pub(crate) async fn authorize_storage_scope(
    service: tauri::State<'_, Arc<StorageService>>,
    scopes: tauri::State<'_, Arc<ScopeService>>,
    request: tauri::ipc::Request<'_>,
) -> Result<RootChoice, StorageCommandError> {
    let input: ScopeInput = decode(&request)?;
    if !valid_id(&input.scope_id) {
        return Err(StorageCommandError::invalid());
    }
    let service = Arc::clone(service.inner());
    let scopes = Arc::clone(scopes.inner());
    tauri::async_runtime::spawn_blocking(move || {
        scopes.authorize_scope(&service, input.module, &input.scope_id)
    })
    .await
    .map_err(|_| StorageCommandError::unavailable())?
    .map_err(Into::into)
}
#[tauri::command]
pub(crate) async fn storage_scan_status(
    service: tauri::State<'_, Arc<StorageService>>,
    request: tauri::ipc::Request<'_>,
) -> Result<StorageStatus, StorageCommandError> {
    let input: SnapshotInput = decode(&request)?;
    input.validate()?;
    let service = Arc::clone(service.inner());
    tauri::async_runtime::spawn_blocking(move || input.status(&service))
        .await
        .map_err(|_| StorageCommandError::unavailable())?
}
#[tauri::command]
pub(crate) async fn storage_scan_page(
    service: tauri::State<'_, Arc<StorageService>>,
    request: tauri::ipc::Request<'_>,
) -> Result<StoragePage, StorageCommandError> {
    let input: PageRequest = decode(&request)?;
    input
        .validate()
        .map_err(|_| StorageCommandError::invalid())?;
    let service = Arc::clone(service.inner());
    tauri::async_runtime::spawn_blocking(move || service.page(&input))
        .await
        .map_err(|_| StorageCommandError::unavailable())?
        .map_err(Into::into)
}
#[tauri::command]
pub(crate) async fn cancel_storage_scan(
    service: tauri::State<'_, Arc<StorageService>>,
    request: tauri::ipc::Request<'_>,
) -> Result<(), StorageCommandError> {
    let input: SnapshotInput = decode(&request)?;
    input.validate()?;
    let service = Arc::clone(service.inner());
    tauri::async_runtime::spawn_blocking(move || {
        input.status(&service)?;
        service.cancel(&input.snapshot_id).map_err(Into::into)
    })
    .await
    .map_err(|_| StorageCommandError::unavailable())?
}
#[tauri::command]
pub(crate) async fn release_storage_scan(
    service: tauri::State<'_, Arc<StorageService>>,
    request: tauri::ipc::Request<'_>,
) -> Result<(), StorageCommandError> {
    let input: SnapshotInput = decode(&request)?;
    input.validate()?;
    let service = Arc::clone(service.inner());
    tauri::async_runtime::spawn_blocking(move || {
        service.release_for(input.module, &input.snapshot_id)
    })
    .await
    .map_err(|_| StorageCommandError::unavailable())?
    .map_err(Into::into)
}
#[tauri::command]
pub(crate) async fn create_storage_plan(
    service: tauri::State<'_, Arc<StorageService>>,
    cleanup: tauri::State<'_, Arc<CleanupService>>,
    request: tauri::ipc::Request<'_>,
) -> Result<StoragePlan, StorageCommandError> {
    let input: PlanInput = decode(&request)?;
    input
        .selection
        .validate()
        .map_err(|_| StorageCommandError::invalid())?;
    let service = Arc::clone(service.inner());
    let cleanup = Arc::clone(cleanup.inner());
    tauri::async_runtime::spawn_blocking(move || {
        cleanup
            .create_storage_plan(&service, &input.selection, input.disposition)
            .map(|plan| StoragePlan {
                plan_id: plan.plan_id,
                disposition: plan.disposition,
                selected_count: plan.selected_count,
                selected_bytes: plan.selected_bytes,
            })
            .map_err(|error| StorageCommandError {
                code: match error {
                    CleanupServiceError::InvalidInput => "invalid_input",
                    CleanupServiceError::NotFound => "snapshot_unavailable",
                    CleanupServiceError::ValidationFailed => "invalid_evidence",
                    CleanupServiceError::RecoveryVolumeUnsupported => "recovery_volume_unsupported",
                    CleanupServiceError::Conflict => "busy",
                    _ => "storage_unavailable",
                },
            })
    })
    .await
    .map_err(|_| StorageCommandError::unavailable())?
}
