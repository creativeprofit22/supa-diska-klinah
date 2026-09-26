//! Shared preview → plan → native confirmation → execute → journal flow for
//! every system-management module.
//!
//! The webview submits typed [`SystemChange`] values (catalog identifiers and
//! bounded scalars only). The service re-describes and re-observes each one
//! through its module adapter, stores the resulting plan under an opaque,
//! single-use identifier that expires after 60 seconds, and executes only after
//! a native Windows dialog the webview cannot answer. Every change is journaled
//! as intent before it runs and outcome after it finishes.

mod broker_client;
pub mod elevated;
mod journal_store;

#[cfg(test)]
mod tests;

use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, Mutex, TryLockError},
    time::{Duration, Instant},
};

use cleanup_core::system_change::{
    ChangeOutcome, ChangeResult, ExecutionReport, FailureCode, ImpactSummary, Journal,
    JournalEntry, MAX_PLAN_CHANGES, PlannedChange, PriorState, Privilege, RollbackStatus,
    SystemChange, SystemModule, UnsupportedReason,
};
use serde::{Deserialize, Serialize};

use crate::i18n::NativeStrings;
use crate::os_info::OsFacts;

pub use broker_client::BrokerHelperClient;
/// The platform-neutral contract, re-exported for the app's IPC layer.
pub use cleanup_core::system_change as contract;
pub use journal_store::{FileJournalStore, JournalStore, MemoryJournalStore};

pub const PLAN_LIFETIME: Duration = Duration::from_secs(60);

/// Untyped JSON value, re-exported for IPC tests that inspect responses.
pub use serde_json::Value as JsonValue;

/// Decode one IPC request body (size and shape are checked by the caller).
pub fn decode_request<T: serde::de::DeserializeOwned>(
    bytes: &[u8],
) -> Result<T, SystemChangeError> {
    serde_json::from_slice(bytes).map_err(|_| SystemChangeError::InvalidChange)
}

/// Failure of an adapter read or write, mapped to bounded outcomes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdapterError {
    Unsupported(UnsupportedReason),
    Denied,
    Failed,
}

impl AdapterError {
    fn outcome(self) -> ChangeOutcome {
        match self {
            Self::Unsupported(reason) => ChangeOutcome::Unsupported { reason },
            Self::Denied => ChangeOutcome::Denied,
            Self::Failed => ChangeOutcome::Failed {
                code: FailureCode::SystemError,
            },
        }
    }
}

/// One module's view of the system. Implementations must resolve every
/// identifier against their compiled catalog or current enumeration and fail
/// closed for anything unknown.
pub trait SystemAdapter: Send + Sync {
    fn describe(&self, change: &SystemChange) -> Result<ImpactSummary, AdapterError>;
    fn observe(&self, change: &SystemChange) -> Result<PriorState, AdapterError>;
    /// Apply a standard-integrity change. Helper-privileged changes never
    /// reach this method.
    fn apply(&self, _change: &SystemChange) -> Result<(), AdapterError> {
        Err(AdapterError::Failed)
    }
    /// Decide satisfaction for changes the core contract cannot (hosts,
    /// restore points).
    fn is_satisfied(&self, _change: &SystemChange, _state: &PriorState) -> bool {
        false
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HelperItemResult {
    pub prior: PriorState,
    pub outcome: ChangeOutcome,
}

/// Failure of the whole helper exchange.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HelperClientError {
    /// UAC was cancelled/denied or the helper was not elevated: nothing ran.
    Denied,
    /// The request never reached the helper: nothing ran.
    NotStarted(FailureCode),
    /// The request may have reached the helper; results are unknown.
    Indeterminate,
}

pub trait HelperClient: Send + Sync {
    fn apply(
        &self,
        items: &[(SystemChange, PriorState)],
    ) -> Result<Vec<HelperItemResult>, HelperClientError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SystemChangeError {
    EmptyPlan,
    TooManyChanges,
    DuplicateChange,
    UnknownModule,
    Unsupported,
    InvalidChange,
    ObservationFailed,
    PlanNotFound,
    PlanExpired,
    NotConfirmed,
    ConfirmationDeclined,
    WindowUnavailable,
    JournalUnavailable,
    RollbackUnavailable,
    Busy,
}

impl fmt::Display for SystemChangeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::EmptyPlan => "a plan needs at least one change",
            Self::TooManyChanges => "a plan may contain at most 32 changes",
            Self::DuplicateChange => "a plan cannot repeat a change",
            Self::UnknownModule => "no adapter handles this change",
            Self::Unsupported => "this change is not supported on this system",
            Self::InvalidChange => "this change is invalid",
            Self::ObservationFailed => "the current system state could not be read",
            Self::PlanNotFound => "the plan does not exist or was already used",
            Self::PlanExpired => "the plan expired; preview again",
            Self::NotConfirmed => "the plan has not been confirmed",
            Self::ConfirmationDeclined => "the plan was not confirmed",
            Self::WindowUnavailable => "the confirmation window is unavailable",
            Self::JournalUnavailable => "the change journal is unavailable",
            Self::RollbackUnavailable => "one or more journal entries cannot be rolled back",
            Self::Busy => "another system change is running",
        })
    }
}

impl std::error::Error for SystemChangeError {}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanTicket {
    pub plan_id: String,
    pub changes: Vec<PlannedChange>,
    pub requires_helper: bool,
    pub expires_in_seconds: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalView {
    pub entry: JournalEntry,
    pub rollback: RollbackStatus,
}

struct StoredPlan {
    changes: Vec<PlannedChange>,
    rollback_of: Vec<Option<String>>,
    created: Instant,
    confirmed_at: Option<Instant>,
}

pub struct SystemChangeService {
    adapters: HashMap<SystemModule, Arc<dyn SystemAdapter>>,
    /// Every preview (and so every plan and rollback) is refused below the
    /// supported build floor.
    os: OsFacts,
    helper: Arc<dyn HelperClient>,
    journal: Arc<dyn JournalStore>,
    plans: Mutex<HashMap<String, StoredPlan>>,
    /// Held by `execute` for the whole plan and by `reconcile_interrupted`.
    /// These are the only journal writers, so holding it serializes every
    /// journal load..save; readers (`journal`, `interrupted`,
    /// `create_rollback_plan`) do not need it.
    running: Mutex<()>,
    /// Test-only time skew added to `Instant::now()`.
    skew: Mutex<Duration>,
}

impl SystemChangeService {
    pub fn new(os: OsFacts, helper: Arc<dyn HelperClient>, journal: Arc<dyn JournalStore>) -> Self {
        Self {
            adapters: HashMap::new(),
            os,
            helper,
            journal,
            plans: Mutex::new(HashMap::new()),
            running: Mutex::new(()),
            skew: Mutex::new(Duration::ZERO),
        }
    }

    pub fn with_adapter(mut self, module: SystemModule, adapter: Arc<dyn SystemAdapter>) -> Self {
        self.adapters.insert(module, adapter);
        self
    }

    /// Production service: every module adapter, the elevated-helper broker,
    /// and the journal under `<app data>\system-changes`.
    pub fn windows(app_data: &std::path::Path) -> std::io::Result<Self> {
        use crate::{
            drivers::DriverAdapter, firewall::FirewallAdapter, hosts::HostsAdapter,
            power::PowerAdapter, privacy::PrivacyAdapter, restore::RestoreAdapter,
            scheduler::SchedulerAdapter, services::ServicesAdapter, startup_items::StartupAdapter,
            updates::UpdatesAdapter,
        };
        let journal = Arc::new(FileJournalStore::open(app_data)?);
        Ok(Self::new(
            crate::os_info::current(),
            Arc::new(BrokerHelperClient),
            journal,
        )
        .with_adapter(SystemModule::Startup, Arc::new(StartupAdapter::new()))
        .with_adapter(SystemModule::Services, Arc::new(ServicesAdapter::new()))
        .with_adapter(SystemModule::Drivers, Arc::new(DriverAdapter::windows()))
        .with_adapter(SystemModule::Firewall, Arc::new(FirewallAdapter::new()))
        .with_adapter(SystemModule::Hosts, Arc::new(HostsAdapter::new()))
        .with_adapter(SystemModule::Privacy, Arc::new(PrivacyAdapter::new()))
        .with_adapter(SystemModule::Power, Arc::new(PowerAdapter::new()))
        .with_adapter(SystemModule::Restore, Arc::new(RestoreAdapter::new()))
        .with_adapter(SystemModule::Updates, Arc::new(UpdatesAdapter::new()))
        .with_adapter(SystemModule::Scheduler, Arc::new(SchedulerAdapter::new())))
    }

    fn now(&self) -> Instant {
        Instant::now() + self.skew.lock().map(|skew| *skew).unwrap_or_default()
    }

    #[cfg(test)]
    fn advance(&self, by: Duration) {
        *self.skew.lock().unwrap() += by;
    }

    fn adapter(&self, module: SystemModule) -> Result<&Arc<dyn SystemAdapter>, SystemChangeError> {
        self.adapters
            .get(&module)
            .ok_or(SystemChangeError::UnknownModule)
    }

    /// Preview one change without storing anything.
    pub fn preview(&self, change: SystemChange) -> Result<PlannedChange, SystemChangeError> {
        if !self.os.is_supported() {
            return Err(SystemChangeError::Unsupported);
        }
        change
            .validate()
            .map_err(|_| SystemChangeError::InvalidChange)?;
        let adapter = self.adapter(change.module())?;
        let impact = adapter.describe(&change).map_err(map_preview_error)?;
        let prior = adapter.observe(&change).map_err(map_preview_error)?;
        PlannedChange::new(change, impact, prior).map_err(|_| SystemChangeError::InvalidChange)
    }

    pub fn create_plan(&self, changes: Vec<SystemChange>) -> Result<PlanTicket, SystemChangeError> {
        let rollback_of = vec![None; changes.len()];
        self.store_plan(changes, rollback_of)
    }

    /// Build a plan that restores the journaled prior state of each entry.
    pub fn create_rollback_plan(
        &self,
        entry_ids: &[String],
    ) -> Result<PlanTicket, SystemChangeError> {
        let journal = self
            .journal
            .load()
            .map_err(|_| SystemChangeError::JournalUnavailable)?;
        let mut changes = Vec::with_capacity(entry_ids.len());
        let mut rollback_of = Vec::with_capacity(entry_ids.len());
        for id in entry_ids {
            let entry = journal
                .find(id)
                .ok_or(SystemChangeError::RollbackUnavailable)?;
            if entry.rollback_status() != RollbackStatus::Available {
                return Err(SystemChangeError::RollbackUnavailable);
            }
            changes.push(
                entry
                    .inverse
                    .clone()
                    .ok_or(SystemChangeError::RollbackUnavailable)?,
            );
            rollback_of.push(Some(id.clone()));
        }
        self.store_plan(changes, rollback_of)
    }

    fn store_plan(
        &self,
        changes: Vec<SystemChange>,
        rollback_of: Vec<Option<String>>,
    ) -> Result<PlanTicket, SystemChangeError> {
        if changes.is_empty() {
            return Err(SystemChangeError::EmptyPlan);
        }
        if changes.len() > MAX_PLAN_CHANGES {
            return Err(SystemChangeError::TooManyChanges);
        }
        for (index, change) in changes.iter().enumerate() {
            if changes[..index].contains(change) {
                return Err(SystemChangeError::DuplicateChange);
            }
        }
        let planned = changes
            .into_iter()
            .map(|change| self.preview(change))
            .collect::<Result<Vec<_>, _>>()?;
        let helper_items: Vec<_> = planned
            .iter()
            .filter(|change| change.privilege == Privilege::Helper)
            .map(|change| (change.change.clone(), change.expected_prior.clone()))
            .collect();
        if !helper_items.is_empty()
            && !broker_client::to_helper_items(&helper_items)
                .is_ok_and(|items| crate::security::system_changes::batch_fits_frame(&items))
        {
            return Err(SystemChangeError::TooManyChanges);
        }
        let plan_id = random_id().map_err(|_| SystemChangeError::JournalUnavailable)?;
        let now = self.now();
        let mut plans = self.plans.lock().map_err(|_| SystemChangeError::Busy)?;
        plans.retain(|_, plan| now.duration_since(plan.created) < PLAN_LIFETIME);
        plans.insert(
            plan_id.clone(),
            StoredPlan {
                changes: planned.clone(),
                rollback_of,
                created: now,
                confirmed_at: None,
            },
        );
        Ok(PlanTicket {
            requires_helper: planned
                .iter()
                .any(|change| change.privilege == Privilege::Helper),
            plan_id,
            changes: planned,
            expires_in_seconds: PLAN_LIFETIME.as_secs(),
        })
    }

    /// Show a native Windows confirmation dialog owned by `owner`. The
    /// webview has no way to answer it.
    pub fn confirm_native(&self, plan_id: &str, owner: isize) -> Result<(), SystemChangeError> {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            IDYES, IsWindow, MB_DEFBUTTON2, MB_ICONWARNING, MB_YESNO, MessageBoxW,
        };
        // SAFETY: IsWindow accepts any handle value and only reports validity.
        if owner == 0 || unsafe { IsWindow(owner as _) } == 0 {
            return Err(SystemChangeError::WindowUnavailable);
        }
        let strings = crate::i18n::native();
        self.confirm_with_strings(plan_id, strings, |message| {
            let text: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
            let title: Vec<u16> = strings
                .confirm_system_changes_title
                .encode_utf16()
                .chain(Some(0))
                .collect();
            // SAFETY: both strings are NUL-terminated and outlive the modal call.
            unsafe {
                MessageBoxW(
                    owner as _,
                    text.as_ptr(),
                    title.as_ptr(),
                    MB_YESNO | MB_DEFBUTTON2 | MB_ICONWARNING,
                ) == IDYES
            }
        })
    }

    pub fn confirm_with(
        &self,
        plan_id: &str,
        prompt: impl FnOnce(&str) -> bool,
    ) -> Result<(), SystemChangeError> {
        self.confirm_with_strings(plan_id, crate::i18n::native(), prompt)
    }

    /// `confirm_with` in an explicit language.
    pub fn confirm_with_strings(
        &self,
        plan_id: &str,
        strings: &NativeStrings,
        prompt: impl FnOnce(&str) -> bool,
    ) -> Result<(), SystemChangeError> {
        let message = {
            let plans = self.plans.lock().map_err(|_| SystemChangeError::Busy)?;
            let plan = plans.get(plan_id).ok_or(SystemChangeError::PlanNotFound)?;
            if self.now().duration_since(plan.created) >= PLAN_LIFETIME {
                return Err(SystemChangeError::PlanExpired);
            }
            confirmation_message(strings, &plan.changes)
        };
        if !prompt(&message) {
            self.plans
                .lock()
                .map_err(|_| SystemChangeError::Busy)?
                .remove(plan_id);
            return Err(SystemChangeError::ConfirmationDeclined);
        }
        let mut plans = self.plans.lock().map_err(|_| SystemChangeError::Busy)?;
        let plan = plans
            .get_mut(plan_id)
            .ok_or(SystemChangeError::PlanNotFound)?;
        plan.confirmed_at = Some(self.now());
        Ok(())
    }

    /// Execute a confirmed plan exactly once. Returns one result per change,
    /// in plan order; partial failure is reported, never hidden.
    pub fn execute(&self, plan_id: &str) -> Result<ExecutionReport, SystemChangeError> {
        let _running = self
            .running
            .try_lock()
            .map_err(|_| SystemChangeError::Busy)?;
        let plan = {
            let mut plans = self.plans.lock().map_err(|_| SystemChangeError::Busy)?;
            let plan = plans.get(plan_id).ok_or(SystemChangeError::PlanNotFound)?;
            let confirmed = plan.confirmed_at.ok_or(SystemChangeError::NotConfirmed)?;
            if self.now().duration_since(confirmed) >= PLAN_LIFETIME {
                plans.remove(plan_id);
                return Err(SystemChangeError::PlanExpired);
            }
            plans
                .remove(plan_id)
                .ok_or(SystemChangeError::PlanNotFound)?
        };
        let mut journal = self
            .journal
            .load()
            .map_err(|_| SystemChangeError::JournalUnavailable)?;
        let mut results: Vec<Option<ChangeResult>> = vec![None; plan.changes.len()];
        let helper_indices: Vec<usize> = plan
            .changes
            .iter()
            .enumerate()
            .filter(|(_, planned)| planned.privilege == Privilege::Helper)
            .map(|(index, _)| index)
            .collect();
        let mut helper_done = false;
        for (index, planned) in plan.changes.iter().enumerate() {
            if planned.privilege == Privilege::Helper {
                if !helper_done {
                    helper_done = true;
                    self.execute_helper_batch(
                        plan_id,
                        &plan,
                        &helper_indices,
                        &mut journal,
                        &mut results,
                    );
                }
                continue;
            }
            results[index] = Some(self.execute_standard(plan_id, planned, &mut journal));
        }
        for (index, result) in results.iter().enumerate() {
            if let (Some(result), Some(Some(original))) = (result, plan.rollback_of.get(index))
                && result.outcome == ChangeOutcome::Applied
                && let Some(entry_id) = &result.journal_entry_id
            {
                journal.mark_rolled_back(original, entry_id);
            }
        }
        let _ = self.journal.save(&journal);
        Ok(ExecutionReport {
            plan_id: plan_id.to_owned(),
            results: results
                .into_iter()
                .zip(&plan.changes)
                .map(|(result, planned)| {
                    result.unwrap_or_else(|| ChangeResult {
                        change: planned.change.clone(),
                        outcome: ChangeOutcome::Failed {
                            code: FailureCode::NotAttempted,
                        },
                        journal_entry_id: None,
                    })
                })
                .collect(),
        })
    }

    fn execute_standard(
        &self,
        plan_id: &str,
        planned: &PlannedChange,
        journal: &mut Journal,
    ) -> ChangeResult {
        let result = |outcome, journal_entry_id| ChangeResult {
            change: planned.change.clone(),
            outcome,
            journal_entry_id,
        };
        let Ok(adapter) = self.adapter(planned.module) else {
            return result(
                ChangeOutcome::Failed {
                    code: FailureCode::InvalidRequest,
                },
                None,
            );
        };
        let current = match adapter.observe(&planned.change) {
            Ok(current) => current,
            Err(error) => return result(error.outcome(), None),
        };
        if satisfied(adapter.as_ref(), &planned.change, &current) {
            return result(ChangeOutcome::AlreadyApplied, None);
        }
        if current != planned.expected_prior {
            return result(ChangeOutcome::StateChanged, None);
        }
        let Some(entry_id) = self.record_intent(plan_id, planned, current, journal) else {
            return result(
                ChangeOutcome::Failed {
                    code: FailureCode::SystemError,
                },
                None,
            );
        };
        let outcome = match adapter.apply(&planned.change) {
            Ok(()) => ChangeOutcome::Applied,
            Err(error) => error.outcome(),
        };
        journal.record_outcome(&entry_id, outcome, None);
        let _ = self.journal.save(journal);
        result(outcome, Some(entry_id))
    }

    fn execute_helper_batch(
        &self,
        plan_id: &str,
        plan: &StoredPlan,
        indices: &[usize],
        journal: &mut Journal,
        results: &mut [Option<ChangeResult>],
    ) {
        let mut entry_ids = Vec::with_capacity(indices.len());
        for &index in indices {
            let planned = &plan.changes[index];
            match self.record_intent(plan_id, planned, planned.expected_prior.clone(), journal) {
                Some(id) => entry_ids.push(id),
                None => {
                    // Never run privileged work without its intent record.
                    for (position, &index) in indices.iter().enumerate() {
                        if let Some(id) = entry_ids.get(position) {
                            journal.record_outcome(
                                id,
                                ChangeOutcome::Failed {
                                    code: FailureCode::NotAttempted,
                                },
                                None,
                            );
                        }
                        results[index] = Some(ChangeResult {
                            change: plan.changes[index].change.clone(),
                            outcome: ChangeOutcome::Failed {
                                code: FailureCode::SystemError,
                            },
                            journal_entry_id: entry_ids.get(position).cloned(),
                        });
                    }
                    let _ = self.journal.save(journal);
                    return;
                }
            }
        }
        let items: Vec<_> = indices
            .iter()
            .map(|&index| {
                (
                    plan.changes[index].change.clone(),
                    plan.changes[index].expected_prior.clone(),
                )
            })
            .collect();
        let response = self.helper.apply(&items);
        for (position, &index) in indices.iter().enumerate() {
            let entry_id = entry_ids[position].clone();
            let (outcome, prior, journal_outcome) = match &response {
                Ok(items) => match items.get(position) {
                    Some(item) => (item.outcome, Some(item.prior.clone()), Some(item.outcome)),
                    None => (
                        ChangeOutcome::Failed {
                            code: FailureCode::Interrupted,
                        },
                        None,
                        None,
                    ),
                },
                Err(HelperClientError::Denied) => {
                    (ChangeOutcome::Denied, None, Some(ChangeOutcome::Denied))
                }
                Err(HelperClientError::NotStarted(code)) => {
                    let outcome = ChangeOutcome::Failed { code: *code };
                    (outcome, None, Some(outcome))
                }
                // Unknown whether the helper wrote anything: leave the intent
                // record open so recovery surfaces it.
                Err(HelperClientError::Indeterminate) => (
                    ChangeOutcome::Failed {
                        code: FailureCode::Interrupted,
                    },
                    None,
                    None,
                ),
            };
            if let Some(journal_outcome) = journal_outcome {
                journal.record_outcome(&entry_id, journal_outcome, prior);
            }
            results[index] = Some(ChangeResult {
                change: plan.changes[index].change.clone(),
                outcome,
                journal_entry_id: Some(entry_id),
            });
        }
        let _ = self.journal.save(journal);
    }

    fn record_intent(
        &self,
        plan_id: &str,
        planned: &PlannedChange,
        prior: PriorState,
        journal: &mut Journal,
    ) -> Option<String> {
        let id = random_id().ok()?;
        journal.record_intent(JournalEntry {
            id: id.clone(),
            plan_id: plan_id.to_owned(),
            recorded_at: unix_now(),
            inverse: planned.change.inverse(&prior).ok().flatten(),
            reversibility: planned.reversibility.clone(),
            change: planned.change.clone(),
            prior,
            outcome: None,
            rolled_back_by: None,
        });
        // Fail closed: if the intent cannot be persisted, do not change the system.
        self.journal.save(journal).ok().map(|()| id)
    }

    pub fn journal(&self) -> Result<Vec<JournalView>, SystemChangeError> {
        let journal = self
            .journal
            .load()
            .map_err(|_| SystemChangeError::JournalUnavailable)?;
        Ok(journal
            .entries
            .into_iter()
            .rev()
            .map(|entry| JournalView {
                rollback: entry.rollback_status(),
                entry,
            })
            .collect())
    }

    /// Intent records without an outcome: changes that may or may not have
    /// happened because the process stopped mid-change.
    pub fn interrupted(&self) -> Result<Vec<JournalEntry>, SystemChangeError> {
        let journal = self
            .journal
            .load()
            .map_err(|_| SystemChangeError::JournalUnavailable)?;
        Ok(journal.interrupted().cloned().collect())
    }

    /// Resolve interrupted records by observing the system: if the target
    /// state holds, the change is recorded as applied (and becomes
    /// roll-back-able); otherwise as not attempted.
    ///
    /// While a plan is executing its open intent records are in flight, not
    /// interrupted, and `execute` owns the journal: return 0 untouched.
    pub fn reconcile_interrupted(&self) -> Result<usize, SystemChangeError> {
        let _running = match self.running.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::WouldBlock) => return Ok(0),
            // A previous execute panicked: nothing is running any more.
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
        };
        let mut journal = self
            .journal
            .load()
            .map_err(|_| SystemChangeError::JournalUnavailable)?;
        let pending: Vec<_> = journal.interrupted().cloned().collect();
        let mut resolved = 0;
        for entry in pending {
            let Ok(adapter) = self.adapter(entry.change.module()) else {
                continue;
            };
            let Ok(current) = adapter.observe(&entry.change) else {
                continue;
            };
            let outcome = if current == entry.prior {
                ChangeOutcome::Failed {
                    code: FailureCode::Interrupted,
                }
            } else if satisfied(adapter.as_ref(), &entry.change, &current) {
                ChangeOutcome::Applied
            } else {
                continue;
            };
            journal.record_outcome(&entry.id, outcome, None);
            resolved += 1;
        }
        self.journal
            .save(&journal)
            .map_err(|_| SystemChangeError::JournalUnavailable)?;
        Ok(resolved)
    }
}

fn satisfied(adapter: &dyn SystemAdapter, change: &SystemChange, state: &PriorState) -> bool {
    change
        .is_satisfied_by(state)
        .unwrap_or_else(|| adapter.is_satisfied(change, state))
}

fn map_preview_error(error: AdapterError) -> SystemChangeError {
    match error {
        AdapterError::Unsupported(_) => SystemChangeError::Unsupported,
        AdapterError::Denied | AdapterError::Failed => SystemChangeError::ObservationFailed,
    }
}

fn confirmation_message(strings: &NativeStrings, changes: &[PlannedChange]) -> String {
    let mut message = (strings.system_changes_intro)(changes.len());
    for (index, change) in changes.iter().enumerate() {
        // Debug-quote so control characters in names cannot forge extra lines.
        message.push_str(&format!(
            "{}. {:?}\n",
            index + 1,
            (strings.system_change_line)(change)
        ));
    }
    if changes
        .iter()
        .any(|change| change.privilege == Privilege::Helper)
    {
        message.push_str(strings.system_changes_admin_note);
    }
    if changes
        .iter()
        .any(|change| !change.reversibility.is_reversible())
    {
        message.push_str(strings.system_changes_irreversible_note);
    }
    message
}

pub(crate) fn random_id() -> std::io::Result<String> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|error| std::io::Error::other(error.to_string()))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
