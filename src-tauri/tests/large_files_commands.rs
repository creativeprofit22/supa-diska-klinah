mod commands {
    pub mod storage {
        include!("../src/commands/storage.rs");
    }
    pub mod large_files {
        include!("../src/commands/large_files.rs");
    }
    pub mod cleanup {
        include!("../src/commands/cleanup.rs");
    }
}
use commands::{cleanup::*, large_files::*, storage::*};
use serde::Deserialize;
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use tauri::test::{INVOKE_KEY, get_ipc_response, mock_builder};
use windows_platform::{
    cleanup::CleanupService,
    storage::{
        self, CandidateEligibility, FileRecord, PageCollection, PageRequest, StorageModule,
        StoragePhase, root_picker::ScopeService, scans::StorageService,
    },
};
struct Fixture {
    path: PathBuf,
    storage: Arc<StorageService>,
    cleanup: Arc<CleanupService>,
}
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "large-files-ipc-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(path.join("scan/nested")).unwrap();
        std::fs::write(path.join("scan/large.txt"), b"123456789").unwrap();
        std::fs::write(path.join("scan/keep.txt"), b"keep").unwrap();
        std::fs::write(path.join("scan/nested/other.bin"), b"nested").unwrap();
        let cleanup = Arc::new(CleanupService::new(path.join("state")).unwrap());
        Self {
            path,
            storage: Arc::new(StorageService::new()),
            cleanup,
        }
    }
    fn root(&self, module: StorageModule) -> String {
        self.storage
            .authorize_root_for(
                &self.path.join("scan"),
                module,
                &storage::current_protection().unwrap(),
            )
            .unwrap()
    }
    fn invoke_at(
        &self,
        cmd: &str,
        input: &str,
        raw: bool,
        label: &str,
        url: &str,
    ) -> Result<tauri::ipc::InvokeResponseBody, String> {
        let app = mock_builder()
            .manage(self.storage.clone())
            .manage(self.cleanup.clone())
            .manage(Arc::new(ScopeService::default()))
            .invoke_handler(tauri::generate_handler![
                start_large_files,
                choose_storage_root,
                list_storage_scopes,
                authorize_storage_scope,
                storage_scan_status,
                storage_scan_page,
                cancel_storage_scan,
                release_storage_scan,
                create_storage_plan,
                preview_cleanup,
                list_project_roots,
                add_project_root,
                set_project_root_paused,
                remove_project_root,
                discover_project_artifacts,
                create_cleanup_plan,
                execute_cleanup_plan,
                execute_permanent_cleanup_plan,
                undo_cleanup,
                cleanup_history,
                get_auto_cleanup_policy,
                set_auto_cleanup_policy
            ])
            .build(tauri::generate_context!())
            .unwrap();
        let window = tauri::WebviewWindowBuilder::new(&app, label, Default::default())
            .build()
            .unwrap();
        get_ipc_response(
            &window,
            tauri::webview::InvokeRequest {
                cmd: cmd.into(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: url.parse().unwrap(),
                body: if raw {
                    tauri::ipc::InvokeBody::Raw(input.as_bytes().to_vec())
                } else {
                    tauri::ipc::InvokeBody::Json(input.parse().unwrap())
                },
                headers: Default::default(),
                invoke_key: INVOKE_KEY.into(),
            },
        )
        .map_err(|e| e.to_string())
    }
    fn invoke(&self, cmd: &str, input: &str) -> Result<tauri::ipc::InvokeResponseBody, String> {
        self.invoke_at(cmd, input, true, "main", "http://tauri.localhost")
    }
    fn input(&self, root: &str) -> String {
        format!(
            r#"{{"rootId":"{root}","depth":64,"filter":{{"minimumBytes":0,"maximumBytes":null,"extensions":[],"category":"any","sort":"size","descending":true}}}}"#
        )
    }
    fn scan(&self) -> String {
        let root = self.root(StorageModule::LargeFiles);
        let id: String = self
            .invoke("start_large_files", &self.input(&root))
            .unwrap()
            .deserialize()
            .unwrap();
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            let (status, error) = self.storage.status(&id).unwrap();
            assert!(error.is_none(), "{error:?}");
            if status.phase == StoragePhase::Complete
                && self
                    .storage
                    .page(&PageRequest {
                        snapshot_id: id.clone(),
                        module: StorageModule::LargeFiles,
                        collection: PageCollection::Files,
                        parent_id: None,
                        cursor: None,
                        page_size: 1,
                    })
                    .is_ok()
            {
                break;
            }
            assert!(Instant::now() < end);
            std::thread::yield_now();
        }
        id
    }
    fn page(&self, id: &str, cursor: Option<&str>) -> Page {
        let extra = cursor
            .map(|c| format!(r#", "cursor":"{c}""#))
            .unwrap_or_default();
        self.invoke("storage_scan_page",&format!(r#"{{"module":"largeFiles","snapshotId":"{id}","collection":"files","pageSize":1{extra}}}"#)).unwrap().deserialize().unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
#[derive(Deserialize)]
#[serde(tag = "kind", content = "record", rename_all = "camelCase")]
enum Row {
    File(FileRecord),
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Page {
    records: Vec<Row>,
    next_cursor: Option<String>,
    retained_total: usize,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Plan {
    plan_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Execution {
    execution_id: String,
    completed: bool,
    items: Vec<Outcome>,
}
#[derive(Deserialize)]
struct Outcome {
    state: String,
    failure: Option<String>,
}

#[test]
#[ignore = "requires STORAGE_TEST_SECOND_VOLUME pointing to an existing disposable-fixture parent on another volume"]
fn cross_volume_recovery_preflight() {
    use windows_platform::cleanup::WindowsFileSystem;
    let mut f = Fixture::new();
    let parent = PathBuf::from(
        std::env::var_os("STORAGE_TEST_SECOND_VOLUME").expect("second-volume fixture parent"),
    );
    assert!(parent.is_absolute() && parent.is_dir());
    let state = parent.join(f.path.file_name().unwrap());
    std::fs::create_dir(&state).unwrap();
    struct StateDirectory(PathBuf);
    impl Drop for StateDirectory {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
    let state = StateDirectory(state);
    assert!(!WindowsFileSystem.same_volume(&f.path, &state.0).unwrap());
    let id = f.scan();
    let page = f.page(&id, None);
    let Row::File(file) = &page.records[0];
    let CandidateEligibility::Eligible { candidate_id } = &file.eligibility else {
        panic!("eligible fixture")
    };
    let input = format!(
        r#"{{"selection":{{"module":"largeFiles","snapshotId":"{id}","candidateIds":["{candidate_id}"]}},"disposition":"quarantine"}}"#
    );
    let plan: Plan = f
        .invoke("create_storage_plan", &input)
        .unwrap()
        .deserialize()
        .unwrap();
    f.cleanup = Arc::new(CleanupService::new(state.0.clone()).unwrap());
    let error = f.invoke("create_storage_plan", &input).unwrap_err();
    assert_eq!(error, r#"{"code":"recovery_volume_unsupported"}"#);
    assert_eq!(
        std::fs::read_dir(state.0.join("cleanup/plans"))
            .unwrap()
            .count(),
        0
    );
    // A previously accepted immutable plan must still be refused after store relocation.
    let name = format!("{}.json", plan.plan_id);
    std::fs::copy(
        f.path.join("state/cleanup/plans").join(&name),
        state.0.join("cleanup/plans").join(name),
    )
    .unwrap();
    assert_eq!(
        f.cleanup.execute(&plan.plan_id).unwrap_err(),
        windows_platform::cleanup::CleanupServiceError::RecoveryVolumeUnsupported
    );
    let permanent: Plan = f
        .invoke(
            "create_storage_plan",
            &input.replace("quarantine", "permanent"),
        )
        .unwrap()
        .deserialize()
        .unwrap();
    assert_ne!(permanent.plan_id, plan.plan_id);
    // The normal execution command cannot bypass separate permanent confirmation.
    assert!(f.cleanup.execute(&permanent.plan_id).is_err());
    assert_eq!(
        std::fs::read(f.path.join("scan/large.txt")).unwrap(),
        b"123456789"
    );
}

#[test]
fn large_file_real_ipc_pages_immutable_recovery_plan_execute_and_undo() {
    let f = Fixture::new();
    let id = f.scan();
    let first = f.page(&id, None);
    assert_eq!(first.retained_total, 3);
    let Row::File(file) = &first.records[0];
    assert_eq!(file.logical_bytes, 9);
    let CandidateEligibility::Eligible { candidate_id } = &file.eligibility else {
        panic!("eligible fixture")
    };
    let second = f.page(&id, first.next_cursor.as_deref());
    let Row::File(file2) = &second.records[0];
    assert_eq!(file2.logical_bytes, 6);
    let input = format!(
        r#"{{"selection":{{"module":"largeFiles","snapshotId":"{id}","candidateIds":["{candidate_id}"]}},"disposition":"quarantine"}}"#
    );
    let response = f.invoke("create_storage_plan", &input).unwrap();
    let plan: Plan = response.clone().deserialize().unwrap();
    // Export actual JSON responses to Raw IPC before disposable-fixture execution.
    if let Ok(destination) = std::env::var("LARGE_FILES_NATIVE_EXPORT") {
        fn json(response: tauri::ipc::InvokeResponseBody) -> String {
            match response {
                tauri::ipc::InvokeResponseBody::Json(value) => value,
                _ => panic!("expected JSON response"),
            }
        }
        let plan_json = json(response);
        let status = json(
            f.invoke(
                "storage_scan_status",
                &format!(r#"{{"module":"largeFiles","snapshotId":"{id}"}}"#),
            )
            .unwrap(),
        );
        let mut pages = Vec::new();
        let mut cursor = None::<String>;
        loop {
            let extra = cursor
                .as_ref()
                .map(|c| format!(r#", "cursor":"{c}""#))
                .unwrap_or_default();
            let response = f.invoke("storage_scan_page", &format!(r#"{{"module":"largeFiles","snapshotId":"{id}","collection":"files","pageSize":1{extra}}}"#)).unwrap();
            let page: Page = response.clone().deserialize().unwrap();
            cursor = page.next_cursor;
            pages.push(json(response));
            if cursor.is_none() {
                break;
            }
        }
        let destination = PathBuf::from(destination);
        std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
        std::fs::write(destination, format!(r#"{{"status":{status},"pages":[{}],"quarantinePlan":{plan_json},"replay":"Actual Raw IPC; pageSize 1 replayed below UI requested 100; exported before execution"}}"#, pages.join(","))).unwrap();
    }
    assert_eq!(
        std::fs::read(f.path.join("scan/large.txt")).unwrap(),
        b"123456789"
    );
    let execution: Execution = f
        .invoke_at(
            "execute_cleanup_plan",
            &format!(r#"{{"planId":"{}"}}"#, plan.plan_id),
            false,
            "main",
            "http://tauri.localhost",
        )
        .unwrap()
        .deserialize()
        .unwrap();
    assert!(execution.completed);
    assert_eq!(execution.items[0].state, "quarantined");
    assert!(execution.items[0].failure.is_none());
    assert!(!f.path.join("scan/large.txt").exists());
    assert_eq!(
        std::fs::read(f.path.join("scan/keep.txt")).unwrap(),
        b"keep"
    );
    f.invoke_at(
        "undo_cleanup",
        &format!(r#"{{"executionId":"{}"}}"#, execution.execution_id),
        false,
        "main",
        "http://tauri.localhost",
    )
    .unwrap();
    assert_eq!(
        std::fs::read(f.path.join("scan/large.txt")).unwrap(),
        b"123456789"
    );
    f.invoke(
        "release_storage_scan",
        &format!(r#"{{"module":"largeFiles","snapshotId":"{id}"}}"#),
    )
    .unwrap();
    assert!(f.invoke("create_storage_plan", &input).is_err());
}
#[test]
fn storage_recycle_never_falls_back_to_path_deletion() {
    let f = Fixture::new();
    let id = f.scan();
    let page = f.page(&id, None);
    let Row::File(file) = &page.records[0];
    let CandidateEligibility::Eligible { candidate_id } = &file.eligibility else {
        panic!("eligible")
    };
    let plan: Plan = f.invoke("create_storage_plan", &format!(r#"{{"selection":{{"module":"largeFiles","snapshotId":"{id}","candidateIds":["{candidate_id}"]}},"disposition":"recycleBin"}}"#)).unwrap().deserialize().unwrap();
    let result: Execution = f
        .invoke_at(
            "execute_cleanup_plan",
            &format!(r#"{{"planId":"{}"}}"#, plan.plan_id),
            false,
            "main",
            "http://tauri.localhost",
        )
        .unwrap()
        .deserialize()
        .unwrap();
    assert_eq!(result.items[0].state, "failed");
    assert_eq!(
        result.items[0].failure.as_deref(),
        Some("identity-required-recycle-unsupported")
    );
    assert_eq!(
        std::fs::read(f.path.join("scan/large.txt")).unwrap(),
        b"123456789"
    );
}
#[test]
fn permanent_command_rejects_unknown_plan_and_foreign_window_before_confirmation() {
    let f = Fixture::new();
    let input = format!(r#"{{"planId":"{}","owner":1}}"#, "a".repeat(32));
    assert!(
        f.invoke_at(
            "execute_permanent_cleanup_plan",
            &input,
            false,
            "main",
            "http://tauri.localhost"
        )
        .is_err()
    );
    assert!(
        f.invoke_at(
            "execute_permanent_cleanup_plan",
            &input,
            false,
            "foreign",
            "http://tauri.localhost"
        )
        .unwrap_err()
        .contains("not allowed")
    );
    assert_eq!(
        std::fs::read(f.path.join("scan/large.txt")).unwrap(),
        b"123456789"
    );
}
#[test]
fn large_file_start_rejects_bad_filters_paths_cross_module_and_foreign_callers() {
    let f = Fixture::new();
    let input = f.input(&"a".repeat(32));
    for bad in [
        input.replace("\"depth\":64", "\"depth\":65"),
        input.replace("\"extensions\":[]", "\"extensions\":[\"../txt\"]"),
        input.replace("\"category\":\"any\"", "\"category\":\"unknown\""),
        input.replace("\"minimumBytes\":0", "\"minimumBytes\":-1"),
        input.replace("\"depth\":64", "\"depth\":64,\"path\":\"C:\\\\Windows\""),
        input
            .replace("\"maximumBytes\":null", "\"maximumBytes\":0")
            .replace("\"minimumBytes\":0", "\"minimumBytes\":9"),
    ] {
        assert_eq!(
            f.invoke("start_large_files", &bad).unwrap_err(),
            r#"{"code":"invalid_input"}"#
        );
    }
    let wrong = f.root(StorageModule::DiskAnalyzer);
    assert!(f.invoke("start_large_files", &f.input(&wrong)).is_err());
    for (label, url) in [
        ("foreign", "http://tauri.localhost"),
        ("main", "https://example.com"),
    ] {
        assert!(
            f.invoke_at("start_large_files", &input, true, label, url)
                .unwrap_err()
                .contains("not allowed")
        );
    }
}
