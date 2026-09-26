mod commands;
mod navigation;

use std::{sync::Arc, time::Duration};
use tauri::Manager;
use windows_platform::{StartupWindowMode, cleanup::CleanupService, startup_window_mode};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    windows_platform::privilege::require_standard_user()?;

    let scheduled_scan = windows_platform::scheduler::parse_scheduled_scan_args(
        &std::env::args_os().skip(1).collect::<Vec<_>>(),
    );
    let context = tauri::generate_context!();

    if let Some(schedule_id) = scheduled_scan.as_ref() {
        // Headless, read-only scan launched by the app's own task. The Tauri app
        // is never built here: building it creates the main webview (which
        // loads the frontend against unmanaged state) and briefly shows a
        // window before `exit` takes effect.
        let app_data = scheduled_scan_app_data(&context.config().identifier)?;
        windows_platform::scheduler::run_scheduled_scan(schedule_id, &app_data)?;
        return Ok(());
    }
    let background_start = startup_window_mode() == StartupWindowMode::Background;

    tauri::Builder::default()
        .setup(move |app| {
            let app_data = app.path().app_data_dir()?;
            app.manage(Arc::new(
                windows_platform::system_change::SystemChangeService::windows(&app_data)
                    .map_err(|_| std::io::Error::other("system change journal unavailable"))?,
            ));
            let cleanup_service = Arc::new(
                CleanupService::new(app_data.clone())
                    .map_err(|_| std::io::Error::other("cleanup service initialization failed"))?,
            );
            app.manage(Arc::clone(&cleanup_service));
            app.manage(Arc::new(commands::drives::DriveInventoryState::default()));
            app.manage(Arc::new(
                windows_platform::storage::scans::StorageService::new(),
            ));
            app.manage(Arc::new(
                windows_platform::storage::root_picker::ScopeService::default(),
            ));
            // Opening the protection service runs rule-store and quarantine recovery.
            app.manage(Arc::new(
                windows_platform::protection::service::ProtectionService::new(&app_data)
                    .map_err(|_| std::io::Error::other("protection storage unavailable"))?,
            ));
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
            commands::cleanup::get_scan_settings,
            commands::cleanup::set_scan_settings,
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
            commands::drives::list_drive_inventory,
            commands::disk_analyzer::start_disk_analyzer,
            commands::large_files::start_large_files,
            commands::duplicates::start_duplicates,
            commands::empty_folders::start_empty_folders,
            commands::cleaner::list_cleaner_catalog,
            commands::cleaner::start_cleaner,
            commands::browser::list_browser_policy,
            commands::browser::start_browser_scan,
            commands::uninstaller::start_program_inventory,
            commands::uninstaller::prepare_vendor_job,
            commands::uninstaller::confirm_vendor_job,
            commands::uninstaller::vendor_job_status,
            commands::uninstaller::cancel_vendor_job,
            commands::uninstaller::release_vendor_job,
            commands::uninstaller::vendor_job_history,
            commands::storage::choose_storage_root,
            commands::storage::list_storage_scopes,
            commands::storage::authorize_storage_scope,
            commands::storage::storage_scan_status,
            commands::storage::storage_scan_page,
            commands::storage::cancel_storage_scan,
            commands::storage::release_storage_scan,
            commands::storage::create_storage_plan,
            commands::foundation::foundation_status,
            commands::security::create_system_restore_point,
            commands::system_change::preview_system_change,
            commands::system_change::create_system_change_plan,
            commands::system_change::confirm_system_change_plan,
            commands::system_change::execute_system_change_plan,
            commands::system_change::system_change_journal,
            commands::system_change::create_system_rollback_plan,
            commands::startup::list_startup_items,
            commands::services::list_services,
            commands::drivers::list_driver_packages,
            commands::firewall::get_firewall_status,
            commands::hosts::get_hosts_report,
            commands::privacy::get_privacy_report,
            commands::power::get_power_status,
            commands::restore::list_restore_points,
            commands::restore::get_restore_protection,
            commands::updates::get_windows_update_status,
            commands::updates::detect_windows_updates,
            commands::scheduler::list_scan_schedules,
            commands::scheduler::list_scheduled_scan_summaries,
            commands::optimizer::get_optimizer_proposals,
            commands::protection::protection_overview,
            commands::protection::set_protection_network_policy,
            commands::protection::list_running_programs,
            commands::protection::start_protection_scan,
            commands::protection::cancel_protection_scan,
            commands::protection::protection_scan_status,
            commands::protection::last_protection_scan,
            commands::protection::quarantine_protection_finding,
            commands::protection::list_quarantine,
            commands::protection::restore_quarantined,
            commands::protection::delete_quarantined,
            commands::protection::allow_protection_finding,
            commands::protection::clear_protection_allowlist,
            commands::protection::import_rule_pack,
            commands::protection::restore_previous_rule_pack,
            commands::protection::download_rule_pack,
            commands::protection::check_password_breach,
            commands::protection::get_defender_history
        ])
        .run(context)?;
    Ok(())
}

/// The same directory Tauri's `app_data_dir()` resolves on Windows
/// (roaming AppData joined with the bundle identifier), without building an app.
fn scheduled_scan_app_data(identifier: &str) -> std::io::Result<std::path::PathBuf> {
    let roaming = std::env::var_os("APPDATA")
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| std::io::Error::other("APPDATA is not an absolute path"))?;
    Ok(roaming.join(identifier))
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
