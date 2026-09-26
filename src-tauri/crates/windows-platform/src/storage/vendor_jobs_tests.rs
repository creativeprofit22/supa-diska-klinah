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

// Test-only enumeration proves the production page API reaches every retained outcome.
fn all_history(manager: &VendorJobManager) -> Result<Vec<VendorJob>, VendorJobError> {
    let mut records = Vec::new();
    let mut cursor = None;
    loop {
        let page = manager.history_page(HistoryRequest {
            cursor,
            limit: Some(64),
        })?;
        records.extend(page.records);
        cursor = page.next_cursor;
        if cursor.is_none() {
            return Ok(records);
        }
    }
}

#[test]
fn retained_history_regression_keeps_cancelled_outcomes_after_release() {
    let process = Process::new(0, Some(0));
    let (manager, _, root) = setup(process.clone());
    let job = prepare(&manager);
    manager.cancel(&job.job_id).unwrap();
    manager.release(&job.job_id).unwrap();
    assert_eq!(all_history(&manager).unwrap().len(), 1);
    assert_eq!(process.launches.load(Ordering::Acquire), 0);
    drop(manager);
    std::fs::remove_dir_all(root).unwrap();
}

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
fn native_confirmation_denial_never_launches_and_cannot_be_replayed() {
    let process = Process::new(0, Some(0));
    let (manager, _, root) = setup(process.clone());
    let job = prepare(&manager);
    assert!(manager.confirm_native(&job.job_id, 0).is_err());
    let denied = manager
        .confirm_with(&job.job_id, |message| {
            assert!(message.contains("Disposable fixture"));
            assert!(message.contains("uninstall.exe"));
            assert!(message.contains("/remove"));
            false
        })
        .unwrap();
    assert_eq!(denied.state, VendorJobState::CancelledBeforeLaunch);
    assert!(manager.confirm(&job.job_id).is_err());
    assert_eq!(process.launches.load(Ordering::Acquire), 0);
    drop(manager);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn native_confirmation_revalidates_after_consent_and_invalid_jobs_never_prompt() {
    let process = Process::new(0, Some(0));
    let (manager, registry, root) = setup(process.clone());
    assert!(
        manager
            .confirm_with(&"a".repeat(32), |_| panic!("invalid job prompted"))
            .is_err()
    );
    let job = prepare(&manager);
    let result = manager.confirm_with(&job.job_id, |_| {
        *registry.command.lock().unwrap() = r"C:\Fixture\uninstall.exe /changed".into();
        true
    });
    assert_eq!(result.unwrap_err(), VendorJobError::RegistryChanged);
    assert_eq!(process.launches.load(Ordering::Acquire), 0);
    drop(manager);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn affirmative_native_confirmation_runs_only_the_fixture_boundary_once() {
    let process = Process::new(0, Some(0));
    let (manager, _, root) = setup(process.clone());
    let job = prepare(&manager);
    manager.confirm_with(&job.job_id, |_| true).unwrap();
    let result = finished(&manager, &job.job_id);
    // Win32 exit codes are vendor-defined; only MSI codes have standardized meaning.
    assert_eq!(result.state, VendorJobState::OutcomeUnknown);
    assert_eq!(result.exit_code, Some(0));
    assert_eq!(process.launches.load(Ordering::Acquire), 1);
    assert!(
        manager
            .confirm_with(&job.job_id, |_| panic!("terminal job prompted"))
            .is_err()
    );
    drop(manager);
    std::fs::remove_dir_all(root).unwrap();
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
fn native_confirmation_seam_denial_and_pre_prompt_validation() {
    let process = Process::new(0, Some(0));
    let (manager, registry, root) = setup(process.clone());
    let job = prepare(&manager);
    assert!(manager.confirm_native(&job.job_id, 0).is_err());
    assert!(
        manager
            .confirm_with("invalid", |_| panic!("invalid ID prompted"))
            .is_err()
    );
    *registry.command.lock().unwrap() = r"C:\Fixture\changed.exe".into();
    assert_eq!(
        manager
            .confirm_with(&job.job_id, |_| panic!("stale command prompted"))
            .unwrap_err(),
        VendorJobError::RegistryChanged
    );
    *registry.command.lock().unwrap() = r"C:\Fixture\uninstall.exe /remove".into();
    let english = crate::i18n::strings(crate::i18n::Locale::En);
    let denied = manager
        .confirm_with_strings(&job.job_id, english, |message| {
            assert!(message.contains("Disposable fixture"));
            assert!(message.contains(&job.job_id));
            assert!(message.contains("Vendor executable"));
            assert!(message.contains("uninstall.exe"));
            assert!(message.contains("/remove"));
            false
        })
        .unwrap();
    assert_eq!(denied.state, VendorJobState::CancelledBeforeLaunch);
    assert_eq!(process.launches.load(Ordering::Acquire), 0);
    assert!(
        manager
            .confirm_with(&job.job_id, |_| panic!("cancelled job prompted"))
            .is_err()
    );
    manager.release(&job.job_id).unwrap();
    let job = prepare(&manager);
    manager
        .shared
        .state
        .lock()
        .unwrap()
        .ledger
        .journals
        .last_mut()
        .unwrap()
        .job
        .created_at = now() - EXPIRY;
    assert_eq!(
        manager
            .confirm_with(&job.job_id, |_| panic!("expired job prompted"))
            .unwrap_err(),
        VendorJobError::Expired
    );
    drop(manager);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn spanish_native_confirmation_is_translated_and_debug_quotes_names() {
    let process = Process::new(0, Some(0));
    let (manager, _, root) = setup(process.clone());
    let job = prepare(&manager);
    let spanish = crate::i18n::strings(crate::i18n::Locale::Es419);
    let denied = manager
        .confirm_with_strings(&job.job_id, spanish, |message| {
            assert!(message.starts_with(
                "¿Ejecutar la desinstalación del proveedor? Esto no se puede deshacer.\n"
            ));
            assert!(message.contains("Programa: \"Disposable fixture\"\n"));
            assert!(message.contains(&format!("ID de trabajo: {}\n", job.job_id)));
            assert!(message.contains("Familia: Ejecutable del proveedor\n"));
            assert!(message.contains("Ejecutable: \"C:\\\\Fixture\\\\uninstall.exe\""));
            assert!(message.contains("Argumentos: [\"/remove\"]"));
            assert!(message.contains("Cancelar la espera no detiene el instalador del proveedor."));
            assert!(!message.contains("cannot be undone"));
            false
        })
        .unwrap();
    assert_eq!(denied.state, VendorJobState::CancelledBeforeLaunch);
    assert_eq!(process.launches.load(Ordering::Acquire), 0);
    drop(manager);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_confirmation_yes_rechecks_after_prompt_without_host_launch() {
    let process = Process::new(0, Some(0));
    let (manager, registry, root) = setup(process.clone());
    let job = prepare(&manager);
    assert_eq!(
        manager
            .confirm_with(&job.job_id, |_| {
                *registry.command.lock().unwrap() = r"C:\Fixture\changed.exe".into();
                true
            })
            .unwrap_err(),
        VendorJobError::RegistryChanged
    );
    assert_eq!(process.launches.load(Ordering::Acquire), 0);
    *registry.command.lock().unwrap() = r"C:\Fixture\uninstall.exe /remove".into();
    manager.confirm_with(&job.job_id, |_| true).unwrap();
    finished(&manager, &job.job_id);
    assert_eq!(process.launches.load(Ordering::Acquire), 1);
    assert!(
        manager
            .confirm_with(&job.job_id, |_| panic!("terminal job prompted"))
            .is_err()
    );
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
fn retention_keeps_safe_results_and_pins_unknown_journals() {
    let (manager, _, root) = setup(Process::new(1, None));
    for _ in 0..5 {
        let job = prepare(&manager);
        manager.confirm(&job.job_id).unwrap();
        finished(&manager, &job.job_id);
    }
    assert_eq!(all_history(&manager).unwrap().len(), 5);
    {
        let mut state = manager.shared.state.lock().unwrap();
        for journal in &mut state.ledger.journals {
            journal.job.updated_at = 0;
        }
        expire_prepared(&mut state);
        assert_eq!(state.ledger.journals.len(), 5);
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
        expire_prepared(&mut state);
        assert_eq!(state.ledger.journals.len(), MAX_JOURNALS);
    }
    assert_eq!(
        prepare_result(&manager).unwrap_err(),
        VendorJobError::HistoryCountCapacity
    );
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

fn reopen(
    manager: VendorJobManager,
    registry: Arc<Registry>,
    process: Arc<Process>,
) -> VendorJobManager {
    let storage = manager.shared.storage.clone();
    drop(manager);
    VendorJobManager::with_boundaries(
        storage,
        Arc::new(Mutex::new(())),
        registry,
        process,
        Duration::from_secs(1),
    )
    .unwrap()
}

fn seed_history(manager: &VendorJobManager, count: usize) {
    let job = prepare(manager);
    let mut state = manager.shared.state.lock().unwrap();
    let prototype = lookup(&state, &job.job_id).unwrap().clone();
    state.active = None;
    state.ledger.journals = (0..count)
        .map(|index| {
            let mut journal = prototype.clone();
            journal.job.job_id = format!("{index:032x}");
            journal.job.created_at = (index / 3) as u64; // deliberate timestamp ties
            journal.job.updated_at = 0;
            journal.job.state = if index % 2 == 0 {
                VendorJobState::Completed
            } else {
                VendorJobState::CancelledBeforeLaunch
            };
            journal.program.location.subkey = format!("other-{index}");
            journal
        })
        .collect();
    manager
        .shared
        .storage
        .write_vendor_jobs(&state.ledger)
        .unwrap();
}

#[test]
fn history_pages_traverse_ties_updates_and_restart_without_replay() {
    let process = Process::new(0, Some(0));
    let (manager, registry, root) = setup(process.clone());
    seed_history(&manager, 135);
    {
        let mut state = manager.shared.state.lock().unwrap();
        state.ledger.journals.last_mut().unwrap().job.state = VendorJobState::Launching;
        manager
            .shared
            .storage
            .write_vendor_jobs(&state.ledger)
            .unwrap();
    }
    let manager = reopen(manager, registry.clone(), process.clone());
    let first = manager.history_page(HistoryRequest::default()).unwrap();
    assert_eq!(first.records.len(), 64);
    assert_eq!(first.records[0].job_id, format!("{:032x}", 134));
    assert_eq!(first.records[0].state, VendorJobState::OutcomeUnknown);
    assert!(manager.confirm(&first.records[0].job_id).is_err());
    assert_eq!(
        manager.release(&first.records[0].job_id),
        Err(VendorJobError::Busy)
    );
    let mut ids: Vec<_> = first.records.into_iter().map(|j| j.job_id).collect();
    let mut cursor = first.next_cursor;
    // Updating an old outcome must not move its immutable ordering key. A newly
    // prepared record belongs only to a refreshed first page, not this traversal.
    {
        let mut state = manager.shared.state.lock().unwrap();
        let mut old = state.ledger.journals[0].job.clone();
        old.updated_at = u64::MAX;
        save_job(&manager.shared, &mut state, old).unwrap();
    }
    let new = prepare(&manager);
    manager.release(&new.job_id).unwrap();
    let manager = reopen(manager, registry, process.clone());
    while let Some(boundary) = cursor {
        let page = manager
            .history_page(HistoryRequest {
                cursor: Some(boundary),
                limit: None,
            })
            .unwrap();
        assert!(page.records.len() <= 64);
        ids.extend(page.records.into_iter().map(|j| j.job_id));
        cursor = page.next_cursor;
    }
    assert_eq!(
        ids,
        (0..135)
            .rev()
            .map(|i| format!("{i:032x}"))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        manager
            .history_page(HistoryRequest::default())
            .unwrap()
            .records[0]
            .job_id,
        new.job_id
    );
    assert_eq!(all_history(&manager).unwrap().len(), 136);
    for request in [
        HistoryRequest {
            cursor: None,
            limit: Some(0),
        },
        HistoryRequest {
            cursor: None,
            limit: Some(65),
        },
        HistoryRequest {
            cursor: Some("bad".into()),
            limit: None,
        },
        HistoryRequest {
            cursor: Some(r#"{"version":1,"kind":"vendor","timestamp":0,"id":"../invalid"}"#.into()),
            limit: None,
        },
        HistoryRequest {
            cursor: Some(HistoryCursor::encode(HistoryKind::Cleanup, 0, &"a".repeat(32)).unwrap()),
            limit: None,
        },
    ] {
        assert_eq!(
            manager.history_page(request).unwrap_err(),
            VendorJobError::InvalidHistoryRequest
        );
    }
    assert_eq!(process.launches.load(Ordering::Acquire), 0);
    drop(manager);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn expired_preparation_and_released_cancellation_survive_restart() {
    let process = Process::new(0, Some(0));
    let (manager, registry, root) = setup(process.clone());
    seed_history(&manager, 70);
    let expired = prepare(&manager);
    {
        let mut state = manager.shared.state.lock().unwrap();
        state.ledger.journals.last_mut().unwrap().job.created_at = 0;
    }
    assert_eq!(
        manager.status(&expired.job_id).unwrap().state,
        VendorJobState::CancelledBeforeLaunch
    );
    assert!(manager.shared.state.lock().unwrap().active.is_none());
    let cancelled = prepare(&manager);
    manager.cancel(&cancelled.job_id).unwrap();
    manager.release(&cancelled.job_id).unwrap();
    let manager = reopen(manager, registry, process.clone());
    assert_eq!(all_history(&manager).unwrap().len(), 72);
    for id in [&expired.job_id, &cancelled.job_id] {
        assert_eq!(
            manager.status(id).unwrap().state,
            VendorJobState::CancelledBeforeLaunch
        );
        assert!(manager.confirm(id).is_err());
    }
    assert_eq!(process.launches.load(Ordering::Acquire), 0);
    drop(manager);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn count_and_size_admission_preserve_bytes_release_reservation_and_restart() {
    for size_limited in [false, true] {
        let process = Process::new(0, Some(0));
        let (manager, registry, root) = setup(process.clone());
        let count = if size_limited { 2 } else { MAX_JOURNALS };
        seed_history(&manager, count);
        if size_limited {
            let mut state = manager.shared.state.lock().unwrap();
            let size = serde_json::to_vec(&state.ledger).unwrap().len();
            // A legal existing file, but no room for another journal or its transitions.
            state.ledger.journals[0]
                .job
                .program_name
                .push_str(&"x".repeat(MAX_LEDGER_BYTES - size - 128));
            manager
                .shared
                .storage
                .write_vendor_jobs(&state.ledger)
                .unwrap();
        }
        let path = root.join("journal/vendor-jobs.json");
        let bytes = std::fs::read(&path).unwrap();
        let expected = if size_limited {
            VendorJobError::HistorySizeCapacity
        } else {
            VendorJobError::HistoryCountCapacity
        };
        for _ in 0..2 {
            assert_eq!(prepare_result(&manager).unwrap_err(), expected);
            assert!(manager.shared.state.lock().unwrap().active.is_none());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
            assert_eq!(all_history(&manager).unwrap().len(), count);
        }
        let manager = reopen(manager, registry, process.clone());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(all_history(&manager).unwrap().len(), count);
        assert_eq!(process.launches.load(Ordering::Acquire), 0);
        drop(manager);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn admission_headroom_covers_maximal_transition_and_restart_growth() {
    let process = Process::new(0, Some(0));
    let (manager, registry, root) = setup(process.clone());
    seed_history(&manager, 3);
    {
        let mut state = manager.shared.state.lock().unwrap();
        for journal in &mut state.ledger.journals {
            journal.job.state = VendorJobState::Launching;
            journal.job.completion_meaning.clear();
        }
        let size = serde_json::to_vec(&state.ledger).unwrap().len();
        let padding = MAX_LEDGER_BYTES - size - 3 * TRANSITION_HEADROOM;
        state.ledger.journals[0]
            .job
            .program_name
            .push_str(&"x".repeat(padding));
        check_admission(&state.ledger).unwrap();
        state.ledger.journals[0].job.program_name.push('x');
        assert_eq!(
            check_admission(&state.ledger),
            Err(VendorJobError::HistorySizeCapacity)
        );
        state.ledger.journals[0].job.program_name.pop();
        for journal in &mut state.ledger.journals {
            journal.job.exit_code = Some(u32::MAX);
            journal.job.launch_error = Some(u32::MAX);
            journal.job.updated_at = u64::MAX;
        }
        manager
            .shared
            .storage
            .write_vendor_jobs(&state.ledger)
            .unwrap();
    }
    let manager = reopen(manager, registry, process.clone());
    assert!(
        all_history(&manager)
            .unwrap()
            .iter()
            .all(|j| j.state == VendorJobState::OutcomeUnknown)
    );
    assert_eq!(process.launches.load(Ordering::Acquire), 0);
    drop(manager);
    std::fs::remove_dir_all(root).unwrap();
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

// Observations of the real MessageBoxW consent window, read from Windows itself.
struct DialogObservation {
    owner_matches: bool,
    default_id: i32,
    has_yes: bool,
    has_no: bool,
}
// A genuine, process-owned top-level window to own the real dialog. The predefined STATIC
// class needs no window procedure, so the owner is real without a test-only message pump.
fn owner_window() -> isize {
    use windows_sys::Win32::UI::WindowsAndMessaging::CreateWindowExW;
    let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
    let name: Vec<u16> = "Vendor confirmation owner\0".encode_utf16().collect();
    let owner = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            name.as_ptr(),
            0,
            0,
            0,
            16,
            16,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
        )
    };
    assert!(!owner.is_null(), "owner window must be created");
    owner as isize
}
// Supplies only the answer a human would click, to the real dialog, after recording what
// Windows reports about it. No production consent check is bypassed or relaxed.
fn drive_real_dialog(owner: isize, answer: i32) -> std::thread::JoinHandle<DialogObservation> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GW_OWNER, GetDlgItem, GetWindow, IDNO, IDYES, PostMessageW, SendMessageW,
        WM_COMMAND,
    };
    const DM_GETDEFID: u32 = 0x0400;
    const DC_HASDEFID: isize = 0x534B;
    std::thread::spawn(move || {
        // The title follows the active language, which is machine- and setting-dependent.
        let titles: Vec<Vec<u16>> = [crate::i18n::Locale::En, crate::i18n::Locale::Es419]
            .into_iter()
            .map(|locale| {
                crate::i18n::strings(locale)
                    .confirm_vendor_uninstall_title
                    .encode_utf16()
                    .chain(Some(0))
                    .collect()
            })
            .collect();
        let deadline = Instant::now() + Duration::from_secs(60);
        let dialog = 'found: loop {
            for title in &titles {
                let found = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
                if !found.is_null() && unsafe { GetWindow(found, GW_OWNER) } as isize == owner {
                    break 'found found;
                }
            }
            assert!(
                Instant::now() < deadline,
                "the real confirmation dialog never appeared"
            );
            std::thread::sleep(Duration::from_millis(20));
        };
        let defid = unsafe { SendMessageW(dialog, DM_GETDEFID, 0, 0) };
        let observation = DialogObservation {
            owner_matches: true,
            default_id: if (defid >> 16) & 0xFFFF == DC_HASDEFID {
                (defid & 0xFFFF) as i32
            } else {
                -1
            },
            has_yes: !unsafe { GetDlgItem(dialog, IDYES) }.is_null(),
            has_no: !unsafe { GetDlgItem(dialog, IDNO) }.is_null(),
        };
        unsafe { PostMessageW(dialog, WM_COMMAND, answer as usize, 0) };
        observation
    })
}
#[test]
fn real_confirmation_dialog_is_app_owned_defaults_to_no_and_drives_both_answers() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyWindow, IDNO, IDYES};
    for answer in [IDNO, IDYES] {
        let (manager, registry, root) = setup(Arc::new(NativeProcessBoundary));
        let executable = root.join("vendor-disposable.exe");
        std::fs::copy(fixture(), &executable).unwrap();
        *registry.command.lock().unwrap() =
            format!("\"{}\" \"{}\" 0 0", executable.display(), root.display());
        let job = prepare(&manager);
        let owner = owner_window();
        let watcher = drive_real_dialog(owner, answer);
        // Opens the actual MessageBoxW consent window on this thread.
        let result = manager.confirm_native(&job.job_id, owner);
        let observed = watcher.join().unwrap();
        unsafe { DestroyWindow(owner as _) };
        assert!(observed.owner_matches, "dialog must be owned by our window");
        assert_eq!(observed.default_id, IDNO, "real dialog must default to No");
        assert!(observed.has_yes && observed.has_no);
        if answer == IDNO {
            assert_eq!(
                result.unwrap().state,
                VendorJobState::CancelledBeforeLaunch,
                "denying the real dialog must cancel before launch"
            );
            std::thread::sleep(Duration::from_millis(500));
            assert!(
                !root.join("started").exists(),
                "denial must launch nothing at all"
            );
        } else {
            result.unwrap();
            until(|| root.join("started").exists());
            assert_eq!(
                finished(&manager, &job.job_id).state,
                VendorJobState::OutcomeUnknown
            );
            until(|| root.join("finished").exists());
        }
        drop(manager);
        until(|| std::fs::remove_dir_all(&root).is_ok());
    }
}

// A disposable uninstall entry this test exclusively owns, under the CURRENT USER hive only.
// It never touches HKLM and never names or modifies a real installed program.
struct OwnedHkcuEntry {
    subkey: Vec<u16>,
    name: String,
}
impl OwnedHkcuEntry {
    fn register(command: &str) -> Self {
        use windows_sys::Win32::System::Registry::{
            HKEY_CURRENT_USER, KEY_WRITE, REG_OPTION_NON_VOLATILE, RegCloseKey, RegCreateKeyExW,
            RegSetValueExW,
        };
        // Unique per run, so the entry can never collide with or shadow a real program.
        let name = format!(
            "ZZ Disposable Vendor Fixture {}",
            super::super::opaque_id().unwrap()
        );
        let subkey: Vec<u16> =
            format!(r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{name}")
                .encode_utf16()
                .chain(Some(0))
                .collect();
        let mut key = std::ptr::null_mut();
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                std::ptr::null(),
                &mut key,
                std::ptr::null_mut(),
            )
        };
        assert_eq!(status, 0, "owned HKCU fixture key must be created");
        for (value, data) in [("DisplayName", name.as_str()), ("UninstallString", command)] {
            let value_name: Vec<u16> = format!("{value}\0").encode_utf16().collect();
            let bytes: Vec<u8> = data
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect();
            let status = unsafe {
                RegSetValueExW(
                    key,
                    value_name.as_ptr(),
                    0,
                    REG_SZ,
                    bytes.as_ptr(),
                    bytes.len() as u32,
                )
            };
            assert_eq!(status, 0, "fixture value must be written");
        }
        unsafe { RegCloseKey(key) };
        Self { subkey, name }
    }
    // Removing our own registration is not an uninstall of anything; nothing else is touched.
    fn unregister(&self) {
        use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RegDeleteTreeW};
        let status = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, self.subkey.as_ptr()) };
        assert!(
            status == 0 || status == 2,
            "fixture cleanup failed: {status}"
        );
    }
}
impl Drop for OwnedHkcuEntry {
    fn drop(&mut self) {
        self.unregister();
    }
}
// Runs the real inventory the UI's refresh calls, returning only records matching our fixture.
fn inventory_matching(
    manager: &VendorJobManager,
    storage: &super::super::scans::StorageService,
    name: &str,
) -> (String, Vec<super::super::uninstaller::InstalledProgram>) {
    use cleanup_core::storage::*;
    let snapshot_id = manager
        .start_inventory(
            storage,
            StorageLimits::default(),
            super::super::uninstaller::ProgramQuery {
                name_contains: name.to_string(),
                largest_first: false,
            },
        )
        .unwrap();
    until(|| storage.status(&snapshot_id).unwrap().0.phase == StoragePhase::Complete);
    let mut found = Vec::new();
    let mut cursor = None;
    loop {
        let page = storage
            .page(&PageRequest {
                snapshot_id: snapshot_id.clone(),
                module: StorageModule::Uninstaller,
                collection: PageCollection::Programs,
                parent_id: None,
                cursor,
                page_size: 64,
            })
            .unwrap();
        for record in &page.records {
            if let StorageRecord::Program(program) = record
                && program.name == name
            {
                found.push(program.clone());
            }
        }
        cursor = page.next_cursor;
        if cursor.is_none() {
            return (snapshot_id, found);
        }
    }
}
#[test]
fn owned_hkcu_entry_flows_through_real_command_path_and_refresh_after_removal() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyWindow, IDNO, IDYES};
    let root = temp();
    let executable = root.join("vendor-disposable.exe");
    std::fs::copy(fixture(), &executable).unwrap();
    let entry = OwnedHkcuEntry::register(&format!(
        "\"{}\" \"{}\" 0 0",
        executable.display(),
        root.display()
    ));
    // Production boundaries only: the real registry reader and the real launcher.
    let manager = VendorJobManager::with_boundaries(
        CleanupStorage::open(root.join("journal")).unwrap(),
        Arc::new(Mutex::new(())),
        Arc::new(super::super::uninstaller::NativeRegistry),
        Arc::new(NativeProcessBoundary),
        Duration::from_secs(10),
    )
    .unwrap();
    let storage = super::super::scans::StorageService::new();

    // 1. The live host registry resolves our disposable entry through the real inventory.
    let (snapshot_id, found) = inventory_matching(&manager, &storage, &entry.name);
    assert_eq!(found.len(), 1, "fixture must resolve exactly once");

    // 2. Prepare and the real consent dialog identify that backend-resolved program.
    let job = manager
        .prepare(&storage, &snapshot_id, &found[0].program_id)
        .unwrap();
    assert_eq!(job.program_name, entry.name);
    let owner = owner_window();
    let watcher = drive_real_dialog(owner, IDYES);
    manager.confirm_native(&job.job_id, owner).unwrap();
    let observed = watcher.join().unwrap();
    unsafe { DestroyWindow(owner as _) };
    assert_eq!(observed.default_id, IDNO, "real dialog defaults to No");

    // 3. Only the revalidated fixture launches, and the outcome stays truthful.
    until(|| root.join("started").exists());
    let completed = finished(&manager, &job.job_id);
    assert_eq!(completed.state, VendorJobState::OutcomeUnknown);
    until(|| root.join("finished").exists());
    let history_before = all_history(&manager).unwrap().len();

    // 4. Restart over the same journal must not replay the command.
    std::fs::remove_file(root.join("started")).unwrap();
    std::fs::remove_file(root.join("finished")).unwrap();
    let persisted = manager.shared.storage.clone();
    drop(manager);
    let manager = VendorJobManager::with_boundaries(
        persisted,
        Arc::new(Mutex::new(())),
        Arc::new(super::super::uninstaller::NativeRegistry),
        Arc::new(NativeProcessBoundary),
        Duration::from_secs(10),
    )
    .unwrap();
    assert_eq!(
        manager.status(&job.job_id).unwrap().state,
        VendorJobState::OutcomeUnknown
    );
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        !root.join("started").exists(),
        "restart must never replay a vendor command"
    );
    assert_eq!(all_history(&manager).unwrap().len(), history_before);

    // 5. Refresh after the fixture registration really disappears: inventory changes,
    //    but nothing about the recorded job implies an uninstall happened.
    entry.unregister();
    let (_, after) = inventory_matching(&manager, &storage, &entry.name);
    assert!(
        after.is_empty(),
        "refresh must reflect the removed fixture registration"
    );
    let refreshed = manager.status(&job.job_id).unwrap();
    assert_eq!(
        refreshed.state,
        VendorJobState::OutcomeUnknown,
        "refresh must not upgrade an unknown outcome into a success"
    );
    assert_eq!(refreshed.exit_code, completed.exit_code);
    assert_eq!(all_history(&manager).unwrap().len(), history_before);
    drop(manager);
    until(|| std::fs::remove_dir_all(&root).is_ok());
}

// -----------------------------------------------------------------------------------------
// HUMAN-INTERACTIVE ONLY. Not run in CI or by `cargo test` without `-- --ignored`.
// This drives the real elevation prompt, which renders on the secure desktop and cannot be
// answered by any program. Run with:
//   cargo +1.90.0 test --manifest-path Cargo.toml -p windows-platform --lib \
//       storage::vendor_jobs::tests::human_uac_prompt_accept_then_deny -- --ignored --nocapture
// and click the two prompts exactly as the printed instructions say.
// -----------------------------------------------------------------------------------------
fn mt_exe() -> PathBuf {
    for candidate in [
        r"C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\mt.exe",
        r"C:\Program Files (x86)\Windows Kits\10\bin\10.0.19041.0\x64\mt.exe",
    ] {
        let path = PathBuf::from(candidate);
        if path.exists() {
            return path;
        }
    }
    panic!("Microsoft Manifest Tool (mt.exe) not found; install the Windows SDK to run this");
}
// An elevation-requiring build of the SAME harmless disposable fixture source, with a
// `requireAdministrator` manifest embedded by the real Microsoft Manifest Tool. Only the
// manifest differs from the fixture already covered by automated tests.
fn elevated_fixture(root: &std::path::Path) -> PathBuf {
    let exe = root.join("vendor-disposable-elevated.exe");
    vendor_uninstall::compile_vendor_fixture(&exe);
    let manifest = root.join("require-admin.manifest");
    std::fs::write(
        &manifest,
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="requireAdministrator" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
</assembly>
"#,
    )
    .unwrap();
    vendor_uninstall::embed_fixture_manifest(&mt_exe(), &manifest, &exe);
    exe
}
#[test]
#[ignore = "requires a human to click the real elevation prompt on the secure desktop"]
fn human_uac_prompt_accept_then_deny() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyWindow, IDYES};
    for (scenario, tell_human) in [
        (
            "ACCEPT",
            "the Windows permission prompt titled about vendor-disposable-elevated.exe: click YES",
        ),
        (
            "DENY",
            "the Windows permission prompt titled about vendor-disposable-elevated.exe: click NO",
        ),
    ] {
        eprintln!(
            "\n=== Scenario {scenario} ===\n1. A real confirmation dialog opens; it is answered automatically (already covered by an automated test).\n2. A REAL Windows permission prompt will then appear. On {tell_human}\nWaiting for the prompt now...\n"
        );
        let root = temp();
        let exe = elevated_fixture(&root);
        let entry =
            OwnedHkcuEntry::register(&format!("\"{}\" \"{}\" 0 0", exe.display(), root.display()));
        let manager = VendorJobManager::with_boundaries(
            CleanupStorage::open(root.join("journal")).unwrap(),
            Arc::new(Mutex::new(())),
            Arc::new(super::super::uninstaller::NativeRegistry),
            Arc::new(NativeProcessBoundary),
            Duration::from_secs(120),
        )
        .unwrap();
        let storage = super::super::scans::StorageService::new();
        let (snapshot_id, found) = inventory_matching(&manager, &storage, &entry.name);
        assert_eq!(found.len(), 1);
        let job = manager
            .prepare(&storage, &snapshot_id, &found[0].program_id)
            .unwrap();
        let owner = owner_window();
        let watcher = drive_real_dialog(owner, IDYES);
        // This call blocks on our own real dialog, then on the real, human-only UAC prompt.
        manager.confirm_native(&job.job_id, owner).unwrap();
        watcher.join().unwrap();
        unsafe { DestroyWindow(owner as _) };
        let deadline = std::time::Instant::now() + Duration::from_secs(120);
        let result = loop {
            let status = manager.status(&job.job_id).unwrap();
            if !status.state.active() {
                break status;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for a human response to the elevation prompt"
            );
            std::thread::sleep(Duration::from_millis(100));
        };
        if scenario == "ACCEPT" {
            assert_eq!(
                result.state,
                VendorJobState::OutcomeUnknown,
                "accepted elevation must launch the fixture and stay truthful about outcome"
            );
            assert!(root.join("started").exists());
            eprintln!(
                "ACCEPT verified: elevation granted, fixture launched, outcome stayed truthful."
            );
        } else {
            assert_eq!(
                result.state,
                VendorJobState::CancelledByVendorOrUAC,
                "denied elevation must be reported truthfully, not as success or silent failure"
            );
            assert!(
                !root.join("started").exists(),
                "denied elevation must launch nothing"
            );
            eprintln!(
                "DENY verified: elevation denied, mapped to CancelledByVendorOrUAC, nothing launched."
            );
        }
        drop(manager);
        until(|| std::fs::remove_dir_all(&root).is_ok());
    }
}
