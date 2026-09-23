mod commands {
    pub mod storage {
        include!("../src/commands/storage.rs");
    }
    pub mod browser {
        include!("../src/commands/browser.rs");
    }
}
use commands::{browser::*, storage::*};
use serde::Deserialize;
use std::sync::Arc;
use tauri::test::{INVOKE_KEY, get_ipc_response, mock_builder};
use windows_platform::storage::{browser, root_picker::ScopeService, scans::StorageService};

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
            list_browser_policy,
            start_browser_scan,
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
#[serde(rename_all = "camelCase")]
struct Policy {
    source: String,
    revision: String,
    lifecycle: windows_platform::storage::Lifecycle,
    risk: windows_platform::storage::Risk,
    consequence: String,
    minimum_age_seconds: u64,
    service_worker_disclosure: String,
    unsupported: Vec<String>,
    exclusions: Vec<String>,
    profile_cache_roots: Vec<String>,
    shared_cache_roots: Vec<String>,
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
fn browser_real_ipc_catalog_scopes_and_opaque_authorization_without_scanning() {
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
    let actual: Policy = call("list_browser_policy", "{}")
        .unwrap()
        .deserialize()
        .unwrap();
    let native = browser::policy();
    assert_eq!(actual.source, native.source);
    assert_eq!(actual.revision, native.revision);
    assert_eq!(actual.lifecycle, native.lifecycle);
    assert_eq!(actual.risk, native.risk);
    assert_eq!(actual.consequence, native.consequence);
    assert_eq!(actual.minimum_age_seconds, native.minimum_age_seconds);
    assert_eq!(
        actual.service_worker_disclosure,
        native.service_worker_disclosure
    );
    assert_eq!(actual.unsupported, native.unsupported);
    assert_eq!(actual.exclusions, native.exclusions);
    assert_eq!(actual.profile_cache_roots, native.profile_cache_roots);
    assert_eq!(actual.shared_cache_roots, native.shared_cache_roots);
    let inventory: Vec<Scope> = call("list_storage_scopes", r#"{"module":"browser"}"#)
        .unwrap()
        .deserialize()
        .unwrap();
    assert!(!inventory.is_empty());
    assert!(inventory.len() <= 32);

    for scope in inventory {
        let id = scope.scope_id;
        assert_eq!(id.len(), 32);
        assert!(
            call(
                "authorize_storage_scope",
                &format!(r#"{{"module":"cleaner","scopeId":"{id}"}}"#)
            )
            .is_err()
        );
        let result = call(
            "authorize_storage_scope",
            &format!(r#"{{"module":"browser","scopeId":"{id}"}}"#),
        );
        if !scope.available {
            assert!(result.is_err());
        }
        // Existing scopes can still be refused by native protection/no-follow policy.
        if let Ok(result) = result {
            let root: Root = result.deserialize().unwrap();
            assert_eq!(root.module, "browser");
            assert_eq!(root.root_id.len(), 32);
        }
    }
}
#[test]
fn browser_real_ipc_rejects_malformed_unknown_fields_and_foreign_callers() {
    let scopes = Arc::new(ScopeService::default());
    for (cmd, input) in [
        ("list_browser_policy", r#"{"path":"C:/"}"#),
        ("list_browser_policy", "[]"),
        ("start_browser_scan", "{}"),
        (
            "start_browser_scan",
            r#"{"rootId":"bad","serviceWorkerOptIn":false}"#,
        ),
        (
            "start_browser_scan",
            r#"{"rootId":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","serviceWorkerOptIn":false,"depth":64}"#,
        ),
        (
            "start_browser_scan",
            r#"{"rootId":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","serviceWorkerOptIn":false}"#,
        ),
        ("start_browser_scan", "{"),
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
    for cmd in ["list_browser_policy", "start_browser_scan"] {
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
