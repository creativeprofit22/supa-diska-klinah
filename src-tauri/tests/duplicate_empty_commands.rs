mod commands {
    pub mod storage {
        include!("../src/commands/storage.rs");
    }
    pub mod duplicates {
        include!("../src/commands/duplicates.rs");
    }
    pub mod empty_folders {
        include!("../src/commands/empty_folders.rs");
    }
    pub mod cleanup {
        include!("../src/commands/cleanup.rs");
    }
}
use commands::{cleanup::*, duplicates::*, empty_folders::*, storage::*};
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
        self, CandidateEligibility, DuplicateGroup, DuplicateMember, EmptyFolderRecord,
        StorageModule, StoragePhase, root_picker::ScopeService, scans::StorageService,
    },
};
static NEXT_FIXTURE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
struct Fixture {
    path: PathBuf,
    storage: Arc<StorageService>,
    cleanup: Arc<CleanupService>,
}
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "duplicate-empty-ipc-{}-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(path.join("scan/nested")).unwrap();
        std::fs::write(path.join("scan/large.txt"), b"123456789").unwrap();
        std::fs::write(path.join("scan/keep.txt"), b"keep").unwrap();
        std::fs::write(path.join("scan/nested/other.bin"), b"123456789").unwrap();
        std::fs::create_dir_all(path.join("scan/empty/leaf")).unwrap();
        std::fs::hard_link(path.join("scan/large.txt"), path.join("scan/link.txt")).unwrap();
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
                start_duplicates,
                start_empty_folders,
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
    fn scan(&self, module: StorageModule, command: &str) -> String {
        let root = self.root(module);
        let extra = if module == StorageModule::Duplicates {
            ",\"minimumBytes\":0"
        } else {
            ""
        };
        let id: String = self
            .invoke(
                command,
                &format!(r#"{{"rootId":"{root}","depth":64{extra}}}"#),
            )
            .unwrap()
            .deserialize()
            .unwrap();
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            let (status, error) = self.storage.status(&id).unwrap();
            assert!(error.is_none(), "{error:?}");
            if status.phase == StoragePhase::Complete {
                break;
            }
            assert!(Instant::now() < end);
            std::thread::yield_now();
        }
        id
    }
    fn page(&self, module: &str, id: &str, collection: &str, parent: Option<&str>) -> Page {
        let extra = parent
            .map(|p| format!(r#", "parentId":"{p}""#))
            .unwrap_or_default();
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            if let Ok(response) = self.invoke("storage_scan_page", &format!(r#"{{"module":"{module}","snapshotId":"{id}","collection":"{collection}","pageSize":100{extra}}}"#)) { return response.deserialize().unwrap(); }
            assert!(Instant::now() < end);
            std::thread::yield_now();
        }
    }
    fn plan(
        &self,
        module: &str,
        id: &str,
        candidates: &[String],
        disposition: &str,
    ) -> Result<tauri::ipc::InvokeResponseBody, String> {
        let ids = candidates
            .iter()
            .map(|id| format!("\"{id}\""))
            .collect::<Vec<_>>()
            .join(",");
        self.invoke("create_storage_plan", &format!(r#"{{"selection":{{"module":"{module}","snapshotId":"{id}","candidateIds":[{ids}]}},"disposition":"{disposition}"}}"#))
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
    DuplicateGroup(DuplicateGroup),
    DuplicateMember(DuplicateMember),
    EmptyFolder(EmptyFolderRecord),
}
#[derive(Deserialize)]
struct Page {
    records: Vec<Row>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Plan {
    plan_id: String,
}
fn candidate(e: &CandidateEligibility) -> String {
    match e {
        CandidateEligibility::Eligible { candidate_id } => candidate_id.clone(),
        _ => panic!("eligible fixture"),
    }
}
#[test]
fn duplicate_ipc_groups_members_hardlinks_and_subset_plan() {
    let f = Fixture::new();
    let id = f.scan(StorageModule::Duplicates, "start_duplicates");
    let groups = f.page("duplicates", &id, "duplicateGroups", None);
    assert_eq!(groups.records.len(), 1);
    let Row::DuplicateGroup(group) = &groups.records[0] else {
        panic!()
    };
    assert_eq!(group.independent_copies, 2);
    assert_eq!(group.member_count, 2);
    assert_eq!(group.bytes_per_copy, 9);
    let members = f.page("duplicates", &id, "duplicateMembers", Some(&group.group_id));
    let ids: Vec<_> = members
        .records
        .iter()
        .map(|r| {
            let Row::DuplicateMember(m) = r else { panic!() };
            assert_eq!(m.group_id, group.group_id);
            candidate(&m.file.eligibility)
        })
        .collect();
    assert_eq!(ids.len(), 2);
    // Selecting every copy is refused as invalid evidence, not as a vanished snapshot.
    assert_eq!(
        f.plan("duplicates", &id, &ids, "quarantine").unwrap_err(),
        r#"{"code":"invalid_evidence"}"#
    );
    let plan: Plan = f
        .plan("duplicates", &id, &ids[..1], "quarantine")
        .unwrap()
        .deserialize()
        .unwrap();
    assert_eq!(plan.plan_id.len(), 32);
    let Row::DuplicateMember(keeper) = &members.records[1] else {
        panic!()
    };
    std::fs::write(&keeper.file.display_path, b"changed keeper").unwrap();
    assert!(
        f.invoke_at(
            "execute_cleanup_plan",
            &format!(r#"{{"planId":"{}"}}"#, plan.plan_id),
            false,
            "main",
            "http://tauri.localhost"
        )
        .is_err()
    );
    let Row::DuplicateMember(selected) = &members.records[0] else {
        panic!()
    };
    assert_eq!(
        std::fs::read(&selected.file.display_path).unwrap(),
        b"123456789"
    );
}
#[test]
fn empty_ipc_root_retained_nonempty_absent_and_recovery_rejected() {
    let f = Fixture::new();
    let id = f.scan(StorageModule::EmptyFolders, "start_empty_folders");
    let rows = f.page("emptyFolders", &id, "emptyFolders", None);
    assert_eq!(rows.records.len(), 2);
    let ids: Vec<_> = rows
        .records
        .iter()
        .map(|r| {
            let Row::EmptyFolder(e) = r else { panic!() };
            assert!(e.depth > 0);
            assert!(e.display_path.contains("empty"));
            candidate(&e.eligibility)
        })
        .collect();
    assert!(f.plan("emptyFolders", &id, &ids, "quarantine").is_err());
    assert!(f.plan("emptyFolders", &id, &ids, "recycleBin").is_err());
    let plan: Plan = f
        .plan("emptyFolders", &id, &ids, "permanent")
        .unwrap()
        .deserialize()
        .unwrap();
    assert_eq!(plan.plan_id.len(), 32);
    assert!(f.path.join("scan/large.txt").exists());
    assert!(f.path.join("scan").exists());
    let child = f.path.join("scan/empty/leaf/new-child");
    std::fs::write(&child, b"must survive").unwrap();
    // Trusted native engine primitive on disposable data only; application IPC
    // separately requires a real native confirmation before this operation.
    let result = f.cleanup.execute_permanent(&plan.plan_id).unwrap();
    assert!(result.items.iter().all(|item| item.failure.is_some()));
    assert_eq!(result.accounting.failed_bytes, 0);
    assert_eq!(result.accounting.reclaimed_bytes, 0);
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct OutcomeWire {
        item_id: String,
        display_path: String,
        state: String,
        logical_bytes: u64,
        failure: Option<String>,
    }
    #[derive(Deserialize)]
    struct SummaryWire {
        items: Vec<OutcomeWire>,
    }
    #[derive(Deserialize)]
    struct HistoryWire {
        records: Vec<SummaryWire>,
    }
    let wire: HistoryWire = f
        .invoke("cleanup_history", "{}")
        .unwrap()
        .deserialize()
        .unwrap();
    for item in &wire.records[0].items {
        assert_eq!(item.state, "failed");
        assert_eq!(item.logical_bytes, 0);
        assert!(item.display_path.contains("empty"));
        assert!(ids.contains(&item.item_id));
        assert!(item.failure.is_some());
    }
    let restarted = CleanupService::new(f.path.join("state")).unwrap();
    let retained = restarted.history_page(Default::default()).unwrap();
    for (before, after) in result.items.iter().zip(&retained.records[0].items) {
        assert_eq!(before.item_id, after.item_id);
        assert_eq!(before.display_path, after.display_path);
        assert_eq!(before.failure, after.failure);
    }
    assert_eq!(std::fs::read(child).unwrap(), b"must survive");
    assert!(f.path.join("scan/empty/leaf").exists());
    assert!(f.path.join("scan").exists());
}
#[test]
fn pinned_kudu_duplicate_maximum_and_extension_filters_reach_native_discovery() {
    // db09e051... duplicate-finder.ipc.ts walkDirectory applies min/max sizes
    // and extensionFilter before forming size groups. Bare lowercase extensions
    // are this app's transport representation of Kudu's leading-dot strings.
    let f = Fixture::new();
    std::fs::write(f.path.join("scan/second.txt"), b"123456789").unwrap();
    for maximum in [9, 8] {
        let root = f.root(StorageModule::Duplicates);
        let id: String = f.invoke("start_duplicates", &format!(r#"{{"rootId":"{root}","depth":64,"minimumBytes":0,"maximumBytes":{maximum},"extensions":["txt"]}}"#)).unwrap().deserialize().unwrap();
        let groups = f.page("duplicates", &id, "duplicateGroups", None);
        if maximum == 9 {
            assert_eq!(groups.records.len(), 1);
            let Row::DuplicateGroup(group) = &groups.records[0] else {
                panic!()
            };
            assert_eq!(group.independent_copies, 2);
            let members = f.page("duplicates", &id, "duplicateMembers", Some(&group.group_id));
            assert!(members.records.iter().all(|row| matches!(row, Row::DuplicateMember(member) if member.file.display_path.ends_with(".txt"))));
        } else {
            assert!(groups.records.is_empty());
        }
    }
}
#[test]
fn starts_are_closed_raw_and_capability_scoped() {
    let f = Fixture::new();
    for (module, command, extra) in [
        (
            StorageModule::Duplicates,
            "start_duplicates",
            ",\"minimumBytes\":0",
        ),
        (StorageModule::EmptyFolders, "start_empty_folders", ""),
    ] {
        let root = f.root(module);
        for input in [
            format!(r#"{{"rootId":"{root}","depth":65{extra}}}"#),
            format!(r#"{{"rootId":"{root}","depth":1{extra},"path":"C:/"}}"#),
            format!(r#"{{"rootId":"bad","depth":1{extra}}}"#),
            "[]".into(),
        ] {
            assert!(f.invoke(command, &input).is_err());
        }
        let input = format!(r#"{{"rootId":"{root}","depth":1{extra}}}"#);
        assert!(
            f.invoke_at(command, &input, false, "main", "http://tauri.localhost")
                .is_err()
        );
        assert!(
            f.invoke_at(command, &input, true, "other", "http://tauri.localhost")
                .is_err()
        );
        assert!(
            f.invoke_at(command, &input, true, "main", "https://example.com")
                .is_err()
        );
    }
    let f = Fixture::new();
    let root = f.root(StorageModule::Duplicates);
    for value in ["-1", "1.5", "18446744073709551616"] {
        assert!(
            f.invoke(
                "start_duplicates",
                &format!(r#"{{"rootId":"{root}","depth":1,"minimumBytes":{value}}}"#)
            )
            .is_err()
        );
    }
}
