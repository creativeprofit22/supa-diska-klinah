// Compile the real private adapter at the integration boundary, without adding
// test-only exports to the app. This target receives the Windows app manifest.
mod adapter {
    include!("../src/commands/drives.rs");

    #[cfg(test)]
    mod tests {
        use super::*;
        use tauri::test::{INVOKE_KEY, get_ipc_response, mock_builder};

        fn invoke(
            state: Arc<DriveInventoryState>,
            label: &str,
            url: &str,
        ) -> Result<tauri::ipc::InvokeResponseBody, String> {
            let app = mock_builder()
                .manage(state)
                .invoke_handler(tauri::generate_handler![list_drive_inventory])
                .build(tauri::generate_context!())
                .unwrap();
            let window = tauri::WebviewWindowBuilder::new(&app, label, Default::default())
                .build()
                .unwrap();
            get_ipc_response(
                &window,
                tauri::webview::InvokeRequest {
                    cmd: "list_drive_inventory".into(),
                    callback: tauri::ipc::CallbackFn(0),
                    error: tauri::ipc::CallbackFn(1),
                    url: url.parse().unwrap(),
                    body: Default::default(),
                    headers: Default::default(),
                    invoke_key: INVOKE_KEY.into(),
                },
            )
            .map_err(|error| error.to_string())
        }
        #[test]
        fn ipc_native_inventory_executes_and_releases_gate() {
            let state = Arc::new(DriveInventoryState::default());
            let response = invoke(state.clone(), "main", "http://tauri.localhost").unwrap();
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct Response {
                drives: Vec<DriveSummary>,
                partial: bool,
                warnings: Vec<Warning>,
            }
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Warning {
                drive: Option<String>,
                code: String,
            }
            let response: Response = response.deserialize().unwrap();
            assert!(response.drives.len() <= DRIVE_LIMIT);
            for drive in response.drives {
                assert_eq!(
                    drive.total_bytes.checked_sub(drive.free_bytes),
                    Some(drive.used_bytes)
                );
            }
            for warning in response.warnings {
                assert!(response.partial);
                assert!(matches!(
                    warning.code.as_str(),
                    "drive_unavailable" | "inventory_partial"
                ));
                assert!(warning.drive.is_none_or(|d| d.len() == 3));
            }
            assert!(!state.busy.load(Ordering::Acquire));
        }
        #[test]
        fn empty_inventory_is_success_and_releases_snapshot() {
            let service = StorageService::new();
            let id = service
                .start_with(
                    StorageModule::Drives,
                    None,
                    StorageLimits::default(),
                    None,
                    |_| Ok(()),
                )
                .unwrap();
            let result = collect(&service, id.clone(), Instant::now() + TIMEOUT).unwrap();
            assert!(result.drives.is_empty());
            assert!(!result.partial);
            assert!(result.warnings.is_empty());
            assert!(service.status(&id).is_err());
        }
        #[test]
        fn unreadable_drives_are_partial_not_empty_success() {
            let service = StorageService::new();
            let id = service
                .start_with(
                    StorageModule::Drives,
                    None,
                    StorageLimits::default(),
                    None,
                    |ctx| {
                        ctx.mark_partial(windows_platform::storage::PartialReason::Unreadable);
                        Ok(())
                    },
                )
                .unwrap();
            let result = collect(&service, id.clone(), Instant::now() + TIMEOUT).unwrap();
            assert!(result.drives.is_empty());
            assert!(result.partial);
            assert_eq!(result.warnings.len(), 1);
            assert_eq!(result.warnings[0].code, "inventory_partial");
            assert_eq!(result.warnings[0].drive, None);
            // Per-drive sanitization is tested at its own boundary; the service
            // intentionally does not expose mutable drive diagnostics publicly.
            let warning = warning(DriveIssue {
                mount: Some("p:\\".into()),
                error: "secret native error".into(),
            });
            assert_eq!(warning.drive.as_deref(), Some("P:\\"));
            assert_eq!(warning.code, "drive_unavailable");
            assert!(service.status(&id).is_err());
        }
        #[test]
        fn unknown_system_classification_reaches_consumer_with_capacity_and_partial_status() {
            let service = StorageService::new();
            let id = service
                .start_with(
                    StorageModule::Drives,
                    None,
                    StorageLimits::default(),
                    None,
                    |ctx| {
                        ctx.mark_partial(windows_platform::storage::PartialReason::Unreadable);
                        ctx.push(
                            StorageRecord::Drive(DriveSummary {
                                drive_id: "a".repeat(32),
                                label: "Readable".into(),
                                filesystem: "NTFS".into(),
                                total_bytes: 100,
                                free_bytes: 40,
                                used_bytes: 60,
                                system: None,
                            }),
                            windows_platform::storage::scans::RecordOrder {
                                numeric: 0,
                                text: "Readable".into(),
                            },
                        )?;
                        Ok(())
                    },
                )
                .unwrap();
            let result = collect(&service, id, Instant::now() + TIMEOUT).unwrap();
            assert_eq!(result.drives.len(), 1);
            assert_eq!(result.drives[0].total_bytes, 100);
            assert_eq!(result.drives[0].free_bytes, 40);
            assert_eq!(result.drives[0].used_bytes, 60);
            assert_eq!(result.drives[0].system, None);
            let tauri::ipc::InvokeResponseBody::Json(json) =
                tauri::ipc::IpcResponse::body(&result).unwrap()
            else {
                panic!()
            };
            assert!(
                json.contains(r#""totalBytes":100,"freeBytes":40,"usedBytes":60,"system":null"#)
            );
            assert!(result.partial);
            assert_eq!(result.warnings.len(), 1);
            assert_eq!(result.warnings[0].code, "inventory_partial");
            let warning = warning(DriveIssue {
                mount: None,
                error: "secret native error".into(),
            });
            let tauri::ipc::InvokeResponseBody::Json(json) =
                tauri::ipc::IpcResponse::body(warning).unwrap()
            else {
                panic!()
            };
            assert_eq!(json, r#"{"drive":null,"code":"inventory_partial"}"#);
        }

        #[test]
        fn ipc_busy_is_stable_and_does_not_clear_existing_permit() {
            let state = Arc::new(DriveInventoryState::default());
            state.busy.store(true, Ordering::Release);
            assert_eq!(
                invoke(state.clone(), "main", "http://tauri.localhost").unwrap_err(),
                r#"{"code":"busy"}"#
            );
            assert!(state.busy.load(Ordering::Acquire));
        }
        #[test]
        fn ipc_acl_denies_other_webviews_and_remote_origins() {
            for (label, url) in [
                ("untrusted", "http://tauri.localhost"),
                ("main", "https://example.com"),
            ] {
                let state = Arc::new(DriveInventoryState::default());
                let error = invoke(state.clone(), label, url).unwrap_err();
                assert!(error.contains("not allowed"), "{error}");
                assert!(!state.busy.load(Ordering::Acquire));
            }
        }
        #[test]
        fn timeout_releases_snapshot_and_cancels_worker() {
            let service = StorageService::new();
            let id = service
                .start_with(
                    StorageModule::Drives,
                    None,
                    StorageLimits::default(),
                    None,
                    |ctx| {
                        while !ctx.cancellation.is_cancelled() {
                            std::thread::yield_now();
                        }
                        Ok(())
                    },
                )
                .unwrap();
            assert_eq!(
                collect(&service, id.clone(), Instant::now()).unwrap_err(),
                InventoryError::Timeout
            );
            assert!(service.status(&id).is_err());
        }
        #[test]
        fn failed_worker_releases_snapshot_and_sanitizes_native_error() {
            let service = StorageService::new();
            let id = service
                .start_with(
                    StorageModule::Drives,
                    None,
                    StorageLimits::default(),
                    None,
                    |_| Err(JobError::Native("secret C:\\private native failure".into())),
                )
                .unwrap();
            assert_eq!(
                collect(&service, id.clone(), Instant::now() + TIMEOUT).unwrap_err(),
                InventoryError::InventoryUnavailable
            );
            assert!(service.status(&id).is_err());
            let warning = warning(DriveIssue {
                mount: Some("C:\\private".into()),
                error: "secret".into(),
            });
            assert_eq!(warning.drive, None);
            assert_eq!(warning.code, "drive_unavailable");
        }
    }
}
