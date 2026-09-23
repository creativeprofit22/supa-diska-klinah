//! Real IPC round-trips for system-management commands. Every test is
//! read-only on the live machine: inventory commands, previews, and plan
//! rejection paths. Nothing here confirms or executes a plan.

#[allow(dead_code)]
mod commands {
    pub mod drivers {
        include!("../src/commands/drivers.rs");
    }
    pub mod firewall {
        include!("../src/commands/firewall.rs");
    }
    pub mod hosts {
        include!("../src/commands/hosts.rs");
    }
    pub mod optimizer {
        include!("../src/commands/optimizer.rs");
    }
    pub mod power {
        include!("../src/commands/power.rs");
    }
    pub mod privacy {
        include!("../src/commands/privacy.rs");
    }
    pub mod restore {
        include!("../src/commands/restore.rs");
    }
    pub mod scheduler {
        include!("../src/commands/scheduler.rs");
    }
    pub mod services {
        include!("../src/commands/services.rs");
    }
    pub mod startup {
        include!("../src/commands/startup.rs");
    }
    pub mod system_change {
        include!("../src/commands/system_change.rs");
    }
    pub mod updates {
        include!("../src/commands/updates.rs");
    }
}

use std::sync::Arc;

use commands::{
    drivers::*, firewall::*, hosts::*, optimizer::*, power::*, privacy::*, restore::*,
    scheduler::*, services::*, startup::*, system_change::*, updates::*,
};
use tauri::test::{INVOKE_KEY, get_ipc_response, mock_builder};
use windows_platform::system_change::{JsonValue as Value, SystemChangeService};

fn invoke(cmd: &str, body: Option<&str>) -> Result<Value, Value> {
    let app_data = std::env::temp_dir().join(format!("sdk-ipc-{}-{cmd}", std::process::id()));
    let service = Arc::new(SystemChangeService::windows(&app_data).unwrap());
    let app = mock_builder()
        .manage(service)
        .invoke_handler(tauri::generate_handler![
            list_startup_items,
            list_services,
            list_driver_packages,
            get_firewall_status,
            get_hosts_report,
            get_privacy_report,
            get_power_status,
            list_restore_points,
            get_restore_protection,
            get_windows_update_status,
            list_scan_schedules,
            list_scheduled_scan_summaries,
            get_optimizer_proposals,
            preview_system_change,
            create_system_change_plan,
            execute_system_change_plan,
            system_change_journal,
            create_system_rollback_plan
        ])
        .build(tauri::generate_context!())
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let response = get_ipc_response(
        &window,
        tauri::webview::InvokeRequest {
            cmd: cmd.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: "http://tauri.localhost".parse().unwrap(),
            body: match body {
                Some(raw) => tauri::ipc::InvokeBody::Raw(raw.as_bytes().to_vec()),
                None => tauri::ipc::InvokeBody::default(),
            },
            headers: Default::default(),
            invoke_key: INVOKE_KEY.into(),
        },
    );
    let _ = std::fs::remove_dir_all(app_data);
    response.map(|body| body.deserialize::<Value>().unwrap())
}

/// Inventory must succeed, or fail with a bounded, coded error (for example
/// when an API is unavailable on this machine) — never panic or hang.
fn ok_or_coded(cmd: &str) -> Option<Value> {
    match invoke(cmd, None) {
        Ok(value) => Some(value),
        Err(error) => {
            assert!(
                error["code"].is_string(),
                "{cmd} returned an uncoded error: {error}"
            );
            None
        }
    }
}

#[test]
fn startup_inventory_lists_items() {
    let items = invoke("list_startup_items", None).unwrap();
    assert!(items.as_array().is_some());
}

#[test]
fn services_inventory_covers_the_catalog() {
    let items = invoke("list_services", None).unwrap();
    let items = items.as_array().unwrap();
    assert!(items.len() >= 10);
    assert!(
        items
            .iter()
            .all(|item| item["id"].is_string() && item["installed"].is_boolean())
    );
}

#[test]
fn drivers_inventory_is_read_only() {
    if let Some(packages) = ok_or_coded("list_driver_packages") {
        for package in packages.as_array().unwrap() {
            assert!(
                package["publishedName"]
                    .as_str()
                    .unwrap()
                    .starts_with("oem")
            );
        }
    }
}

#[test]
fn firewall_status_reports_profiles() {
    if let Some(status) = ok_or_coded("get_firewall_status") {
        assert!(status["profiles"].is_array());
    }
}

#[test]
fn hosts_report_hashes_the_file() {
    if let Some(report) = ok_or_coded("get_hosts_report") {
        assert_eq!(report["sha256"].as_str().unwrap().len(), 64);
    }
}

#[test]
fn privacy_report_lists_settings_and_tasks() {
    let report = invoke("get_privacy_report", None).unwrap();
    assert!(!report["settings"].as_array().unwrap().is_empty());
    assert!(!report["tasks"].as_array().unwrap().is_empty());
}

#[test]
fn power_status_reports_hibernation() {
    if let Some(status) = ok_or_coded("get_power_status") {
        assert!(status["hibernation"].is_object());
    }
}

#[test]
fn restore_listing_and_protection_are_read_only() {
    ok_or_coded("list_restore_points");
    ok_or_coded("get_restore_protection");
}

#[test]
fn windows_update_status_reports_gating() {
    if let Some(status) = ok_or_coded("get_windows_update_status") {
        assert!(status["policySupported"].is_boolean());
    }
}

#[test]
fn scheduler_lists_own_tasks_and_summaries() {
    ok_or_coded("list_scan_schedules");
    assert!(
        invoke("list_scheduled_scan_summaries", None)
            .unwrap()
            .is_array()
    );
}

#[test]
fn optimizer_proposals_are_individual_changes() {
    let report = invoke("get_optimizer_proposals", None).unwrap();
    for proposal in report["proposals"].as_array().unwrap() {
        assert!(proposal["planned"]["change"]["kind"].is_string());
    }
}

#[test]
fn preview_reads_state_without_writing() {
    match invoke(
        "preview_system_change",
        Some(r#"{"change":{"kind":"setHibernation","enabled":false}}"#),
    ) {
        Ok(planned) => assert!(planned["expectedPrior"]["kind"].is_string()),
        Err(error) => assert!(error["code"].is_string()),
    }
}

#[test]
fn plan_commands_reject_untrusted_input() {
    for (cmd, body) in [
        (
            "create_system_change_plan",
            r#"{"changes":[{"kind":"runCommand","command":"whoami"}]}"#,
        ),
        (
            "create_system_change_plan",
            r#"{"changes":[],"extra":true}"#,
        ),
        (
            "preview_system_change",
            r#"{"change":{"kind":"deleteDriverPackage","publishedName":"..\\evil.inf"}}"#,
        ),
        ("preview_system_change", "[]"),
    ] {
        assert_eq!(
            invoke(cmd, Some(body)).unwrap_err()["code"],
            "invalidChange",
            "{cmd} {body}"
        );
    }
    assert_eq!(
        invoke(
            "execute_system_change_plan",
            Some(&format!(r#"{{"planId":"{}"}}"#, "a".repeat(32)))
        )
        .unwrap_err()["code"],
        "planNotFound"
    );
    assert_eq!(
        invoke(
            "create_system_rollback_plan",
            Some(r#"{"entryIds":["../x"]}"#)
        )
        .unwrap_err()["code"],
        "rollbackUnavailable"
    );
    assert!(
        invoke("system_change_journal", Some("{}"))
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
}
