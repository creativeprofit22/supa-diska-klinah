fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(
                tauri_build::AppManifest::new()
                    .commands(&["foundation_status", "create_system_restore_point"]),
            )
            .windows_attributes(
                tauri_build::WindowsAttributes::new()
                    .app_manifest(include_str!("windows-app-manifest.xml")),
            ),
    )
    .expect("failed to prepare the Tauri application");
}
