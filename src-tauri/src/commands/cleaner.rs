use super::storage::{StorageCommandError, decode, scan_limits, valid_id};
use serde::Deserialize;
use std::sync::Arc;
use windows_platform::cleanup::CleanupService;
use windows_platform::storage::{
    StorageModule, cleaner, current_protection,
    scans::{JobError, StorageService},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogInput {}

#[tauri::command]
pub(crate) async fn list_cleaner_catalog(
    request: tauri::ipc::Request<'_>,
) -> Result<cleaner::CleanerCatalog, StorageCommandError> {
    let _: CatalogInput = decode(&request)?;
    Ok(cleaner::catalog_inventory())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StartInput {
    root_id: String,
}

#[tauri::command]
pub(crate) async fn start_cleaner(
    service: tauri::State<'_, Arc<StorageService>>,
    cleanup: tauri::State<'_, Arc<CleanupService>>,
    request: tauri::ipc::Request<'_>,
) -> Result<String, StorageCommandError> {
    let input: StartInput = decode(&request)?;
    if !valid_id(&input.root_id) {
        return Err(StorageCommandError::invalid());
    }
    let service = Arc::clone(service.inner());
    let cleanup = Arc::clone(cleanup.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let protection = current_protection().map_err(JobError::Storage)?;
        service.start_with(
            StorageModule::Cleaner,
            Some(&input.root_id),
            scan_limits(&cleanup, &service, &input.root_id),
            Some(protection.clone()),
            move |context| cleaner::discover(context, &protection).map_err(JobError::Storage),
        )
    })
    .await
    .map_err(|_| StorageCommandError::unavailable())?
    .map_err(Into::into)
}
