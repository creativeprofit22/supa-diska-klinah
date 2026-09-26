//! Backend-owned, separately confirmed vendor jobs. No raw command input and no cleanup proofs.
use super::{
    uninstaller::{self, NativeRegistry, ProgramRecord, RegistryReader, RegistryValue, text},
    vendor_uninstall::{
        self, ExecutableEvidence, LaunchResult, NativeProcessBoundary, ProcessBoundary,
        VendorCommand,
    },
};
use crate::{
    cleanup::{CleanupStorage, StorageError},
    history::{HistoryCursor, HistoryKind, HistoryPage, HistoryRequest},
};
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

// simplification: one bounded ledger retains 1,000 outcomes; migrate to an indexed
// journal store for further growth, never evict outcomes to admit another job.
const MAX_JOURNALS: usize = 1_000;
// Keep the shared atomic writer's 8 MiB safeguard unchanged.
const MAX_LEDGER_BYTES: usize = 8 * 1024 * 1024;
// Per retained journal: state (<=24 bytes), meaning (<=64), two u32 codes
// (<=20), and updated_at (<=20) can all grow, including on reconciliation.
// Reserving 256 bytes per journal conservatively covers every such transition.
const TRANSITION_HEADROOM: usize = 256;
const EXPIRY: u64 = 600;
const WAIT_LIMIT: Duration = Duration::from_secs(30 * 60);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum VendorJobState {
    AwaitingConfirmation,
    Queued,
    Launching,
    Completed,
    CancelledByVendorOrUAC,
    CancelledBeforeLaunch,
    Failed,
    RebootRequired,
    OutcomeUnknown,
}
impl VendorJobState {
    fn active(&self) -> bool {
        matches!(
            self,
            Self::AwaitingConfirmation | Self::Queued | Self::Launching
        )
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VendorJob {
    pub job_id: String,
    pub program_id: String,
    pub program_name: String,
    pub state: VendorJobState,
    pub created_at: u64,
    pub updated_at: u64,
    pub exit_code: Option<u32>,
    pub launch_error: Option<u32>,
    pub persistence_error: bool,
    pub completion_meaning: String,
    pub leftover_support: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VendorJobError {
    Busy,
    InvalidId,
    Expired,
    RegistryChanged,
    UnsupportedCommand,
    ExecutableChanged,
    Storage,
    Limit,
    HistoryCountCapacity,
    HistorySizeCapacity,
    InvalidHistoryRequest,
    Conflict,
}

/// A registry path is a suggestion ONLY. Identity records what was observed, not ownership.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RootSuggestion {
    canonical_path: std::path::PathBuf,
    identity: cleanup_core::FileIdentity,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    job: VendorJob,
    program: ProgramRecord,
    // Prepared argv is transient. Restart records are never executable or replayed.
    #[serde(skip)]
    command: VendorCommand,
    executable: ExecutableEvidence,
    registry_stamp: [u8; 32],
    root_suggestion: Option<RootSuggestion>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Ledger {
    schema_version: u32,
    journals: Vec<Journal>,
}
struct Active {
    id: String,
    cancel: Arc<AtomicBool>,
    worker: bool,
}
struct State {
    ledger: Ledger,
    active: Option<Active>,
}
struct Shared {
    state: Mutex<State>,
    writer: Arc<Mutex<()>>,
    storage: CleanupStorage,
    registry: Arc<dyn RegistryReader>,
    process: Arc<dyn ProcessBoundary>,
    shutdown: AtomicBool,
    timeout: Duration,
}
pub struct VendorJobManager {
    shared: Arc<Shared>,
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())
}
fn stamp(
    reader: &dyn RegistryReader,
    program: &ProgramRecord,
) -> Result<Vec<Option<RegistryValue>>, VendorJobError> {
    let mut values = Vec::new();
    for name in [
        "DisplayName",
        "Publisher",
        "DisplayVersion",
        "InstallDate",
        "EstimatedSize",
        "WindowsInstaller",
        "UninstallString",
        "InstallLocation",
    ] {
        let value = reader
            .value(&program.location, name)
            .map_err(|_| VendorJobError::RegistryChanged)?;
        if value.as_ref().is_some_and(|v| v.bytes.len() > 32768) {
            return Err(VendorJobError::Limit);
        }
        values.push(value);
    }
    if text(values[0].clone()).ok().flatten().as_ref() != Some(&program.display.name)
        || text(values[1].clone()).ok().flatten() != program.display.publisher
        || text(values[2].clone()).ok().flatten() != program.display.version
    {
        return Err(VendorJobError::RegistryChanged);
    }
    Ok(values)
}
fn fingerprint(values: &[Option<RegistryValue>]) -> Result<[u8; 32], VendorJobError> {
    use sha2::{Digest, Sha256};
    let bytes = serde_json::to_vec(values).map_err(|_| VendorJobError::Storage)?;
    Ok(Sha256::digest(bytes).into())
}
fn root_suggestion(values: &[Option<RegistryValue>]) -> Option<RootSuggestion> {
    use cleanup_core::FileSystem;
    let raw = text(values.get(7)?.clone()).ok()??;
    let path = std::path::PathBuf::from(raw);
    if !cleanup_core::is_local_storage_path(&path) {
        return None;
    }
    let parent = path.parent()?;
    let validated = crate::security::path_policy::validate_contained(parent, &path).ok()?;
    let fs = crate::cleanup::WindowsFileSystem;
    let metadata = fs.metadata_no_follow(validated.as_path()).ok()?;
    if metadata.kind != cleanup_core::EntryKind::Directory {
        return None;
    }
    Some(RootSuggestion {
        canonical_path: validated.as_path().to_path_buf(),
        identity: metadata.identity?,
    })
}
impl VendorJobManager {
    pub(crate) fn new(
        storage: CleanupStorage,
        writer: Arc<Mutex<()>>,
    ) -> Result<Self, VendorJobError> {
        Self::with_boundaries(
            storage,
            writer,
            Arc::new(NativeRegistry),
            Arc::new(NativeProcessBoundary),
            WAIT_LIMIT,
        )
    }
    fn with_boundaries(
        storage: CleanupStorage,
        writer: Arc<Mutex<()>>,
        registry: Arc<dyn RegistryReader>,
        process: Arc<dyn ProcessBoundary>,
        timeout: Duration,
    ) -> Result<Self, VendorJobError> {
        let mut ledger: Ledger = storage
            .read_vendor_jobs()
            .map_err(|_| VendorJobError::Storage)?
            .unwrap_or(Ledger {
                schema_version: 1,
                journals: Vec::new(),
            });
        if ledger.schema_version != 1 || ledger.journals.len() > MAX_JOURNALS {
            return Err(VendorJobError::Storage);
        }
        let mut ids = std::collections::HashSet::new();
        for journal in &mut ledger.journals {
            if !valid_id(&journal.job.job_id)
                || !valid_id(&journal.job.program_id)
                || !ids.insert(journal.job.job_id.clone())
                || journal.program.location.subkey.len() > 1024
                || journal.program.location.subkey.contains(['\\', '\0'])
            {
                return Err(VendorJobError::Storage);
            }
            // Persisted commands are NEVER launchable after restart, even if confirmation existed.
            match journal.job.state {
                VendorJobState::Launching => {
                    journal.job.state = VendorJobState::OutcomeUnknown;
                    journal.job.completion_meaning =
                        "restartDuringVendorLaunchOrWaitNeverReplay".into();
                }
                VendorJobState::AwaitingConfirmation | VendorJobState::Queued => {
                    journal.job.state = VendorJobState::CancelledBeforeLaunch;
                }
                _ => {}
            }
        }
        {
            let _writer = writer.lock().map_err(|_| VendorJobError::Conflict)?;
            storage
                .write_vendor_jobs(&ledger)
                .map_err(persistence_error)?;
        }
        Ok(Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    ledger,
                    active: None,
                }),
                storage,
                writer,
                registry,
                process,
                shutdown: AtomicBool::new(false),
                timeout,
            }),
        })
    }
    pub fn start_inventory(
        &self,
        storage: &super::scans::StorageService,
        limits: cleanup_core::storage::StorageLimits,
        query: uninstaller::ProgramQuery,
    ) -> Result<String, super::scans::JobError> {
        storage.start_programs_with(limits, query, Arc::clone(&self.shared.registry))
    }
    /// Prepare is NOT confirmation. Only a program ID in a live immutable snapshot resolves.
    pub fn prepare(
        &self,
        storage: &super::scans::StorageService,
        snapshot_id: &str,
        program_id: &str,
    ) -> Result<VendorJob, VendorJobError> {
        if !valid_id(program_id) {
            return Err(VendorJobError::InvalidId);
        }
        let selected = storage
            .resolve_program(snapshot_id, program_id)
            .map_err(|_| VendorJobError::InvalidId)?;
        let reservation_id = super::opaque_id().map_err(|_| VendorJobError::Storage)?;
        let cancel = Arc::new(AtomicBool::new(false));
        let program = {
            let mut state = self
                .shared
                .state
                .lock()
                .map_err(|_| VendorJobError::Conflict)?;
            expire_prepared(&mut state);
            if state.active.is_some() {
                return Err(VendorJobError::Busy);
            }
            if state.ledger.journals.len() >= MAX_JOURNALS {
                return Err(VendorJobError::HistoryCountCapacity);
            }
            let program = selected;
            if state.ledger.journals.iter().any(|j| {
                j.program.location == program.location
                    && j.job.state == VendorJobState::OutcomeUnknown
            }) {
                return Err(VendorJobError::Busy);
            }
            state.active = Some(Active {
                id: reservation_id.clone(),
                cancel: Arc::clone(&cancel),
                worker: true,
            });
            program
        };
        let mut reservation = Reservation {
            shared: &self.shared,
            id: &reservation_id,
            committed: false,
        };
        // Native registry/hash work holds only file identity guards, never service/writer locks.
        let registry_stamp = stamp(self.shared.registry.as_ref(), &program)?;
        let command = vendor_uninstall::resolve(self.shared.registry.as_ref(), &program.location)
            .map_err(|_| VendorJobError::UnsupportedCommand)?;
        let executable = self.shared.process.validate(&command)?;
        if stamp(self.shared.registry.as_ref(), &program)? != registry_stamp {
            return Err(VendorJobError::RegistryChanged);
        }
        let job = VendorJob {
            job_id: reservation_id.clone(),
            program_id: program_id.into(),
            program_name: program.display.name.clone(),
            state: VendorJobState::AwaitingConfirmation,
            created_at: now(),
            updated_at: now(),
            exit_code: None,
            launch_error: None,
            persistence_error: false,
            completion_meaning: "vendorUninstallIsNotUndoableRequiresSeparateConfirmation".into(),
            leftover_support: "unsupportedUnknownOwnership".into(),
        };
        let journal = Journal {
            job: job.clone(),
            program,
            command,
            executable,
            root_suggestion: root_suggestion(&registry_stamp),
            registry_stamp: fingerprint(&registry_stamp)?,
        };
        let _writer = self
            .shared
            .writer
            .try_lock()
            .map_err(|_| VendorJobError::Busy)?;
        let mut state = self
            .shared
            .state
            .lock()
            .map_err(|_| VendorJobError::Conflict)?;
        if cancel.load(Ordering::Acquire)
            || self.shared.shutdown.load(Ordering::Acquire)
            || state.active.as_ref().is_none_or(|a| a.id != reservation_id)
        {
            return Err(VendorJobError::Conflict);
        }
        let mut ledger = state.ledger.clone();
        ledger.journals.push(journal);
        check_admission(&ledger)?;
        self.shared
            .storage
            .write_vendor_jobs(&ledger)
            .map_err(persistence_error)?;
        state.ledger = ledger;
        state.active.as_mut().unwrap().worker = false;
        reservation.committed = true;
        Ok(job)
    }
    /// Consent text is resolved by the backend, never supplied by the renderer.
    pub fn confirm_native(&self, job_id: &str, owner: isize) -> Result<VendorJob, VendorJobError> {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            IDYES, IsWindow, MB_DEFBUTTON2, MB_ICONWARNING, MB_YESNO, MessageBoxW,
        };
        if owner == 0 || unsafe { IsWindow(owner as _) } == 0 {
            return Err(VendorJobError::Conflict);
        }
        let strings = crate::i18n::native();
        self.confirm_with_strings(job_id, strings, |message| {
            let text: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
            let title: Vec<u16> = strings
                .confirm_vendor_uninstall_title
                .encode_utf16()
                .chain(Some(0))
                .collect();
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

    #[cfg(test)]
    fn confirm_with(
        &self,
        job_id: &str,
        prompt: impl FnOnce(&str) -> bool,
    ) -> Result<VendorJob, VendorJobError> {
        self.confirm_with_strings(job_id, crate::i18n::native(), prompt)
    }

    fn confirm_with_strings(
        &self,
        job_id: &str,
        strings: &crate::i18n::NativeStrings,
        prompt: impl FnOnce(&str) -> bool,
    ) -> Result<VendorJob, VendorJobError> {
        let journal = {
            let state = self
                .shared
                .state
                .lock()
                .map_err(|_| VendorJobError::Conflict)?;
            let journal = lookup(&state, job_id)?;
            if journal.job.state != VendorJobState::AwaitingConfirmation
                || !state.active.as_ref().is_some_and(|a| {
                    a.id == job_id && !a.worker && !a.cancel.load(Ordering::Acquire)
                })
            {
                return Err(VendorJobError::Conflict);
            }
            if now().saturating_sub(journal.job.created_at) >= EXPIRY {
                return Err(VendorJobError::Expired);
            }
            journal.clone()
        };
        fresh(&self.shared, &journal)?;
        // Debug quoting escapes controls, quotes and backslashes. Fail closed instead of
        // truncating the command the user is being asked to approve.
        let message = (strings.vendor_uninstall_body)(
            &journal.job.program_name,
            &journal.job.job_id,
            journal.command.msi,
            &journal.command.executable,
            &journal.command.arguments,
        );
        if message.encode_utf16().count() > 16384 {
            return Err(VendorJobError::Limit);
        }
        if prompt(&message) {
            self.confirm(job_id)
        } else {
            self.cancel(job_id)
        }
    }

    /// The only launch request is a separately confirmed, backend-issued job ID.
    pub fn confirm(&self, job_id: &str) -> Result<VendorJob, VendorJobError> {
        let (journal, cancel) = {
            let mut state = self
                .shared
                .state
                .lock()
                .map_err(|_| VendorJobError::Conflict)?;
            let journal = lookup(&state, job_id)?.clone();
            if journal.job.state != VendorJobState::AwaitingConfirmation {
                return Err(VendorJobError::Conflict);
            }
            if now().saturating_sub(journal.job.created_at) >= EXPIRY {
                return Err(VendorJobError::Expired);
            }
            let active = state
                .active
                .as_mut()
                .filter(|a| a.id == job_id && !a.worker && !a.cancel.load(Ordering::Acquire))
                .ok_or(VendorJobError::Conflict)?;
            active.worker = true;
            (journal, Arc::clone(&active.cancel))
        };
        let mut reservation = ConfirmationReservation {
            shared: &self.shared,
            id: job_id,
            committed: false,
        };
        fresh(&self.shared, &journal)?;
        let _writer = self
            .shared
            .writer
            .try_lock()
            .map_err(|_| VendorJobError::Busy)?;
        let mut state = self
            .shared
            .state
            .lock()
            .map_err(|_| VendorJobError::Conflict)?;
        if cancel.load(Ordering::Acquire)
            || self.shared.shutdown.load(Ordering::Acquire)
            || lookup(&state, job_id)?.job.state != VendorJobState::AwaitingConfirmation
        {
            return Err(VendorJobError::Conflict);
        }
        let mut queued = journal.job.clone();
        queued.state = VendorJobState::Queued;
        queued.updated_at = now();
        save_job(&self.shared, &mut state, queued.clone())?;
        state.active.as_mut().unwrap().worker = true;
        reservation.committed = true;
        let shared = Arc::clone(&self.shared);
        let id = job_id.to_owned();
        if std::thread::Builder::new()
            .name("vendor-uninstall-sta".into())
            .spawn(move || run(shared, id, cancel))
            .is_err()
        {
            queued.state = VendorJobState::Failed;
            queued.completion_meaning = "workerNotStarted".into();
            let result = save_job(&self.shared, &mut state, queued.clone());
            state.active = None;
            result?;
        }
        Ok(queued)
    }
    pub fn status(&self, job_id: &str) -> Result<VendorJob, VendorJobError> {
        let mut state = self
            .shared
            .state
            .lock()
            .map_err(|_| VendorJobError::Conflict)?;
        expire_prepared(&mut state);
        let mut job = lookup(&state, job_id)?.job.clone();
        if job.state == VendorJobState::Launching
            && now().saturating_sub(job.updated_at) >= self.shared.timeout.as_secs().max(1)
            && let Some(active) = state.active.as_ref().filter(|a| a.id == job_id)
        {
            active.cancel.store(true, Ordering::Release);
        }
        if state
            .active
            .as_ref()
            .is_some_and(|a| a.id == job_id && a.cancel.load(Ordering::Acquire))
            && job.state.active()
        {
            job.state = if job.state == VendorJobState::Launching {
                VendorJobState::OutcomeUnknown
            } else {
                VendorJobState::CancelledBeforeLaunch
            };
            job.completion_meaning = "waitingCancelledVendorNotTerminated".into();
        }
        Ok(job)
    }
    /// Cancellation is lock-free with respect to the global writer / possible vendor UAC prompt.
    pub fn cancel(&self, job_id: &str) -> Result<VendorJob, VendorJobError> {
        {
            let mut state = self
                .shared
                .state
                .lock()
                .map_err(|_| VendorJobError::Conflict)?;
            lookup(&state, job_id)?;
            if let Some(active) = state.active.as_mut().filter(|a| a.id == job_id) {
                active.cancel.store(true, Ordering::Release);
            }
        }
        self.status(job_id)
    }
    /// Unknown journals cannot be erased or replayed. Release only retires safe terminal UI state.
    pub fn release(&self, job_id: &str) -> Result<(), VendorJobError> {
        let _writer = self
            .shared
            .writer
            .try_lock()
            .map_err(|_| VendorJobError::Busy)?;
        let mut state = self
            .shared
            .state
            .lock()
            .map_err(|_| VendorJobError::Conflict)?;
        let journal = lookup(&state, job_id)?;
        if journal.job.state == VendorJobState::OutcomeUnknown
            || state
                .active
                .as_ref()
                .is_some_and(|a| a.id == job_id && a.worker)
        {
            return Err(VendorJobError::Busy);
        }
        let mut ledger = state.ledger.clone();
        let journal = ledger
            .journals
            .iter_mut()
            .find(|j| j.job.job_id == job_id)
            .unwrap();
        if journal.job.state.active() {
            journal.job.state = VendorJobState::CancelledBeforeLaunch;
            journal.job.updated_at = now();
        }
        journal.command = VendorCommand::default();
        self.shared
            .storage
            .write_vendor_jobs(&ledger)
            .map_err(persistence_error)?;
        state.ledger = ledger;
        if state.active.as_ref().is_some_and(|a| a.id == job_id) {
            state.active = None;
        }
        Ok(())
    }
    pub fn history_page(
        &self,
        request: HistoryRequest,
    ) -> Result<HistoryPage<VendorJob>, VendorJobError> {
        let (cursor, limit) = request
            .validate(HistoryKind::Vendor)
            .map_err(|_| VendorJobError::InvalidHistoryRequest)?;
        let mut state = self
            .shared
            .state
            .lock()
            .map_err(|_| VendorJobError::Conflict)?;
        expire_prepared(&mut state);
        // Keep only a bounded page (+ one lookahead), not a second ledger-sized collector.
        let mut selected: Vec<&VendorJob> = Vec::with_capacity(limit + 1);
        for journal in &state.ledger.journals {
            let job = &journal.job;
            let key = (job.created_at, job.job_id.as_str());
            if cursor
                .as_ref()
                .is_some_and(|c| key >= (c.timestamp, c.id.as_str()))
            {
                continue;
            }
            let index =
                selected.partition_point(|other| (other.created_at, other.job_id.as_str()) > key);
            if index <= limit {
                selected.insert(index, job);
                selected.truncate(limit + 1);
            }
        }
        let more = selected.len() > limit;
        selected.truncate(limit);
        let next_cursor = if more {
            let last = selected.last().unwrap();
            Some(
                HistoryCursor::encode(HistoryKind::Vendor, last.created_at, &last.job_id)
                    .map_err(|_| VendorJobError::InvalidHistoryRequest)?,
            )
        } else {
            None
        };
        Ok(HistoryPage {
            records: selected.into_iter().cloned().collect(),
            next_cursor,
        })
    }
}
impl Drop for VendorJobManager {
    fn drop(&mut self) {
        self.shared.shutdown.store(true, Ordering::Release);
        if let Ok(state) = self.shared.state.lock()
            && let Some(active) = &state.active
        {
            active.cancel.store(true, Ordering::Release);
        }
        // Never join an STA blocked inside vendor UI/UAC. The prelaunch journal remains uncertain.
    }
}
struct Reservation<'a> {
    shared: &'a Shared,
    id: &'a str,
    committed: bool,
}
impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        if !self.committed
            && let Ok(mut state) = self.shared.state.lock()
            && state.active.as_ref().is_some_and(|a| a.id == self.id)
        {
            state.active = None;
        }
    }
}
struct ConfirmationReservation<'a> {
    shared: &'a Shared,
    id: &'a str,
    committed: bool,
}
impl Drop for ConfirmationReservation<'_> {
    fn drop(&mut self) {
        if !self.committed
            && let Ok(mut state) = self.shared.state.lock()
            && let Some(active) = state.active.as_mut().filter(|a| a.id == self.id)
        {
            active.worker = false;
        }
    }
}
fn lookup<'a>(state: &'a State, id: &str) -> Result<&'a Journal, VendorJobError> {
    if !valid_id(id) {
        return Err(VendorJobError::InvalidId);
    }
    state
        .ledger
        .journals
        .iter()
        .find(|j| j.job.job_id == id)
        .ok_or(VendorJobError::InvalidId)
}
fn expire_prepared(state: &mut State) {
    if state.active.as_ref().is_some_and(|a| {
        !a.worker
            && (a.cancel.load(Ordering::Acquire)
                || lookup(state, &a.id)
                    .is_ok_and(|j| now().saturating_sub(j.job.created_at) >= EXPIRY))
    }) {
        let id = state.active.take().unwrap().id;
        if let Some(j) = state
            .ledger
            .journals
            .iter_mut()
            .find(|j| j.job.job_id == id)
        {
            j.job.state = VendorJobState::CancelledBeforeLaunch;
        }
    }
}
fn persistence_error(error: StorageError) -> VendorJobError {
    match error {
        StorageError::TooLarge => VendorJobError::HistorySizeCapacity,
        _ => VendorJobError::Storage,
    }
}

fn check_admission(ledger: &Ledger) -> Result<(), VendorJobError> {
    if ledger.journals.len() > MAX_JOURNALS {
        return Err(VendorJobError::HistoryCountCapacity);
    }
    let bytes = serde_json::to_vec(ledger).map_err(|_| VendorJobError::Storage)?;
    if bytes.len() + ledger.journals.len() * TRANSITION_HEADROOM > MAX_LEDGER_BYTES {
        return Err(VendorJobError::HistorySizeCapacity);
    }
    Ok(())
}

fn save_job(shared: &Shared, state: &mut State, job: VendorJob) -> Result<(), VendorJobError> {
    let mut ledger = state.ledger.clone();
    let journal = ledger
        .journals
        .iter_mut()
        .find(|j| j.job.job_id == job.job_id)
        .ok_or(VendorJobError::InvalidId)?;
    journal.job = job;
    shared
        .storage
        .write_vendor_jobs(&ledger)
        .map_err(persistence_error)?;
    state.ledger = ledger;
    Ok(())
}
fn fresh(shared: &Shared, journal: &Journal) -> Result<(), VendorJobError> {
    if fingerprint(&stamp(shared.registry.as_ref(), &journal.program)?)? != journal.registry_stamp
        || vendor_uninstall::resolve(shared.registry.as_ref(), &journal.program.location)
            .map_err(|_| VendorJobError::RegistryChanged)?
            != journal.command
        || shared.process.validate(&journal.command)? != journal.executable
        || fingerprint(&stamp(shared.registry.as_ref(), &journal.program)?)?
            != journal.registry_stamp
    {
        return Err(VendorJobError::RegistryChanged);
    }
    Ok(())
}
fn run(shared: Arc<Shared>, id: String, cancel: Arc<AtomicBool>) {
    let fresh_result = {
        let journal = {
            let Ok(state) = shared.state.lock() else {
                return;
            };
            let Ok(journal) = lookup(&state, &id).cloned() else {
                return;
            };
            journal
        };
        fresh(&shared, &journal)
    };
    // Only persistence / launch intent is serialized. No mutex spans hashing, UAC or waiting.
    let mut writer = Some(loop {
        match shared.writer.try_lock() {
            Ok(guard) => break guard,
            Err(std::sync::TryLockError::Poisoned(_)) => return,
            Err(std::sync::TryLockError::WouldBlock) => {
                if cancel.load(Ordering::Acquire) || shared.shutdown.load(Ordering::Acquire) {
                    if let Ok(mut state) = shared.state.lock() {
                        if let Some(j) = state
                            .ledger
                            .journals
                            .iter_mut()
                            .find(|j| j.job.job_id == id)
                        {
                            j.job.state = VendorJobState::CancelledBeforeLaunch;
                        }
                        state.active = None;
                    }
                    // Durable Queued is also cancelled on restart; never bypass the writer to persist.
                    return;
                }
                std::thread::park_timeout(Duration::from_millis(25));
            }
        }
    });
    let journal = {
        let Ok(mut state) = shared.state.lock() else {
            return;
        };
        let Ok(journal) = lookup(&state, &id).cloned() else {
            return;
        };
        if cancel.load(Ordering::Acquire) || shared.shutdown.load(Ordering::Acquire) {
            let mut job = journal.job;
            job.state = VendorJobState::CancelledBeforeLaunch;
            let _ = save_job(&shared, &mut state, job);
            state.active = None;
            return;
        }
        journal
    };
    let mut job = journal.job.clone();
    // A queued writer wait may outlive the earlier hash. Re-read registry evidence at the
    // serialized launch gate; the native owner independently re-pins and hashes the executable.
    let registry_current =
        stamp(shared.registry.as_ref(), &journal.program).and_then(|values| fingerprint(&values));
    if fresh_result.is_err() || registry_current != Ok(journal.registry_stamp) {
        job.state = VendorJobState::Failed;
        job.completion_meaning = "freshRegistryOrExecutableValidationFailed".into();
    } else if cancel.load(Ordering::Acquire) || shared.shutdown.load(Ordering::Acquire) {
        job.state = VendorJobState::CancelledBeforeLaunch;
    } else {
        job.state = VendorJobState::Launching;
        job.updated_at = now();
        {
            let Ok(mut state) = shared.state.lock() else {
                return;
            };
            if save_job(&shared, &mut state, job.clone()).is_err() {
                state.active = None;
                if let Some(j) = state
                    .ledger
                    .journals
                    .iter_mut()
                    .find(|j| j.job.job_id == id)
                {
                    j.job.state = VendorJobState::Failed;
                    j.job.persistence_error = true;
                }
                return; // No durable launch intent means NO vendor call.
            }
        }
        drop(writer.take());
        let launch = if cancel.load(Ordering::Acquire) || shared.shutdown.load(Ordering::Acquire) {
            job.state = VendorJobState::CancelledBeforeLaunch;
            None
        } else {
            Some(
                shared
                    .process
                    .launch(&journal.command, &journal.executable, &cancel),
            )
        };
        match launch {
            None => {}
            Some(LaunchResult::NotStarted) => {
                job.state = VendorJobState::CancelledBeforeLaunch;
            }
            Some(LaunchResult::Cancelled) => {
                job.state = VendorJobState::CancelledByVendorOrUAC;
                job.launch_error = Some(1223);
            }
            Some(LaunchResult::Failed(error)) => {
                job.state = VendorJobState::Failed;
                job.launch_error = Some(error);
            }
            Some(LaunchResult::Unknown) => {
                job.state = VendorJobState::OutcomeUnknown;
            }
            Some(LaunchResult::Process(mut process)) => {
                job.exit_code = process.wait(&cancel, shared.timeout);
                job.state = classify(journal.command.msi, job.exit_code);
            }
        }
        job.completion_meaning = if journal.command.msi {
            "msiProcessResultNotLeftoverAuthority"
        } else {
            "genericVendorProcessExitDoesNotProveTransactionCompletion"
        }
        .into();
    }
    job.updated_at = now();
    let _completion_writer = if writer.is_none() {
        shared.writer.lock().ok()
    } else {
        None
    };
    if writer.is_none() && _completion_writer.is_none() {
        return;
    }
    if let Ok(mut state) = shared.state.lock() {
        if save_job(&shared, &mut state, job).is_err()
            && let Some(j) = state
                .ledger
                .journals
                .iter_mut()
                .find(|j| j.job.job_id == id)
        {
            j.job.state = VendorJobState::OutcomeUnknown;
            j.job.persistence_error = true;
        }
        // Publish a free slot only after the terminal journal writer has been released.
        drop(_completion_writer);
        drop(writer.take());
        state.active = None;
        expire_prepared(&mut state);
    }
    drop(writer);
}
fn classify(msi: bool, code: Option<u32>) -> VendorJobState {
    if !msi {
        return VendorJobState::OutcomeUnknown;
    }
    match code {
        Some(0) => VendorJobState::Completed,
        Some(1602) => VendorJobState::CancelledByVendorOrUAC,
        Some(1641 | 3010) => VendorJobState::RebootRequired,
        Some(_) => VendorJobState::Failed,
        None => VendorJobState::OutcomeUnknown,
    }
}

#[cfg(test)]
#[path = "vendor_jobs_tests.rs"]
mod tests;
