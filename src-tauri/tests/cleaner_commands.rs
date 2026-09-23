mod commands {
    pub mod storage {
        include!("../src/commands/storage.rs");
    }
    pub mod cleaner {
        include!("../src/commands/cleaner.rs");
    }
}
use commands::{cleaner::*, storage::*};
use serde::Deserialize;
use std::sync::Arc;
use tauri::test::{INVOKE_KEY, get_ipc_response, mock_builder};
use windows_platform::storage::{cleaner, root_picker::ScopeService, scans::StorageService};

fn invoke(
    cmd: &str,
    input: &str,
    raw: bool,
    label: &str,
    url: &str,
    scopes: Arc<ScopeService>,
) -> Result<tauri::ipc::InvokeResponseBody, String> {
    let app = mock_builder()
        .manage(Arc::new(StorageService::new()))
        .manage(scopes)
        .invoke_handler(tauri::generate_handler![
            list_cleaner_catalog,
            start_cleaner,
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
#[derive(Deserialize)]
struct Catalog {
    targets: Vec<Target>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Target {
    catalog_id: String,
    target_id: String,
    path: String,
    source: String,
    revision: String,
    rule_version: u32,
    minimum_age_seconds: u64,
    consequence: String,
    exclusions: Vec<String>,
    matcher: String,
    unsupported_reason: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Scope {
    scope_id: String,
    available: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Root {
    root_id: String,
    module: String,
}
#[test]
fn cleaner_real_ipc_catalog_scopes_and_opaque_authorization_without_scanning() {
    let scopes = Arc::new(ScopeService::default());
    let call = |cmd: &str, input: &str| {
        invoke(
            cmd,
            input,
            true,
            "main",
            "http://tauri.localhost",
            scopes.clone(),
        )
    };
    let catalog: Catalog = call("list_cleaner_catalog", "{}")
        .unwrap()
        .deserialize()
        .unwrap();
    let targets = catalog.targets;
    assert!(!targets.is_empty());
    assert!(targets.len() <= 1024);
    assert!(targets.iter().any(|t| t.unsupported_reason.is_some()));
    let native = cleaner::catalog_inventory();
    assert_eq!(targets.len(), native.targets.len());
    for (target, expected) in targets.iter().zip(&native.targets) {
        assert_eq!(target.catalog_id, expected.catalog_id);
        assert_eq!(target.target_id, expected.target_id);
        assert_eq!(target.path, expected.path);
        assert_eq!(target.source, expected.source);
        assert_eq!(target.revision, expected.revision);
        assert_eq!(target.rule_version, expected.rule_version);
        assert_eq!(target.minimum_age_seconds, expected.minimum_age_seconds);
        assert_eq!(target.consequence, expected.consequence);
        assert_eq!(target.exclusions, expected.exclusions);
        assert_eq!(target.matcher, expected.matcher);
        assert_eq!(target.unsupported_reason, expected.unsupported_reason);
    }
    let inventory: Vec<Scope> = call("list_storage_scopes", r#"{"module":"cleaner"}"#)
        .unwrap()
        .deserialize()
        .unwrap();
    assert!(!inventory.is_empty());
    assert!(inventory.len() <= targets.len());
    assert!(inventory.iter().any(|s| !s.available));
    for scope in inventory {
        let id = scope.scope_id;
        assert_eq!(id.len(), 32);
        assert!(
            call(
                "authorize_storage_scope",
                &format!(r#"{{"module":"browser","scopeId":"{id}"}}"#)
            )
            .is_err()
        );
        let result = call(
            "authorize_storage_scope",
            &format!(r#"{{"module":"cleaner","scopeId":"{id}"}}"#),
        );
        if !scope.available {
            assert!(result.is_err());
        }
        // Existing scopes can still be refused by native protection/no-follow policy.
        if let Ok(result) = result {
            let root: Root = result.deserialize().unwrap();
            assert_eq!(root.module, "cleaner");
            assert_eq!(root.root_id.len(), 32);
        }
    }
}
#[test]
fn cleaner_real_ipc_rejects_malformed_unknown_fields_and_foreign_callers() {
    let scopes = Arc::new(ScopeService::default());
    for (cmd, input) in [
        ("list_cleaner_catalog", r#"{"path":"C:/"}"#),
        ("list_cleaner_catalog", "[]"),
        ("start_cleaner", "{}"),
        ("start_cleaner", r#"{"rootId":"bad"}"#),
        (
            "start_cleaner",
            r#"{"rootId":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","depth":64}"#,
        ),
        (
            "start_cleaner",
            r#"{"rootId":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#,
        ),
        ("start_cleaner", "{"),
    ] {
        assert!(
            invoke(
                cmd,
                input,
                true,
                "main",
                "http://tauri.localhost",
                scopes.clone()
            )
            .is_err()
        );
    }
    for cmd in ["list_cleaner_catalog", "start_cleaner"] {
        assert!(
            invoke(
                cmd,
                "{}",
                false,
                "main",
                "http://tauri.localhost",
                scopes.clone()
            )
            .is_err()
        );
        for (label, url) in [
            ("foreign", "http://tauri.localhost"),
            ("main", "https://foreign.invalid"),
        ] {
            assert!(invoke(cmd, "{}", true, label, url, scopes.clone()).is_err());
        }
    }
}
