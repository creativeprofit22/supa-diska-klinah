use super::storage::{StorageCommandError, decode, scan_limits, valid_id};
use serde::Deserialize;
use std::sync::Arc;
use windows_platform::cleanup::CleanupService;
use windows_platform::storage::{
    StorageModule, browser, current_protection,
    scans::{JobError, StorageService},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyInput {}

#[tauri::command]
pub(crate) async fn list_browser_policy(
    request: tauri::ipc::Request<'_>,
) -> Result<browser::BrowserPolicy, StorageCommandError> {
    let _: PolicyInput = decode(&request)?;
    Ok(browser::policy())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StartInput {
    root_id: String,
    service_worker_opt_in: bool,
}

#[tauri::command]
pub(crate) async fn start_browser_scan(
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
            StorageModule::Browser,
            Some(&input.root_id),
            scan_limits(&cleanup, &service, &input.root_id),
            Some(protection.clone()),
            move |context| {
                browser::discover(context, &protection, input.service_worker_opt_in)
                    .map_err(JobError::Storage)
            },
        )
    })
    .await
    .map_err(|_| StorageCommandError::unavailable())?
    .map_err(Into::into)
}
