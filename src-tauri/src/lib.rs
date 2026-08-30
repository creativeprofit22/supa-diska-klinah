mod commands;
mod navigation;

use tauri::Manager;
use windows_platform::{StartupWindowMode, startup_window_mode};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    windows_platform::privilege::require_standard_user()?;

    let background_start = startup_window_mode() == StartupWindowMode::Background;
    let context = tauri::generate_context!();

    tauri::Builder::default()
        .setup(move |app| {
            if !background_start {
                let window = app.get_webview_window("main").ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::NotFound, "main window unavailable")
                })?;
                window.show()?;
                window.set_focus()?;
            }
            Ok(())
        })
        .plugin(navigation::plugin())
        .invoke_handler(tauri::generate_handler![
            commands::cleanup::preview_cleanup,
            commands::foundation::foundation_status,
            commands::security::create_system_restore_point
        ])
        .run(context)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn main_window_is_created_hidden_until_startup_policy_runs() {
        let context: tauri::Context<tauri::Wry> = tauri::generate_context!();
        assert!(
            context
                .config()
                .app
                .windows
                .iter()
                .all(|window| !window.visible && !window.focus)
        );
    }
}
