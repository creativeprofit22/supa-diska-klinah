use std::{
    collections::{HashMap, HashSet},
    sync::Mutex,
};

use cleanup_core::system_change::{
    CatalogId, ChangeOutcome, FailureCode, PriorState, RiskLevel, ServiceStartState,
    ServiceStartType, SystemChange, UnsupportedReason,
};

use super::*;
use crate::{
    os_info::{Edition, OsFacts, SUPPORTED_BUILDS},
    security::system_changes::{HelperChange, HelperChangeItem, SystemChangeBackend, apply_batch},
    system_change::{AdapterError, SystemAdapter},
};

#[derive(Default)]
struct FakeScm {
    services: Mutex<HashMap<&'static str, Result<ServiceStatus, AdapterError>>>,
    write_error: Option<AdapterError>,
    reads: Mutex<u32>,
    writes: Mutex<Vec<(String, ServiceStartType)>>,
}

impl FakeScm {
    fn with(entries: &[(&'static str, Result<ServiceStatus, AdapterError>)]) -> Self {
        Self {
            services: Mutex::new(entries.iter().copied().collect()),
            ..Self::default()
        }
    }

    fn writes(&self) -> usize {
        self.writes.lock().unwrap().len()
    }
}

impl ServiceReader for FakeScm {
    fn query(&self, service_name: &str) -> Result<ServiceStatus, AdapterError> {
        *self.reads.lock().unwrap() += 1;
        self.services
            .lock()
            .unwrap()
            .get(service_name)
            .copied()
            .unwrap_or(Err(AdapterError::Unsupported(
                UnsupportedReason::NotPresent,
            )))
    }
}

impl ServiceWriter for FakeScm {
    fn set_start_type(
        &self,
        service_name: &str,
        start_type: ServiceStartType,
    ) -> Result<(), AdapterError> {
        self.writes
            .lock()
            .unwrap()
            .push((service_name.to_owned(), start_type));
        if let Some(error) = self.write_error {
            return Err(error);
        }
        let mut services = self.services.lock().unwrap();
        let Some(Ok(status)) = services.get_mut(service_name) else {
            return Err(AdapterError::Unsupported(UnsupportedReason::NotPresent));
        };
        status.start = start_type.into();
        Ok(())
    }
}

impl SystemChangeBackend for FakeScm {
    fn observe(&self, change: &HelperChange) -> Result<PriorState, AdapterError> {
        elevated::observe_with(self, change)
    }

    fn apply(&self, change: &HelperChange) -> Result<(), AdapterError> {
        elevated::apply_with(self, self, change)
    }
}

fn status(start: ServiceStartState) -> Result<ServiceStatus, AdapterError> {
    Ok(ServiceStatus {
        start,
        delayed_auto_start: false,
        running: false,
    })
}

fn change(id: &str, start_type: ServiceStartType) -> SystemChange {
    SystemChange::SetServiceStartType {
        catalog_id: CatalogId::parse(id).unwrap(),
        start_type,
    }
}

fn helper(id: &str, start_type: ServiceStartType) -> HelperChange {
    HelperChange::from_system_change(&change(id, start_type)).unwrap()
}

fn start(start: ServiceStartState) -> PriorState {
    PriorState::ServiceStart { start }
}

const NOT_PRESENT: AdapterError = AdapterError::Unsupported(UnsupportedReason::NotPresent);

#[test]
fn catalog_is_well_formed() {
    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    for entry in catalog() {
        let id = CatalogId::parse(entry.id).expect(entry.id);
        assert_eq!(id.as_str(), entry.id);
        assert_eq!(entry.id, entry.id.to_ascii_lowercase());
        assert!(ids.insert(entry.id), "duplicate id {}", entry.id);
        assert!(
            names.insert(entry.service_name.to_ascii_lowercase()),
            "duplicate service {}",
            entry.service_name
        );
        assert!(!entry.service_name.is_empty() && entry.service_name.len() <= 256);
        assert!(!entry.label.is_empty() && !entry.description.is_empty());
        assert_ne!(entry.recommended, ServiceStartType::Automatic);
    }
    for required in [
        "diagtrack",
        "dmwappushservice",
        "mapsbroker",
        "lfsvc",
        "retaildemo",
        "remoteregistry",
        "fax",
        "xblauthmanager",
        "xblgamesave",
        "xboxnetapisvc",
        "xboxgipsvc",
        "wersvc",
        "sysmain",
        "wsearch",
        "spooler",
        "phonesvc",
        "wisvc",
    ] {
        assert!(ids.contains(required), "{required}");
    }
    let find = |id: &str| catalog().iter().find(|entry| entry.id == id).unwrap();
    assert_eq!(
        find("remoteregistry").recommended,
        ServiceStartType::Disabled
    );
    assert_eq!(find("sysmain").recommended, ServiceStartType::Manual);
    assert_eq!(find("sysmain").risk, RiskLevel::Medium);
    assert_eq!(find("wsearch").risk, RiskLevel::Medium);
    assert_eq!(find("spooler").risk, RiskLevel::High);
    assert_eq!(find("spooler").recommended, ServiceStartType::Manual);
}

#[test]
fn services_module_is_available_on_every_supported_build() {
    for build in SUPPORTED_BUILDS {
        for edition in [Edition::Home, Edition::Pro] {
            assert!(OsFacts::fixture(build, edition, false).is_supported());
            assert!(OsFacts::fixture(build, edition, true).is_supported());
        }
    }
}

#[test]
fn unknown_catalog_ids_fail_closed_without_system_access() {
    let fake = FakeScm::with(&[("NotInCatalog", status(ServiceStartState::Manual))]);
    let adapter = ServicesAdapter::with_reader(Box::new(FakeScm::default()));
    let unknown = change("notincatalog", ServiceStartType::Disabled);
    assert_eq!(adapter.describe(&unknown), Err(NOT_PRESENT));
    assert_eq!(adapter.observe(&unknown), Err(NOT_PRESENT));
    let unknown = helper("notincatalog", ServiceStartType::Disabled);
    assert_eq!(elevated::observe_with(&fake, &unknown), Err(NOT_PRESENT));
    assert_eq!(
        elevated::apply_with(&fake, &fake, &unknown),
        Err(NOT_PRESENT)
    );
    assert_eq!(*fake.reads.lock().unwrap(), 0);
    assert_eq!(fake.writes(), 0);
    // Other helper variants are not ours.
    let other = HelperChange::SetHibernation { enabled: false };
    assert_eq!(elevated::observe(&other), Err(AdapterError::Failed));
    assert_eq!(elevated::apply(&other), Err(AdapterError::Failed));
    assert_eq!(
        adapter.observe(&SystemChange::SetHibernation { enabled: false }),
        Err(AdapterError::Failed)
    );
}

#[test]
fn missing_services_are_not_present() {
    let fake = FakeScm::default();
    let adapter = ServicesAdapter::with_reader(Box::new(FakeScm::default()));
    let target = change("fax", ServiceStartType::Disabled);
    assert!(adapter.describe(&target).is_ok());
    assert_eq!(adapter.observe(&target), Err(NOT_PRESENT));
    let target = helper("fax", ServiceStartType::Disabled);
    assert_eq!(
        elevated::apply_with(&fake, &fake, &target),
        Err(NOT_PRESENT)
    );
    assert_eq!(fake.writes(), 0);
    let items = inventory(&fake);
    assert_eq!(items.len(), catalog().len());
    assert!(
        items
            .iter()
            .all(|item| !item.installed && item.start.is_none())
    );
}

#[test]
fn boot_and_system_start_types_are_observed_but_never_written() {
    for state in [ServiceStartState::Boot, ServiceStartState::System] {
        let fake = FakeScm::with(&[("Spooler", status(state))]);
        let target = helper("spooler", ServiceStartType::Disabled);
        assert_eq!(elevated::observe_with(&fake, &target), Ok(start(state)));
        assert_eq!(
            elevated::apply_with(&fake, &fake, &target),
            Err(AdapterError::Failed)
        );
        assert_eq!(fake.writes(), 0);
        let results = apply_batch(
            &[HelperChangeItem {
                change: target,
                expected_prior: start(state),
            }],
            &fake,
        );
        assert_eq!(
            results[0].outcome,
            ChangeOutcome::Failed {
                code: FailureCode::SystemError
            }
        );
        assert_eq!(fake.writes(), 0);
    }
}

#[test]
fn access_denied_maps_to_denied() {
    let fake = FakeScm::with(&[("RemoteRegistry", Err(AdapterError::Denied))]);
    let adapter = ServicesAdapter::with_reader(Box::new(FakeScm::with(&[(
        "RemoteRegistry",
        Err(AdapterError::Denied),
    )])));
    assert_eq!(
        adapter.observe(&change("remoteregistry", ServiceStartType::Disabled)),
        Err(AdapterError::Denied)
    );
    let target = helper("remoteregistry", ServiceStartType::Disabled);
    assert_eq!(
        elevated::apply_with(&fake, &fake, &target),
        Err(AdapterError::Denied)
    );
    let item = inventory(&fake)
        .into_iter()
        .find(|item| item.id == "remoteregistry")
        .unwrap();
    assert!(item.installed && item.start.is_none());

    // A protected service refuses the write.
    let protected = FakeScm {
        write_error: Some(AdapterError::Denied),
        ..FakeScm::with(&[("WSearch", status(ServiceStartState::Automatic))])
    };
    let results = apply_batch(
        &[HelperChangeItem {
            change: helper("wsearch", ServiceStartType::Manual),
            expected_prior: start(ServiceStartState::Automatic),
        }],
        &protected,
    );
    assert_eq!(results[0].outcome, ChangeOutcome::Denied);
}

#[test]
fn unavailable_scm_is_reported_as_api_unavailable() {
    const UNAVAILABLE: AdapterError = AdapterError::Unsupported(UnsupportedReason::ApiUnavailable);
    let fake = FakeScm::with(&[("SysMain", Err(UNAVAILABLE))]);
    let target = helper("sysmain", ServiceStartType::Manual);
    assert_eq!(elevated::observe_with(&fake, &target), Err(UNAVAILABLE));
    assert_eq!(
        elevated::apply_with(&fake, &fake, &target),
        Err(UNAVAILABLE)
    );
    assert_eq!(fake.writes(), 0);
    let item = inventory(&fake)
        .into_iter()
        .find(|item| item.id == "sysmain")
        .unwrap();
    assert!(!item.installed && item.start.is_none());
}

#[test]
fn applying_twice_is_idempotent() {
    let fake = FakeScm::with(&[("DiagTrack", status(ServiceStartState::Automatic))]);
    let item = HelperChangeItem {
        change: helper("diagtrack", ServiceStartType::Disabled),
        expected_prior: start(ServiceStartState::Automatic),
    };
    let first = apply_batch(std::slice::from_ref(&item), &fake);
    assert_eq!(first[0].outcome, ChangeOutcome::Applied);
    assert_eq!(first[0].prior, start(ServiceStartState::Automatic));
    assert_eq!(
        fake.writes.lock().unwrap()[0],
        ("DiagTrack".to_owned(), ServiceStartType::Disabled)
    );
    let second = apply_batch(&[item], &fake);
    assert_eq!(second[0].outcome, ChangeOutcome::AlreadyApplied);
    assert_eq!(fake.writes(), 1);
    assert_eq!(
        change("diagtrack", ServiceStartType::Disabled)
            .is_satisfied_by(&start(ServiceStartState::Disabled)),
        Some(true)
    );
}

#[test]
fn observed_prior_state_yields_the_right_inverse() {
    let adapter = ServicesAdapter::with_reader(Box::new(FakeScm::with(&[(
        "SysMain",
        Ok(ServiceStatus {
            start: ServiceStartState::Automatic,
            delayed_auto_start: true,
            running: true,
        }),
    )])));
    let target = change("sysmain", ServiceStartType::Manual);
    let prior = adapter.observe(&target).unwrap();
    assert_eq!(prior, start(ServiceStartState::Automatic));
    assert_eq!(
        target.inverse(&prior).unwrap(),
        Some(change("sysmain", ServiceStartType::Automatic))
    );
    let impact = adapter.describe(&target).unwrap();
    assert_eq!(impact.risk, RiskLevel::Medium);
    assert_eq!(impact.component, "SysMain (Superfetch)");
    assert!(
        target
            .inverse(&start(ServiceStartState::System))
            .map_or(true, |inverse| inverse.is_none())
    );
}

#[test]
fn inventory_reports_state_for_installed_services() {
    let fake = FakeScm::with(&[(
        "WSearch",
        Ok(ServiceStatus {
            start: ServiceStartState::Automatic,
            delayed_auto_start: true,
            running: true,
        }),
    )]);
    let items = inventory(&fake);
    let search = items.iter().find(|item| item.id == "wsearch").unwrap();
    assert!(search.installed && search.running && search.delayed_auto_start);
    assert_eq!(search.start, Some(ServiceStartState::Automatic));
    let json = serde_json::to_value(search).unwrap();
    assert_eq!(json["serviceName"], "WSearch");
    assert_eq!(json["start"], "automatic");
    assert_eq!(json["delayedAutoStart"], true);
    assert_eq!(json["category"], "performance");
}

#[test]
fn live_inventory_reads_the_real_scm() {
    let items = list_services();
    assert_eq!(items.len(), catalog().len());
    for (item, entry) in items.iter().zip(catalog()) {
        assert_eq!(item.id, entry.id);
        if !item.installed {
            assert!(item.start.is_none() && !item.running);
        }
        if item.delayed_auto_start {
            assert_eq!(item.start, Some(ServiceStartState::Automatic));
        }
    }
    assert_eq!(
        ScmServices.query("SupaDiskaKlinahNoSuchService"),
        Err(NOT_PRESENT)
    );
    let adapter = ServicesAdapter::new();
    let observed = adapter.observe(&change("spooler", ServiceStartType::Manual));
    assert!(matches!(
        observed,
        Ok(PriorState::ServiceStart { .. }) | Err(NOT_PRESENT) | Err(AdapterError::Denied)
    ));
}
