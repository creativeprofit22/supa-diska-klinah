use super::storage::{StorageCommandError, decode, valid_id};
use serde::Deserialize;
use std::sync::Arc;
use windows_platform::storage::{
    FileFilter, StorageLimits, StorageModule, current_protection, duplicates,
    scans::{JobError, StorageService},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StartInput {
    root_id: String,
    depth: u16,
    minimum_bytes: u64,
    #[serde(default)]
    maximum_bytes: Option<u64>,
    #[serde(default)]
    extensions: Vec<String>,
}

#[tauri::command]
pub(crate) async fn start_duplicates(
    service: tauri::State<'_, Arc<StorageService>>,
    request: tauri::ipc::Request<'_>,
) -> Result<String, StorageCommandError> {
    let input: StartInput = decode(&request)?;
    let filter = FileFilter {
        minimum_bytes: input.minimum_bytes,
        maximum_bytes: input.maximum_bytes,
        extensions: input.extensions,
        ..Default::default()
    };
    if !valid_id(&input.root_id)
        || input.depth > StorageLimits::default().depth
        || filter.validate().is_err()
    {
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
            StorageModule::Duplicates,
            Some(&input.root_id),
            limits,
            Some(protection.clone()),
            move |context| {
                duplicates::discover(context, &protection, filter).map_err(JobError::Storage)
            },
        )
    })
    .await
    .map_err(|_| StorageCommandError::unavailable())?
    .map_err(Into::into)
}
