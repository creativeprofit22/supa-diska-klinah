use super::storage::{StorageCommandError, decode, valid_id};
use serde::Deserialize;
use std::sync::Arc;
use windows_platform::storage::{
    StorageLimits, StorageModule, current_protection, empty_folders,
    scans::{JobError, StorageService},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StartInput {
    root_id: String,
    depth: u16,
}

#[tauri::command]
pub(crate) async fn start_empty_folders(
    service: tauri::State<'_, Arc<StorageService>>,
    request: tauri::ipc::Request<'_>,
) -> Result<String, StorageCommandError> {
    let input: StartInput = decode(&request)?;
    if !valid_id(&input.root_id) || input.depth > StorageLimits::default().depth {
        return Err(StorageCommandError::invalid());
    }
    let service = Arc::clone(service.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let protection = current_protection().map_err(JobError::Storage)?;
        let limits = StorageLimits {
            depth: input.depth,
            ..Default::default()
        };
        service.start_with(
            StorageModule::EmptyFolders,
            Some(&input.root_id),
            limits,
            Some(protection.clone()),
            move |context| empty_folders::discover(context, &protection).map_err(JobError::Storage),
        )
    })
    .await
    .map_err(|_| StorageCommandError::unavailable())?
    .map_err(Into::into)
}
