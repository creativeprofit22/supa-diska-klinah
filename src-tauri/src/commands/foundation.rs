#[tauri::command]
pub(crate) fn foundation_status() -> windows_platform::FoundationStatus {
    windows_platform::foundation_status()
}
