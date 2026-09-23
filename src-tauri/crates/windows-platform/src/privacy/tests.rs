use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use cleanup_core::system_change::{CatalogId, PriorState, SystemChange, UnsupportedReason};

use super::*;
use crate::os_info::{Edition, OsFacts, SUPPORTED_BUILDS};
use crate::security::system_changes::HelperChange;

type ValueMap = HashMap<(bool, String, String), u32>;

#[derive(Clone, Default)]
struct FakeStore {
    values: Arc<Mutex<ValueMap>>,
    tasks: Arc<Mutex<HashMap<String, bool>>>,
    denied: bool,
}

fn hk(hive: Hive) -> bool {
    matches!(hive, Hive::LocalMachine)
}

impl PrivacyStore for FakeStore {
    fn read_value(&self, hive: Hive, key: &str, name: &str) -> Result<Option<u32>, AdapterError> {
        if self.denied {
            return Err(AdapterError::Denied);
        }
        let map = self.values.lock().unwrap();
        Ok(map.get(&(hk(hive), key.into(), name.into())).copied())
    }

    fn write_value(
        &self,
        hive: Hive,
        key: &str,
        name: &str,
        value: Option<u32>,
    ) -> Result<(), AdapterError> {
        if self.denied {
            return Err(AdapterError::Denied);
        }
        let mut map = self.values.lock().unwrap();
        let k = (hk(hive), key.to_string(), name.to_string());
        match value {
            Some(v) => map.insert(k, v),
            None => map.remove(&k),
        };
        Ok(())
    }

    fn task_enabled(&self, path: &str) -> Result<Option<bool>, AdapterError> {
        if self.denied {
            return Err(AdapterError::Denied);
        }
        Ok(self.tasks.lock().unwrap().get(path).copied())
    }

    fn set_task_enabled(&self, path: &str, enabled: bool) -> Result<(), AdapterError> {
        if self.denied {
            return Err(AdapterError::Denied);
        }
        self.tasks.lock().unwrap().insert(path.into(), enabled);
        Ok(())
    }
}

fn pro() -> OsFacts {
    OsFacts::fixture(26100, Edition::Pro, false)
}

fn id(value: &str) -> CatalogId {
    CatalogId::parse(value).unwrap()
}

fn user_change(setting: &str, value: Option<u32>) -> SystemChange {
    SystemChange::SetUserSetting {
        setting_id: id(setting),
        value,
    }
}

fn entry(id: &str) -> &'static SettingEntry {
    settings_catalog().iter().find(|e| e.id == id).unwrap()
}

#[test]
fn privacy_none_deletes_the_value() {
    let store = FakeStore::default();
    let adapter = PrivacyAdapter::with(Box::new(store.clone()), pro());
    adapter
        .apply(&user_change("advertising-id", Some(0)))
        .unwrap();
    assert_eq!(
        adapter
            .observe(&user_change("advertising-id", None))
            .unwrap(),
        PriorState::RegistryValue { value: Some(0) }
    );
    adapter.apply(&user_change("advertising-id", None)).unwrap();
    assert!(store.values.lock().unwrap().is_empty());
}

#[test]
fn privacy_disallowed_value_is_refused() {
    let store = FakeStore::default();
    let adapter = PrivacyAdapter::with(Box::new(store.clone()), pro());
    assert_eq!(
        adapter.apply(&user_change("advertising-id", Some(7))),
        Err(AdapterError::Failed)
    );
    assert!(store.values.lock().unwrap().is_empty());
}

#[test]
fn privacy_user_and_machine_ids_are_not_interchangeable() {
    let adapter = PrivacyAdapter::with(Box::new(FakeStore::default()), pro());
    let wrong = SystemChange::SetMachineSetting {
        setting_id: id("advertising-id"),
        value: Some(0),
    };
    assert_eq!(
        adapter.describe(&wrong),
        Err(AdapterError::Unsupported(UnsupportedReason::NotPresent))
    );
    assert!(
        adapter
            .describe(&user_change("advertising-id", Some(0)))
            .is_ok()
    );
    assert_eq!(
        adapter.describe(&user_change("nope", Some(0))),
        Err(AdapterError::Unsupported(UnsupportedReason::NotPresent))
    );
}

#[test]
fn privacy_policy_entry_is_unsupported_on_home() {
    let change = HelperChange::SetMachinePolicyValue {
        setting_id: id("consumer-features"),
        value: Some(1),
    };
    for build in SUPPORTED_BUILDS {
        for edition in [Edition::Home, Edition::Pro, Edition::Enterprise] {
            let store = FakeStore::default();
            let os = OsFacts::fixture(build, edition, false);
            let observed = elevated::observe_with(&store, &os, &change);
            let applied = elevated::apply_with(&store, &os, &change);
            if edition == Edition::Home {
                let unsupported = Err(AdapterError::Unsupported(
                    UnsupportedReason::EditionUnsupported,
                ));
                assert_eq!(observed, unsupported);
                assert_eq!(
                    applied,
                    Err(AdapterError::Unsupported(
                        UnsupportedReason::EditionUnsupported
                    ))
                );
            } else {
                assert_eq!(observed, Ok(PriorState::RegistryValue { value: None }));
                assert_eq!(applied, Ok(()));
            }
        }
    }
}

#[test]
fn privacy_missing_task_is_not_present() {
    let store = FakeStore::default();
    let change = HelperChange::SetSystemTaskEnabled {
        catalog_id: id("wer-queue"),
        enabled: false,
    };
    let not_present = AdapterError::Unsupported(UnsupportedReason::NotPresent);
    assert_eq!(
        elevated::observe_with(&store, &pro(), &change),
        Err(not_present)
    );
    assert_eq!(
        elevated::apply_with(&store, &pro(), &change),
        Err(not_present)
    );

    let task = task_catalog().iter().find(|t| t.id == "wer-queue").unwrap();
    store.tasks.lock().unwrap().insert(task.path.into(), true);
    assert_eq!(
        elevated::observe_with(&store, &pro(), &change),
        Ok(PriorState::Enabled { enabled: true })
    );
    elevated::apply_with(&store, &pro(), &change).unwrap();
    assert!(!store.tasks.lock().unwrap()[task.path]);
}

#[test]
fn privacy_access_denied_is_reported() {
    let store = FakeStore {
        denied: true,
        ..FakeStore::default()
    };
    let adapter = PrivacyAdapter::with(Box::new(store.clone()), pro());
    assert_eq!(
        adapter.observe(&user_change("advertising-id", Some(0))),
        Err(AdapterError::Denied)
    );
    assert_eq!(
        adapter.apply(&user_change("advertising-id", Some(0))),
        Err(AdapterError::Denied)
    );
    let change = HelperChange::SetMachinePolicyValue {
        setting_id: id("telemetry-level"),
        value: Some(1),
    };
    assert_eq!(
        elevated::apply_with(&store, &pro(), &change),
        Err(AdapterError::Denied)
    );
}

#[test]
fn privacy_idempotency_and_inverse_from_none() {
    let store = FakeStore::default();
    let adapter = PrivacyAdapter::with(Box::new(store.clone()), pro());
    let change = user_change("bing-start-search", Some(0));
    let prior = adapter.observe(&change).unwrap();
    assert_eq!(prior, PriorState::RegistryValue { value: None });
    assert_eq!(change.is_satisfied_by(&prior), Some(false));
    adapter.apply(&change).unwrap();
    let after = adapter.observe(&change).unwrap();
    assert_eq!(change.is_satisfied_by(&after), Some(true));

    let inverse = change.inverse(&prior).unwrap().expect("inverse");
    assert_eq!(inverse, user_change("bing-start-search", None));
    adapter.apply(&inverse).unwrap();
    assert!(store.values.lock().unwrap().is_empty());
}

#[test]
fn privacy_catalog_sanity() {
    let mut ids = HashSet::new();
    for setting in settings_catalog() {
        assert!(ids.insert(setting.id), "duplicate {}", setting.id);
        assert!(CatalogId::parse(setting.id).is_ok(), "{}", setting.id);
        if let Some(recommended) = setting.recommended {
            assert!(setting.allowed.contains(&recommended), "{}", setting.id);
        }
        assert!(!setting.key.is_empty() && !setting.value_name.is_empty());
    }
    for task in task_catalog() {
        assert!(ids.insert(task.id), "duplicate {}", task.id);
        assert!(CatalogId::parse(task.id).is_ok(), "{}", task.id);
        assert!(task.path.starts_with('\\'));
    }
    assert_eq!(
        entry("advertising-id").key,
        r"Software\Microsoft\Windows\CurrentVersion\AdvertisingInfo"
    );
    assert_eq!(entry("telemetry-level").hive, SettingHive::Machine);
}

#[test]
fn privacy_report_with_fake_store() {
    let store = FakeStore::default();
    let e = entry("advertising-id");
    store
        .values
        .lock()
        .unwrap()
        .insert((false, e.key.into(), e.value_name.into()), 0);
    let report = privacy_report_with(&store, &OsFacts::fixture(22631, Edition::Home, false));
    let ad = report
        .settings
        .iter()
        .find(|s| s.id == "advertising-id")
        .unwrap();
    assert!(ad.applied && ad.supported);
    let cf = report
        .settings
        .iter()
        .find(|s| s.id == "consumer-features")
        .unwrap();
    assert_eq!(
        cf.unsupported_reason,
        Some(UnsupportedReason::EditionUnsupported)
    );
    assert!(report.tasks.iter().all(|t| !t.present));
    let json = serde_json::to_value(&report).unwrap();
    assert!(json["relatedServices"].is_array());
    assert!(json["settings"][0].get("unsupportedReason").is_some());
}

#[test]
fn privacy_live_report_is_read_only() {
    let report = privacy_report();
    assert_eq!(report.settings.len(), settings_catalog().len());
    assert_eq!(report.tasks.len(), task_catalog().len());
    assert_eq!(report.related_services, RELATED_SERVICES);
}
