mod commands {
    pub mod storage {
        include!("../src/commands/storage.rs");
    }
    pub mod disk_analyzer {
        include!("../src/commands/disk_analyzer.rs");
    }
}
use commands::{disk_analyzer::start_disk_analyzer, storage::*};
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
        self, DirectorySummary, ExtensionSummary, PageCollection, PageRequest, StorageModule,
        StoragePhase, root_picker::ScopeService, scans::StorageService,
    },
};

struct Fixture {
    path: PathBuf,
    service: Arc<StorageService>,
    cleanup: Arc<CleanupService>,
}
impl Fixture {
    fn new() -> Self {
        // Parallel tests can read the same clock tick; the counter keeps fixture roots distinct.
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "analyzer-ipc-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(path.join("scan")).unwrap();
        for i in 0..205 {
            let dir = path.join("scan").join(format!("d{i:03}"));
            std::fs::create_dir(&dir).unwrap();
            std::fs::write(dir.join("file.dat"), [1]).unwrap();
        }
        std::fs::create_dir(path.join("scan/d000/deep")).unwrap();
        std::fs::write(path.join("scan/d000/deep/deep.bin"), [2; 7]).unwrap();
        std::fs::write(path.join("scan/root.txt"), [3; 10]).unwrap();
        let cleanup = Arc::new(CleanupService::new(path.join("state")).unwrap());
        Self {
            path,
            cleanup,
            service: Arc::new(StorageService::new()),
        }
    }
    fn authorize(&self, module: StorageModule) -> String {
        self.service
            .authorize_root_for(
                &self.path.join("scan"),
                module,
                &storage::current_protection().unwrap(),
            )
            .unwrap()
    }
    fn invoke_at(
        &self,
        command: &str,
        json: &str,
        label: &str,
        url: &str,
    ) -> Result<tauri::ipc::InvokeResponseBody, String> {
        let app = mock_builder()
            .manage(self.service.clone())
            .manage(self.cleanup.clone())
            .manage(Arc::new(ScopeService::default()))
            .invoke_handler(tauri::generate_handler![
                start_disk_analyzer,
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
                body: tauri::ipc::InvokeBody::Raw(json.as_bytes().to_vec()),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.into(),
            },
        )
        .map_err(|e| e.to_string())
    }
    fn invoke(&self, command: &str, json: &str) -> Result<tauri::ipc::InvokeResponseBody, String> {
        self.invoke_at(command, json, "main", "http://tauri.localhost")
    }
    fn start(&self, depth: u16) -> String {
        let root = self.authorize(StorageModule::DiskAnalyzer);
        self.invoke(
            "start_disk_analyzer",
            &format!(r#"{{"rootId":"{root}","displayedDepth":{depth}}}"#),
        )
        .unwrap()
        .deserialize()
        .unwrap()
    }
    fn wait(&self, id: &str) {
        let end = Instant::now() + Duration::from_secs(15);
        loop {
            let (status, error) = self.service.status(id).unwrap();
            assert!(error.is_none(), "{error:?}");
            if matches!(
                status.phase,
                StoragePhase::Complete | StoragePhase::Cancelled
            ) && self
                .service
                .page(&PageRequest {
                    snapshot_id: id.into(),
                    module: StorageModule::DiskAnalyzer,
                    collection: PageCollection::Tree,
                    parent_id: None,
                    cursor: None,
                    page_size: 100,
                })
                .is_ok()
            {
                return;
            }
            assert!(Instant::now() < end, "scan timeout");
            std::thread::yield_now();
        }
    }
    fn raw_page(&self, id: &str, collection: &str, extra: &str) -> String {
        match self.invoke("storage_scan_page", &format!(r#"{{"snapshotId":"{id}","module":"diskAnalyzer","collection":"{collection}","pageSize":100{extra}}}"#)).unwrap() {
            tauri::ipc::InvokeResponseBody::Json(json) => json,
            _ => panic!("expected JSON response"),
        }
    }
    fn export_render_fixture(&self, id: &str, root: &DirectorySummary) {
        // Only generated disposable-root data is exported, never developer folders.
        let root_json = self.raw_page(id, "tree", "");
        let tauri::ipc::InvokeResponseBody::Json(status_json) = self
            .invoke(
                "storage_scan_status",
                &format!(r#"{{"module":"diskAnalyzer","snapshotId":"{id}"}}"#),
            )
            .unwrap()
        else {
            panic!("JSON status")
        };
        let mut pages = Vec::new();
        let mut extra = format!(r#", "parentId":"{}""#, root.node_id);
        let mut child_id = None;
        loop {
            let json = self.raw_page(id, "tree", &extra);
            let page: Page = storage::decode_command(json.as_bytes()).unwrap();
            if child_id.is_none()
                && let Some(Row::Directory(row)) = page.records.first()
            {
                child_id = Some(row.node_id.clone());
            }
            pages.push(json);
            let Some(cursor) = page.next_cursor else {
                break;
            };
            extra = format!(r#", "parentId":"{}", "cursor":"{cursor}""#, root.node_id);
        }
        let extensions = self.raw_page(id, "extensions", "");
        let leaf = self.raw_page(
            id,
            "tree",
            &format!(r#", "parentId":"{}""#, child_id.unwrap()),
        );
        let output = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join(".gg/smoke-artifacts/analyzer-native.json");
        std::fs::create_dir_all(output.parent().unwrap()).unwrap();
        std::fs::write(
            output,
            format!(
                r#"{{"displayedDepth":1,"status":{status_json},"root":{root_json},"children":[{}],"extensions":{extensions},"leaf":{leaf}}}"#,
                pages.join(",")
            ),
        )
        .unwrap();
    }
    fn page(&self, id: &str, collection: &str, extra: &str) -> Result<Page, String> {
        self.invoke("storage_scan_page", &format!(r#"{{"snapshotId":"{id}","module":"diskAnalyzer","collection":"{collection}","pageSize":100{extra}}}"#)).map(|body|body.deserialize().unwrap())
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
    Directory(DirectorySummary),
    Extension(ExtensionSummary),
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Page {
    records: Vec<Row>,
    next_cursor: Option<String>,
    retained_total: usize,
}

#[test]
fn analyzer_real_ipc_full_subtree_totals_bounded_pages_extensions_and_no_mutation() {
    let f = Fixture::new();
    let id = f.start(1);
    f.wait(&id);
    let root_page = f.page(&id, "tree", "").unwrap();
    assert_eq!(root_page.records.len(), 1);
    let Row::Directory(root) = &root_page.records[0] else {
        panic!("directory root")
    };
    assert_eq!(root.logical_bytes, 222);
    f.export_render_fixture(&id, root);
    assert_eq!(root.independent_files, 207);
    assert!(root.parent_id.is_none());
    let mut extra = format!(r#", "parentId":"{}""#, root.node_id);
    let mut count = 0;
    let mut first_cursor = None;
    loop {
        let page = f.page(&id, "tree", &extra).unwrap();
        assert_eq!(page.retained_total, 205);
        assert!(page.records.len() <= 100);
        count += page.records.len();
        for row in page.records {
            let Row::Directory(row) = row else {
                panic!("directory")
            };
            if row.display_path.ends_with("d000") {
                assert_eq!(row.logical_bytes, 8);
            }
        }
        let Some(cursor) = page.next_cursor else {
            break;
        };
        if first_cursor.is_none() {
            first_cursor = Some(cursor.clone());
        }
        extra = format!(r#", "parentId":"{}", "cursor":"{cursor}""#, root.node_id);
    }
    assert_eq!(count, 205);
    let extensions = f.page(&id, "extensions", "").unwrap();
    let mut total = 0;
    for row in extensions.records {
        let Row::Extension(row) = row else {
            panic!("extension")
        };
        total += row.logical_bytes;
    }
    assert_eq!(total, 222);
    assert!(
        f.page(
            &id,
            "extensions",
            &format!(r#", "cursor":"{}""#, first_cursor.unwrap())
        )
        .is_err()
    );
    assert!(f.invoke("create_storage_plan",&format!(r#"{{"selection":{{"snapshotId":"{id}","module":"diskAnalyzer","candidateIds":["{}"]}},"disposition":"recycleBin"}}"#,root.node_id)).is_err());
    assert_eq!(
        std::fs::read(f.path.join("scan/root.txt")).unwrap(),
        [3; 10]
    );
    assert_eq!(
        std::fs::read(f.path.join("scan/d000/deep/deep.bin")).unwrap(),
        [2; 7]
    );
}
#[test]
fn pinned_kudu_analyzer_orders_subtrees_and_extensions_by_descending_bytes() {
    // Kudu db09e051... disk-analyzer.ipc.ts sorts children and extension totals
    // by size. Names intentionally oppose size order to prevent a false positive.
    let f = Fixture::new();
    std::fs::create_dir(f.path.join("scan/z-largest")).unwrap();
    std::fs::write(f.path.join("scan/z-largest/content.zz"), [0; 50]).unwrap();
    std::fs::create_dir(f.path.join("scan/a-small")).unwrap();
    std::fs::write(f.path.join("scan/a-small/content.txt"), [0; 2]).unwrap();
    let id = f.start(1);
    f.wait(&id);
    let roots = f.page(&id, "tree", "").unwrap();
    let Row::Directory(root) = &roots.records[0] else {
        panic!()
    };
    let children = f
        .page(&id, "tree", &format!(r#", "parentId":"{}""#, root.node_id))
        .unwrap();
    let Row::Directory(largest) = &children.records[0] else {
        panic!()
    };
    assert!(largest.display_path.ends_with("z-largest"));
    assert_eq!(largest.logical_bytes, 50);
    let extensions = f.page(&id, "extensions", "").unwrap();
    let Row::Extension(first) = &extensions.records[0] else {
        panic!()
    };
    assert_eq!(first.extension, "dat");
    assert_eq!(first.logical_bytes, 205);
}
#[test]
fn analyzer_rejects_untrusted_filters_paths_modules_and_foreign_callers() {
    let f = Fixture::new();
    for input in [
        r#"{"rootId":"C:\\Windows","displayedDepth":3}"#.to_string(),
        format!(r#"{{"rootId":"{}","displayedDepth":65}}"#, "a".repeat(32)),
        format!(
            r#"{{"rootId":"{}","displayedDepth":1,"path":"C:\\Windows"}}"#,
            "a".repeat(32)
        ),
        format!(r#"{{"rootId":"{}","displayedDepth":-1}}"#, "a".repeat(32)),
        format!(
            r#"{{"rootId":"{}","displayedDepth":1,"limits":{{"depth":1}}}}"#,
            "a".repeat(32)
        ),
    ] {
        assert_eq!(
            f.invoke("start_disk_analyzer", &input).unwrap_err(),
            r#"{"code":"invalid_input"}"#
        );
    }
    let wrong = f.authorize(StorageModule::LargeFiles);
    assert!(
        f.invoke(
            "start_disk_analyzer",
            &format!(r#"{{"rootId":"{wrong}","displayedDepth":1}}"#)
        )
        .is_err()
    );
    for (label, url) in [
        ("foreign", "http://tauri.localhost"),
        ("main", "https://example.com"),
    ] {
        assert!(
            f.invoke_at("start_disk_analyzer", "{}", label, url)
                .unwrap_err()
                .contains("not allowed")
        );
    }
}
#[test]
fn analyzer_snapshot_ids_cursors_and_parent_nodes_cannot_cross_scans_or_survive_release() {
    let f = Fixture::new();
    let first = f.start(1);
    f.wait(&first);
    let page = f.page(&first, "tree", "").unwrap();
    let Row::Directory(root) = &page.records[0] else {
        panic!()
    };
    let children = f
        .page(
            &first,
            "tree",
            &format!(r#", "parentId":"{}""#, root.node_id),
        )
        .unwrap();
    let second = f.start(1);
    f.wait(&second);
    assert!(
        f.page(
            &second,
            "tree",
            &format!(r#", "parentId":"{}""#, root.node_id)
        )
        .is_err()
    );
    assert!(
        f.page(
            &second,
            "tree",
            &format!(r#", "cursor":"{}""#, children.next_cursor.unwrap())
        )
        .is_err()
    );
    f.invoke(
        "release_storage_scan",
        &format!(r#"{{"module":"diskAnalyzer","snapshotId":"{first}"}}"#),
    )
    .unwrap();
    assert!(f.page(&first, "tree", "").is_err());
    f.page(&second, "tree", "").unwrap();
}
#[test]
fn analyzer_cancellation_discards_native_results_and_revokes_authority() {
    let f = Fixture::new();
    let root = f.authorize(StorageModule::DiskAnalyzer);
    let protection = storage::current_protection().unwrap();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    // Deterministic cancellation seam: real discovery over the disposable root,
    // then pause publication so the real IPC cancel can interrupt finalization.
    let id = f
        .service
        .start_with(
            StorageModule::DiskAnalyzer,
            Some(&root),
            Default::default(),
            Some(protection.clone()),
            move |ctx| {
                storage::disk_analyzer::discover(ctx, &protection, 1)?;
                ready_tx.send(()).unwrap();
                let end = Instant::now() + Duration::from_secs(15);
                while !ctx.cancellation.is_cancelled() {
                    if Instant::now() >= end {
                        return Err(storage::StorageError::SnapshotUnavailable.into());
                    }
                    std::thread::yield_now();
                }
                Ok(())
            },
        )
        .unwrap();
    ready_rx.recv_timeout(Duration::from_secs(15)).unwrap();
    f.invoke(
        "cancel_storage_scan",
        &format!(r#"{{"module":"diskAnalyzer","snapshotId":"{id}"}}"#),
    )
    .unwrap();
    f.wait(&id);
    assert_eq!(
        f.service.status(&id).unwrap().0.phase,
        StoragePhase::Cancelled
    );
    assert!(f.page(&id, "tree", "").unwrap().records.is_empty());
}
