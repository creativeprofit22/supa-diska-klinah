mod commands {
    pub mod storage {
        include!("../src/commands/storage.rs");
    }
    // Only `cleanup_history` is exercised here; the rest of the included file is unused.
    #[allow(dead_code)]
    pub mod cleanup {
        include!("../src/commands/cleanup.rs");
    }
    pub mod uninstaller {
        include!("../src/commands/uninstaller.rs");
    }
}
use commands::{cleanup::cleanup_history, storage::*, uninstaller::*};
use serde::Deserialize;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tauri::test::{INVOKE_KEY, get_ipc_response, mock_builder};
use windows_platform::{
    cleanup::CleanupService,
    storage::{scans::StorageService, uninstaller::InstalledProgram},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Status {
    snapshot_id: String,
    module: String,
    phase: String,
    visited_entries: u64,
    retained_records: u64,
    hashed_bytes: u64,
    completed_hashes: u64,
    completeness: Completeness,
}
#[derive(Deserialize)]
struct Completeness {
    reasons: Vec<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Page {
    snapshot_id: String,
    records: Vec<Row>,
    next_cursor: Option<String>,
    retained_total: u64,
    completeness: Completeness,
}
#[derive(Deserialize)]
#[serde(tag = "kind", content = "record", rename_all = "camelCase")]
enum Row {
    Program(InstalledProgram),
}

#[test]
fn readonly_inventory_and_closed_vendor_ipc_capabilities() {
    let root = std::env::temp_dir().join(format!(
        "uninstaller-ipc-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let app = mock_builder()
        .manage(Arc::new(StorageService::new()))
        .manage(Arc::new(CleanupService::new(root.clone()).unwrap()))
        .manage(Arc::new(
            windows_platform::storage::root_picker::ScopeService::default(),
        ))
        .invoke_handler(tauri::generate_handler![
            start_program_inventory,
            prepare_vendor_job,
            confirm_vendor_job,
            vendor_job_status,
            cancel_vendor_job,
            release_vendor_job,
            vendor_job_history,
            cleanup_history,
            storage_scan_status,
            storage_scan_page,
            cancel_storage_scan,
            release_storage_scan,
            choose_storage_root,
            list_storage_scopes,
            authorize_storage_scope,
            create_storage_plan
        ])
        .build(tauri::generate_context!())
        .unwrap();
    let main = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let foreign = tauri::WebviewWindowBuilder::new(&app, "foreign", Default::default())
        .build()
        .unwrap();
    let call = |command: &str, input: &str, raw: bool, other: bool, remote: bool| {
        get_ipc_response(
            if other { &foreign } else { &main },
            tauri::webview::InvokeRequest {
                cmd: command.into(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: if remote {
                    "https://foreign.invalid"
                } else {
                    "http://tauri.localhost"
                }
                .parse()
                .unwrap(),
                body: if raw {
                    tauri::ipc::InvokeBody::Raw(input.as_bytes().to_vec())
                } else {
                    tauri::ipc::InvokeBody::Json(input.parse().unwrap())
                },
                headers: Default::default(),
                invoke_key: INVOKE_KEY.into(),
            },
        )
    };
    for command in [
        "start_program_inventory",
        "prepare_vendor_job",
        "confirm_vendor_job",
        "vendor_job_status",
        "cancel_vendor_job",
        "release_vendor_job",
        "vendor_job_history",
        "cleanup_history",
    ] {
        for input in [
            "[]",
            "{",
            r#"{"path":"C:/","argv":[],"backend":"registry"}"#,
        ] {
            assert!(
                call(command, input, true, false, false).is_err(),
                "{command}"
            );
        }
        assert!(call(command, "{}", false, false, false).is_err());
        assert!(
            call(command, "{}", true, true, false)
                .unwrap_err()
                .to_string()
                .contains("not allowed")
        );
        assert!(
            call(command, "{}", true, false, true)
                .unwrap_err()
                .to_string()
                .contains("not allowed")
        );
    }
    for input in [
        format!(
            r#"{{"nameContains":"{}","largestFirst":false}}"#,
            "a".repeat(129)
        ),
        r#"{"nameContains":"bad\nname","largestFirst":false}"#.into(),
        format!(r#"{{"nameContains":"{}"}}"#, "😀".repeat(65)),
    ] {
        assert!(call("start_program_inventory", &input, true, false, false).is_err());
    }
    // No real program is ever prepared or confirmed. IDs below are malformed.
    for command in [
        "confirm_vendor_job",
        "vendor_job_status",
        "cancel_vendor_job",
        "release_vendor_job",
    ] {
        assert!(call(command, r#"{"jobId":"bad"}"#, true, false, false).is_err());
        assert!(
            call(
                command,
                r#"{"jobId":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","argv":[]}"#,
                true,
                false,
                false
            )
            .is_err()
        );
    }
    assert!(
        call(
            "prepare_vendor_job",
            r#"{"snapshotId":"bad","programId":"bad"}"#,
            true,
            false,
            false
        )
        .is_err()
    );
    for (command, kind, max, code) in [
        ("cleanup_history", "cleanup", 100, "invalidInput"),
        ("vendor_job_history", "vendor", 64, "invalid_input"),
    ] {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct EmptyPage {
            records: Vec<()>,
            next_cursor: Option<String>,
        }
        let history: EmptyPage = call(command, r#"{"cursor":null,"limit":1}"#, true, false, false)
            .unwrap()
            .deserialize()
            .unwrap();
        assert!(history.records.is_empty());
        assert!(history.next_cursor.is_none());
        let cursor = |value: String| format!(r#"{{"cursor":"{}"}}"#, value.replace('"', "\\\""));
        for input in [
            r#"{"limit":0}"#.to_string(),
            format!(r#"{{"limit":{}}}"#, max + 1),
            r#"{"limit":-1}"#.into(),
            r#"{"limit":1.5}"#.into(),
            r#"{"cursor":{}}"#.into(),
            r#"{"offset":0}"#.into(),
            format!(r#"{{"cursor":"{}"}}"#, "x".repeat(257)),
            cursor(format!(
                r#"{{"version":2,"kind":"{kind}","timestamp":0,"id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}}"#
            )),
            cursor(format!(
                r#"{{"version":1,"kind":"{}","timestamp":0,"id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}}"#,
                if kind == "cleanup" {
                    "vendor"
                } else {
                    "cleanup"
                }
            )),
            cursor(format!(
                r#"{{"version":1,"kind":"{kind}","timestamp":0,"id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","extra":true}}"#
            )),
        ] {
            let error = call(command, &input, true, false, false).unwrap_err();
            assert_eq!(error["code"], code, "{command}: {input}");
        }
        assert!(
            call(
                command,
                &format!("{{{}", " ".repeat(70_000)),
                true,
                false,
                false
            )
            .is_err()
        );
        let valid = cursor(format!(
            r#"{{"version":1,"kind":"{kind}","timestamp":0,"id":"{}"}}"#,
            "a".repeat(32)
        ));
        assert!(call(command, &valid, true, false, false).is_ok());
    }
    let id: String = call(
        "start_program_inventory",
        r#"{"nameContains":"","largestFirst":true}"#,
        true,
        false,
        false,
    )
    .unwrap()
    .deserialize()
    .unwrap();
    assert_eq!(id.len(), 32);
    let input = format!(r#"{{"module":"uninstaller","snapshotId":"{id}"}}"#);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let status: Status = call("storage_scan_status", &input, true, false, false)
            .unwrap()
            .deserialize()
            .unwrap();
        assert_eq!(status.snapshot_id, id);
        assert_eq!(status.module, "uninstaller");
        assert_eq!(status.hashed_bytes, 0);
        assert_eq!(status.completed_hashes, 0);
        assert!(status.retained_records <= status.visited_entries);
        let _ = status.completeness.reasons;
        if status.phase == "complete" {
            break;
        }
        assert_ne!(status.phase, "failed");
        assert!(Instant::now() < deadline);
        std::thread::park_timeout(Duration::from_millis(10));
    }
    let page: Page = call("storage_scan_page", &format!(r#"{{"module":"uninstaller","snapshotId":"{id}","collection":"programs","pageSize":32}}"#), true, false, false).unwrap().deserialize().unwrap();
    assert_eq!(page.snapshot_id, id);
    assert!(page.records.len() <= 32);
    assert!(page.retained_total >= page.records.len() as u64);
    assert!(page.next_cursor.is_none_or(|id| id.len() == 32));
    let _ = page.completeness.reasons;
    for Row::Program(program) in page.records {
        assert_eq!(program.program_id.len(), 32);
        assert_eq!(program.leftover_support, "unsupportedUnknownOwnership");
    }
    assert!(call("create_storage_plan", &format!(r#"{{"selection":{{"module":"uninstaller","snapshotId":"{id}","candidateIds":["{}"]}},"disposition":"permanent"}}"#, "a".repeat(32)), true, false, false).is_err());
    call("release_storage_scan", &input, true, false, false).unwrap();
    drop(main);
    drop(foreign);
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
