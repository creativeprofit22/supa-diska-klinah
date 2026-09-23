use serde::Serialize;
use tauri::Manager;
use windows_platform::{
    scheduler::{ScanSchedule, ScheduledScanSummary},
    system_change::AdapterError,
};

/// Summaries recorded by `--scheduled-scan` runs, newest first.
#[tauri::command]
pub(crate) async fn list_scheduled_scan_summaries<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<Vec<ScheduledScanSummary>, SchedulerCommandError> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|_| SchedulerCommandError::from(AdapterError::Failed))?;
    tauri::async_runtime::spawn_blocking(move || {
        windows_platform::scheduler::read_summaries(&app_data)
    })
    .await
    .map_err(|_| SchedulerCommandError::from(AdapterError::Failed))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SchedulerCommandError {
    code: &'static str,
    message: &'static str,
}

impl From<AdapterError> for SchedulerCommandError {
    fn from(error: AdapterError) -> Self {
        let (code, message) = match error {
            AdapterError::Unsupported(_) => {
                ("apiUnavailable", "Windows Task Scheduler is not available.")
            }
            AdapterError::Denied => (
                "accessDenied",
                "Windows denied access to the scheduled tasks.",
            ),
            AdapterError::Failed => ("systemError", "Scheduled scans could not be listed."),
        };
        Self { code, message }
    }
}

/// Read-only list of this app's scheduled scans. Create/update/remove go
/// through the shared system-change plan commands.
#[tauri::command]
pub(crate) async fn list_scan_schedules() -> Result<Vec<ScanSchedule>, SchedulerCommandError> {
    tauri::async_runtime::spawn_blocking(windows_platform::scheduler::list_scan_schedules)
        .await
        .map_err(|_| SchedulerCommandError::from(AdapterError::Failed))?
        .map_err(Into::into)
}
