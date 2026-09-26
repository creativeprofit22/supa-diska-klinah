use std::{collections::HashMap, io, sync::Mutex};

use cleanup_core::system_change::{
    CatalogId, PriorState, RiskLevel, SystemChange, UnsupportedReason,
};

use super::*;
use crate::{
    os_info::{Edition, OsFacts, SUPPORTED_BUILDS},
    security::system_changes::HelperChange,
};

#[derive(Default)]
struct FakeStore {
    values: Mutex<HashMap<(&'static str, &'static str), RegistryData>>,
    deny: bool,
    writes: Mutex<usize>,
}

impl FakeStore {
    fn with(key: &'static str, name: &'static str, data: RegistryData) -> Self {
        let store = Self::default();
        store.values.lock().unwrap().insert((key, name), data);
        store
    }

    fn denied() -> Self {
        Self {
            deny: true,
            ..Self::default()
        }
    }

    fn get(&self, key: &'static str, name: &'static str) -> Option<RegistryData> {
        self.values.lock().unwrap().get(&(key, name)).cloned()
    }
}

impl PolicyStore for FakeStore {
    fn read(&self, key: &'static str, name: &'static str) -> io::Result<Option<RegistryData>> {
        if self.deny {
            return Err(io::Error::from_raw_os_error(5));
        }
        Ok(self.get(key, name))
    }

    fn write(&self, key: &'static str, name: &'static str, value: Option<u32>) -> io::Result<()> {
        if self.deny {
            return Err(io::Error::from_raw_os_error(5));
        }
        *self.writes.lock().unwrap() += 1;
        let mut values = self.values.lock().unwrap();
        match value {
            Some(value) => {
                values.insert((key, name), RegistryData::Dword(value));
            }
            None => {
                values.remove(&(key, name));
            }
        }
        Ok(())
    }
}

struct FakeAgent {
    automatic: Result<AutoUpdateInfo, AdapterError>,
    system: Result<bool, AdapterError>,
    key: bool,
    detect_calls: Mutex<usize>,
}

impl FakeAgent {
    fn ok() -> Self {
        Self {
            automatic: Ok(AutoUpdateInfo {
                service_enabled: Some(true),
                last_search_success: Some(1_700_000_000),
                last_install_success: Some(1_690_000_000),
            }),
            system: Ok(false),
            key: false,
            detect_calls: Mutex::new(0),
        }
    }
}

impl UpdateAgent for FakeAgent {
    fn automatic_updates(&self) -> Result<AutoUpdateInfo, AdapterError> {
        self.automatic
    }
    fn system_reboot_required(&self) -> Result<bool, AdapterError> {
        self.system
    }
    fn reboot_pending_key(&self) -> bool {
        self.key
    }
    fn detect_now(&self) -> Result<(), AdapterError> {
        *self.detect_calls.lock().unwrap() += 1;
        self.automatic.map(|_| ())
    }
}

const UNAVAILABLE: AdapterError = AdapterError::Unsupported(UnsupportedReason::ApiUnavailable);

fn pro() -> OsFacts {
    OsFacts::fixture(26100, Edition::Pro, false)
}

fn change(id: &str, value: Option<u32>) -> SystemChange {
    SystemChange::SetWindowsUpdatePolicy {
        setting_id: CatalogId::parse(id).unwrap(),
        value,
    }
}

fn adapter(os: OsFacts, store: FakeStore) -> UpdatesAdapter {
    UpdatesAdapter::with(os, Box::new(store))
}

#[test]
fn gating_matrix_covers_builds_editions_and_management() {
    for build in SUPPORTED_BUILDS {
        for edition in [Edition::Home, Edition::Pro, Edition::Enterprise] {
            for managed in [true, false] {
                let os = OsFacts::fixture(build, edition, managed);
                let expected = if edition == Edition::Home {
                    Err(UnsupportedReason::EditionUnsupported)
                } else if managed {
                    Err(UnsupportedReason::ManagedDevice)
                } else {
                    Ok(())
                };
                assert_eq!(
                    policy_support(&os),
                    expected,
                    "{build} {edition:?} {managed}"
                );

                let store = FakeStore::default();
                let observed = adapter(os.clone(), FakeStore::default())
                    .observe(&change("no-auto-update", Some(1)));
                let observed_helper = observe_policy(&os, &store, "no-auto-update", Some(1));
                let applied = apply_policy(&os, &store, "no-auto-update", Some(1));
                match expected {
                    Ok(()) => {
                        assert_eq!(observed, Ok(PriorState::RegistryValue { value: None }));
                        assert_eq!(observed_helper, observed);
                        assert_eq!(applied, Ok(()));
                        assert_eq!(
                            store.get(AU_POLICY_KEY, "NoAutoUpdate"),
                            Some(RegistryData::Dword(1))
                        );
                    }
                    Err(reason) => {
                        assert_eq!(observed, Err(AdapterError::Unsupported(reason)));
                        assert_eq!(observed_helper, Err(AdapterError::Unsupported(reason)));
                        assert_eq!(applied, Err(AdapterError::Unsupported(reason)));
                        assert_eq!(*store.writes.lock().unwrap(), 0);
                    }
                }

                let status = status_with(&FakeAgent::ok(), &os, &store);
                assert_eq!(status.policy_supported, expected.is_ok());
                assert_eq!(status.unsupported_reason, expected.err());
                assert_eq!(status.edition, edition);
                assert_eq!(status.managed, managed);
            }
        }
    }
}

#[test]
fn unsupported_build_is_refused() {
    let os = OsFacts::fixture(17763, Edition::Pro, false);
    assert_eq!(
        adapter(os, FakeStore::default()).observe(&change("no-auto-update", None)),
        Err(AdapterError::Unsupported(UnsupportedReason::OsVersion))
    );
}

#[test]
fn unavailable_com_class_yields_null_fields() {
    let agent = FakeAgent {
        automatic: Err(UNAVAILABLE),
        system: Err(UNAVAILABLE),
        key: false,
        detect_calls: Mutex::new(0),
    };
    let status = status_with(&agent, &pro(), &FakeStore::default());
    assert!(!status.api_available);
    assert_eq!(status.service_enabled, None);
    assert_eq!(status.last_search_success, None);
    assert_eq!(status.last_install_success, None);
    assert_eq!(status.reboot_required, None);
    assert_eq!(status.policies.len(), POLICY_CATALOG.len());

    // Only one class missing still reports the API as unavailable.
    let agent = FakeAgent {
        system: Err(UNAVAILABLE),
        ..FakeAgent::ok()
    };
    let status = status_with(&agent, &pro(), &FakeStore::default());
    assert!(!status.api_available);
    assert_eq!(status.service_enabled, Some(true));
    assert_eq!(status.reboot_required, None);
    assert_eq!(
        trigger_detection_with(&FakeAgent {
            automatic: Err(UNAVAILABLE),
            ..FakeAgent::ok()
        }),
        Err(UNAVAILABLE)
    );
}

#[test]
fn hresults_classify_to_adapter_errors() {
    use windows::Win32::Foundation::{E_ACCESSDENIED, E_FAIL, REGDB_E_CLASSNOTREG};
    assert_eq!(wua::classify(REGDB_E_CLASSNOTREG), UNAVAILABLE);
    assert_eq!(wua::classify(E_ACCESSDENIED), AdapterError::Denied);
    assert_eq!(wua::classify(E_FAIL), AdapterError::Failed);
}

#[test]
fn reboot_key_marks_reboot_pending() {
    let agent = FakeAgent {
        key: true,
        ..FakeAgent::ok()
    };
    let status = status_with(&agent, &pro(), &FakeStore::default());
    assert!(status.api_available);
    assert_eq!(status.reboot_required, Some(true));
    assert_eq!(
        status_with(&FakeAgent::ok(), &pro(), &FakeStore::default()).reboot_required,
        Some(false)
    );
}

#[test]
fn access_denied_maps_to_denied() {
    let os = pro();
    let store = FakeStore::denied();
    assert_eq!(
        observe_policy(&os, &store, "au-options", Some(3)),
        Err(AdapterError::Denied)
    );
    assert_eq!(
        apply_policy(&os, &store, "au-options", Some(3)),
        Err(AdapterError::Denied)
    );
    assert_eq!(
        trigger_detection_with(&FakeAgent {
            automatic: Err(AdapterError::Denied),
            ..FakeAgent::ok()
        }),
        Err(AdapterError::Denied)
    );
}

#[test]
fn unknown_ids_and_disallowed_values_are_refused() {
    let os = pro();
    let store = FakeStore::default();
    let not_present: Result<PriorState, AdapterError> =
        Err(AdapterError::Unsupported(UnsupportedReason::NotPresent));
    let a = adapter(os.clone(), FakeStore::default());
    for (id, value) in [
        ("unknown-policy", Some(1)),
        ("au-options", Some(1)),
        ("au-options", Some(5)),
        ("no-auto-update", Some(2)),
        ("no-auto-reboot-with-users", Some(7)),
        ("defer-feature-updates-days", Some(366)),
    ] {
        assert_eq!(a.observe(&change(id, value)), not_present, "{id} {value:?}");
        assert!(a.describe(&change(id, value)).is_err());
        assert_eq!(
            apply_policy(&os, &store, id, value).map(|_| ()),
            Err(AdapterError::Unsupported(UnsupportedReason::NotPresent))
        );
    }
    assert_eq!(*store.writes.lock().unwrap(), 0);
    // Range bounds are inclusive.
    assert!(apply_policy(&os, &store, "defer-feature-updates-days", Some(0)).is_ok());
    assert!(apply_policy(&os, &store, "defer-feature-updates-days", Some(365)).is_ok());
    assert_eq!(
        store.get(WINDOWS_UPDATE_POLICY_KEY, "DeferFeatureUpdatesPeriodInDays"),
        Some(RegistryData::Dword(365))
    );
    for (value, _) in match POLICY_CATALOG[0].allowed {
        Allowed::Set(values) => values,
        Allowed::Range { .. } => unreachable!(),
    } {
        assert!(apply_policy(&os, &store, "au-options", Some(*value)).is_ok());
    }
}

#[test]
fn non_dword_value_is_not_present() {
    let store = FakeStore::with(AU_POLICY_KEY, "AUOptions", RegistryData::String("3".into()));
    assert_eq!(
        adapter(pro(), store).observe(&change("au-options", Some(3))),
        Err(AdapterError::Unsupported(UnsupportedReason::NotPresent))
    );
}

#[test]
fn none_deletes_the_value() {
    let os = pro();
    let store = FakeStore::with(AU_POLICY_KEY, "NoAutoUpdate", RegistryData::Dword(1));
    assert_eq!(
        observe_policy(&os, &store, "no-auto-update", None),
        Ok(PriorState::RegistryValue { value: Some(1) })
    );
    apply_policy(&os, &store, "no-auto-update", None).unwrap();
    assert_eq!(store.get(AU_POLICY_KEY, "NoAutoUpdate"), None);
    assert_eq!(
        observe_policy(&os, &store, "no-auto-update", None),
        Ok(PriorState::RegistryValue { value: None })
    );
    let summary = adapter(os, FakeStore::default())
        .describe(&change("no-auto-update", None))
        .unwrap();
    assert!(summary.effect.contains("default"));
    assert_eq!(summary.risk, RiskLevel::Low);
}

#[test]
fn idempotent_and_inverse_restores_prior() {
    let os = pro();
    let store = FakeStore::with(AU_POLICY_KEY, "AUOptions", RegistryData::Dword(3));
    let target = change("au-options", Some(4));
    let prior = observe_policy(&os, &store, "au-options", Some(4)).unwrap();
    assert_eq!(prior, PriorState::RegistryValue { value: Some(3) });
    assert_eq!(target.is_satisfied_by(&prior), Some(false));

    apply_policy(&os, &store, "au-options", Some(4)).unwrap();
    let after = observe_policy(&os, &store, "au-options", Some(4)).unwrap();
    assert_eq!(target.is_satisfied_by(&after), Some(true));
    let writes = *store.writes.lock().unwrap();
    apply_policy(&os, &store, "au-options", Some(4)).unwrap();
    assert_eq!(
        *store.writes.lock().unwrap(),
        writes,
        "already applied: no write"
    );

    let inverse = target.inverse(&prior).unwrap().unwrap();
    assert_eq!(inverse, change("au-options", Some(3)));
    let SystemChange::SetWindowsUpdatePolicy { setting_id, value } = &inverse else {
        unreachable!()
    };
    apply_policy(&os, &store, setting_id.as_str(), *value).unwrap();
    assert_eq!(
        store.get(AU_POLICY_KEY, "AUOptions"),
        Some(RegistryData::Dword(3))
    );

    // Inverse of a change on an unset value deletes it.
    let unset_inverse = target
        .inverse(&PriorState::RegistryValue { value: None })
        .unwrap()
        .unwrap();
    assert_eq!(unset_inverse, change("au-options", None));
}

#[test]
fn describe_labels_values_and_apply_is_helper_only() {
    let a = adapter(pro(), FakeStore::default());
    let summary = a.describe(&change("au-options", Some(2))).unwrap();
    assert!(summary.effect.contains("Notify before download"));
    assert_eq!(
        a.describe(&change("no-auto-update", Some(1))).unwrap().risk,
        RiskLevel::High
    );
    assert!(
        a.describe(&change("defer-feature-updates-days", Some(30)))
            .unwrap()
            .effect
            .contains("30 days")
    );
    assert_eq!(
        a.apply(&change("au-options", Some(2))),
        Err(AdapterError::Failed)
    );
    assert_eq!(
        a.observe(&SystemChange::SetHibernation { enabled: true }),
        Err(AdapterError::Failed)
    );
}

#[test]
fn elevated_rejects_other_helper_variants() {
    let other = HelperChange::SetHibernation { enabled: true };
    assert_eq!(elevated::observe(&other), Err(AdapterError::Failed));
    assert_eq!(elevated::apply(&other), Err(AdapterError::Failed));
}

#[test]
fn status_reports_policies_and_allowed_values() {
    let store = FakeStore::with(AU_POLICY_KEY, "AUOptions", RegistryData::Dword(4));
    let agent = FakeAgent::ok();
    let status = status_with(&agent, &pro(), &store);
    assert_eq!(
        *agent.detect_calls.lock().unwrap(),
        0,
        "status never triggers DetectNow"
    );
    assert_eq!(*store.writes.lock().unwrap(), 0, "status never writes");
    assert!(status.api_available);
    assert_eq!(status.last_search_success, Some(1_700_000_000));
    let au = &status.policies[0];
    assert_eq!(
        (au.id, au.current, au.applied),
        ("au-options", Some(4), true)
    );
    let json = serde_json::to_value(&status).unwrap();
    assert!(json.get("apiAvailable").is_some());
    assert!(json.get("lastInstallSuccess").is_some());
    assert_eq!(json["policies"][0]["allowed"]["options"][2]["value"], 4);
    assert_eq!(json["policies"][3]["allowed"]["max"], 365);

    let home = status_with(
        &FakeAgent::ok(),
        &OsFacts::fixture(22631, Edition::Home, false),
        &store,
    );
    assert!(!home.policies[0].applied);
    assert_eq!(home.policies[0].current, Some(4));
}

#[test]
fn ole_dates_convert_from_1899_12_30_epoch() {
    assert_eq!(ole_date_to_unix(25_569.0), Some(0));
    assert_eq!(ole_date_to_unix(25_569.5), Some(43_200));
    // 2024-01-01T00:00:00Z
    assert_eq!(ole_date_to_unix(45_292.0), Some(1_704_067_200));
    assert_eq!(ole_date_to_unix(0.0), None);
    assert_eq!(ole_date_to_unix(-1.0), None);
    assert_eq!(ole_date_to_unix(f64::NAN), None);
    assert_eq!(ole_date_to_unix(f64::INFINITY), None);
}

#[test]
fn live_read_only_status() {
    // Reads only; never calls DetectNow and never writes.
    let status = status_with(&WuaAgent, &os_info::current(), &RegistryPolicyStore);
    assert_eq!(status.policies.len(), POLICY_CATALOG.len());
    if let Some(seconds) = status.last_search_success {
        assert!(seconds > 1_000_000_000);
    }
    serde_json::to_string(&status).unwrap();
}
