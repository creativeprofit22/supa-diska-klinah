mod commands;
mod navigation;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    windows_platform::privilege::require_standard_user()?;

    let mut context = tauri::generate_context!();
    if std::env::var_os("SUPA_DISKA_KLINAH_SMOKE_MINIMIZED").is_some() {
        for window in &mut context.config_mut().app.windows {
            window.visible = false;
        }
    }

    tauri::Builder::default()
        .plugin(navigation::plugin())
        .invoke_handler(tauri::generate_handler![
            commands::foundation::foundation_status,
            commands::security::create_system_restore_point
        ])
        .run(context)?;
    Ok(())
}
