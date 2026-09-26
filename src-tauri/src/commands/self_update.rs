//! Thin IPC wrappers over the opt-in self-updater. The update-check opt-in is
//! read from saved app settings here, never taken from the webview, so a page
//! cannot enable network access by itself.

use std::sync::Arc;

use windows_platform::app_settings::AppSettingsService;
use windows_platform::protection::net::SystemTransport;
use windows_platform::self_update::{
    UpdateCheckPolicy, UpdateError, UpdateService, UpdateStatus, native_ui,
};

pub(crate) type AppUpdateService = UpdateService<SystemTransport>;

fn policy(settings: &AppSettingsService) -> UpdateCheckPolicy {
    UpdateCheckPolicy {
        enabled: settings.get().update_check,
    }
}

async fn run_blocking<T: Send + 'static>(
    operation: impl FnOnce() -> Result<T, UpdateError> + Send + 'static,
) -> Result<T, UpdateError> {
    tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(|_| UpdateError::Busy)?
}

#[tauri::command]
pub(crate) fn get_update_status(service: tauri::State<'_, Arc<AppUpdateService>>) -> UpdateStatus {
    service.status()
}

#[tauri::command]
pub(crate) async fn check_for_update(
    service: tauri::State<'_, Arc<AppUpdateService>>,
    settings: tauri::State<'_, Arc<AppSettingsService>>,
) -> Result<UpdateStatus, UpdateError> {
    let (service, policy) = (Arc::clone(service.inner()), policy(settings.inner()));
    run_blocking(move || service.check(policy)).await
}

#[tauri::command]
pub(crate) async fn download_update(
    service: tauri::State<'_, Arc<AppUpdateService>>,
    settings: tauri::State<'_, Arc<AppSettingsService>>,
) -> Result<UpdateStatus, UpdateError> {
    let (service, policy) = (Arc::clone(service.inner()), policy(settings.inner()));
    run_blocking(move || service.download(policy)).await
}

/// Asks natively (default No), then opens the verified installer and exits so
/// the installer can replace the app's files.
#[tauri::command]
pub(crate) async fn install_update<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    window: tauri::WebviewWindow<R>,
    service: tauri::State<'_, Arc<AppUpdateService>>,
) -> Result<UpdateStatus, UpdateError> {
    let owner = window.hwnd().map_err(|_| UpdateError::LaunchFailed)?.0 as isize;
    let service = Arc::clone(service.inner());
    let status = run_blocking(move || {
        service.install(&|title, body| native_ui::confirm_install(owner, title, body))
    })
    .await?;
    app.exit(0);
    Ok(status)
}

#[tauri::command]
pub(crate) async fn discard_update(
    service: tauri::State<'_, Arc<AppUpdateService>>,
) -> Result<UpdateStatus, UpdateError> {
    let service = Arc::clone(service.inner());
    run_blocking(move || service.discard()).await
}

/// Hides the one-time startup recovery notice. Never touches staged files, so
/// an interrupted update can still be retried or discarded in Settings.
#[tauri::command]
pub(crate) fn acknowledge_update_recovery(
    service: tauri::State<'_, Arc<AppUpdateService>>,
) -> UpdateStatus {
    service.acknowledge_recovery()
}
