use super::storage::{StorageCommandError, decode, valid_id};
use serde::Deserialize;
use std::sync::Arc;
use windows_platform::storage::{
    StorageLimits, StorageModule, current_protection, disk_analyzer,
    scans::{JobError, StorageService},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StartInput {
    root_id: String,
    displayed_depth: u16,
}

/// Read-only runner chosen by Rust. Display depth does not truncate subtree totals.
#[tauri::command]
pub(crate) async fn start_disk_analyzer(
    service: tauri::State<'_, Arc<StorageService>>,
    request: tauri::ipc::Request<'_>,
) -> Result<String, StorageCommandError> {
    let input: StartInput = decode(&request)?;
    let limits = StorageLimits::default();
    if !valid_id(&input.root_id) || input.displayed_depth > limits.depth {
        return Err(StorageCommandError::invalid());
    }
    let service = Arc::clone(service.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let protection = current_protection().map_err(JobError::Storage)?;
        service.start_with(
            StorageModule::DiskAnalyzer,
            Some(&input.root_id),
            limits,
            Some(protection.clone()),
            move |context| {
                disk_analyzer::discover(context, &protection, input.displayed_depth)
                    .map_err(JobError::Storage)
            },
        )
    })
    .await
    .map_err(|_| StorageCommandError::unavailable())?
    .map_err(Into::into)
}
