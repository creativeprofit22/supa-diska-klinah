use super::storage::{StorageCommandError, decode, scan_limits, valid_id};
use serde::Deserialize;
use std::sync::Arc;
use windows_platform::cleanup::CleanupService;
use windows_platform::storage::{
    FileFilter, StorageLimits, StorageModule, current_protection, large_files,
    scans::{JobError, StorageService},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StartInput {
    root_id: String,
    depth: u16,
    filter: FileFilter,
}

#[tauri::command]
pub(crate) async fn start_large_files(
    service: tauri::State<'_, Arc<StorageService>>,
    cleanup: tauri::State<'_, Arc<CleanupService>>,
    request: tauri::ipc::Request<'_>,
) -> Result<String, StorageCommandError> {
    let input: StartInput = decode(&request)?;
    if !valid_id(&input.root_id)
        || input.depth > StorageLimits::default().depth
        || input.filter.validate().is_err()
    {
        return Err(StorageCommandError::invalid());
    }
    let service = Arc::clone(service.inner());
    let cleanup = Arc::clone(cleanup.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let protection = current_protection().map_err(JobError::Storage)?;
        let limits = StorageLimits {
            depth: input.depth,
            ..scan_limits(&cleanup, &service, &input.root_id)
        };
        service.start_with(
            StorageModule::LargeFiles,
            Some(&input.root_id),
            limits,
            Some(protection.clone()),
            move |context| {
                large_files::discover(context, &protection, input.filter).map_err(JobError::Storage)
            },
        )
    })
    .await
    .map_err(|_| StorageCommandError::unavailable())?
    .map_err(Into::into)
}
