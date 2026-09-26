use serde::Serialize;
use windows_platform::privacy::{PrivacyReport, privacy_report};

#[derive(Debug, Serialize)]
pub(crate) struct PrivacyCommandError {
    code: &'static str,
    message: &'static str,
}

#[tauri::command]
pub(crate) async fn get_privacy_report() -> Result<PrivacyReport, PrivacyCommandError> {
    tauri::async_runtime::spawn_blocking(privacy_report)
        .await
        .map_err(|_| PrivacyCommandError {
            code: "privacyReportFailed",
            message: "The privacy report could not be generated.",
        })
}
