//! One cooperative worker per service, immutable core paging, and snapshot-scoped
//! backend root/volume bindings. Feature runners are Rust callbacks, never IPC input.
#[cfg(test)]
#[path = "scans_tests.rs"]
mod tests;
use super::{
    NativeEntropy,
    drives::{self, BoundDrive},
    opaque_id,
};
use crate::WindowsFileSystem;
pub use cleanup_core::storage::RecordOrder;
use cleanup_core::storage::*;
use cleanup_core::{
    CancellationToken, EntryKind, FileSystem, ProgressEvent, ProgressSink, ProtectionPolicy,
};
use std::{
    collections::{HashMap, VecDeque},
    path::Path,
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Debug)]
pub enum JobError {
    Storage(StorageError),
    Busy,
    Native(String),
    WorkerFailed,
}
impl From<StorageError> for JobError {
    fn from(e: StorageError) -> Self {
        Self::Storage(e)
    }
}
impl std::fmt::Display for JobError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for JobError {}
fn native(e: impl std::fmt::Display) -> JobError {
    JobError::Native(e.to_string())
}

/// Clock injection permits expiry tests without sleeping or changing the OS clock.
pub trait Clock: Send + Sync {
    fn now(&self) -> Duration;
}
struct Monotonic(Instant);
impl Clock for Monotonic {
    fn now(&self) -> Duration {
        self.0.elapsed()
    }
}

/// A runner owns its builder; shared traversal and hashes execute outside the
/// service lock. Only one OS worker is spawned (within the four-worker cap).
pub struct ScanContext {
    pub snapshot_id: String,
    pub root: Option<RootAuthorization>,
    pub limits: StorageLimits,
    pub cancellation: CancellationToken,
    status: Arc<Mutex<StorageStatus>>,
    builder: SnapshotBuilder,
    drives: HashMap<String, BoundDrive>,
    drive_issues: Vec<drives::DriveIssue>,
    programs: HashMap<String, super::uninstaller::ProgramRecord>,
}
impl ProgressSink for ScanContext {
    fn report(&self, event: ProgressEvent) {
        let mut status = self.status.lock().unwrap_or_else(|e| e.into_inner());
        status.visited_entries = event.visited_entries.min(self.limits.visited_entries);
    }
}
impl ScanContext {
    pub(super) fn hash_progress(&self, bytes: u64, completed: bool) {
        let mut status = self.status.lock().unwrap_or_else(|e| e.into_inner());
        status.hashed_bytes = status.hashed_bytes.saturating_add(bytes);
        status.completed_hashes = status
            .completed_hashes
            .saturating_add(usize::from(completed));
    }
    pub fn phase(&self, phase: StoragePhase) {
        if matches!(
            phase,
            StoragePhase::Walking
                | StoragePhase::Grouping
                | StoragePhase::PartialHash
                | StoragePhase::FullHash
                | StoragePhase::Finalizing
        ) {
            self.status.lock().unwrap_or_else(|e| e.into_inner()).phase = phase;
        }
    }
    pub fn push(&mut self, record: StorageRecord, order: RecordOrder) -> Result<(), StorageError> {
        if self.cancellation.is_cancelled() {
            return Err(StorageError::SnapshotUnavailable);
        }
        if let Err(error) = self.builder.push(record, order) {
            if error == StorageError::LimitReached {
                self.mark_partial(PartialReason::RecordLimit);
            }
            return Err(error);
        }
        self.status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retained_records += 1;
        Ok(())
    }
    pub fn add_candidate(
        &mut self,
        id: String,
        evidence: StorageEvidence,
    ) -> Result<(), StorageError> {
        if self.root.as_ref() != Some(evidence.root()) {
            return Err(StorageError::InvalidEvidence);
        }
        self.builder.add_candidate(id, evidence)
    }
    pub fn mark_partial(&mut self, reason: PartialReason) {
        self.builder.mark_partial(reason);
        let mut status = self.status.lock().unwrap_or_else(|e| e.into_inner());
        if !status.completeness.reasons.contains(&reason) {
            status.completeness.reasons.push(reason);
        }
    }
    /// Adapter to the existing bounded core walker, not a second traversal implementation.
    pub fn walk(
        &mut self,
        protection: &ProtectionPolicy,
        excluded: &dyn Fn(&Path) -> bool,
        visitor: &mut dyn FnMut(&mut ScanContext, walk::WalkEvent<'_>) -> walk::WalkControl,
    ) -> Result<walk::WalkReport, StorageError> {
        let root = self.root.clone().ok_or(StorageError::InvalidEvidence)?;
        let cancellation = self.cancellation.clone();
        let status = self.status.clone();
        let limits = self.limits;
        let progress = move |event: ProgressEvent| {
            status
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .visited_entries = event.visited_entries.min(limits.visited_entries);
        };
        let report = walk::walk(
            &WindowsFileSystem,
            &root,
            walk::WalkPolicy {
                protection,
                excluded,
            },
            &cancellation,
            limits,
            &progress,
            &mut |event| visitor(self, event),
        )?;
        for reason in &report.completeness.reasons {
            self.mark_partial(*reason);
        }
        Ok(report)
    }
}
struct Finished {
    programs: HashMap<String, super::uninstaller::ProgramRecord>,
    snapshot: StorageSnapshot,
    root: Option<RootAuthorization>,
    drives: HashMap<String, BoundDrive>,
    drive_issues: Vec<drives::DriveIssue>,
}
struct Active {
    id: String,
    status: Arc<Mutex<StorageStatus>>,
    cancellation: CancellationToken,
    released: bool,
    finished_at: Arc<Mutex<Option<Duration>>>,
    worker: JoinHandle<Result<Finished, JobError>>,
}
struct Completed {
    programs: HashMap<String, super::uninstaller::ProgramRecord>,
    status: StorageStatus,
    root: Option<RootAuthorization>,
    error: Option<String>,
    access: Duration,
    drives: HashMap<String, BoundDrive>,
    drive_issues: Vec<drives::DriveIssue>,
}
#[derive(Default)]
struct State {
    active: Option<Active>,
    completed: VecDeque<Completed>,
    pages: SnapshotPages,
    roots: HashMap<String, (Duration, RootAuthorization)>,
}
impl State {
    fn maintain(&mut self, now: Duration) {
        let fresh = |access: Duration| {
            // A worker can finish just after the caller sampled `now`.
            now.saturating_sub(access) < Duration::from_secs(SNAPSHOT_IDLE_SECONDS)
        };
        self.roots.retain(|_, (access, _)| fresh(*access));
        self.completed.retain(|c| {
            if fresh(c.access) {
                true
            } else {
                let _ = self.pages.release(&c.status.snapshot_id);
                false
            }
        });
        if self.active.as_ref().is_some_and(|a| a.worker.is_finished()) {
            let active = self.active.take().unwrap();
            let result = active.worker.join().unwrap_or(Err(JobError::WorkerFailed));
            let completed_at = active
                .finished_at
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .unwrap_or(now);
            if active.released || !fresh(completed_at) {
                return;
            }
            let mut status = active
                .status
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            let (error, drives, drive_issues, root, programs) = match result {
                Ok(done) => {
                    // Retention clock is owned here so successful status polls and
                    // drive resolution refresh the same inactivity deadline as pages.
                    match self.pages.insert(done.snapshot, Duration::ZERO) {
                        Ok(()) => (
                            None,
                            done.drives,
                            done.drive_issues,
                            done.root,
                            done.programs,
                        ),
                        Err(e) => (
                            Some(e.to_string()),
                            HashMap::new(),
                            Vec::new(),
                            None,
                            HashMap::new(),
                        ),
                    }
                }
                Err(e) => (
                    Some(e.to_string()),
                    HashMap::new(),
                    Vec::new(),
                    None,
                    HashMap::new(),
                ),
            };
            if error.is_some() {
                status.phase = StoragePhase::Failed;
            }
            if self.completed.len() == MAX_COMPLETED_SNAPSHOTS {
                let old = self.completed.pop_front().unwrap();
                let _ = self.pages.release(&old.status.snapshot_id);
            }
            self.completed.push_back(Completed {
                status,
                error,
                access: completed_at,
                drives,
                drive_issues,
                root,
                programs,
            });
        }
    }
}

pub struct StorageService {
    state: Arc<Mutex<State>>,
    clock: Arc<dyn Clock>,
    shutdown: std::sync::mpsc::Sender<()>,
    reaper: Option<JoinHandle<()>>,
}
impl Default for StorageService {
    fn default() -> Self {
        Self::new()
    }
}
impl StorageService {
    pub fn new() -> Self {
        Self::with_clock(Arc::new(Monotonic(Instant::now())))
    }
    pub fn with_clock(clock: Arc<dyn Clock>) -> Self {
        let state = Arc::new(Mutex::new(State::default()));
        let (shutdown, receiver) = std::sync::mpsc::channel();
        let owned = state.clone();
        let timer = clock.clone();
        // A waiting maintenance thread plus one runner remains below MAX_WORKERS.
        // Idle state is physically reclaimed even when the caller stops polling.
        let reaper = thread::spawn(move || {
            while matches!(
                receiver.recv_timeout(Duration::from_secs(1)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            ) {
                owned
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .maintain(timer.now());
            }
        });
        Self {
            state,
            clock,
            shutdown,
            reaper: Some(reaper),
        }
    }
    /// Called only by a trusted native root picker/owner, never with renderer proof.
    /// Registry is bounded independently of snapshots; start consumes the binding.
    pub fn authorize_root(
        &self,
        path: &Path,
        protection: &ProtectionPolicy,
    ) -> Result<String, JobError> {
        let id = opaque_id()?;
        let root = bind_root(path, &id, &id, protection)?;
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let now = self.clock.now();
        state.maintain(now);
        if state.roots.len() >= MAX_COMPLETED_SNAPSHOTS {
            return Err(StorageError::LimitReached.into());
        }
        state.roots.insert(id.clone(), (now, root));
        Ok(id)
    }
    /// Internal typed feature runner. No algorithms for future features are registered.
    /// Runners must be cooperative and must not spawn additional unmanaged workers.
    pub fn start_with<F>(
        &self,
        module: StorageModule,
        root_id: Option<&str>,
        limits: StorageLimits,
        protection: Option<ProtectionPolicy>,
        runner: F,
    ) -> Result<String, JobError>
    where
        F: FnOnce(&mut ScanContext) -> Result<(), JobError> + Send + 'static,
    {
        limits.validate()?;
        if !matches!(module, StorageModule::Drives | StorageModule::Uninstaller)
            && (root_id.is_none() || protection.is_none())
        {
            return Err(StorageError::InvalidRequest.into());
        }
        if matches!(module, StorageModule::Drives | StorageModule::Uninstaller) && root_id.is_some()
        {
            return Err(StorageError::InvalidRequest.into());
        }
        let id = opaque_id()?;
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.maintain(self.clock.now());
        if state.active.is_some() {
            return Err(JobError::Busy);
        }
        let root = match root_id {
            Some(root_id) => {
                let (_, mut root) = state
                    .roots
                    .remove(root_id)
                    .ok_or(StorageError::InvalidEvidence)?;
                root.snapshot_id = id.clone();
                Some(root)
            }
            None => None,
        };
        let status = Arc::new(Mutex::new(StorageStatus {
            snapshot_id: id.clone(),
            module,
            phase: StoragePhase::Queued,
            visited_entries: 0,
            retained_records: 0,
            hashed_bytes: 0,
            completed_hashes: 0,
            completeness: Completeness::default(),
        }));
        let cancellation = CancellationToken::new();
        let mut context = ScanContext {
            snapshot_id: id.clone(),
            root,
            limits,
            cancellation: cancellation.clone(),
            status: status.clone(),
            builder: SnapshotBuilder::new(id.clone(), module, limits.retained_records)?,
            drives: HashMap::new(),
            drive_issues: Vec::new(),
            programs: HashMap::new(),
        };
        let finished_at = Arc::new(Mutex::new(None));
        let finished = finished_at.clone();
        let clock = self.clock.clone();
        let worker = thread::Builder::new()
            .name("storage-scan".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if let Some(root) = &context.root {
                        let current = bind_root(
                            &root.canonical_path,
                            &root.root_id,
                            &root.snapshot_id,
                            protection.as_ref().ok_or(StorageError::InvalidEvidence)?,
                        )?;
                        if &current != root {
                            return Err(StorageError::InvalidEvidence.into());
                        }
                    }
                    context.phase(StoragePhase::Walking);
                    let result = runner(&mut context);
                    if context.cancellation.is_cancelled() {
                        context.mark_partial(PartialReason::Cancelled);
                    } else {
                        result?;
                    }
                    let phase = if context.cancellation.is_cancelled() {
                        StoragePhase::Cancelled
                    } else {
                        StoragePhase::Complete
                    };
                    let snapshot = context.builder.finish(&NativeEntropy, false)?;
                    context
                        .status
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .phase = phase;
                    Ok(Finished {
                        snapshot,
                        root: context.root,
                        drives: context.drives,
                        drive_issues: context.drive_issues,
                        programs: context.programs,
                    })
                }))
                .unwrap_or(Err(JobError::WorkerFailed));
                *finished.lock().unwrap_or_else(|e| e.into_inner()) = Some(clock.now());
                result
            })
            .map_err(native)?;
        state.active = Some(Active {
            id: id.clone(),
            status,
            cancellation,
            released: false,
            finished_at,
            worker,
        });
        Ok(id)
    }
    pub fn start_programs(
        &self,
        limits: StorageLimits,
        query: super::uninstaller::ProgramQuery,
    ) -> Result<String, JobError> {
        self.start_programs_with(limits, query, Arc::new(super::uninstaller::NativeRegistry))
    }
    pub(crate) fn start_programs_with(
        &self,
        limits: StorageLimits,
        query: super::uninstaller::ProgramQuery,
        reader: Arc<dyn super::uninstaller::RegistryReader>,
    ) -> Result<String, JobError> {
        query.validate()?;
        self.start_with(
            StorageModule::Uninstaller,
            None,
            limits,
            None,
            move |context| {
                let cancel = context.cancellation.clone();
                let mut diagnostics = 0;
                let result = super::uninstaller::inventory_stream(
                    reader.as_ref(),
                    &cancel,
                    &mut |visited, record| {
                        if visited > context.limits.visited_entries {
                            context.mark_partial(PartialReason::EntryLimit);
                            return false;
                        }
                        context.report(ProgressEvent {
                            phase: cleanup_core::ScanPhase::Discovering,
                            visited_entries: visited,
                            candidates: 0,
                            completed_jobs: 0,
                            total_jobs: 1,
                        });
                        match record {
                            None => true,
                            Some(Ok(program)) => {
                                if !program
                                    .display
                                    .name
                                    .to_lowercase()
                                    .contains(&query.name_contains.to_lowercase())
                                {
                                    return true;
                                }
                                let order = RecordOrder {
                                    numeric: if query.largest_first {
                                        u64::MAX - program.display.estimated_size_bytes.unwrap_or(0)
                                    } else {
                                        0
                                    },
                                    text: program.display.name.to_lowercase(),
                                };
                                if context
                                    .push(StorageRecord::Program(program.display.clone()), order)
                                    .is_err()
                                {
                                    return false;
                                }
                                context
                                    .programs
                                    .insert(program.display.program_id.clone(), program);
                                true
                            }
                            Some(Err(error)) => {
                                context.mark_partial(
                                    if error == super::uninstaller::InventoryError::Limit {
                                        PartialReason::RecordLimit
                                    } else {
                                        PartialReason::Unreadable
                                    },
                                );
                                diagnostics += 1;
                                if diagnostics >= context.limits.diagnostics {
                                    context.mark_partial(PartialReason::DiagnosticLimit);
                                    false
                                } else {
                                    true
                                }
                            }
                        }
                    },
                );
                if let Err(error) = result {
                    context.mark_partial(if error == super::uninstaller::InventoryError::Limit {
                        PartialReason::RecordLimit
                    } else {
                        PartialReason::Unreadable
                    });
                }
                Ok(())
            },
        )
    }
    /// Snapshot-bound authority, not a registry path or renderer-provided program proof.
    pub(crate) fn resolve_program(
        &self,
        snapshot_id: &str,
        program_id: &str,
    ) -> Result<super::uninstaller::ProgramRecord, JobError> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let now = self.clock.now();
        state.maintain(now);
        let done = state
            .completed
            .iter_mut()
            .find(|done| {
                done.status.snapshot_id == snapshot_id
                    && done.status.module == StorageModule::Uninstaller
                    && done.status.phase == StoragePhase::Complete
            })
            .ok_or(StorageError::SnapshotUnavailable)?;
        let program = done
            .programs
            .get(program_id)
            .cloned()
            .ok_or(StorageError::InvalidEvidence)?;
        done.access = now;
        Ok(program)
    }
    pub fn start_drives(&self, limits: StorageLimits) -> Result<String, JobError> {
        self.start_drives_with_system(limits, drives::system_guid)
    }
    fn start_drives_with_system(
        &self,
        limits: StorageLimits,
        system: fn() -> std::io::Result<String>,
    ) -> Result<String, JobError> {
        self.start_with(StorageModule::Drives, None, limits, None, move |context| {
            let cancel = context.cancellation.clone();
            let mut push_error = None;
            let mut visited = 0;
            drives::inventory(&cancel, system(), |drive| {
                if visited >= context.limits.visited_entries {
                    context.mark_partial(PartialReason::EntryLimit);
                    return false;
                }
                visited += 1;
                context.report(ProgressEvent {
                    phase: cleanup_core::ScanPhase::Discovering,
                    visited_entries: visited,
                    candidates: 0,
                    completed_jobs: 0,
                    total_jobs: 1,
                });
                let drive = match drive {
                    Ok(drive) => drive,
                    Err(issue) => {
                        context.mark_partial(PartialReason::Unreadable);
                        if context.drive_issues.len() >= context.limits.diagnostics {
                            context.mark_partial(PartialReason::DiagnosticLimit);
                            return false;
                        }
                        context.drive_issues.push(issue);
                        return true;
                    }
                };
                let id = drive.summary.drive_id.clone();
                let order = RecordOrder {
                    numeric: 0,
                    text: drive.summary.label.clone(),
                };
                match context.push(StorageRecord::Drive(drive.summary.clone()), order) {
                    Ok(()) => {
                        context.drives.insert(id, drive);
                        true
                    }
                    Err(StorageError::LimitReached) => {
                        context.mark_partial(PartialReason::RecordLimit);
                        false
                    }
                    Err(e) => {
                        push_error = Some(e);
                        false
                    }
                }
            })
            .map_err(native)?;
            if let Some(e) = push_error {
                return Err(e.into());
            }
            Ok(())
        })
    }
    pub fn status(&self, id: &str) -> Result<(StorageStatus, Option<String>), JobError> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let now = self.clock.now();
        state.maintain(now);
        if let Some(active) = &state.active
            && active.id == id
            && !active.released
        {
            let mut status = active
                .status
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            // Terminal status means pages have actually been published.
            if matches!(
                status.phase,
                StoragePhase::Complete | StoragePhase::Cancelled
            ) {
                status.phase = StoragePhase::Finalizing;
            }
            return Ok((status, None));
        }
        let done = state
            .completed
            .iter_mut()
            .find(|c| c.status.snapshot_id == id)
            .ok_or(StorageError::SnapshotUnavailable)?;
        done.access = now;
        Ok((done.status.clone(), done.error.clone()))
    }
    pub fn page(&self, request: &PageRequest) -> Result<StoragePage, JobError> {
        request.validate()?;
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let now = self.clock.now();
        state.maintain(now);
        let page = state.pages.page(request, Duration::ZERO)?;
        if let Some(done) = state
            .completed
            .iter_mut()
            .find(|c| c.status.snapshot_id == request.snapshot_id)
        {
            done.access = now;
        }
        Ok(page)
    }
    /// Backend-only selection bridge. IDs resolve only in the live, same-module snapshot.
    pub(crate) fn resolve_selection(
        &self,
        selection: &StorageSelection,
    ) -> Result<Vec<StorageEvidence>, JobError> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let now = self.clock.now();
        state.maintain(now);
        let evidence = state.pages.resolve(selection, Duration::ZERO)?;
        if let Some(done) = state
            .completed
            .iter_mut()
            .find(|c| c.status.snapshot_id == selection.snapshot_id)
        {
            done.access = now;
        }
        Ok(evidence)
    }
    pub fn drive_issues(&self, id: &str) -> Result<Vec<drives::DriveIssue>, JobError> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let now = self.clock.now();
        state.maintain(now);
        let done = state
            .completed
            .iter_mut()
            .find(|c| c.status.snapshot_id == id && c.status.module == StorageModule::Drives)
            .ok_or(StorageError::SnapshotUnavailable)?;
        done.access = now;
        Ok(done.drive_issues.clone())
    }
    pub fn cancel(&self, id: &str) -> Result<(), JobError> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.maintain(self.clock.now());
        if let Some(active) = &state.active
            && active.id == id
            && !active.released
        {
            active.cancellation.cancel();
            return Ok(());
        }
        Err(StorageError::SnapshotUnavailable.into())
    }
    pub fn release(&self, id: &str) -> Result<(), JobError> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.maintain(self.clock.now());
        if state.roots.remove(id).is_some() {
            return Ok(());
        }
        if let Some(active) = &mut state.active
            && active.id == id
            && !active.released
        {
            active.released = true;
            active.cancellation.cancel();
            return Ok(());
        }
        let index = state
            .completed
            .iter()
            .position(|c| c.status.snapshot_id == id)
            .ok_or(StorageError::SnapshotUnavailable)?;
        state.completed.remove(index);
        let _ = state.pages.release(id);
        Ok(())
    }
    pub fn resolve_root(
        &self,
        snapshot_id: &str,
        root_id: &str,
        protection: &ProtectionPolicy,
    ) -> Result<RootAuthorization, JobError> {
        let root = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            let now = self.clock.now();
            state.maintain(now);
            let done = state
                .completed
                .iter_mut()
                .find(|c| c.status.snapshot_id == snapshot_id)
                .ok_or(StorageError::SnapshotUnavailable)?;
            let root = done
                .root
                .as_ref()
                .filter(|r| r.root_id == root_id)
                .ok_or(StorageError::InvalidEvidence)?
                .clone();
            done.access = now;
            root
        };
        let current = bind_root(
            &root.canonical_path,
            &root.root_id,
            &root.snapshot_id,
            protection,
        )?;
        if current != root {
            return Err(StorageError::InvalidEvidence.into());
        }
        Ok(current)
    }
    /// Returns current capacity only after re-querying the snapshot's opaque volume binding.
    pub fn resolve_drive(
        &self,
        snapshot_id: &str,
        drive_id: &str,
    ) -> Result<analysis::DriveSummary, JobError> {
        let drive = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            let now = self.clock.now();
            state.maintain(now);
            let done = state
                .completed
                .iter_mut()
                .find(|c| c.status.snapshot_id == snapshot_id)
                .ok_or(StorageError::SnapshotUnavailable)?;
            let drive = done
                .drives
                .get(drive_id)
                .ok_or(StorageError::InvalidEvidence)?
                .clone();
            done.access = now;
            drive
        };
        drives::resolve(&drive).map_err(native)
    }
}
impl Drop for StorageService {
    fn drop(&mut self) {
        let _ = self.shutdown.send(());
        if let Some(reaper) = self.reaper.take() {
            let _ = reaper.join();
        }
        let active = self
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .active
            .take();
        if let Some(active) = active {
            active.cancellation.cancel();
            let _ = active.worker.join();
        }
    }
}
fn bind_root(
    path: &Path,
    root_id: &str,
    snapshot_id: &str,
    protection: &ProtectionPolicy,
) -> Result<RootAuthorization, JobError> {
    if !cleanup_core::is_local_storage_path(path) {
        return Err(StorageError::InvalidEvidence.into());
    }
    let fs = WindowsFileSystem;
    for ancestor in path.ancestors().filter(|p| !p.as_os_str().is_empty()) {
        let metadata = fs.metadata_no_follow(ancestor).map_err(native)?;
        if metadata.kind != EntryKind::Directory || metadata.identity.is_none() {
            return Err(StorageError::InvalidEvidence.into());
        }
    }
    let before = fs.metadata_no_follow(path).map_err(native)?;
    let canonical = fs.canonicalize(path).map_err(native)?;
    if !cleanup_core::is_local_storage_path(&canonical)
        || protection.is_protected(&canonical)
        || !super::protection::personal_path_allowed(&canonical)
    {
        return Err(StorageError::InvalidEvidence.into());
    }
    let metadata = fs.metadata_no_follow(&canonical).map_err(native)?;
    if metadata.kind != EntryKind::Directory || metadata.identity != before.identity {
        return Err(StorageError::InvalidEvidence.into());
    }
    let root = RootAuthorization {
        snapshot_id: snapshot_id.into(),
        root_id: root_id.into(),
        canonical_path: canonical,
        identity: metadata.identity.ok_or(StorageError::InvalidEvidence)?,
    };
    root.validate()?;
    Ok(root)
}
