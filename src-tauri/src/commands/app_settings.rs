use serde::Serialize;
use std::sync::Arc;
use windows_platform::app_settings::{AppSettings, AppSettingsError, AppSettingsService};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppSettingsCommandError {
    code: &'static str,
    message: &'static str,
}

impl From<AppSettingsError> for AppSettingsCommandError {
    fn from(error: AppSettingsError) -> Self {
        match error {
            AppSettingsError::Invalid => Self {
                code: "invalidInput",
                message: "The settings were invalid.",
            },
            AppSettingsError::Io => Self {
                code: "persistenceFailed",
                message: "Settings could not be saved.",
            },
        }
    }
}

#[tauri::command]
pub(crate) fn get_app_settings(service: tauri::State<'_, Arc<AppSettingsService>>) -> AppSettings {
    service.get()
}

#[tauri::command]
pub(crate) async fn set_app_settings(
    service: tauri::State<'_, Arc<AppSettingsService>>,
    settings: AppSettings,
) -> Result<AppSettings, AppSettingsCommandError> {
    let service = Arc::clone(service.inner());
    tauri::async_runtime::spawn_blocking(move || service.set(settings))
        .await
        .map_err(|_| AppSettingsCommandError::from(AppSettingsError::Io))?
        .map_err(Into::into)
}
