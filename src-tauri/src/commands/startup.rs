use serde::Serialize;
use windows_platform::startup_items::StartupItem;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StartupCommandError {
    code: &'static str,
    message: &'static str,
}

/// Read-only startup inventory. Enable/disable goes through the shared
/// system-change plan commands.
#[tauri::command]
pub(crate) async fn list_startup_items() -> Result<Vec<StartupItem>, StartupCommandError> {
    tauri::async_runtime::spawn_blocking(windows_platform::startup_items::list_startup_items)
        .await
        .map_err(|_| StartupCommandError {
            code: "startupUnavailable",
            message: "Startup items could not be listed.",
        })
}
