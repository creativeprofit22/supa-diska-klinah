use super::super::uninstaller::{Hive, InventoryError, RegistryLocation, View};
use super::super::vendor_uninstall::VendorProcess;
use super::*;
use cleanup_core::storage::{ObservedEntry, RootAuthorization};
use cleanup_core::{EntryKind, FileIdentity};
use std::{
    path::PathBuf,
    sync::{OnceLock, atomic::AtomicUsize},
    time::Instant,
};
use windows_sys::Win32::System::Registry::REG_SZ;

fn until(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !predicate() {
        assert!(Instant::now() < deadline, "bounded fixture wait expired");
        std::thread::park_timeout(Duration::from_millis(5));
    }
}
fn temp() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "vendor-fixture-{}",
        super::super::opaque_id().unwrap()
    ));
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join("fixture-only"), b"disposable").unwrap();
    path
}
fn string(s: &str) -> RegistryValue {
    RegistryValue {
        kind: REG_SZ,
        bytes: s
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect(),
    }
}
struct Registry {
    command: Mutex<String>,
}
impl RegistryReader for Registry {
    fn keys(&self, hive: Hive, view: View) -> Result<Vec<String>, InventoryError> {
        Ok(if hive == Hive::Machine && view == View::Native64 {
            vec!["disposable".into()]
        } else {
            Vec::new()
        })
    }
    fn value(
        &self,
        _: &RegistryLocation,
        name: &str,
    ) -> Result<Option<RegistryValue>, InventoryError> {
        Ok(match name {
            "DisplayName" => Some(string("Disposable fixture")),
            "UninstallString" => Some(string(&self.command.lock().unwrap())),
            _ => None,
        })
    }
}
struct Process {
    launches: AtomicUsize,
    validations: AtomicUsize,
    block_validation: AtomicBool,
    block_launch: AtomicBool,
    waiting: AtomicBool,
    outcome: usize,
    code: Option<u32>,
}
impl Process {
    fn new(outcome: usize, code: Option<u32>) -> Arc<Self> {
        Arc::new(Self {
            launches: AtomicUsize::new(0),
            validations: AtomicUsize::new(0),
            block_validation: AtomicBool::new(false),
            block_launch: AtomicBool::new(false),
            waiting: AtomicBool::new(false),
            outcome,
            code,
        })
    }
}
struct Child {
    code: Option<u32>,
}
impl VendorProcess for Child {
    fn wait(&mut self, cancel: &AtomicBool, timeout: Duration) -> Option<u32> {
        if self.code.is_some() {
            return self.code;
        }
        let start = Instant::now();
        while !cancel.load(Ordering::Acquire) && start.elapsed() < timeout {
            std::thread::park_timeout(Duration::from_millis(5));
        }
        None
    }
}
fn evidence() -> ExecutableEvidence {
    ExecutableEvidence {
        root: RootAuthorization {
            snapshot_id: "vendor-executable".into(),
            root_id: "vendor-executable".into(),
            canonical_path: r"C:\Fixture".into(),
            identity: FileIdentity { volume: 1, file: 1 },
        },
        entry: ObservedEntry {
            canonical_path: r"C:\Fixture\uninstall.exe".into(),
            identity: FileIdentity { volume: 1, file: 2 },
            kind: EntryKind::File,
            logical_bytes: 1024,
            allocated_bytes: None,
            modified_unix_nanos: 1,
        },
        digest: [1; 32],
    }
}
impl ProcessBoundary for Process {
    fn validate(&self, _: &VendorCommand) -> Result<ExecutableEvidence, VendorJobError> {
        self.validations.fetch_add(1, Ordering::AcqRel);
        until(|| !self.block_validation.load(Ordering::Acquire));
        Ok(evidence())
    }
    fn launch(
        &self,
        _: &VendorCommand,
        _: &ExecutableEvidence,
        cancel: &AtomicBool,
    ) -> LaunchResult {
        self.waiting.store(true, Ordering::Release);
        until(|| !self.block_launch.load(Ordering::Acquire));
        if cancel.load(Ordering::Acquire) {
            return LaunchResult::NotStarted;
        }
        self.launches.fetch_add(1, Ordering::AcqRel);
        match self.outcome {
            1 => LaunchResult::Cancelled,
            2 => LaunchResult::Failed(5),
            3 => LaunchResult::Unknown,
            _ => LaunchResult::Process(Box::new(Child { code: self.code })),
        }
    }
}
fn setup(process: Arc<dyn ProcessBoundary>) -> (VendorJobManager, Arc<Registry>, PathBuf) {
    let root = temp();
    let registry = Arc::new(Registry {
        command: Mutex::new(r"C:\Fixture\uninstall.exe /remove".into()),
    });
    let manager = VendorJobManager::with_boundaries(
        CleanupStorage::open(root.join("journal")).unwrap(),
        Arc::new(Mutex::new(())),
        registry.clone(),
        process,
        Duration::from_secs(2),
    )
    .unwrap();
    (manager, registry, root)
}
fn prepare_result(manager: &VendorJobManager) -> Result<VendorJob, VendorJobError> {
    use cleanup_core::storage::*;
    let storage = super::super::scans::StorageService::new();
    let snapshot_id = manager
        .start_inventory(&storage, StorageLimits::default(), Default::default())
        .unwrap();
    until(|| storage.status(&snapshot_id).unwrap().0.phase == StoragePhase::Complete);
    let page = storage
        .page(&PageRequest {
            snapshot_id: snapshot_id.clone(),
            module: StorageModule::Uninstaller,
            collection: PageCollection::Programs,
            parent_id: None,
            cursor: None,
            page_size: 1,
        })
        .unwrap();
    let StorageRecord::Program(program) = &page.records[0] else {
        panic!("not a program");
    };
    manager.prepare(&storage, &snapshot_id, &program.program_id)
}
fn prepare(manager: &VendorJobManager) -> VendorJob {
    prepare_result(manager).unwrap()
}
fn finished(manager: &VendorJobManager, id: &str) -> VendorJob {
    until(|| manager.shared.state.lock().unwrap().active.is_none());
    manager.status(id).unwrap()
}

#[test]
fn separate_confirmation_fresh_registry_and_no_arbitrary_id() {
    let process = Process::new(0, Some(0));
    let (manager, registry, root) = setup(process.clone());
    let job = prepare(&manager);
    assert_eq!(process.launches.load(Ordering::Acquire), 0);
    assert_eq!(
        manager.confirm("C:\\arbitrary.exe").unwrap_err(),
        VendorJobError::InvalidId
    );
    *registry.command.lock().unwrap() = r"C:\Fixture\different.exe".into();
    assert_eq!(
        manager.confirm(&job.job_id).unwrap_err(),
        VendorJobError::RegistryChanged
    );
    *registry.command.lock().unwrap() = r"C:\Fixture\uninstall.exe /remove".into();
    manager.confirm(&job.job_id).unwrap();
    assert_eq!(
        finished(&manager, &job.job_id).state,
        VendorJobState::OutcomeUnknown
    );
    assert!(manager.confirm(&job.job_id).is_err());
    assert_eq!(process.launches.load(Ordering::Acquire), 1);
    assert_eq!(prepare_result(&manager).unwrap_err(), VendorJobError::Busy);
    drop(manager);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn outcome_table_is_msi_only() {
    for (code, expected) in [
        (0, VendorJobState::Completed),
        (1602, VendorJobState::CancelledByVendorOrUAC),
        (1641, VendorJobState::RebootRequired),
        (3010, VendorJobState::RebootRequired),
        (5, VendorJobState::Failed),
    ] {
        assert_eq!(classify(true, Some(code)), expected);
        assert_eq!(classify(false, Some(code)), VendorJobState::OutcomeUnknown);
    }
    assert_eq!(classify(true, None), VendorJobState::OutcomeUnknown);
    for (outcome, expected) in [
        (1, VendorJobState::CancelledByVendorOrUAC),
        (2, VendorJobState::Failed),
        (3, VendorJobState::OutcomeUnknown),
    ] {
        let (manager, _, root) = setup(Process::new(outcome, Some(0)));
        let job = prepare(&manager);
        manager.confirm(&job.job_id).unwrap();
        let result = finished(&manager, &job.job_id);
        assert_eq!(result.state, expected);
        assert_eq!(
            result.launch_error,
            match outcome {
                1 => Some(1223),
                2 => Some(5),
                _ => None,
            }
        );
        drop(manager);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn cancelled_prepare_and_expiry_never_launch() {
    let process = Process::new(0, Some(0));
    let (manager, _, root) = setup(process.clone());
    let job = prepare(&manager);
    assert_eq!(
        manager.cancel(&job.job_id).unwrap().state,
        VendorJobState::CancelledBeforeLaunch
    );
    assert!(manager.confirm(&job.job_id).is_err());
    let job = prepare(&manager);
    manager
        .shared
        .state
        .lock()
        .unwrap()
        .ledger
        .journals
        .iter_mut()
        .find(|j| j.job.job_id == job.job_id)
        .unwrap()
        .job
        .created_at = 0;
    assert_eq!(
        manager.confirm(&job.job_id).unwrap_err(),
        VendorJobError::Expired
    );
    assert_eq!(process.launches.load(Ordering::Acquire), 0);
    drop(manager);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn hashing_holds_neither_mutex_and_confirmation_can_cancel() {
    let process = Process::new(0, Some(0));
    let (manager, _, root) = setup(process.clone());
    let manager = Arc::new(manager);
    process.block_validation.store(true, Ordering::Release);
    let clone = manager.clone();
    let thread = std::thread::spawn(move || prepare(&clone));
    until(|| process.validations.load(Ordering::Acquire) >= 1);
    assert!(manager.shared.state.try_lock().is_ok());
    assert!(manager.shared.writer.try_lock().is_ok());
    process.block_validation.store(false, Ordering::Release);
    let job = thread.join().unwrap();
    process.block_validation.store(true, Ordering::Release);
    let clone = manager.clone();
    let id = job.job_id.clone();
    let thread = std::thread::spawn(move || clone.confirm(&id));
    until(|| process.validations.load(Ordering::Acquire) >= 2);
    assert!(manager.shared.writer.try_lock().is_ok());
    assert_eq!(
        manager.cancel(&job.job_id).unwrap().state,
        VendorJobState::CancelledBeforeLaunch
    );
    process.block_validation.store(false, Ordering::Release);
    assert_eq!(
        thread.join().unwrap().unwrap_err(),
        VendorJobError::Conflict
    );
    assert_eq!(process.launches.load(Ordering::Acquire), 0);
    drop(manager);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn queued_cancel_stops_worker_behind_global_writer() {
    let process = Process::new(0, Some(0));
    let (manager, _, root) = setup(process.clone());
    let job = prepare(&manager);
    // Deterministically stage the already-durable queued state while another mutation owns writer.
    let writer = manager.shared.writer.lock().unwrap();
    let cancel = {
        let mut state = manager.shared.state.lock().unwrap();
        let mut queued = lookup(&state, &job.job_id).unwrap().job.clone();
        queued.state = VendorJobState::Queued;
        save_job(&manager.shared, &mut state, queued).unwrap();
        let active = state.active.as_mut().unwrap();
        active.worker = true;
        active.cancel.clone()
    };
    let shared = manager.shared.clone();
    let id = job.job_id.clone();
    let thread = std::thread::spawn(move || run(shared, id, cancel));
    manager.cancel(&job.job_id).unwrap();
    thread.join().unwrap();
    assert_eq!(process.launches.load(Ordering::Acquire), 0);
    assert_eq!(
        manager.status(&job.job_id).unwrap().state,
        VendorJobState::CancelledBeforeLaunch
    );
    drop(writer);
    drop(manager);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn registry_is_rechecked_after_queued_writer_wait() {
    let process = Process::new(0, Some(0));
    let (manager, registry, root) = setup(process.clone());
    let job = prepare(&manager);
    let writer = manager.shared.writer.lock().unwrap();
    let cancel = {
        let mut state = manager.shared.state.lock().unwrap();
        let mut queued = lookup(&state, &job.job_id).unwrap().job.clone();
        queued.state = VendorJobState::Queued;
        save_job(&manager.shared, &mut state, queued).unwrap();
        let active = state.active.as_mut().unwrap();
        active.worker = true;
        active.cancel.clone()
    };
    let shared = manager.shared.clone();
    let id = job.job_id.clone();
    let thread = std::thread::spawn(move || run(shared, id, cancel));
    until(|| process.validations.load(Ordering::Acquire) >= 2);
    *registry.command.lock().unwrap() = r"C:\Fixture\changed.exe".into();
    drop(writer);
    thread.join().unwrap();
    assert_eq!(
        finished(&manager, &job.job_id).state,
        VendorJobState::Failed
    );
    assert_eq!(process.launches.load(Ordering::Acquire), 0);
    drop(manager);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn persistence_failures_prevent_launch_and_retain_uncertainty() {
    for failing_write in [1, 2, 3] {
        let process = Process::new(0, Some(0));
        let (manager, _, root) = setup(process.clone());
        let job = prepare(&manager);
        manager.shared.storage.fail_write(failing_write);
        let confirmation = manager.confirm(&job.job_id);
        if failing_write == 1 {
            assert_eq!(confirmation.unwrap_err(), VendorJobError::Storage);
        } else {
            confirmation.unwrap();
            finished(&manager, &job.job_id);
        }
        assert_eq!(
            process.launches.load(Ordering::Acquire),
            usize::from(failing_write == 3)
        );
        if failing_write == 3 {
            assert!(manager.status(&job.job_id).unwrap().persistence_error);
        }
        drop(manager);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn restart_never_replays_and_unknown_cannot_expire_or_release() {
    let process = Process::new(0, Some(0));
    let (manager, registry, root) = setup(process.clone());
    let job = prepare(&manager);
    let mut ledger = manager.shared.state.lock().unwrap().ledger.clone();
    ledger.journals[0].job.state = VendorJobState::Launching;
    ledger.journals[0].job.updated_at = 0;
    manager.shared.storage.write_vendor_jobs(&ledger).unwrap();
    let storage = manager.shared.storage.clone();
    drop(manager);
    let manager = VendorJobManager::with_boundaries(
        storage,
        Arc::new(Mutex::new(())),
        registry,
        process.clone(),
        Duration::from_secs(1),
    )
    .unwrap();
    assert_eq!(
        manager.status(&job.job_id).unwrap().state,
        VendorJobState::OutcomeUnknown
    );
    assert!(manager.confirm(&job.job_id).is_err());
    assert_eq!(manager.release(&job.job_id), Err(VendorJobError::Busy));
    assert_eq!(prepare_result(&manager).unwrap_err(), VendorJobError::Busy);
    assert_eq!(process.launches.load(Ordering::Acquire), 0);
    drop(manager);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn vendor_wait_and_blocked_launch_do_not_hold_service_or_writer_and_drop_never_joins() {
    let process = Process::new(0, None);
    process.block_launch.store(true, Ordering::Release);
    let (manager, _, root) = setup(process.clone());
    let job = prepare(&manager);
    manager.confirm(&job.job_id).unwrap();
    until(|| process.waiting.load(Ordering::Acquire));
    assert!(manager.shared.state.try_lock().is_ok());
    assert!(manager.shared.writer.try_lock().is_ok());
    assert_eq!(
        manager.cancel(&job.job_id).unwrap().state,
        VendorJobState::OutcomeUnknown
    );
    let shared = manager.shared.clone();
    let start = Instant::now();
    drop(manager);
    assert!(start.elapsed() < Duration::from_secs(1));
    process.block_launch.store(false, Ordering::Release);
    until(|| shared.state.lock().unwrap().active.is_none());
    assert_eq!(process.launches.load(Ordering::Acquire), 0);
    drop(shared);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn journals_redact_commands_and_registry_secrets() {
    let (manager, registry, root) = setup(Process::new(0, Some(0)));
    *registry.command.lock().unwrap() =
        r"C:\Fixture\uninstall.exe --token=fixture-secret-never-persist".into();
    let job = prepare(&manager);
    let ledger: Ledger = manager.shared.storage.read_vendor_jobs().unwrap().unwrap();
    let json = serde_json::to_string(&ledger).unwrap();
    assert!(!json.contains("fixture-secret-never-persist"));
    assert!(!json.contains("--token"));
    assert!(!json.contains("arguments"));
    assert!(!json.contains("UninstallString"));
    assert!(!json.contains("\"command\""));
    assert!(ledger.journals[0].command.arguments.is_empty());
    let record = manager.shared.state.lock().unwrap().ledger.journals[0]
        .program
        .clone();
    assert_eq!(
        ledger.journals[0].registry_stamp,
        fingerprint(&stamp(registry.as_ref(), &record).unwrap()).unwrap()
    );
    *registry.command.lock().unwrap() = r"C:\Fixture\uninstall.exe --token=changed-secret".into();
    assert_eq!(
        manager.confirm(&job.job_id).unwrap_err(),
        VendorJobError::RegistryChanged
    );
    drop(manager);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn retention_limits_safe_results_but_pins_unknown_journals() {
    let (manager, _, root) = setup(Process::new(1, None));
    for _ in 0..5 {
        let job = prepare(&manager);
        manager.confirm(&job.job_id).unwrap();
        finished(&manager, &job.job_id);
    }
    assert_eq!(manager.retained_jobs().unwrap().len(), MAX_COMPLETED);
    {
        let mut state = manager.shared.state.lock().unwrap();
        for journal in &mut state.ledger.journals {
            journal.job.updated_at = 0;
        }
        prune(&mut state);
        assert!(state.ledger.journals.is_empty());
    }
    let job = prepare(&manager);
    {
        let mut state = manager.shared.state.lock().unwrap();
        let mut prototype = lookup(&state, &job.job_id).unwrap().clone();
        prototype.job.state = VendorJobState::OutcomeUnknown;
        prototype.job.updated_at = 0;
        state.active = None;
        state.ledger.journals = (0..MAX_JOURNALS)
            .map(|index| {
                let mut journal = prototype.clone();
                journal.job.job_id = format!("{index:032x}");
                journal.program.location.subkey = format!("other-{index}");
                journal
            })
            .collect();
        prune(&mut state);
        assert_eq!(state.ledger.journals.len(), MAX_JOURNALS);
    }
    assert_eq!(prepare_result(&manager).unwrap_err(), VendorJobError::Limit);
    drop(manager);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn invalid_schema_and_duplicate_ids_fail_closed_and_queued_restart_cancels() {
    for malformed in [0, 1, 2, 3] {
        let process = Process::new(0, Some(0));
        let (manager, registry, root) = setup(process.clone());
        let job = prepare(&manager);
        let mut ledger = manager.shared.state.lock().unwrap().ledger.clone();
        match malformed {
            0 => ledger.schema_version = 999,
            1 => ledger.journals.push(ledger.journals[0].clone()),
            2 => ledger.journals = vec![ledger.journals[0].clone(); MAX_JOURNALS + 1],
            _ => ledger.journals[0].job.state = VendorJobState::Queued,
        }
        manager.shared.storage.write_vendor_jobs(&ledger).unwrap();
        let storage = manager.shared.storage.clone();
        drop(manager);
        let reopened = VendorJobManager::with_boundaries(
            storage,
            Arc::new(Mutex::new(())),
            registry,
            process.clone(),
            Duration::from_secs(1),
        );
        if malformed < 3 {
            assert!(matches!(reopened, Err(VendorJobError::Storage)));
        } else {
            let reopened = reopened.unwrap();
            assert_eq!(
                reopened.status(&job.job_id).unwrap().state,
                VendorJobState::CancelledBeforeLaunch
            );
            assert!(reopened.confirm(&job.job_id).is_err());
        }
        assert_eq!(process.launches.load(Ordering::Acquire), 0);
        std::fs::remove_dir_all(root).unwrap();
    }
}

fn fixture() -> PathBuf {
    static FIXTURE: OnceLock<PathBuf> = OnceLock::new();
    FIXTURE
        .get_or_init(|| {
            let directory = temp();
            let path = directory.join("vendor-disposable.exe");
            vendor_uninstall::compile_vendor_fixture(&path);
            path
        })
        .clone()
}
#[test]
fn native_disposable_launch_identity_cancellation_and_timeout_never_kill() {
    for (delay, cancel_wait, timeout_wait) in
        [(0, false, false), (2000, true, false), (2000, false, true)]
    {
        let (mut manager, registry, root) = setup(Arc::new(NativeProcessBoundary));
        if timeout_wait {
            Arc::get_mut(&mut manager.shared).unwrap().timeout = Duration::from_millis(100);
        }
        let executable = root.join("vendor-disposable.exe");
        std::fs::copy(fixture(), &executable).unwrap();
        *registry.command.lock().unwrap() = format!(
            "\"{}\" \"{}\" {delay} 0",
            executable.display(),
            root.display()
        );
        let job = prepare(&manager);
        manager.confirm(&job.job_id).unwrap();
        until(|| root.join("started").exists());
        if cancel_wait {
            manager.cancel(&job.job_id).unwrap();
        }
        let result = finished(&manager, &job.job_id);
        assert_eq!(result.state, VendorJobState::OutcomeUnknown);
        if cancel_wait || timeout_wait {
            assert_eq!(result.exit_code, None);
            assert!(
                !root.join("finished").exists(),
                "wait must return while dedicated fixture is still running"
            );
        }
        // The child still finishes its dedicated write after our process handle/wait is released.
        until(|| root.join("finished").exists());
        drop(manager);
        until(|| std::fs::remove_dir_all(&root).is_ok());
    }
    let (manager, registry, root) = setup(Arc::new(NativeProcessBoundary));
    let executable = root.join("vendor-disposable.exe");
    std::fs::copy(fixture(), &executable).unwrap();
    *registry.command.lock().unwrap() =
        format!("\"{}\" \"{}\" 0 0", executable.display(), root.display());
    let job = prepare(&manager);
    std::fs::write(&executable, b"not a PE executable").unwrap();
    assert!(manager.confirm(&job.job_id).is_err());
    assert!(!root.join("started").exists());
    drop(manager);
    std::fs::remove_dir_all(root).unwrap();
}
