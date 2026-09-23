// Exercise the actual private command adapters with the app's generated ACL.
mod adapter {
    include!("../src/commands/storage.rs");
    #[cfg(test)]
    mod tests {
        use super::*;
        use std::{
            path::PathBuf,
            time::{Duration, Instant},
        };
        use tauri::test::{INVOKE_KEY, get_ipc_response, mock_builder};
        use windows_platform::storage::{
            PageCollection, StorageLimits, StoragePhase, StorageRecord,
        };

        struct Fixture {
            path: PathBuf,
            storage: Arc<StorageService>,
            cleanup: Arc<CleanupService>,
            scopes: Arc<ScopeService>,
        }
        impl Fixture {
            fn new() -> Self {
                let path = std::env::temp_dir().join(format!(
                    "storage-ipc-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                ));
                std::fs::create_dir_all(path.join("scan")).unwrap();
                let cleanup = Arc::new(CleanupService::new(path.join("state")).unwrap());
                Self {
                    path,
                    cleanup,
                    storage: Arc::new(StorageService::new()),
                    scopes: Arc::new(ScopeService::default()),
                }
            }
            fn invoke_at(
                &self,
                command: &str,
                bytes: Vec<u8>,
                label: &str,
                url: &str,
            ) -> Result<tauri::ipc::InvokeResponseBody, String> {
                let app = mock_builder()
                    .manage(self.storage.clone())
                    .manage(self.cleanup.clone())
                    .manage(self.scopes.clone())
                    .invoke_handler(tauri::generate_handler![
                        choose_storage_root,
                        list_storage_scopes,
                        authorize_storage_scope,
                        storage_scan_status,
                        storage_scan_page,
                        cancel_storage_scan,
                        release_storage_scan,
                        create_storage_plan
                    ])
                    .build(tauri::generate_context!())
                    .unwrap();
                let window = tauri::WebviewWindowBuilder::new(&app, label, Default::default())
                    .build()
                    .unwrap();
                get_ipc_response(
                    &window,
                    tauri::webview::InvokeRequest {
                        cmd: command.into(),
                        callback: tauri::ipc::CallbackFn(0),
                        error: tauri::ipc::CallbackFn(1),
                        url: url.parse().unwrap(),
                        body: tauri::ipc::InvokeBody::Raw(bytes),
                        headers: Default::default(),
                        invoke_key: INVOKE_KEY.into(),
                    },
                )
                .map_err(|e| e.to_string())
            }
            fn invoke(
                &self,
                command: &str,
                json: &str,
            ) -> Result<tauri::ipc::InvokeResponseBody, String> {
                self.invoke_at(
                    command,
                    json.as_bytes().to_vec(),
                    "main",
                    "http://tauri.localhost",
                )
            }
            fn scan(&self, cancel: bool) -> String {
                let path = self.path.join("scan");
                std::fs::File::create(path.join("personal.bin"))
                    .unwrap()
                    .set_len(200 * 1024 * 1024)
                    .unwrap();
                let protection = storage::current_protection().unwrap();
                let root = self
                    .storage
                    .authorize_root_for(&path, StorageModule::LargeFiles, &protection)
                    .unwrap();
                let id = self
                    .storage
                    .start_with(
                        StorageModule::LargeFiles,
                        Some(&root),
                        StorageLimits::default(),
                        Some(protection.clone()),
                        move |ctx| {
                            storage::large_files::discover(ctx, &protection, Default::default())?;
                            if cancel {
                                while !ctx.cancellation.is_cancelled() {
                                    std::thread::yield_now();
                                }
                            }
                            Ok(())
                        },
                    )
                    .unwrap();
                if !cancel {
                    wait(&self.storage, &id);
                }
                id
            }
        }
        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.path);
            }
        }
        fn wait(service: &StorageService, id: &str) -> StorageStatus {
            let deadline = Instant::now() + Duration::from_secs(15);
            loop {
                let (status, error) = service.status(id).unwrap();
                assert!(error.is_none(), "{error:?}");
                if matches!(
                    status.phase,
                    StoragePhase::Complete | StoragePhase::Cancelled
                ) {
                    return status;
                }
                assert!(Instant::now() < deadline, "scan timed out");
                std::thread::yield_now();
            }
        }
        fn snapshot(id: &str, module: &str) -> String {
            format!(r#"{{"snapshotId":"{id}","module":"{module}"}}"#)
        }
        fn page(id: &str, extra: &str) -> String {
            format!(
                r#"{{"snapshotId":"{id}","module":"largeFiles","collection":"files","pageSize":1{extra}}}"#
            )
        }
        fn selection(id: &str, candidate: &str) -> String {
            format!(
                r#"{{"selection":{{"snapshotId":"{id}","module":"largeFiles","candidateIds":["{candidate}"]}},"disposition":"recycleBin"}}"#
            )
        }
        #[test]
        fn ipc_all_storage_commands_deny_foreign_windows_and_origins() {
            let f = Fixture::new();
            for command in [
                "choose_storage_root",
                "list_storage_scopes",
                "authorize_storage_scope",
                "storage_scan_status",
                "storage_scan_page",
                "cancel_storage_scan",
                "release_storage_scan",
                "create_storage_plan",
            ] {
                for (label, url) in [
                    ("foreign", "http://tauri.localhost"),
                    ("main", "https://example.com"),
                ] {
                    let error = f
                        .invoke_at(command, b"{}".to_vec(), label, url)
                        .unwrap_err();
                    assert!(error.contains("not allowed"), "{command}: {error}");
                }
            }
        }
        #[test]
        fn ipc_rejects_unknown_fields_arbitrary_authority_and_oversized_payloads() {
            let f = Fixture::new();
            for (command, json) in [
                (
                    "choose_storage_root",
                    r#"{"module":"largeFiles","path":"C:\\Windows"}"#,
                ),
                ("choose_storage_root", r#"{"module":"browser"}"#),
                (
                    "choose_storage_root",
                    r#"{"module":"largeFiles","owner":123}"#,
                ),
                (
                    "choose_storage_root",
                    r#"{"module":"largeFiles","limits":{"workers":999}}"#,
                ),
                (
                    "list_storage_scopes",
                    r#"{"module":"cleaner","command":"anything"}"#,
                ),
                (
                    "authorize_storage_scope",
                    r#"{"module":"cleaner","scopeId":"C:\\Windows"}"#,
                ),
                (
                    "storage_scan_status",
                    r#"{"module":"largeFiles","snapshotId":"bad"}"#,
                ),
                (
                    "storage_scan_page",
                    r#"{"snapshotId":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","module":"largeFiles","collection":"tree"}"#,
                ),
                (
                    "storage_scan_page",
                    r#"{"snapshotId":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","module":"largeFiles","collection":"files","pageSize":201}"#,
                ),
                (
                    "create_storage_plan",
                    r#"{"selection":{"snapshotId":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","module":"largeFiles","candidateIds":[],"proof":{}},"disposition":"recycleBin"}"#,
                ),
            ] {
                assert_eq!(
                    f.invoke(command, json).unwrap_err(),
                    r#"{"code":"invalid_input"}"#,
                    "{command}"
                );
            }
            let error = f
                .invoke_at(
                    "list_storage_scopes",
                    vec![b' '; storage::MAX_REQUEST_BYTES + 1],
                    "main",
                    "http://tauri.localhost",
                )
                .unwrap_err();
            assert_eq!(error, r#"{"code":"limit_reached"}"#);
        }
        #[test]
        fn ipc_native_scopes_are_opaque_module_bound_and_refresh_expires_ids() {
            let f = Fixture::new();
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Scope {
                scope_id: String,
            }
            let scopes: Vec<Scope> = f
                .invoke("list_storage_scopes", r#"{"module":"cleaner"}"#)
                .unwrap()
                .deserialize()
                .unwrap();
            assert!(!scopes.is_empty());
            let id = &scopes[0].scope_id;
            assert!(valid_id(id));
            assert_eq!(
                f.invoke(
                    "authorize_storage_scope",
                    &format!(r#"{{"module":"browser","scopeId":"{id}"}}"#)
                )
                .unwrap_err(),
                r#"{"code":"snapshot_unavailable"}"#
            );
            f.invoke("list_storage_scopes", r#"{"module":"cleaner"}"#)
                .unwrap();
            assert_eq!(
                f.invoke(
                    "authorize_storage_scope",
                    &format!(r#"{{"module":"cleaner","scopeId":"{id}"}}"#)
                )
                .unwrap_err(),
                r#"{"code":"snapshot_unavailable"}"#
            );
        }
        #[test]
        fn ipc_real_scan_page_and_plan_use_retained_ids_then_release_revokes_them() {
            let f = Fixture::new();
            let id = f.scan(false);
            #[derive(Deserialize)]
            struct Status {
                phase: StoragePhase,
            }
            let status: Status = f
                .invoke("storage_scan_status", &snapshot(&id, "largeFiles"))
                .unwrap()
                .deserialize()
                .unwrap();
            assert_eq!(status.phase, StoragePhase::Complete);
            assert_eq!(
                f.invoke("storage_scan_status", &snapshot(&id, "duplicates"))
                    .unwrap_err(),
                r#"{"code":"invalid_evidence"}"#
            );
            f.invoke("storage_scan_page", &page(&id, "")).unwrap();
            assert!(
                f.invoke(
                    "storage_scan_page",
                    &page(&id, r#", "cursor":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb""#)
                )
                .is_err()
            );
            let result = f
                .storage
                .page(&PageRequest {
                    snapshot_id: id.clone(),
                    module: StorageModule::LargeFiles,
                    collection: PageCollection::Files,
                    parent_id: None,
                    cursor: None,
                    page_size: 1,
                })
                .unwrap();
            let StorageRecord::File(file) = &result.records[0] else {
                panic!()
            };
            // Serialize eligibility through the real response, without exposing proof records.
            #[derive(Deserialize)]
            struct Eligibility {
                candidate_id: String,
            }
            let encoded = tauri::ipc::IpcResponse::body(&file.eligibility).unwrap();
            let candidate: Eligibility = encoded.deserialize().unwrap();
            let plan = selection(&id, &candidate.candidate_id);
            f.invoke("create_storage_plan", &plan).unwrap();
            let other = f.scan(false);
            assert!(
                f.invoke(
                    "create_storage_plan",
                    &selection(&other, &candidate.candidate_id)
                )
                .is_err()
            );
            assert!(
                f.invoke(
                    "create_storage_plan",
                    &plan.replace("largeFiles", "cleaner")
                )
                .is_err()
            );
            assert!(
                f.invoke("cancel_storage_scan", &snapshot(&other, "browser"))
                    .is_err()
            );
            assert!(
                f.invoke("release_storage_scan", &snapshot(&other, "browser"))
                    .is_err()
            );
            f.invoke("release_storage_scan", &snapshot(&other, "largeFiles"))
                .unwrap();
            assert!(
                f.path.join("scan/personal.bin").is_file(),
                "plan creation must not execute cleanup"
            );
            f.invoke("release_storage_scan", &snapshot(&id, "largeFiles"))
                .unwrap();
            assert!(f.invoke("storage_scan_page", &page(&id, "")).is_err());
            assert_eq!(
                f.invoke("create_storage_plan", &plan).unwrap_err(),
                r#"{"code":"snapshot_unavailable"}"#
            );
        }
        #[test]
        fn ipc_cancelled_scan_has_no_plan_authority_and_errors_hide_native_paths() {
            let f = Fixture::new();
            let id = f.scan(true);
            f.invoke("cancel_storage_scan", &snapshot(&id, "largeFiles"))
                .unwrap();
            assert_eq!(wait(&f.storage, &id).phase, StoragePhase::Cancelled);
            assert_eq!(
                f.invoke("create_storage_plan", &selection(&id, &"a".repeat(32)))
                    .unwrap_err(),
                r#"{"code":"snapshot_unavailable"}"#
            );
            f.invoke("release_storage_scan", &snapshot(&id, "largeFiles"))
                .unwrap();
            let id = f
                .storage
                .start_with(
                    StorageModule::Drives,
                    None,
                    StorageLimits::default(),
                    None,
                    |_| Err(JobError::Native("C:\\private secret".into())),
                )
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(15);
            while f.storage.status(&id).unwrap().0.phase != StoragePhase::Failed {
                assert!(Instant::now() < deadline);
                std::thread::yield_now();
            }
            let response = f
                .invoke("storage_scan_status", &snapshot(&id, "drives"))
                .unwrap();
            let tauri::ipc::InvokeResponseBody::Json(json) = response else {
                panic!()
            };
            assert!(!json.contains("secret") && !json.contains("private"));
            assert_eq!(
                StorageCommandError::from(JobError::Native("secret".into())).code,
                "storage_unavailable"
            );
        }
        #[test]
        fn ipc_selection_cap_and_read_only_module_cannot_create_plans() {
            let f = Fixture::new();
            let candidates =
                vec![format!("\"{}\"", "a".repeat(32)); storage::MAX_SELECTION + 1].join(",");
            let payload = format!(
                r#"{{"selection":{{"snapshotId":"{}","module":"largeFiles","candidateIds":[{candidates}]}},"disposition":"recycleBin"}}"#,
                "b".repeat(32)
            );
            assert_eq!(
                f.invoke("create_storage_plan", &payload).unwrap_err(),
                r#"{"code":"invalid_input"}"#
            );
            let payload =
                selection(&"a".repeat(32), &"b".repeat(32)).replace("largeFiles", "diskAnalyzer");
            assert!(f.invoke("create_storage_plan", &payload).is_err());
        }
    }
}
