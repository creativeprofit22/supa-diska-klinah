mod commands;
mod navigation;

use std::{sync::Arc, time::Duration};
use tauri::Manager;
use windows_platform::{StartupWindowMode, cleanup::CleanupService, startup_window_mode};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    windows_platform::privilege::require_standard_user()?;

    let background_start = startup_window_mode() == StartupWindowMode::Background;
    let context = tauri::generate_context!();

    tauri::Builder::default()
        .setup(move |app| {
            let app_data = app.path().app_data_dir()?;
            let cleanup_service = Arc::new(
                CleanupService::new(app_data)
                    .map_err(|_| std::io::Error::other("cleanup service initialization failed"))?,
            );
            app.manage(Arc::clone(&cleanup_service));
            let maintenance_service = Arc::clone(&cleanup_service);
            tauri::async_runtime::spawn_blocking(move || {
                let _ = maintenance_service.run_maintenance();
                let _ = maintenance_service.analyze_due_build_artifacts();
            });
            tauri::async_runtime::spawn(async move {
                let start = tokio::time::Instant::now() + Duration::from_secs(3_600);
                let mut hourly = tokio::time::interval_at(start, Duration::from_secs(3_600));
                loop {
                    hourly.tick().await;
                    let service = Arc::clone(&cleanup_service);
                    let _ = tauri::async_runtime::spawn_blocking(move || {
                        service.analyze_due_build_artifacts()
                    })
                    .await;
                }
            });
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
            commands::cleanup::list_project_roots,
            commands::cleanup::add_project_root,
            commands::cleanup::set_project_root_paused,
            commands::cleanup::remove_project_root,
            commands::cleanup::discover_project_artifacts,
            commands::cleanup::create_cleanup_plan,
            commands::cleanup::execute_cleanup_plan,
            commands::cleanup::execute_permanent_cleanup_plan,
            commands::cleanup::undo_cleanup,
            commands::cleanup::cleanup_history,
            commands::cleanup::get_auto_cleanup_policy,
            commands::cleanup::set_auto_cleanup_policy,
            commands::build_artifacts::list_build_profiles,
            commands::build_artifacts::register_build_profile,
            commands::build_artifacts::remove_build_profile,
            commands::build_artifacts::start_build_run,
            commands::build_artifacts::get_build_run,
            commands::build_artifacts::get_active_build_run,
            commands::build_artifacts::cancel_build_run,
            commands::build_artifacts::get_artifact_budget_policy,
            commands::build_artifacts::set_artifact_budget_policy,
            commands::build_artifacts::preview_artifact_budgets,
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
