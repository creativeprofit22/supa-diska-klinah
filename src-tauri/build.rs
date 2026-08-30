fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(&[
                "preview_cleanup",
                "create_cleanup_plan",
                "execute_cleanup_plan",
                "execute_permanent_cleanup_plan",
                "undo_cleanup",
                "cleanup_history",
                "get_auto_cleanup_policy",
                "set_auto_cleanup_policy",
                "foundation_status",
                "create_system_restore_point",
            ]))
            .windows_attributes(
                tauri_build::WindowsAttributes::new()
                    .app_manifest(include_str!("windows-app-manifest.xml")),
            ),
    )
    .expect("failed to prepare the Tauri application");
}
