use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use cleanup_core::system_change::{
    CatalogId, ChangeOutcome, DriverPackageName, EntryName, FailureCode, ImpactSummary, PriorState,
    RestartRequirement, RiskLevel, RollbackStatus, ServiceStartState, ServiceStartType,
    StartupEntryRef, StartupLocation, StartupScope, SystemChange, SystemModule, UnsupportedReason,
};

use super::*;
use crate::os_info::{Edition, SUPPORTED_BUILDS};

fn supported_os() -> OsFacts {
    OsFacts::fixture(SUPPORTED_BUILDS[0], Edition::Pro, false)
}

fn impact() -> ImpactSummary {
    ImpactSummary {
        component: "Test".into(),
        effect: "Changes a test value".into(),
        restart: RestartRequirement::None,
        risk: RiskLevel::Low,
    }
}

fn user_startup(name: &str, enabled: bool) -> SystemChange {
    SystemChange::SetStartupEntry {
        entry: StartupEntryRef {
            scope: StartupScope::User,
            location: StartupLocation::Run,
            name: EntryName::parse(name).unwrap(),
        },
        enabled,
    }
}

fn service(id: &str, start_type: ServiceStartType) -> SystemChange {
    SystemChange::SetServiceStartType {
        catalog_id: CatalogId::parse(id).unwrap(),
        start_type,
    }
}

/// Fake system state keyed by the change's target identity.
#[derive(Default)]
struct FakeSystem {
    state: Mutex<HashMap<String, PriorState>>,
    fail_apply: Mutex<Vec<String>>,
    unsupported: Mutex<Vec<String>>,
    applies: Mutex<u32>,
}

fn key(change: &SystemChange) -> String {
    match change {
        SystemChange::SetStartupEntry { entry, .. } => format!("startup:{}", entry.name),
        SystemChange::SetServiceStartType { catalog_id, .. } => format!("service:{catalog_id}"),
        SystemChange::DeleteDriverPackage { published_name } => format!("driver:{published_name}"),
        other => format!("{other:?}"),
    }
}

impl FakeSystem {
    fn set(&self, change: &SystemChange, state: PriorState) {
        self.state.lock().unwrap().insert(key(change), state);
    }

    fn get(&self, change: &SystemChange) -> Option<PriorState> {
        self.state.lock().unwrap().get(&key(change)).cloned()
    }

    fn target(change: &SystemChange) -> PriorState {
        match change {
            SystemChange::SetStartupEntry { enabled, .. } => {
                PriorState::Enabled { enabled: *enabled }
            }
            SystemChange::SetServiceStartType { start_type, .. } => PriorState::ServiceStart {
                start: (*start_type).into(),
            },
            SystemChange::DeleteDriverPackage { .. } => {
                PriorState::DriverPackage { present: false }
            }
            _ => PriorState::NotApplicable,
        }
    }
}

impl SystemAdapter for FakeSystem {
    fn describe(&self, _change: &SystemChange) -> Result<ImpactSummary, AdapterError> {
        Ok(impact())
    }

    fn observe(&self, change: &SystemChange) -> Result<PriorState, AdapterError> {
        if self.unsupported.lock().unwrap().contains(&key(change)) {
            return Err(AdapterError::Unsupported(UnsupportedReason::ApiUnavailable));
        }
        self.get(change)
            .ok_or(AdapterError::Unsupported(UnsupportedReason::NotPresent))
    }

    fn apply(&self, change: &SystemChange) -> Result<(), AdapterError> {
        *self.applies.lock().unwrap() += 1;
        if self.fail_apply.lock().unwrap().contains(&key(change)) {
            return Err(AdapterError::Failed);
        }
        self.set(change, Self::target(change));
        Ok(())
    }
}

/// Fake helper that applies against the same FakeSystem, emulating the
/// elevated re-read and per-item outcomes.
struct FakeHelper {
    system: Arc<FakeSystem>,
    response: Mutex<Option<Result<(), HelperClientError>>>,
    fail_items: Mutex<Vec<String>>,
    calls: Mutex<u32>,
    truncate_to: Mutex<Option<usize>>,
    /// Runs at the start of `apply`, while `execute` is mid-plan.
    on_apply: Mutex<Option<Box<dyn Fn() + Send>>>,
}

impl FakeHelper {
    fn new(system: Arc<FakeSystem>) -> Self {
        Self {
            system,
            response: Mutex::new(None),
            fail_items: Mutex::default(),
            calls: Mutex::new(0),
            truncate_to: Mutex::new(None),
            on_apply: Mutex::new(None),
        }
    }
}

impl HelperClient for FakeHelper {
    fn apply(
        &self,
        items: &[(SystemChange, PriorState)],
    ) -> Result<Vec<HelperItemResult>, HelperClientError> {
        *self.calls.lock().unwrap() += 1;
        if let Some(hook) = &*self.on_apply.lock().unwrap() {
            hook();
        }
        if let Some(Err(error)) = *self.response.lock().unwrap() {
            return Err(error);
        }
        let mut results = Vec::new();
        for (change, expected) in items {
            let prior = self.system.get(change).unwrap_or(PriorState::NotApplicable);
            let outcome = if change.is_satisfied_by(&prior) == Some(true) {
                ChangeOutcome::AlreadyApplied
            } else if &prior != expected {
                ChangeOutcome::StateChanged
            } else if self.fail_items.lock().unwrap().contains(&key(change)) {
                ChangeOutcome::Failed {
                    code: FailureCode::SystemError,
                }
            } else {
                self.system.set(change, FakeSystem::target(change));
                ChangeOutcome::Applied
            };
            results.push(HelperItemResult { prior, outcome });
        }
        if let Some(limit) = *self.truncate_to.lock().unwrap() {
            results.truncate(limit);
        }
        Ok(results)
    }
}

struct Harness {
    system: Arc<FakeSystem>,
    helper: Arc<FakeHelper>,
    journal: Arc<MemoryJournalStore>,
    service: SystemChangeService,
}

fn harness_with(journal: MemoryJournalStore) -> Harness {
    harness_on(supported_os(), journal)
}

fn harness_on(os: OsFacts, journal: MemoryJournalStore) -> Harness {
    let system = Arc::new(FakeSystem::default());
    let helper = Arc::new(FakeHelper::new(Arc::clone(&system)));
    let journal = Arc::new(journal);
    let service = SystemChangeService::new(os, helper.clone(), journal.clone())
        .with_adapter(SystemModule::Startup, system.clone())
        .with_adapter(SystemModule::Services, system.clone())
        .with_adapter(SystemModule::Drivers, system.clone());
    Harness {
        system,
        helper,
        journal,
        service,
    }
}

fn harness() -> Harness {
    harness_with(MemoryJournalStore::default())
}

fn run(h: &Harness, changes: Vec<SystemChange>) -> ExecutionReport {
    let ticket = h.service.create_plan(changes).unwrap();
    h.service.confirm_with(&ticket.plan_id, |_| true).unwrap();
    h.service.execute(&ticket.plan_id).unwrap()
}

#[test]
fn builds_below_the_supported_floor_reject_preview_and_plans() {
    let h = harness_on(
        OsFacts::fixture(19044, Edition::Pro, false),
        MemoryJournalStore::default(),
    );
    let change = user_startup("App", false);
    h.system.set(&change, PriorState::Enabled { enabled: true });
    assert_eq!(
        h.service.preview(change.clone()).unwrap_err(),
        SystemChangeError::Unsupported
    );
    assert_eq!(
        h.service.create_plan(vec![change]).unwrap_err(),
        SystemChangeError::Unsupported
    );
    assert_eq!(*h.system.applies.lock().unwrap(), 0);
}

#[test]
fn every_supported_build_accepts_preview_and_plans() {
    for build in SUPPORTED_BUILDS {
        for edition in [Edition::Home, Edition::Pro] {
            let h = harness_on(
                OsFacts::fixture(build, edition, false),
                MemoryJournalStore::default(),
            );
            let change = user_startup("App", false);
            h.system.set(&change, PriorState::Enabled { enabled: true });
            assert!(h.service.preview(change.clone()).is_ok(), "{build}");
            assert!(h.service.create_plan(vec![change]).is_ok(), "{build}");
        }
    }
}

#[test]
fn plan_is_single_use_and_requires_native_confirmation() {
    let h = harness();
    let change = user_startup("App", false);
    h.system.set(&change, PriorState::Enabled { enabled: true });
    let ticket = h.service.create_plan(vec![change]).unwrap();
    assert!(!ticket.requires_helper);
    assert_eq!(
        h.service.execute(&ticket.plan_id).unwrap_err(),
        SystemChangeError::NotConfirmed
    );
    h.service.confirm_with(&ticket.plan_id, |_| true).unwrap();
    assert!(h.service.execute(&ticket.plan_id).unwrap().all_succeeded());
    assert_eq!(
        h.service.execute(&ticket.plan_id).unwrap_err(),
        SystemChangeError::PlanNotFound
    );
}

#[test]
fn declined_confirmation_discards_the_plan_without_changes() {
    let h = harness();
    let change = user_startup("App", false);
    h.system.set(&change, PriorState::Enabled { enabled: true });
    let ticket = h.service.create_plan(vec![change.clone()]).unwrap();
    assert_eq!(
        h.service
            .confirm_with(&ticket.plan_id, |_| false)
            .unwrap_err(),
        SystemChangeError::ConfirmationDeclined
    );
    assert_eq!(
        h.service.execute(&ticket.plan_id).unwrap_err(),
        SystemChangeError::PlanNotFound
    );
    assert_eq!(*h.system.applies.lock().unwrap(), 0);
    assert_eq!(
        h.system.get(&change),
        Some(PriorState::Enabled { enabled: true })
    );
}

#[test]
fn confirmation_lists_every_change_with_reversibility_and_escapes_names() {
    let h = harness();
    let normal = user_startup("App", false);
    let hostile = user_startup("Evil\u{202e}name", false);
    let driver = SystemChange::DeleteDriverPackage {
        published_name: DriverPackageName::parse("oem12.inf").unwrap(),
    };
    h.system.set(&normal, PriorState::Enabled { enabled: true });
    h.system
        .set(&hostile, PriorState::Enabled { enabled: true });
    h.system
        .set(&driver, PriorState::DriverPackage { present: true });
    let ticket = h
        .service
        .create_plan(vec![normal, hostile, driver])
        .unwrap();
    let mut shown = String::new();
    h.service
        .confirm_with(&ticket.plan_id, |message| {
            shown = message.to_owned();
            true
        })
        .unwrap();
    assert!(shown.contains("Apply 3 system change(s)?"));
    assert!(shown.contains("1. ") && shown.contains("2. ") && shown.contains("3. "));
    assert!(shown.contains("CANNOT be undone"));
    assert!(shown.contains("administrator approval once"));
}

#[test]
fn plans_expire_after_sixty_seconds() {
    let h = harness();
    let change = user_startup("App", false);
    h.system.set(&change, PriorState::Enabled { enabled: true });
    let unconfirmed = h.service.create_plan(vec![change.clone()]).unwrap();
    let confirmed = h.service.create_plan(vec![change]).unwrap();
    h.service
        .confirm_with(&confirmed.plan_id, |_| true)
        .unwrap();
    h.service.advance(PLAN_LIFETIME);
    assert_eq!(
        h.service
            .confirm_with(&unconfirmed.plan_id, |_| true)
            .unwrap_err(),
        SystemChangeError::PlanExpired
    );
    assert_eq!(
        h.service.execute(&confirmed.plan_id).unwrap_err(),
        SystemChangeError::PlanExpired
    );
    assert_eq!(*h.system.applies.lock().unwrap(), 0);
}

#[test]
fn plan_bounds_duplicates_and_unknown_modules_are_rejected() {
    let h = harness();
    assert_eq!(
        h.service.create_plan(vec![]).unwrap_err(),
        SystemChangeError::EmptyPlan
    );
    let many: Vec<_> = (0..33)
        .map(|i| user_startup(&format!("App{i}"), false))
        .collect();
    assert_eq!(
        h.service.create_plan(many).unwrap_err(),
        SystemChangeError::TooManyChanges
    );
    let change = user_startup("App", false);
    h.system.set(&change, PriorState::Enabled { enabled: true });
    assert_eq!(
        h.service
            .create_plan(vec![change.clone(), change])
            .unwrap_err(),
        SystemChangeError::DuplicateChange
    );
    let firewall = SystemChange::SetHibernation { enabled: false };
    assert_eq!(
        h.service.create_plan(vec![firewall]).unwrap_err(),
        SystemChangeError::UnknownModule
    );
}

#[test]
fn unavailable_api_is_reported_at_preview() {
    let h = harness();
    let change = user_startup("App", false);
    h.system.unsupported.lock().unwrap().push(key(&change));
    assert_eq!(
        h.service.preview(change).unwrap_err(),
        SystemChangeError::Unsupported
    );
}

#[test]
fn stale_prior_state_is_not_overwritten() {
    let h = harness();
    let change = service("diagtrack", ServiceStartType::Disabled);
    h.system.set(
        &change,
        PriorState::ServiceStart {
            start: ServiceStartState::Automatic,
        },
    );
    let standard = user_startup("App", false);
    h.system
        .set(&standard, PriorState::Enabled { enabled: true });
    let ticket = h
        .service
        .create_plan(vec![change.clone(), standard.clone()])
        .unwrap();
    // Something else changes both between preview and execution.
    h.system.set(
        &change,
        PriorState::ServiceStart {
            start: ServiceStartState::Manual,
        },
    );
    h.system.set(&standard, PriorState::NotApplicable);
    h.service.confirm_with(&ticket.plan_id, |_| true).unwrap();
    let report = h.service.execute(&ticket.plan_id).unwrap();
    assert_eq!(report.results[0].outcome, ChangeOutcome::StateChanged);
    assert_eq!(report.results[1].outcome, ChangeOutcome::StateChanged);
    assert_eq!(
        h.system.get(&change),
        Some(PriorState::ServiceStart {
            start: ServiceStartState::Manual
        })
    );
    assert_eq!(*h.system.applies.lock().unwrap(), 0);
}

#[test]
fn reapplying_a_satisfied_change_is_idempotent() {
    let h = harness();
    let change = user_startup("App", false);
    h.system.set(&change, PriorState::Enabled { enabled: true });
    let first = h.service.create_plan(vec![change.clone()]).unwrap();
    let second = h.service.create_plan(vec![change.clone()]).unwrap();
    for ticket in [&first, &second] {
        h.service.confirm_with(&ticket.plan_id, |_| true).unwrap();
    }
    assert_eq!(
        h.service.execute(&first.plan_id).unwrap().results[0].outcome,
        ChangeOutcome::Applied
    );
    let again = h.service.execute(&second.plan_id).unwrap();
    assert_eq!(again.results[0].outcome, ChangeOutcome::AlreadyApplied);
    assert!(again.results[0].journal_entry_id.is_none());
    assert_eq!(*h.system.applies.lock().unwrap(), 1);
}

#[test]
fn partial_failure_reports_each_result_and_continues() {
    let h = harness();
    let a = user_startup("A", false);
    let b = user_startup("B", false);
    let c = user_startup("C", false);
    let s1 = service("diagtrack", ServiceStartType::Disabled);
    let s2 = service("dmwappushservice", ServiceStartType::Disabled);
    for change in [&a, &b, &c] {
        h.system.set(change, PriorState::Enabled { enabled: true });
    }
    for change in [&s1, &s2] {
        h.system.set(
            change,
            PriorState::ServiceStart {
                start: ServiceStartState::Automatic,
            },
        );
    }
    h.system.fail_apply.lock().unwrap().push(key(&b));
    h.helper.fail_items.lock().unwrap().push(key(&s1));
    let report = run(
        &h,
        vec![a.clone(), s1.clone(), b.clone(), s2.clone(), c.clone()],
    );
    let outcomes: Vec<_> = report.results.iter().map(|result| result.outcome).collect();
    assert_eq!(
        outcomes,
        vec![
            ChangeOutcome::Applied,
            ChangeOutcome::Failed {
                code: FailureCode::SystemError
            },
            ChangeOutcome::Failed {
                code: FailureCode::SystemError
            },
            ChangeOutcome::Applied,
            ChangeOutcome::Applied,
        ]
    );
    assert!(!report.all_succeeded());
    assert_eq!(
        *h.helper.calls.lock().unwrap(),
        1,
        "one UAC prompt per plan"
    );
    let journal = h.journal.snapshot();
    assert_eq!(journal.entries.len(), 5);
    assert!(journal.interrupted().next().is_none());
}

#[test]
fn privilege_denial_changes_nothing_and_is_journaled() {
    let h = harness();
    let change = service("diagtrack", ServiceStartType::Disabled);
    h.system.set(
        &change,
        PriorState::ServiceStart {
            start: ServiceStartState::Automatic,
        },
    );
    *h.helper.response.lock().unwrap() = Some(Err(HelperClientError::Denied));
    let report = run(&h, vec![change.clone()]);
    assert_eq!(report.results[0].outcome, ChangeOutcome::Denied);
    assert_eq!(
        h.system.get(&change),
        Some(PriorState::ServiceStart {
            start: ServiceStartState::Automatic
        })
    );
    let entry = &h.journal.snapshot().entries[0];
    assert_eq!(entry.outcome, Some(ChangeOutcome::Denied));
    assert_eq!(entry.rollback_status(), RollbackStatus::NothingApplied);
}

#[test]
fn indeterminate_helper_failure_leaves_intent_for_recovery() {
    let h = harness();
    let change = service("diagtrack", ServiceStartType::Disabled);
    h.system.set(
        &change,
        PriorState::ServiceStart {
            start: ServiceStartState::Automatic,
        },
    );
    *h.helper.response.lock().unwrap() = Some(Err(HelperClientError::Indeterminate));
    let report = run(&h, vec![change.clone()]);
    assert_eq!(
        report.results[0].outcome,
        ChangeOutcome::Failed {
            code: FailureCode::Interrupted
        }
    );
    assert_eq!(h.service.interrupted().unwrap().len(), 1);
    // The helper actually applied it before the connection dropped.
    h.system.set(
        &change,
        PriorState::ServiceStart {
            start: ServiceStartState::Disabled,
        },
    );
    assert_eq!(h.service.reconcile_interrupted().unwrap(), 1);
    let views = h.service.journal().unwrap();
    assert_eq!(views[0].entry.outcome, Some(ChangeOutcome::Applied));
    assert_eq!(views[0].rollback, RollbackStatus::Available);
}

#[test]
fn reconcile_during_execution_leaves_in_flight_intents_alone() {
    let system = Arc::new(FakeSystem::default());
    let helper = Arc::new(FakeHelper::new(Arc::clone(&system)));
    let journal = Arc::new(MemoryJournalStore::default());
    let svc = Arc::new(
        SystemChangeService::new(supported_os(), helper.clone(), journal.clone())
            .with_adapter(SystemModule::Startup, system.clone())
            .with_adapter(SystemModule::Services, system.clone()),
    );
    let standard = user_startup("App", false);
    let privileged = service("diagtrack", ServiceStartType::Disabled);
    system.set(&standard, PriorState::Enabled { enabled: true });
    system.set(
        &privileged,
        PriorState::ServiceStart {
            start: ServiceStartState::Automatic,
        },
    );

    // A page mounting the Change history while the helper batch runs.
    let seen = Arc::new(Mutex::new(None));
    {
        let svc = Arc::downgrade(&svc);
        let journal = Arc::clone(&journal);
        let seen = Arc::clone(&seen);
        *helper.on_apply.lock().unwrap() = Some(Box::new(move || {
            let reconciled = svc.upgrade().unwrap().reconcile_interrupted();
            *seen.lock().unwrap() = Some((reconciled, journal.snapshot()));
        }));
    }

    let ticket = svc
        .create_plan(vec![standard.clone(), privileged.clone()])
        .unwrap();
    svc.confirm_with(&ticket.plan_id, |_| true).unwrap();
    let report = svc.execute(&ticket.plan_id).unwrap();
    assert!(report.all_succeeded());

    let (reconciled, during) = seen.lock().unwrap().take().expect("helper ran");
    assert_eq!(reconciled, Ok(0));
    let helper_id = report.results[1].journal_entry_id.clone().unwrap();
    let in_flight = during.find(&helper_id).unwrap();
    assert_eq!(in_flight.outcome, None, "intent must stay open");
    assert_eq!(during.interrupted().count(), 1);

    let after = journal.snapshot();
    assert!(after.interrupted().next().is_none());
    assert_eq!(
        after.find(&helper_id).unwrap().outcome,
        Some(ChangeOutcome::Applied)
    );
    assert_eq!(
        after
            .find(report.results[0].journal_entry_id.as_ref().unwrap())
            .unwrap()
            .outcome,
        Some(ChangeOutcome::Applied)
    );
    let views = svc.journal().unwrap();
    assert!(
        views
            .iter()
            .all(|view| view.rollback == RollbackStatus::Available)
    );
}

#[test]
fn truncated_helper_response_marks_missing_items_interrupted() {
    let h = harness();
    let s1 = service("diagtrack", ServiceStartType::Disabled);
    let s2 = service("dmwappushservice", ServiceStartType::Disabled);
    for change in [&s1, &s2] {
        h.system.set(
            change,
            PriorState::ServiceStart {
                start: ServiceStartState::Automatic,
            },
        );
    }
    *h.helper.truncate_to.lock().unwrap() = Some(1);
    let report = run(&h, vec![s1, s2]);
    assert_eq!(report.results[0].outcome, ChangeOutcome::Applied);
    assert_eq!(
        report.results[1].outcome,
        ChangeOutcome::Failed {
            code: FailureCode::Interrupted
        }
    );
    assert_eq!(h.service.interrupted().unwrap().len(), 1);
}

#[test]
fn journal_failure_blocks_changes_before_they_run() {
    let h = harness_with(MemoryJournalStore::failing_after(0));
    let standard = user_startup("App", false);
    let privileged = service("diagtrack", ServiceStartType::Disabled);
    h.system
        .set(&standard, PriorState::Enabled { enabled: true });
    h.system.set(
        &privileged,
        PriorState::ServiceStart {
            start: ServiceStartState::Automatic,
        },
    );
    let report = run(&h, vec![standard, privileged]);
    assert!(
        report
            .results
            .iter()
            .all(|result| !result.outcome.modified_system())
    );
    assert_eq!(*h.system.applies.lock().unwrap(), 0);
    assert_eq!(*h.helper.calls.lock().unwrap(), 0);
}

#[test]
fn rollback_restores_recorded_prior_state_for_applied_entries_only() {
    let h = harness();
    let applied = user_startup("A", false);
    let failed = user_startup("B", false);
    let svc = service("diagtrack", ServiceStartType::Disabled);
    h.system
        .set(&applied, PriorState::Enabled { enabled: true });
    h.system.set(&failed, PriorState::Enabled { enabled: true });
    h.system.set(
        &svc,
        PriorState::ServiceStart {
            start: ServiceStartState::Manual,
        },
    );
    h.system.fail_apply.lock().unwrap().push(key(&failed));
    let report = run(&h, vec![applied.clone(), failed.clone(), svc.clone()]);
    let ids: Vec<_> = report
        .results
        .iter()
        .map(|result| result.journal_entry_id.clone().unwrap())
        .collect();

    assert_eq!(
        h.service
            .create_rollback_plan(&[ids[1].clone()])
            .unwrap_err(),
        SystemChangeError::RollbackUnavailable
    );
    let ticket = h
        .service
        .create_rollback_plan(&[ids[0].clone(), ids[2].clone()])
        .unwrap();
    assert_eq!(ticket.changes[0].change, user_startup("A", true));
    assert_eq!(
        ticket.changes[1].change,
        service("diagtrack", ServiceStartType::Manual)
    );
    h.service.confirm_with(&ticket.plan_id, |_| true).unwrap();
    assert!(h.service.execute(&ticket.plan_id).unwrap().all_succeeded());
    assert_eq!(
        h.system.get(&applied),
        Some(PriorState::Enabled { enabled: true })
    );
    assert_eq!(
        h.system.get(&svc),
        Some(PriorState::ServiceStart {
            start: ServiceStartState::Manual
        })
    );

    let journal = h.journal.snapshot();
    assert_eq!(
        journal.find(&ids[0]).unwrap().rollback_status(),
        RollbackStatus::AlreadyRolledBack
    );
    assert_eq!(
        h.service
            .create_rollback_plan(&[ids[0].clone()])
            .unwrap_err(),
        SystemChangeError::RollbackUnavailable
    );
}

#[test]
fn irreversible_changes_cannot_be_rolled_back() {
    let h = harness();
    let driver = SystemChange::DeleteDriverPackage {
        published_name: DriverPackageName::parse("oem7.inf").unwrap(),
    };
    h.system
        .set(&driver, PriorState::DriverPackage { present: true });
    let report = run(&h, vec![driver]);
    assert_eq!(report.results[0].outcome, ChangeOutcome::Applied);
    let id = report.results[0].journal_entry_id.clone().unwrap();
    assert_eq!(
        h.service.journal().unwrap()[0].rollback,
        RollbackStatus::Irreversible
    );
    assert_eq!(
        h.service.create_rollback_plan(&[id]).unwrap_err(),
        SystemChangeError::RollbackUnavailable
    );
}
