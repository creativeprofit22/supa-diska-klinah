use cleanup_core::{
    ArtifactSnapshot, BudgetContext, BudgetDecision, BudgetLimits, CandidateProofScope,
    CleanupRule, EntryKind, FileSystem, FsErrorKind, GenerationState, Lifecycle, Markers,
    ProtectedArtifactPath, Provenance, ResolvedCandidate, Risk, RuleRoot, ScannerKind, TargetType,
    select_artifact_generations, snapshot_registered_path,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    ffi::OsStr,
    mem::size_of,
    os::windows::{
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
        process::CommandExt,
    },
    path::{Component, Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use windows_sys::Win32::{
    Foundation::{
        ERROR_INVALID_PARAMETER, ERROR_MORE_DATA, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
        WAIT_OBJECT_0,
    },
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
        },
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_BASIC_PROCESS_ID_LIST,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectBasicProcessIdList,
            JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
        },
        Threading::{
            CREATE_SUSPENDED, OpenProcess, OpenThread, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, ResumeThread, THREAD_SUSPEND_RESUME,
            TerminateProcess, WaitForSingleObject,
        },
    },
    UI::WindowsAndMessaging::{IDYES, MB_DEFBUTTON2, MB_ICONWARNING, MB_YESNO, MessageBoxW},
};

use super::{
    execution::execute_registered_artifact_plan,
    filesystem::{WindowsFileSystem, wide},
    storage::{
        ArtifactBudgetPolicy, ArtifactGenerationLedger, BuildEcosystem, BuildProfile, CleanupPlan,
        CleanupStorage, PlanItem, ProjectBudgetOverride, RegisteredArtifactPath, StorageError,
        SuccessfulBuildStamp,
    },
};

const MAX_RUNS: usize = 100;
const POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegisterBuildProfileInput {
    pub root_id: String,
    pub display_name: String,
    pub ecosystem: BuildEcosystem,
    pub executable: String,
    pub argv: Vec<String>,
    pub working_directory: String,
    pub profile_label: String,
    pub toolchain_label: String,
    pub target_label: String,
    pub rebuild_cost: cleanup_core::RebuildCost,
    pub artifact_paths: Vec<RegisteredArtifactPath>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BuildRunState {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    AnalysisFailed,
}

impl BuildRunState {
    fn terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::AnalysisFailed
        )
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildRun {
    pub run_id: String,
    pub profile_id: String,
    pub state: BuildRunState,
    pub started_at: Option<u64>,
    pub completed_at: Option<u64>,
    pub exit_code: Option<i32>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactBudgetPreview {
    pub enabled: bool,
    pub decision: BudgetDecision,
    pub generations: Vec<GenerationState>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ArtifactBudgetAnalysisStatus {
    NotRequested,
    Completed,
    Skipped,
    Failed,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetArtifactBudgetPolicyResult {
    pub policy_saved: bool,
    pub policy: ArtifactBudgetPolicy,
    pub analysis_status: ArtifactBudgetAnalysisStatus,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuildArtifactError {
    InvalidInput,
    ApprovalDeclined,
    NotFound,
    BuildBusy,
    ValidationFailed,
    OperationFailed,
}

pub trait ProfileApprover: Send + Sync {
    fn approve(&self, profile: &BuildProfile) -> Result<bool, BuildArtifactError>;
}

#[derive(Default)]
pub struct NativeProfileApprover;

impl ProfileApprover for NativeProfileApprover {
    fn approve(&self, profile: &BuildProfile) -> Result<bool, BuildArtifactError> {
        let mut message = format!(
            "Allow this repeatable build profile?\n\nExecutable:\n{}\n\nArguments:",
            profile.executable
        );
        if profile.argv.is_empty() {
            message.push_str("\n(none)");
        } else {
            for argument in &profile.argv {
                message.push_str("\n• ");
                message.push_str(argument);
            }
        }
        message.push_str("\n\nWorking directory:\n");
        message.push_str(&profile.working_directory);
        message.push_str("\n\nArtifact paths:");
        for artifact in &profile.artifact_paths {
            message.push_str("\n• ");
            message.push_str(&artifact.relative_path);
        }
        let message =
            wide(OsStr::new(&message)).map_err(|_| BuildArtifactError::OperationFailed)?;
        let title = wide(OsStr::new("Approve build profile"))
            .map_err(|_| BuildArtifactError::OperationFailed)?;
        // SAFETY: both strings are valid, NUL-terminated UTF-16 and no owner handle is required.
        let result = unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                message.as_ptr(),
                title.as_ptr(),
                MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2,
            )
        };
        Ok(result == IDYES)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessOutcome {
    Exited(i32),
    Cancelled,
}

#[derive(Clone, Copy)]
enum SuccessfulProcessing {
    Succeeded,
    Failed(i32),
    Cancelled,
    AnalysisFailed,
}

pub trait ProcessRunner: Send + Sync {
    fn run(
        &self,
        executable: &Path,
        argv: &[String],
        working_directory: &Path,
        cancelled: &AtomicBool,
    ) -> Result<ProcessOutcome, BuildArtifactError>;
}

#[derive(Default)]
pub struct NativeProcessRunner;

impl ProcessRunner for NativeProcessRunner {
    fn run(
        &self,
        executable: &Path,
        argv: &[String],
        working_directory: &Path,
        cancelled: &AtomicBool,
    ) -> Result<ProcessOutcome, BuildArtifactError> {
        if cancelled.load(Ordering::Acquire) {
            return Ok(ProcessOutcome::Cancelled);
        }
        let job = create_kill_on_close_job()?;
        let mut child = Command::new(executable)
            .args(argv)
            .current_dir(working_directory)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_SUSPENDED)
            .spawn()
            .map_err(|_| BuildArtifactError::OperationFailed)?;
        // SAFETY: both handles are live; the suspended child cannot create descendants yet.
        if unsafe {
            AssignProcessToJobObject(
                job.as_raw_handle() as HANDLE,
                child.as_raw_handle() as HANDLE,
            )
        } == 0
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err(BuildArtifactError::OperationFailed);
        }
        if resume_process(child.id()).is_err() {
            let _ = stop_process_job(&job, &mut child);
            return Err(BuildArtifactError::OperationFailed);
        }
        loop {
            if cancelled.load(Ordering::Acquire) {
                stop_process_job(&job, &mut child)?;
                return Ok(ProcessOutcome::Cancelled);
            }
            if let Some(status) = child
                .try_wait()
                .map_err(|_| BuildArtifactError::OperationFailed)?
            {
                let exit_code = status.code().unwrap_or(-1);
                stop_process_job(&job, &mut child)?;
                return Ok(ProcessOutcome::Exited(exit_code));
            }
            thread::sleep(POLL_INTERVAL);
        }
    }
}

fn create_kill_on_close_job() -> Result<OwnedHandle, BuildArtifactError> {
    // SAFETY: null security attributes and name request a private job object.
    let raw_job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if raw_job.is_null() {
        return Err(BuildArtifactError::OperationFailed);
    }
    // SAFETY: CreateJobObjectW returned an owned handle.
    let job = unsafe { OwnedHandle::from_raw_handle(raw_job) };
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: job is live and limits points to the declared information structure.
    if unsafe {
        SetInformationJobObject(
            job.as_raw_handle() as HANDLE,
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    } == 0
    {
        return Err(BuildArtifactError::OperationFailed);
    }
    Ok(job)
}

fn resume_process(process_id: u32) -> Result<(), BuildArtifactError> {
    // SAFETY: no borrowed pointers are passed; the returned snapshot is owned.
    let raw_snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if raw_snapshot == INVALID_HANDLE_VALUE {
        return Err(BuildArtifactError::OperationFailed);
    }
    // SAFETY: CreateToolhelp32Snapshot returned an owned handle.
    let snapshot = unsafe { OwnedHandle::from_raw_handle(raw_snapshot) };
    let mut entry = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    // SAFETY: snapshot is live and entry has the required size.
    if unsafe { Thread32First(snapshot.as_raw_handle() as HANDLE, &mut entry) } == 0 {
        return Err(BuildArtifactError::OperationFailed);
    }
    loop {
        if entry.th32OwnerProcessID == process_id {
            // SAFETY: the thread ID belongs to the suspended child process.
            let raw_thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
            if raw_thread.is_null() {
                return Err(BuildArtifactError::OperationFailed);
            }
            // SAFETY: OpenThread returned an owned handle.
            let thread = unsafe { OwnedHandle::from_raw_handle(raw_thread) };
            // SAFETY: thread is the suspended main thread and remains live for this call.
            return (unsafe { ResumeThread(thread.as_raw_handle() as HANDLE) } != u32::MAX)
                .then_some(())
                .ok_or(BuildArtifactError::OperationFailed);
        }
        // SAFETY: snapshot and entry remain valid across enumeration calls.
        if unsafe { Thread32Next(snapshot.as_raw_handle() as HANDLE, &mut entry) } == 0 {
            return Err(BuildArtifactError::OperationFailed);
        }
    }
}

fn stop_process_job(job: &OwnedHandle, child: &mut Child) -> Result<(), BuildArtifactError> {
    // Retain the complete snapshot before terminating anything: killing Cargo closes
    // its nested job and can remove descendants from the list before they signal.
    // Wait on every retained handle, then re-query for racing descendants.
    let deadline = Instant::now() + Duration::from_secs(5);
    let header_words =
        std::mem::offset_of!(JOBOBJECT_BASIC_PROCESS_ID_LIST, ProcessIdList) / size_of::<usize>();
    let mut buffer = vec![0usize; header_words + 16];
    loop {
        if Instant::now() >= deadline {
            return Err(BuildArtifactError::OperationFailed);
        }
        let bytes = u32::try_from(buffer.len() * size_of::<usize>())
            .map_err(|_| BuildArtifactError::OperationFailed)?;
        // SAFETY: the usize buffer is aligned for the header and variable-length IDs.
        let queried = unsafe {
            QueryInformationJobObject(
                job.as_raw_handle() as HANDLE,
                JobObjectBasicProcessIdList,
                buffer.as_mut_ptr().cast(),
                bytes,
                std::ptr::null_mut(),
            )
        };
        if queried == 0 {
            // SAFETY: read the error immediately after the failed query.
            if unsafe { GetLastError() } != ERROR_MORE_DATA {
                return Err(BuildArtifactError::OperationFailed);
            }
            let growth = buffer.len();
            buffer
                .try_reserve_exact(growth)
                .map_err(|_| BuildArtifactError::OperationFailed)?;
            buffer.resize(buffer.len() + growth, 0);
            continue;
        }
        // SAFETY: the successful query initialized a properly aligned, full header.
        let processes = unsafe { &*buffer.as_ptr().cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>() };
        let count = processes.NumberOfProcessIdsInList as usize;
        if count != processes.NumberOfAssignedProcesses as usize
            || count > buffer.len() - header_words
        {
            return Err(BuildArtifactError::OperationFailed);
        }
        if count == 0 {
            return child
                .wait()
                .map(|_| ())
                .map_err(|_| BuildArtifactError::OperationFailed);
        }
        let mut retained = Vec::new();
        for &pid in &buffer[header_words..header_words + count] {
            // SAFETY: this ID came from the job, but membership is checked again below.
            let raw_process = unsafe {
                OpenProcess(
                    PROCESS_TERMINATE | PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                    0,
                    pid as u32,
                )
            };
            if raw_process.is_null() {
                // SAFETY: read the error immediately after OpenProcess failed.
                if unsafe { GetLastError() } == ERROR_INVALID_PARAMETER {
                    continue; // The process exited between enumeration and opening its handle.
                }
                return Err(BuildArtifactError::OperationFailed);
            }
            // SAFETY: OpenProcess returned an owned handle.
            let process = unsafe { OwnedHandle::from_raw_handle(raw_process) };
            let mut in_job = 0;
            // SAFETY: both handles and the output pointer are live.
            if unsafe {
                IsProcessInJob(
                    process.as_raw_handle() as HANDLE,
                    job.as_raw_handle() as HANDLE,
                    &mut in_job,
                )
            } == 0
            {
                return Err(BuildArtifactError::OperationFailed);
            }
            if in_job == 0 {
                continue; // Never terminate an unrelated process after PID reuse.
            }
            retained.push(process);
        }
        for process in &retained {
            // SAFETY: the retained process is a member of our private, non-breakaway job.
            // An already exiting process may reject termination; only a signalled handle
            // below establishes success, not the termination request or job accounting.
            unsafe { TerminateProcess(process.as_raw_handle() as HANDLE, 1) };
        }
        for process in &retained {
            let remaining = deadline
                .saturating_duration_since(Instant::now())
                .as_millis() as u32;
            // SAFETY: the handle stays live throughout the bounded wait.
            if unsafe { WaitForSingleObject(process.as_raw_handle() as HANDLE, remaining) }
                != WAIT_OBJECT_0
            {
                return Err(BuildArtifactError::OperationFailed);
            }
        }
    }
}

#[derive(Clone)]
struct ActiveRun {
    run_id: String,
    profile_id: String,
    cancelled: Arc<AtomicBool>,
}

#[derive(Default)]
struct RunRegistry {
    active: Option<ActiveRun>,
    runs: VecDeque<BuildRun>,
}

#[cfg(test)]
type BeforeBudgetEnforcement = Arc<dyn Fn(&AtomicBool) + Send + Sync>;
#[cfg(test)]
type BeforeRegisteredArtifactExecution = Arc<dyn Fn(&CleanupPlan) + Send + Sync>;

#[derive(Clone)]
pub struct BuildArtifactManager {
    storage: CleanupStorage,
    file_system: WindowsFileSystem,
    writer: Arc<Mutex<()>>,
    approver: Arc<dyn ProfileApprover>,
    runner: Arc<dyn ProcessRunner>,
    runs: Arc<Mutex<RunRegistry>>,
    #[cfg(test)]
    before_budget_enforcement: Option<BeforeBudgetEnforcement>,
    #[cfg(test)]
    before_registered_artifact_execution: Option<BeforeRegisteredArtifactExecution>,
}

impl BuildArtifactManager {
    pub fn new(storage: CleanupStorage, writer: Arc<Mutex<()>>) -> Self {
        Self::with_components(
            storage,
            writer,
            Arc::new(NativeProfileApprover),
            Arc::new(NativeProcessRunner),
        )
    }

    fn with_components(
        storage: CleanupStorage,
        writer: Arc<Mutex<()>>,
        approver: Arc<dyn ProfileApprover>,
        runner: Arc<dyn ProcessRunner>,
    ) -> Self {
        Self {
            storage,
            file_system: WindowsFileSystem,
            writer,
            approver,
            runner,
            runs: Arc::new(Mutex::new(RunRegistry::default())),
            #[cfg(test)]
            before_budget_enforcement: None,
            #[cfg(test)]
            before_registered_artifact_execution: None,
        }
    }

    #[cfg(test)]
    fn set_before_budget_enforcement(
        &mut self,
        hook: impl Fn(&AtomicBool) + Send + Sync + 'static,
    ) {
        self.before_budget_enforcement = Some(Arc::new(hook));
    }

    #[cfg(test)]
    fn set_before_registered_artifact_execution(
        &mut self,
        hook: impl Fn(&CleanupPlan) + Send + Sync + 'static,
    ) {
        self.before_registered_artifact_execution = Some(Arc::new(hook));
    }

    pub fn profiles(&self) -> Result<Vec<BuildProfile>, BuildArtifactError> {
        self.storage.build_profiles().map_err(map_storage_error)
    }

    pub fn policy(&self) -> Result<ArtifactBudgetPolicy, BuildArtifactError> {
        self.storage
            .artifact_budget_policy()
            .map_err(map_storage_error)
    }

    pub fn set_policy(
        &self,
        policy: ArtifactBudgetPolicy,
    ) -> Result<SetArtifactBudgetPolicyResult, BuildArtifactError> {
        if self.active_profile_id()?.is_some() {
            return Err(BuildArtifactError::BuildBusy);
        }
        {
            let _writer = self
                .writer
                .lock()
                .map_err(|_| BuildArtifactError::OperationFailed)?;
            self.storage
                .write_artifact_budget_policy(&policy)
                .map_err(map_storage_error)?;
        }
        let analysis_status = if policy.enabled {
            match self.analyze_external(true) {
                Ok(true) => ArtifactBudgetAnalysisStatus::Completed,
                Ok(false) => ArtifactBudgetAnalysisStatus::Skipped,
                Err(_) => ArtifactBudgetAnalysisStatus::Failed,
            }
        } else {
            ArtifactBudgetAnalysisStatus::NotRequested
        };
        Ok(SetArtifactBudgetPolicyResult {
            policy_saved: true,
            policy,
            analysis_status,
        })
    }
    pub fn preview_budget(&self) -> Result<ArtifactBudgetPreview, BuildArtifactError> {
        let policy = self.policy()?;
        let ledger = self
            .storage
            .artifact_generations()
            .map_err(map_storage_error)?;
        let decision = select_artifact_generations(
            &ledger.generations,
            &budget_context(
                &policy,
                &ledger,
                self.storage.project_roots().map_err(map_storage_error)?,
                self.storage.build_profiles().map_err(map_storage_error)?,
            ),
        )
        .map_err(|_| BuildArtifactError::ValidationFailed)?;
        Ok(ArtifactBudgetPreview {
            enabled: policy.enabled,
            decision,
            generations: ledger.generations,
        })
    }

    pub fn analyze_due_external(&self) -> Result<bool, BuildArtifactError> {
        self.analyze_external(false)
    }

    fn analyze_external(&self, force: bool) -> Result<bool, BuildArtifactError> {
        let policy = self.policy()?;
        if !policy.enabled || self.active_profile_id()?.is_some() {
            return Ok(false);
        }
        let now = now_seconds()?;
        let _writer = self
            .writer
            .lock()
            .map_err(|_| BuildArtifactError::OperationFailed)?;
        if self.active_profile_id()?.is_some() {
            return Ok(false);
        }
        let mut ledger = self
            .storage
            .artifact_generations()
            .map_err(map_storage_error)?;
        if !force
            && ledger.last_analysis_at.is_some_and(|last| {
                now < last.saturating_add(policy.scheduled_analysis_interval_seconds)
            })
        {
            return Ok(false);
        }
        let profiles = self.storage.build_profiles().map_err(map_storage_error)?;
        let roots = self
            .storage
            .project_roots()
            .map_err(map_storage_error)?
            .into_iter()
            .map(|root| (root.id.clone(), root))
            .collect::<HashMap<_, _>>();
        for profile in &profiles {
            let root = roots
                .get(&profile.root_id)
                .ok_or(BuildArtifactError::ValidationFailed)?;
            for artifact in &profile.artifact_paths {
                let normalized = artifact
                    .relative_path
                    .replace('\\', "/")
                    .to_ascii_lowercase();
                let relative = normalize_relative_path(&normalized, false)?;
                let snapshot = snapshot_registered_path(
                    &self.file_system,
                    Path::new(&root.display_path),
                    &relative,
                );
                let existing = ledger.generations.iter().position(|generation| {
                    generation.profile_id == profile.profile_id
                        && generation.normalized_path == normalized
                });
                match (existing, snapshot) {
                    (Some(index), Ok(snapshot)) => {
                        let generation = &mut ledger.generations[index];
                        let identity_changed = generation.identity != Some(snapshot.identity);
                        let changed = identity_changed
                            || generation.allocated_bytes != snapshot.allocated_bytes
                            || generation.observed_modified_at_unix_nanos
                                != snapshot.modified_at_unix_nanos;
                        if changed {
                            generation.last_external_change = Some(now);
                        }
                        if identity_changed {
                            generation.owned = false;
                            generation.ambiguous = true;
                        }
                        generation.identity = Some(snapshot.identity);
                        generation.allocated_bytes = snapshot.allocated_bytes;
                        generation.observed_modified_at_unix_nanos =
                            snapshot.modified_at_unix_nanos;
                        generation.readable = true;
                    }
                    (Some(index), Err(_)) => {
                        ledger.generations[index].readable = false;
                        ledger.generations[index].ambiguous = true;
                    }
                    (None, Ok(snapshot)) => ledger.generations.push(GenerationState {
                        generation_id: random_id()?,
                        profile_id: profile.profile_id.clone(),
                        root_id: profile.root_id.clone(),
                        normalized_path: normalized,
                        allocated_bytes: snapshot.allocated_bytes,
                        last_successful_touch: None,
                        last_external_change: Some(now),
                        profile_label: profile.profile_label.clone(),
                        toolchain_label: profile.toolchain_label.clone(),
                        target_label: profile.target_label.clone(),
                        rebuild_cost: profile.rebuild_cost,
                        owned: false,
                        identity: Some(snapshot.identity),
                        observed_modified_at_unix_nanos: snapshot.modified_at_unix_nanos,
                        role: artifact.role,
                        active: false,
                        readable: true,
                        ambiguous: false,
                        touched_by_latest_success: false,
                    }),
                    (None, Err(_)) => {}
                }
            }
        }
        ledger.last_analysis_at = Some(now);
        self.storage
            .write_artifact_generations(&ledger)
            .map_err(map_storage_error)?;
        if ledger.last_successful_build.is_some() {
            let decision = select_artifact_generations(
                &ledger.generations,
                &budget_context(&policy, &ledger, roots.into_values().collect(), profiles),
            )
            .map_err(|_| BuildArtifactError::ValidationFailed)?;
            self.enforce_budget(&ledger, &decision, policy.quarantine_grace_seconds)?;
        }
        Ok(true)
    }

    pub fn register_profile(
        &self,
        input: RegisterBuildProfileInput,
    ) -> Result<BuildProfile, BuildArtifactError> {
        let _writer = self
            .writer
            .lock()
            .map_err(|_| BuildArtifactError::OperationFailed)?;
        let roots = self.storage.project_roots().map_err(map_storage_error)?;
        let root = roots
            .iter()
            .find(|root| root.id == input.root_id)
            .ok_or(BuildArtifactError::NotFound)?;
        let root_path = Path::new(&root.display_path);
        let canonical_root = self
            .file_system
            .canonicalize(root_path)
            .map_err(|_| BuildArtifactError::ValidationFailed)?;
        if !self
            .file_system
            .semantics()
            .equivalent(root_path, &canonical_root)
        {
            return Err(BuildArtifactError::ValidationFailed);
        }
        let executable = Path::new(&input.executable);
        if !executable.is_absolute() || forbidden_executable(executable) {
            return Err(BuildArtifactError::InvalidInput);
        }
        let canonical_executable = self
            .file_system
            .canonicalize(executable)
            .map_err(|_| BuildArtifactError::ValidationFailed)?;
        let executable_metadata = self
            .file_system
            .metadata_no_follow(executable)
            .map_err(|_| BuildArtifactError::ValidationFailed)?;
        if executable_metadata.kind != EntryKind::File
            || !self
                .file_system
                .semantics()
                .equivalent(executable, &canonical_executable)
        {
            return Err(BuildArtifactError::ValidationFailed);
        }
        let executable_identity = executable_metadata
            .identity
            .ok_or(BuildArtifactError::ValidationFailed)?;
        let working_relative = normalize_relative_path(&input.working_directory, true)?;
        let working_directory = canonical_root.join(working_relative);
        let canonical_working = self
            .file_system
            .canonicalize(&working_directory)
            .map_err(|_| BuildArtifactError::ValidationFailed)?;
        if !self
            .file_system
            .semantics()
            .equivalent(&working_directory, &canonical_working)
            || !self
                .file_system
                .semantics()
                .contains(&canonical_root, &canonical_working)
        {
            return Err(BuildArtifactError::ValidationFailed);
        }
        for artifact in &input.artifact_paths {
            normalize_relative_path(&artifact.relative_path, false)?;
        }
        let profile = BuildProfile {
            profile_id: random_id()?,
            root_id: input.root_id,
            display_name: input.display_name,
            ecosystem: input.ecosystem,
            executable: canonical_executable.to_string_lossy().into_owned(),
            executable_identity,
            argv: input.argv,
            working_directory: canonical_working.to_string_lossy().into_owned(),
            profile_label: input.profile_label,
            toolchain_label: input.toolchain_label,
            target_label: input.target_label,
            rebuild_cost: input.rebuild_cost,
            artifact_paths: input.artifact_paths,
        };
        profile.validate().map_err(map_storage_error)?;
        if !self.approver.approve(&profile)? {
            return Err(BuildArtifactError::ApprovalDeclined);
        }
        let mut profiles = self.storage.build_profiles().map_err(map_storage_error)?;
        profiles.push(profile.clone());
        self.storage
            .write_build_profiles(&profiles)
            .map_err(map_storage_error)?;
        Ok(profile)
    }

    pub fn remove_profile(&self, profile_id: &str) -> Result<(), BuildArtifactError> {
        if !valid_id(profile_id) {
            return Err(BuildArtifactError::InvalidInput);
        }
        let _writer = self
            .writer
            .lock()
            .map_err(|_| BuildArtifactError::OperationFailed)?;
        if self
            .runs
            .lock()
            .map_err(|_| BuildArtifactError::OperationFailed)?
            .active
            .as_ref()
            .is_some_and(|run| run.profile_id == profile_id)
        {
            return Err(BuildArtifactError::BuildBusy);
        }
        let mut profiles = self.storage.build_profiles().map_err(map_storage_error)?;
        let before = profiles.len();
        profiles.retain(|profile| profile.profile_id != profile_id);
        if profiles.len() == before {
            return Err(BuildArtifactError::NotFound);
        }
        let mut ledger = self
            .storage
            .artifact_generations()
            .map_err(map_storage_error)?;
        ledger
            .generations
            .retain(|generation| generation.profile_id != profile_id);
        if ledger
            .last_successful_build
            .as_ref()
            .is_some_and(|stamp| stamp.profile_id == profile_id)
        {
            ledger.last_successful_build = None;
        }
        self.storage
            .write_artifact_generations(&ledger)
            .map_err(map_storage_error)?;
        self.storage
            .write_build_profiles(&profiles)
            .map_err(map_storage_error)
    }

    pub fn start_run(&self, profile_id: &str) -> Result<BuildRun, BuildArtifactError> {
        if !valid_id(profile_id) {
            return Err(BuildArtifactError::InvalidInput);
        }
        let profile = self
            .storage
            .build_profiles()
            .map_err(map_storage_error)?
            .into_iter()
            .find(|profile| profile.profile_id == profile_id)
            .ok_or(BuildArtifactError::NotFound)?;
        self.revalidate_launch(&profile)?;
        let run_id = random_id()?;
        let cancelled = Arc::new(AtomicBool::new(false));
        let run = BuildRun {
            run_id: run_id.clone(),
            profile_id: profile_id.to_owned(),
            state: BuildRunState::Queued,
            started_at: None,
            completed_at: None,
            exit_code: None,
        };
        {
            let mut registry = self
                .runs
                .lock()
                .map_err(|_| BuildArtifactError::OperationFailed)?;
            if registry.active.is_some() {
                return Err(BuildArtifactError::BuildBusy);
            }
            while registry.runs.len() >= MAX_RUNS {
                if registry
                    .runs
                    .front()
                    .is_some_and(|existing| existing.state.terminal())
                {
                    registry.runs.pop_front();
                } else {
                    return Err(BuildArtifactError::BuildBusy);
                }
            }
            registry.active = Some(ActiveRun {
                run_id: run_id.clone(),
                profile_id: profile_id.to_owned(),
                cancelled: Arc::clone(&cancelled),
            });
            registry.runs.push_back(run.clone());
        }
        let manager = self.clone();
        thread::spawn(move || manager.run_profile(run_id, profile, cancelled));
        Ok(run)
    }

    pub fn run(&self, run_id: &str) -> Result<BuildRun, BuildArtifactError> {
        if !valid_id(run_id) {
            return Err(BuildArtifactError::InvalidInput);
        }
        self.runs
            .lock()
            .map_err(|_| BuildArtifactError::OperationFailed)?
            .runs
            .iter()
            .find(|run| run.run_id == run_id)
            .cloned()
            .ok_or(BuildArtifactError::NotFound)
    }

    pub fn cancel_run(&self, run_id: &str) -> Result<BuildRun, BuildArtifactError> {
        if !valid_id(run_id) {
            return Err(BuildArtifactError::InvalidInput);
        }
        let registry = self
            .runs
            .lock()
            .map_err(|_| BuildArtifactError::OperationFailed)?;
        let active = registry
            .active
            .as_ref()
            .filter(|run| run.run_id == run_id)
            .ok_or(BuildArtifactError::NotFound)?;
        active.cancelled.store(true, Ordering::Release);
        registry
            .runs
            .iter()
            .find(|run| run.run_id == run_id)
            .cloned()
            .ok_or(BuildArtifactError::NotFound)
    }

    /// Snapshot the authoritative global slot without changing build authority.
    pub fn active_run(&self) -> Result<Option<BuildRun>, BuildArtifactError> {
        let registry = self
            .runs
            .lock()
            .map_err(|_| BuildArtifactError::OperationFailed)?;
        let Some(active) = &registry.active else {
            return Ok(None);
        };
        registry
            .runs
            .iter()
            .find(|run| run.run_id == active.run_id)
            .cloned()
            .map(Some)
            .ok_or(BuildArtifactError::OperationFailed)
    }

    pub fn active_profile_id(&self) -> Result<Option<String>, BuildArtifactError> {
        Ok(self
            .runs
            .lock()
            .map_err(|_| BuildArtifactError::OperationFailed)?
            .active
            .as_ref()
            .map(|run| run.profile_id.clone()))
    }

    fn run_profile(&self, run_id: String, profile: BuildProfile, cancelled: Arc<AtomicBool>) {
        let started_at = now_seconds().ok();
        self.update_run(&run_id, |run| {
            run.state = BuildRunState::Running;
            run.started_at = started_at;
        });
        let before = if cancelled.load(Ordering::Acquire) {
            Ok(BTreeMap::new())
        } else {
            self.snapshots(&profile)
        };
        let process = match &before {
            Err(error) => Err(*error),
            Ok(_) if cancelled.load(Ordering::Acquire) => Ok(ProcessOutcome::Cancelled),
            Ok(_) if self.revalidate_launch(&profile).is_err() => {
                Err(BuildArtifactError::ValidationFailed)
            }
            Ok(_) => self.runner.run(
                Path::new(&profile.executable),
                &profile.argv,
                Path::new(&profile.working_directory),
                &cancelled,
            ),
        };
        let completed = match (before, process) {
            (_, Ok(ProcessOutcome::Cancelled)) => SuccessfulProcessing::Cancelled,
            (_, Ok(ProcessOutcome::Exited(_))) if cancelled.load(Ordering::Acquire) => {
                SuccessfulProcessing::Cancelled
            }
            (Ok(before), Ok(ProcessOutcome::Exited(0))) => self
                .snapshots(&profile)
                .and_then(|after| self.commit_success(&profile, &before, &after, &cancelled))
                .unwrap_or(SuccessfulProcessing::AnalysisFailed),
            (_, Ok(ProcessOutcome::Exited(code))) => SuccessfulProcessing::Failed(code),
            _ => SuccessfulProcessing::AnalysisFailed,
        };
        self.update_run(&run_id, |run| {
            run.completed_at = now_seconds().ok();
            match completed {
                SuccessfulProcessing::Succeeded => {
                    run.state = BuildRunState::Succeeded;
                    run.exit_code = Some(0);
                }
                SuccessfulProcessing::Failed(code) => {
                    run.state = BuildRunState::Failed;
                    run.exit_code = Some(code);
                }
                SuccessfulProcessing::Cancelled => run.state = BuildRunState::Cancelled,
                SuccessfulProcessing::AnalysisFailed => run.state = BuildRunState::AnalysisFailed,
            }
        });
        if let Ok(mut registry) = self.runs.lock()
            && registry
                .active
                .as_ref()
                .is_some_and(|active| active.run_id == run_id)
        {
            registry.active = None;
        }
    }

    fn snapshots(
        &self,
        profile: &BuildProfile,
    ) -> Result<BTreeMap<String, Option<ArtifactSnapshot>>, BuildArtifactError> {
        let root = self
            .storage
            .project_roots()
            .map_err(map_storage_error)?
            .into_iter()
            .find(|root| root.id == profile.root_id)
            .ok_or(BuildArtifactError::ValidationFailed)?;
        let root = Path::new(&root.display_path);
        let mut snapshots = BTreeMap::new();
        for artifact in &profile.artifact_paths {
            let relative = normalize_relative_path(&artifact.relative_path, false)?;
            let path = root.join(&relative);
            let snapshot = match self.file_system.metadata_no_follow(&path) {
                Err(error) if error.kind == FsErrorKind::NotFound => None,
                Err(_) => return Err(BuildArtifactError::ValidationFailed),
                Ok(_) => Some(
                    snapshot_registered_path(&self.file_system, root, &relative)
                        .map_err(|_| BuildArtifactError::ValidationFailed)?,
                ),
            };
            snapshots.insert(
                artifact
                    .relative_path
                    .replace('\\', "/")
                    .to_ascii_lowercase(),
                snapshot,
            );
        }
        Ok(snapshots)
    }

    fn commit_success(
        &self,
        profile: &BuildProfile,
        before: &BTreeMap<String, Option<ArtifactSnapshot>>,
        after: &BTreeMap<String, Option<ArtifactSnapshot>>,
        cancelled: &AtomicBool,
    ) -> Result<SuccessfulProcessing, BuildArtifactError> {
        let _writer = self
            .writer
            .lock()
            .map_err(|_| BuildArtifactError::OperationFailed)?;
        if cancelled.load(Ordering::Acquire) {
            return Ok(SuccessfulProcessing::Cancelled);
        }
        let saved_profile = self
            .storage
            .build_profiles()
            .map_err(map_storage_error)?
            .into_iter()
            .find(|saved| saved.profile_id == profile.profile_id)
            .filter(|saved| saved == profile)
            .ok_or(BuildArtifactError::ValidationFailed)?;
        self.revalidate_launch(&saved_profile)?;
        let previous = self
            .storage
            .artifact_generations()
            .map_err(map_storage_error)?;
        let mut ledger = previous.clone();
        for generation in &mut ledger.generations {
            generation.touched_by_latest_success = false;
        }
        let now = now_seconds()?;
        self.observe_external_changes(&mut ledger, &profile.profile_id, now)?;
        let registered = profile
            .artifact_paths
            .iter()
            .map(|artifact| {
                (
                    artifact
                        .relative_path
                        .replace('\\', "/")
                        .to_ascii_lowercase(),
                    artifact,
                )
            })
            .collect::<HashMap<_, _>>();
        ledger.generations.retain(|generation| {
            generation.profile_id != profile.profile_id
                || after
                    .get(&generation.normalized_path)
                    .is_some_and(Option::is_some)
        });
        let mut touched_generation_ids = Vec::new();
        for (path, artifact) in registered {
            let Some(snapshot) = after.get(&path).and_then(Option::as_ref) else {
                continue;
            };
            let touched = before.get(&path).and_then(Option::as_ref) != Some(snapshot);
            let existing = ledger.generations.iter().position(|generation| {
                generation.profile_id == profile.profile_id && generation.normalized_path == path
            });
            let prior = existing.map(|index| ledger.generations[index].clone());
            let generation_id = match &prior {
                Some(generation) => generation.generation_id.clone(),
                None => random_id()?,
            };
            let owned = prior.as_ref().is_some_and(|generation| generation.owned)
                || (touched && artifact.role == cleanup_core::ArtifactRole::Generation);
            let state = GenerationState {
                generation_id: generation_id.clone(),
                profile_id: profile.profile_id.clone(),
                root_id: profile.root_id.clone(),
                normalized_path: path,
                allocated_bytes: snapshot.allocated_bytes,
                last_successful_touch: if touched {
                    Some(now)
                } else {
                    prior
                        .as_ref()
                        .and_then(|generation| generation.last_successful_touch)
                },
                last_external_change: if touched {
                    None
                } else {
                    prior
                        .as_ref()
                        .and_then(|generation| generation.last_external_change)
                },
                profile_label: profile.profile_label.clone(),
                toolchain_label: profile.toolchain_label.clone(),
                target_label: profile.target_label.clone(),
                rebuild_cost: profile.rebuild_cost,
                owned,
                identity: Some(snapshot.identity),
                observed_modified_at_unix_nanos: snapshot.modified_at_unix_nanos,
                role: artifact.role,
                active: false,
                readable: true,
                ambiguous: false,
                touched_by_latest_success: touched,
            };
            if touched {
                touched_generation_ids.push(generation_id);
            }
            if let Some(index) = existing {
                ledger.generations[index] = state;
            } else {
                ledger.generations.push(state);
            }
        }
        ledger.last_successful_build = Some(SuccessfulBuildStamp {
            profile_id: profile.profile_id.clone(),
            root_id: profile.root_id.clone(),
            succeeded_at: now,
            profile_label: profile.profile_label.clone(),
            toolchain_label: profile.toolchain_label.clone(),
            target_label: profile.target_label.clone(),
            touched_generation_ids,
        });
        self.storage
            .write_artifact_generations(&ledger)
            .map_err(map_storage_error)?;
        if cancelled.load(Ordering::Acquire) {
            self.storage
                .write_artifact_generations(&previous)
                .map_err(map_storage_error)?;
            return Ok(SuccessfulProcessing::Cancelled);
        }
        let policy = self
            .storage
            .artifact_budget_policy()
            .map_err(map_storage_error)?;
        if !policy.enabled {
            return Ok(SuccessfulProcessing::Succeeded);
        }
        let decision = select_artifact_generations(
            &ledger.generations,
            &budget_context(
                &policy,
                &ledger,
                self.storage.project_roots().map_err(map_storage_error)?,
                self.storage.build_profiles().map_err(map_storage_error)?,
            ),
        )
        .map_err(|_| BuildArtifactError::ValidationFailed)?;
        #[cfg(test)]
        if let Some(hook) = &self.before_budget_enforcement {
            hook(cancelled);
        }
        if cancelled.load(Ordering::Acquire) {
            self.storage
                .write_artifact_generations(&previous)
                .map_err(map_storage_error)?;
            return Ok(SuccessfulProcessing::Cancelled);
        }
        self.enforce_budget(&ledger, &decision, policy.quarantine_grace_seconds)?;
        Ok(SuccessfulProcessing::Succeeded)
    }

    fn observe_external_changes(
        &self,
        ledger: &mut ArtifactGenerationLedger,
        current_profile_id: &str,
        observed_at: u64,
    ) -> Result<(), BuildArtifactError> {
        let profiles = self
            .storage
            .build_profiles()
            .map_err(map_storage_error)?
            .into_iter()
            .map(|profile| (profile.profile_id.clone(), profile))
            .collect::<HashMap<_, _>>();
        let roots = self
            .storage
            .project_roots()
            .map_err(map_storage_error)?
            .into_iter()
            .map(|root| (root.id.clone(), root))
            .collect::<HashMap<_, _>>();
        for generation in &mut ledger.generations {
            if generation.profile_id == current_profile_id {
                continue;
            }
            let Some(profile) = profiles.get(&generation.profile_id) else {
                return Err(BuildArtifactError::ValidationFailed);
            };
            let Some(root) = roots.get(&generation.root_id) else {
                return Err(BuildArtifactError::ValidationFailed);
            };
            if !profile.artifact_paths.iter().any(|artifact| {
                artifact
                    .relative_path
                    .replace('\\', "/")
                    .eq_ignore_ascii_case(&generation.normalized_path)
            }) {
                generation.ambiguous = true;
                continue;
            }
            let relative = normalize_relative_path(&generation.normalized_path, false)?;
            match snapshot_registered_path(
                &self.file_system,
                Path::new(&root.display_path),
                &relative,
            ) {
                Ok(snapshot) => {
                    let identity_changed = generation.identity != Some(snapshot.identity);
                    let changed = identity_changed
                        || generation.allocated_bytes != snapshot.allocated_bytes
                        || generation.observed_modified_at_unix_nanos
                            != snapshot.modified_at_unix_nanos;
                    if changed {
                        generation.last_external_change = Some(observed_at);
                    }
                    if identity_changed {
                        generation.owned = false;
                        generation.ambiguous = true;
                    }
                    generation.identity = Some(snapshot.identity);
                    generation.allocated_bytes = snapshot.allocated_bytes;
                    generation.observed_modified_at_unix_nanos = snapshot.modified_at_unix_nanos;
                    generation.readable = true;
                }
                Err(_) => {
                    generation.readable = false;
                    generation.ambiguous = true;
                }
            }
        }
        Ok(())
    }

    fn enforce_budget(
        &self,
        ledger: &ArtifactGenerationLedger,
        decision: &BudgetDecision,
        quarantine_grace_seconds: u64,
    ) -> Result<(), BuildArtifactError> {
        let selected = decision
            .selected_generation_ids
            .iter()
            .map(String::as_str)
            .collect::<std::collections::HashSet<_>>();
        let roots = self
            .storage
            .project_roots()
            .map_err(map_storage_error)?
            .into_iter()
            .map(|root| (root.id.clone(), root))
            .collect::<HashMap<_, _>>();
        let profiles = self
            .storage
            .build_profiles()
            .map_err(map_storage_error)?
            .into_iter()
            .map(|profile| (profile.profile_id.clone(), profile))
            .collect::<HashMap<_, _>>();
        let mut groups = BTreeMap::<(String, String), Vec<&GenerationState>>::new();
        for generation in &ledger.generations {
            if selected.contains(generation.generation_id.as_str()) {
                groups
                    .entry((generation.root_id.clone(), generation.profile_id.clone()))
                    .or_default()
                    .push(generation);
            }
        }
        let mut quarantined = Vec::new();
        let mut failed = false;
        for ((root_id, profile_id), mut generations) in groups {
            let outcome = (|| {
                generations.sort_by(|left, right| left.generation_id.cmp(&right.generation_id));
                let root = roots
                    .get(&root_id)
                    .ok_or(BuildArtifactError::ValidationFailed)?;
                let profile = profiles
                    .get(&profile_id)
                    .ok_or(BuildArtifactError::ValidationFailed)?;
                let root_path = Path::new(&root.display_path);
                let context_identity = self
                    .file_system
                    .metadata_no_follow(root_path)
                    .map_err(|_| BuildArtifactError::ValidationFailed)?
                    .identity
                    .ok_or(BuildArtifactError::ValidationFailed)?;
                let mut items = Vec::with_capacity(generations.len());
                for generation in generations {
                    let relative = normalize_relative_path(&generation.normalized_path, false)?;
                    let snapshot =
                        snapshot_registered_path(&self.file_system, root_path, &relative)
                            .map_err(|_| BuildArtifactError::ValidationFailed)?;
                    if Some(snapshot.identity) != generation.identity
                        || snapshot.allocated_bytes != generation.allocated_bytes
                        || snapshot.modified_at_unix_nanos
                            != generation.observed_modified_at_unix_nanos
                    {
                        return Err(BuildArtifactError::ValidationFailed);
                    }
                    let path = root_path.join(relative);
                    let scan_root = path
                        .parent()
                        .ok_or(BuildArtifactError::ValidationFailed)?
                        .to_path_buf();
                    let target = path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .ok_or(BuildArtifactError::ValidationFailed)?
                        .to_owned();
                    let rule = CleanupRule {
                        id: "registered-build-artifact".into(),
                        rule_version: 1,
                        lifecycle: Lifecycle::Stable,
                        risk: Risk::Recoverable,
                        provenance: Provenance {
                            source: "approved build profile".into(),
                            verified_at: "runtime".into(),
                        },
                        default_selected: false,
                        artifact: None,
                        scanner: ScannerKind::Direct,
                        roots: vec![RuleRoot {
                            binding: "registered".into(),
                            suffix: PathBuf::new(),
                        }],
                        markers: Markers::default(),
                        targets: vec![target],
                        target_prefixes: Vec::new(),
                        target_suffixes: Vec::new(),
                        target_type: match snapshot.kind {
                            EntryKind::File => TargetType::File,
                            EntryKind::Directory => TargetType::Directory,
                            EntryKind::LinkLike => {
                                return Err(BuildArtifactError::ValidationFailed);
                            }
                        },
                        root_depth: 0,
                        project_depth: None,
                        target_depth: None,
                        minimum_age_seconds: 0,
                        excluded_names: Vec::new(),
                        excluded_paths: Vec::new(),
                    };
                    items.push(PlanItem {
                        item_id: random_id()?,
                        proof: ResolvedCandidate {
                            scope: CandidateProofScope::RegisteredBuildArtifact {
                                root_id: root_id.clone(),
                                profile_id: profile.profile_id.clone(),
                                generation_id: generation.generation_id.clone(),
                            },
                            path,
                            scan_root,
                            context_root: root_path.to_path_buf(),
                            context_identity: Some(context_identity),
                            rule,
                            identity: snapshot.identity,
                            kind: snapshot.kind,
                            logical_bytes: snapshot.allocated_bytes,
                            allocated_bytes: snapshot.allocated_bytes,
                            scanned_at: SystemTime::now(),
                        },
                    });
                }
                let plan = CleanupPlan::build_artifact(
                    random_id()?,
                    random_id()?,
                    now_seconds()?,
                    root_id,
                    profile_id,
                    items,
                )
                .map_err(map_storage_error)?;
                #[cfg(test)]
                if let Some(hook) = &self.before_registered_artifact_execution {
                    hook(&plan);
                }
                execute_registered_artifact_plan(
                    &self.storage,
                    &self.file_system,
                    &plan,
                    quarantine_grace_seconds,
                )
                .map_err(|_| BuildArtifactError::OperationFailed)
            })();
            match outcome {
                Ok(outcome) => {
                    failed |= !outcome.failed_items.is_empty();
                    quarantined.extend(outcome.quarantined_generation_ids);
                }
                Err(_) => {
                    failed = true;
                    break;
                }
            }
        }
        if !quarantined.is_empty() {
            let quarantined = quarantined
                .into_iter()
                .collect::<std::collections::HashSet<_>>();
            let mut current = self
                .storage
                .artifact_generations()
                .map_err(map_storage_error)?;
            current
                .generations
                .retain(|generation| !quarantined.contains(&generation.generation_id));
            self.storage
                .write_artifact_generations(&current)
                .map_err(map_storage_error)?;
        }
        if failed {
            Err(BuildArtifactError::OperationFailed)
        } else {
            Ok(())
        }
    }

    fn update_run(&self, run_id: &str, update: impl FnOnce(&mut BuildRun)) {
        if let Ok(mut registry) = self.runs.lock()
            && let Some(run) = registry.runs.iter_mut().find(|run| run.run_id == run_id)
        {
            update(run);
        }
    }

    fn revalidate_launch(&self, profile: &BuildProfile) -> Result<(), BuildArtifactError> {
        profile.validate().map_err(map_storage_error)?;
        let executable = Path::new(&profile.executable);
        if forbidden_executable(executable) {
            return Err(BuildArtifactError::ValidationFailed);
        }
        let metadata = self
            .file_system
            .metadata_no_follow(executable)
            .map_err(|_| BuildArtifactError::ValidationFailed)?;
        if metadata.kind != EntryKind::File
            || metadata.identity != Some(profile.executable_identity)
            || self
                .file_system
                .canonicalize(executable)
                .ok()
                .is_none_or(|canonical| {
                    !self
                        .file_system
                        .semantics()
                        .equivalent(executable, &canonical)
                })
        {
            return Err(BuildArtifactError::ValidationFailed);
        }
        let root = self
            .storage
            .project_roots()
            .map_err(map_storage_error)?
            .into_iter()
            .find(|root| root.id == profile.root_id)
            .ok_or(BuildArtifactError::ValidationFailed)?;
        let root = Path::new(&root.display_path);
        let working = Path::new(&profile.working_directory);
        if !self.file_system.semantics().contains(root, working)
            || self
                .file_system
                .canonicalize(working)
                .ok()
                .is_none_or(|canonical| {
                    !self.file_system.semantics().equivalent(working, &canonical)
                        || !self.file_system.semantics().contains(root, &canonical)
                })
        {
            return Err(BuildArtifactError::ValidationFailed);
        }
        Ok(())
    }
}

fn budget_context(
    policy: &ArtifactBudgetPolicy,
    ledger: &ArtifactGenerationLedger,
    roots: Vec<super::storage::ProjectRoot>,
    profiles: Vec<BuildProfile>,
) -> BudgetContext {
    let project_limits = roots
        .into_iter()
        .map(|root| {
            let limits = policy
                .project_overrides
                .iter()
                .find(|entry| entry.root_id == root.id)
                .map(|entry| match entry.policy {
                    ProjectBudgetOverride::Inherit => policy.global_limits,
                    ProjectBudgetOverride::Disabled => BudgetLimits::default(),
                    ProjectBudgetOverride::Explicit { limits } => limits,
                })
                .unwrap_or(policy.global_limits);
            (root.id, limits)
        })
        .collect();
    let protected_artifact_paths = profiles
        .into_iter()
        .flat_map(|profile| {
            profile
                .artifact_paths
                .into_iter()
                .filter_map(move |artifact| {
                    matches!(
                        artifact.role,
                        cleanup_core::ArtifactRole::Dependency
                            | cleanup_core::ArtifactRole::Incremental
                    )
                    .then(|| ProtectedArtifactPath {
                        root_id: profile.root_id.clone(),
                        normalized_path: artifact
                            .relative_path
                            .replace('\\', "/")
                            .to_ascii_lowercase(),
                        role: artifact.role,
                    })
                })
        })
        .collect();
    let stamp = ledger.last_successful_build.as_ref();
    BudgetContext {
        global_limits: policy.global_limits,
        project_limits,
        protected_artifact_paths,
        current_successful_profile_id: stamp.map(|stamp| stamp.profile_id.clone()),
        current_toolchain_label: stamp.map(|stamp| stamp.toolchain_label.clone()),
        current_target_label: stamp.map(|stamp| stamp.target_label.clone()),
        stale_grace_seconds: policy.stale_change_grace_seconds,
        now: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_secs()),
    }
}

fn normalize_relative_path(value: &str, allow_empty: bool) -> Result<PathBuf, BuildArtifactError> {
    let path = Path::new(value);
    if path.is_absolute() || value.len() > 4_096 || value.chars().any(|character| character == '\0')
    {
        return Err(BuildArtifactError::InvalidInput);
    }
    let mut output = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => output.push(part),
            _ => return Err(BuildArtifactError::InvalidInput),
        }
    }
    if output.as_os_str().is_empty() && !allow_empty {
        return Err(BuildArtifactError::InvalidInput);
    }
    Ok(output)
}

fn forbidden_executable(path: &Path) -> bool {
    if !path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
    {
        return true;
    }
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            matches!(
                name.to_ascii_lowercase().as_str(),
                "cmd.exe"
                    | "powershell.exe"
                    | "pwsh.exe"
                    | "wscript.exe"
                    | "cscript.exe"
                    | "mshta.exe"
                    | "rundll32.exe"
            )
        })
}

fn valid_id(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn random_id() -> Result<String, BuildArtifactError> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| BuildArtifactError::OperationFailed)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn now_seconds() -> Result<u64, BuildArtifactError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| BuildArtifactError::OperationFailed)
}

fn map_storage_error(error: StorageError) -> BuildArtifactError {
    match error {
        StorageError::Invalid | StorageError::TooLarge => BuildArtifactError::ValidationFailed,
        StorageError::Exists | StorageError::Io => BuildArtifactError::OperationFailed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cleanup::storage::ItemState;
    use cleanup_core::{ArtifactRole, ProtectionReason, RebuildCost};
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn active_run_is_a_read_only_snapshot_of_the_global_slot() {
        let (root, manager, _, runner, _) = setup(true, RunnerMode::Success);
        assert!(manager.active_run().unwrap().is_none());
        let run_id = "a".repeat(32);
        let profile_id = "b".repeat(32);
        let cancelled = Arc::new(AtomicBool::new(false));
        {
            let mut registry = manager.runs.lock().unwrap();
            registry.active = Some(ActiveRun {
                run_id: run_id.clone(),
                profile_id: profile_id.clone(),
                cancelled: cancelled.clone(),
            });
            registry.runs.push_back(BuildRun {
                run_id: run_id.clone(),
                profile_id: profile_id.clone(),
                state: BuildRunState::Queued,
                started_at: None,
                completed_at: None,
                exit_code: None,
            });
        }
        for state in [
            BuildRunState::Queued,
            BuildRunState::Running,
            BuildRunState::Succeeded,
        ] {
            manager.update_run(&run_id, |run| run.state = state);
            let mut snapshot = manager.clone().active_run().unwrap().unwrap();
            assert_eq!(snapshot.run_id, run_id);
            assert_eq!(snapshot.profile_id, profile_id);
            assert_eq!(snapshot.state, state);
            snapshot.run_id.clear();
            assert_eq!(manager.active_run().unwrap().unwrap().run_id, run_id);
            assert_eq!(
                manager.active_profile_id().unwrap(),
                Some(profile_id.clone())
            );
            assert_eq!(
                manager
                    .set_policy(ArtifactBudgetPolicy::default())
                    .unwrap_err(),
                BuildArtifactError::BuildBusy
            );
        }
        assert!(!cancelled.load(Ordering::Acquire));
        assert_eq!(runner.calls.load(Ordering::Relaxed), 0);
        manager.runs.lock().unwrap().active = None;
        assert!(manager.active_run().unwrap().is_none());
        assert_eq!(
            manager.run(&run_id).unwrap().state,
            BuildRunState::Succeeded
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn active_run_fails_closed_for_missing_record_and_poisoned_registry() {
        let (root, manager, _, _, _) = setup(true, RunnerMode::Success);
        manager.runs.lock().unwrap().active = Some(ActiveRun {
            run_id: "a".repeat(32),
            profile_id: "b".repeat(32),
            cancelled: Arc::new(AtomicBool::new(false)),
        });
        assert_eq!(
            manager.active_run().unwrap_err(),
            BuildArtifactError::OperationFailed
        );
        let runs = manager.runs.clone();
        let _ = thread::spawn(move || {
            let _guard = runs.lock().unwrap();
            panic!("poison registry for test");
        })
        .join();
        assert_eq!(
            manager.active_run().unwrap_err(),
            BuildArtifactError::OperationFailed
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    struct TestApprover {
        approved: bool,
        seen: Mutex<Vec<Vec<String>>>,
    }

    impl ProfileApprover for TestApprover {
        fn approve(&self, profile: &BuildProfile) -> Result<bool, BuildArtifactError> {
            self.seen.lock().unwrap().push(profile.argv.clone());
            Ok(self.approved)
        }
    }

    #[derive(Clone, Copy)]
    enum RunnerMode {
        Success,
        Failure,
        WaitForCancellation,
    }

    struct TestRunner {
        mode: RunnerMode,
        calls: AtomicUsize,
        argv: Mutex<Vec<Vec<String>>>,
    }

    impl ProcessRunner for TestRunner {
        fn run(
            &self,
            _executable: &Path,
            argv: &[String],
            working_directory: &Path,
            cancelled: &AtomicBool,
        ) -> Result<ProcessOutcome, BuildArtifactError> {
            let call = self.calls.fetch_add(1, Ordering::Relaxed);
            self.argv.lock().unwrap().push(argv.to_vec());
            let artifact =
                working_directory.join(if argv.iter().any(|value| value == "--release") {
                    "target/release"
                } else {
                    "target/debug"
                });
            std::fs::create_dir_all(&artifact).map_err(|_| BuildArtifactError::OperationFailed)?;
            std::fs::write(artifact.join("output.bin"), format!("build-{call}"))
                .map_err(|_| BuildArtifactError::OperationFailed)?;
            match self.mode {
                RunnerMode::Success => Ok(ProcessOutcome::Exited(0)),
                RunnerMode::Failure => Ok(ProcessOutcome::Exited(9)),
                RunnerMode::WaitForCancellation => {
                    while !cancelled.load(Ordering::Acquire) {
                        thread::sleep(Duration::from_millis(1));
                    }
                    Ok(ProcessOutcome::Cancelled)
                }
            }
        }
    }

    fn setup(
        approved: bool,
        mode: RunnerMode,
    ) -> (
        PathBuf,
        BuildArtifactManager,
        Arc<TestApprover>,
        Arc<TestRunner>,
        RegisterBuildProfileInput,
    ) {
        let root = std::env::temp_dir().join(format!(
            "supa-diska-build-profile-{}-{}",
            std::process::id(),
            getrandom::u64().unwrap()
        ));
        std::fs::create_dir(&root).unwrap();
        let canonical_root = std::fs::canonicalize(&root).unwrap();
        let executable = canonical_root.join("builder.exe");
        std::fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
        let storage = CleanupStorage::open(canonical_root.join("app-data/cleanup")).unwrap();
        storage
            .write_project_roots(&[super::super::storage::ProjectRoot {
                id: "1".repeat(32),
                display_path: canonical_root.to_string_lossy().into_owned(),
                paused: false,
                added_at_unix_seconds: 1,
                last_scanned_at_unix_seconds: None,
            }])
            .unwrap();
        let approver = Arc::new(TestApprover {
            approved,
            seen: Mutex::new(Vec::new()),
        });
        let runner = Arc::new(TestRunner {
            mode,
            calls: AtomicUsize::new(0),
            argv: Mutex::new(Vec::new()),
        });
        let manager = BuildArtifactManager::with_components(
            storage,
            Arc::new(Mutex::new(())),
            approver.clone(),
            runner.clone(),
        );
        let input = RegisterBuildProfileInput {
            root_id: "1".repeat(32),
            display_name: "Local debug".into(),
            ecosystem: BuildEcosystem::Rust,
            executable: executable.to_string_lossy().into_owned(),
            argv: vec!["build".into(), "--profile".into(), "debug local".into()],
            working_directory: String::new(),
            profile_label: "debug".into(),
            toolchain_label: "stable".into(),
            target_label: "x86_64-pc-windows-msvc".into(),
            rebuild_cost: RebuildCost::Low,
            artifact_paths: vec![RegisteredArtifactPath {
                relative_path: "target/debug".into(),
                role: ArtifactRole::Generation,
            }],
        };
        (root, manager, approver, runner, input)
    }

    fn wait_for_terminal(manager: &BuildArtifactManager, run_id: &str) -> BuildRun {
        for _ in 0..2_000 {
            let run = manager.run(run_id).unwrap();
            if run.state.terminal() {
                return run;
            }
            thread::sleep(Duration::from_millis(1));
        }
        panic!("build run did not finish")
    }

    #[test]
    fn registration_preserves_argument_boundaries_and_runs_without_runtime_arguments() {
        let (root, manager, approver, runner, input) = setup(true, RunnerMode::Success);
        let expected = input.argv.clone();
        let profile = manager.register_profile(input).unwrap();
        assert_eq!(approver.seen.lock().unwrap()[0], expected);
        let run = manager.start_run(&profile.profile_id).unwrap();
        let terminal = wait_for_terminal(&manager, &run.run_id);
        assert_eq!(terminal.state, BuildRunState::Succeeded);
        assert_eq!(runner.argv.lock().unwrap()[0], expected);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn registration_rejects_overlapping_paths_but_accepts_non_overlapping_generations() {
        let (root, manager, _, _, mut input) = setup(true, RunnerMode::Success);
        input.artifact_paths.extend([
            RegisteredArtifactPath {
                relative_path: "target/debug/deps".into(),
                role: ArtifactRole::Dependency,
            },
            RegisteredArtifactPath {
                relative_path: "target/debug/incremental".into(),
                role: ArtifactRole::Incremental,
            },
        ]);
        assert_eq!(
            manager.register_profile(input.clone()).unwrap_err(),
            BuildArtifactError::ValidationFailed
        );
        assert!(manager.profiles().unwrap().is_empty());

        input.artifact_paths[0].relative_path = "target/debug/generations/run-1".into();
        assert!(manager.register_profile(input).is_ok());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn native_rejection_and_script_hosts_persist_nothing() {
        let (root, manager, _, _, input) = setup(false, RunnerMode::Success);
        assert_eq!(
            manager.register_profile(input).unwrap_err(),
            BuildArtifactError::ApprovalDeclined
        );
        assert!(manager.profiles().unwrap().is_empty());
        std::fs::remove_dir_all(root).unwrap();

        let (root, manager, _, _, mut input) = setup(true, RunnerMode::Success);
        let shell = root.join("cmd.exe");
        std::fs::copy(std::env::current_exe().unwrap(), &shell).unwrap();
        input.executable = shell.to_string_lossy().into_owned();
        assert_eq!(
            manager.register_profile(input).unwrap_err(),
            BuildArtifactError::InvalidInput
        );
        assert!(manager.profiles().unwrap().is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn executable_replacement_is_refused_before_launch() {
        let (root, manager, _, runner, input) = setup(true, RunnerMode::Success);
        let executable = PathBuf::from(&input.executable);
        let profile = manager.register_profile(input).unwrap();
        std::fs::remove_file(&executable).unwrap();
        std::fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
        assert_eq!(
            manager.start_run(&profile.profile_id).unwrap_err(),
            BuildArtifactError::ValidationFailed
        );
        assert_eq!(runner.calls.load(Ordering::Relaxed), 0);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cancellation_wins_and_clears_the_global_run_slot() {
        let (root, manager, _, runner, input) = setup(true, RunnerMode::WaitForCancellation);
        let profile = manager.register_profile(input).unwrap();
        let run = manager.start_run(&profile.profile_id).unwrap();
        assert_eq!(
            manager.start_run(&profile.profile_id).unwrap_err(),
            BuildArtifactError::BuildBusy
        );
        while runner.calls.load(Ordering::Acquire) == 0 {
            thread::sleep(Duration::from_millis(1));
        }
        manager.cancel_run(&run.run_id).unwrap();
        let terminal = wait_for_terminal(&manager, &run.run_id);
        assert_eq!(terminal.state, BuildRunState::Cancelled);
        assert_eq!(terminal.exit_code, None);
        assert_eq!(manager.active_profile_id().unwrap(), None);
        assert!(
            manager
                .storage
                .artifact_generations()
                .unwrap()
                .last_successful_build
                .is_none()
        );
        assert!(root.join("target/debug/output.bin").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_build_preserves_prior_stamp_and_build_created_files() {
        let (root, manager, approver, _, input) = setup(true, RunnerMode::Success);
        let profile = manager.register_profile(input).unwrap();
        let success = manager.start_run(&profile.profile_id).unwrap();
        assert_eq!(
            wait_for_terminal(&manager, &success.run_id).state,
            BuildRunState::Succeeded
        );
        let prior = manager.storage.artifact_generations().unwrap();
        let failure_runner = Arc::new(TestRunner {
            mode: RunnerMode::Failure,
            calls: AtomicUsize::new(0),
            argv: Mutex::new(Vec::new()),
        });
        let failing = BuildArtifactManager::with_components(
            manager.storage.clone(),
            manager.writer.clone(),
            approver,
            failure_runner,
        );
        let failed = failing.start_run(&profile.profile_id).unwrap();
        assert_eq!(
            wait_for_terminal(&failing, &failed.run_id).state,
            BuildRunState::Failed
        );
        assert_eq!(failing.storage.artifact_generations().unwrap(), prior);
        assert!(root.join("target/debug/output.bin").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    fn assert_profile_removal_write_failure_is_restart_readable(write_number: usize) {
        let (root, manager, _, _, input) = setup(true, RunnerMode::Success);
        let profile = manager.register_profile(input).unwrap();
        let run = manager.start_run(&profile.profile_id).unwrap();
        assert_eq!(
            wait_for_terminal(&manager, &run.run_id).state,
            BuildRunState::Succeeded
        );
        let artifact = root.join("target/debug/output.bin");
        let artifact_contents = std::fs::read(&artifact).unwrap();
        let ledger = manager.storage.artifact_generations().unwrap();
        assert!(ledger.last_successful_build.is_some());
        assert!(!ledger.generations.is_empty());
        let expected_ledger = if write_number == 1 {
            ledger
        } else {
            ArtifactGenerationLedger::default()
        };

        manager.storage.fail_write(write_number);
        assert_eq!(
            manager.remove_profile(&profile.profile_id).unwrap_err(),
            BuildArtifactError::OperationFailed
        );

        let restarted = CleanupStorage::open(root.join("app-data/cleanup")).unwrap();
        assert_eq!(restarted.build_profiles().unwrap(), vec![profile.clone()]);
        assert_eq!(restarted.artifact_generations().unwrap(), expected_ledger);
        assert_eq!(std::fs::read(&artifact).unwrap(), artifact_contents);

        manager.remove_profile(&profile.profile_id).unwrap();
        let restarted = CleanupStorage::open(root.join("app-data/cleanup")).unwrap();
        assert!(restarted.build_profiles().unwrap().is_empty());
        assert_eq!(
            restarted.artifact_generations().unwrap(),
            ArtifactGenerationLedger::default()
        );
        assert_eq!(std::fs::read(&artifact).unwrap(), artifact_contents);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn profile_removal_first_write_failure_keeps_restart_state_readable() {
        assert_profile_removal_write_failure_is_restart_readable(1);
    }

    #[test]
    fn profile_removal_second_write_failure_keeps_restart_state_readable() {
        assert_profile_removal_write_failure_is_restart_readable(2);
    }

    #[test]
    fn scheduled_observations_never_infer_success_or_ownership() {
        let (root, manager, _, _, input) = setup(true, RunnerMode::Success);
        manager.register_profile(input).unwrap();
        let artifact = root.join("target/debug");
        std::fs::create_dir_all(&artifact).unwrap();
        std::fs::write(artifact.join("external.bin"), b"external build").unwrap();
        let mut policy = ArtifactBudgetPolicy {
            enabled: true,
            ..ArtifactBudgetPolicy::default()
        };
        policy.global_limits.maximum_allocated_bytes = Some(1);
        manager.set_policy(policy).unwrap();
        manager.analyze_external(true).unwrap();
        let ledger = manager.storage.artifact_generations().unwrap();
        assert!(ledger.last_successful_build.is_none());
        assert_eq!(ledger.generations.len(), 1);
        assert!(!ledger.generations[0].owned);
        assert!(ledger.generations[0].last_external_change.is_some());
        assert!(artifact.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn policy_save_reports_forced_analysis_failure_without_losing_persisted_policy() {
        let (root, manager, _, _, _) = setup(true, RunnerMode::Success);
        std::fs::write(
            root.join("app-data/cleanup/artifact-generations.json"),
            b"not valid json",
        )
        .unwrap();
        let policy = ArtifactBudgetPolicy {
            enabled: true,
            ..ArtifactBudgetPolicy::default()
        };

        let result = manager.set_policy(policy.clone()).unwrap();

        assert!(result.policy_saved);
        assert_eq!(result.policy, policy);
        assert_eq!(result.analysis_status, ArtifactBudgetAnalysisStatus::Failed);
        assert_eq!(manager.policy().unwrap(), policy);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn nested_registered_build_state_prevents_parent_quarantine() {
        let (root, manager, _, _, debug_input) = setup(true, RunnerMode::Success);
        let debug = manager.register_profile(debug_input.clone()).unwrap();

        let mut protected_input = debug_input.clone();
        protected_input.display_name = "Protected build state".into();
        protected_input.profile_label = "protected".into();
        protected_input.target_label = "protected-target".into();
        protected_input.artifact_paths = vec![
            RegisteredArtifactPath {
                relative_path: "target/debug/deps".into(),
                role: ArtifactRole::Dependency,
            },
            RegisteredArtifactPath {
                relative_path: "target/debug/incremental".into(),
                role: ArtifactRole::Incremental,
            },
        ];
        manager.register_profile(protected_input).unwrap();

        let mut release_input = debug_input;
        release_input.display_name = "Local release".into();
        release_input.argv = vec!["build".into(), "--release".into()];
        release_input.profile_label = "release".into();
        release_input.target_label = "release-target".into();
        release_input.artifact_paths[0].relative_path = "target/release".into();
        let release = manager.register_profile(release_input).unwrap();
        let run = manager.start_run(&release.profile_id).unwrap();
        assert_eq!(
            wait_for_terminal(&manager, &run.run_id).state,
            BuildRunState::Succeeded
        );

        let mut policy = ArtifactBudgetPolicy {
            enabled: true,
            ..ArtifactBudgetPolicy::default()
        };
        policy.global_limits.maximum_allocated_bytes = Some(1);
        manager.set_policy(policy).unwrap();

        std::fs::create_dir_all(root.join("target/debug/deps")).unwrap();
        std::fs::write(root.join("target/debug/deps/cache.bin"), b"dependency").unwrap();
        std::fs::create_dir_all(root.join("target/debug/incremental")).unwrap();
        std::fs::write(
            root.join("target/debug/incremental/state.bin"),
            b"incremental",
        )
        .unwrap();
        let run = manager.start_run(&debug.profile_id).unwrap();
        assert_eq!(
            wait_for_terminal(&manager, &run.run_id).state,
            BuildRunState::Succeeded
        );
        assert!(
            !manager
                .preview_budget()
                .unwrap()
                .generations
                .iter()
                .any(|generation| matches!(
                    generation.role,
                    ArtifactRole::Dependency | ArtifactRole::Incremental
                ))
        );

        let run = manager.start_run(&release.profile_id).unwrap();
        assert_eq!(
            wait_for_terminal(&manager, &run.run_id).state,
            BuildRunState::Succeeded
        );
        assert!(root.join("target/debug/output.bin").exists());
        assert!(root.join("target/debug/deps/cache.bin").exists());
        assert!(root.join("target/debug/incremental/state.bin").exists());
        let preview = manager.preview_budget().unwrap();
        let parent = preview
            .decision
            .protected
            .iter()
            .find(|item| {
                preview.generations.iter().any(|generation| {
                    generation.generation_id == item.generation_id
                        && generation.normalized_path == "target/debug"
                })
            })
            .unwrap();
        assert!(parent.reasons.contains(&ProtectionReason::Dependency));
        assert!(parent.reasons.contains(&ProtectionReason::Incremental));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn cancellation_after_stamp_commit_restores_ledger_without_cleanup() {
        let (root, mut manager, _, _, debug_input) = setup(true, RunnerMode::Success);
        let debug = manager.register_profile(debug_input.clone()).unwrap();
        let first = manager.start_run(&debug.profile_id).unwrap();
        assert_eq!(
            wait_for_terminal(&manager, &first.run_id).state,
            BuildRunState::Succeeded
        );

        let mut policy = ArtifactBudgetPolicy {
            enabled: true,
            ..ArtifactBudgetPolicy::default()
        };
        policy.global_limits.maximum_allocated_bytes = Some(1);
        manager.set_policy(policy).unwrap();
        let prior = manager.storage.artifact_generations().unwrap();

        let mut release_input = debug_input;
        release_input.display_name = "Local release".into();
        release_input.argv = vec!["build".into(), "--release".into()];
        release_input.profile_label = "release".into();
        release_input.target_label = "release-target".into();
        release_input.artifact_paths[0].relative_path = "target/release".into();
        let release = manager.register_profile(release_input).unwrap();
        manager.set_before_budget_enforcement(|cancelled| {
            cancelled.store(true, Ordering::Release);
        });

        let run = manager.start_run(&release.profile_id).unwrap();
        let terminal = wait_for_terminal(&manager, &run.run_id);
        assert_eq!(terminal.state, BuildRunState::Cancelled);
        assert_eq!(terminal.exit_code, None);
        assert_eq!(manager.storage.artifact_generations().unwrap(), prior);
        assert!(root.join("target/debug/output.bin").exists());
        assert!(root.join("target/release/output.bin").exists());

        let service = super::super::execution::CleanupService::new(root.join("app-data")).unwrap();
        assert!(service.history().unwrap().is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stale_generation_uses_journaled_quarantine_and_remains_undoable() {
        let (root, manager, _, _, debug_input) = setup(true, RunnerMode::Success);
        let debug = manager.register_profile(debug_input.clone()).unwrap();
        let first = manager.start_run(&debug.profile_id).unwrap();
        assert_eq!(
            wait_for_terminal(&manager, &first.run_id).state,
            BuildRunState::Succeeded
        );
        let mut policy = ArtifactBudgetPolicy {
            enabled: true,
            ..ArtifactBudgetPolicy::default()
        };
        policy.global_limits.maximum_allocated_bytes = Some(1);
        manager.set_policy(policy).unwrap();

        let mut release_input = debug_input;
        release_input.display_name = "Local release".into();
        release_input.argv = vec!["build".into(), "--release".into()];
        release_input.profile_label = "release".into();
        release_input.target_label = "release-target".into();
        release_input.artifact_paths[0].relative_path = "target/release".into();
        let release = manager.register_profile(release_input).unwrap();
        let second = manager.start_run(&release.profile_id).unwrap();
        assert_eq!(
            wait_for_terminal(&manager, &second.run_id).state,
            BuildRunState::Succeeded
        );
        assert!(!root.join("target/debug").exists());
        assert!(root.join("target/release/output.bin").exists());

        let service = super::super::execution::CleanupService::new(root.join("app-data")).unwrap();
        let execution = service
            .history()
            .unwrap()
            .into_iter()
            .find(|entry| {
                entry.disposition == super::super::storage::CleanupDisposition::Quarantine
            })
            .unwrap();
        service.undo(&execution.execution_id).unwrap();
        assert!(root.join("target/debug/output.bin").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn partial_budget_failure_prunes_successes_and_marks_run_analysis_failed() {
        let (root, mut manager, _, _, debug_input) = setup(true, RunnerMode::Success);
        let debug = manager.register_profile(debug_input.clone()).unwrap();
        let first = manager.start_run(&debug.profile_id).unwrap();
        assert_eq!(
            wait_for_terminal(&manager, &first.run_id).state,
            BuildRunState::Succeeded
        );
        let debug_generation_id = manager.storage.artifact_generations().unwrap().generations[0]
            .generation_id
            .clone();

        let project_root = PathBuf::from(&debug.working_directory);
        let missing_artifact = project_root.join("target/older");
        std::fs::create_dir_all(&missing_artifact).unwrap();
        std::fs::write(missing_artifact.join("output.bin"), b"older").unwrap();
        let snapshot = snapshot_registered_path(
            &manager.file_system,
            &project_root,
            Path::new("target/older"),
        )
        .unwrap();
        let failed_generation_id = "e".repeat(32);
        let mut profiles = manager.storage.build_profiles().unwrap();
        let debug_profile = profiles
            .iter_mut()
            .find(|profile| profile.profile_id == debug.profile_id)
            .unwrap();
        debug_profile.artifact_paths.push(RegisteredArtifactPath {
            relative_path: "target/older".into(),
            role: ArtifactRole::Generation,
        });
        manager.storage.write_build_profiles(&profiles).unwrap();
        let mut ledger = manager.storage.artifact_generations().unwrap();
        let mut older = ledger.generations[0].clone();
        older.generation_id = failed_generation_id.clone();
        older.normalized_path = "target/older".into();
        older.allocated_bytes = snapshot.allocated_bytes;
        older.identity = Some(snapshot.identity);
        older.observed_modified_at_unix_nanos = snapshot.modified_at_unix_nanos;
        older.last_successful_touch = Some(1);
        older.touched_by_latest_success = false;
        ledger.generations.push(older);
        manager.storage.write_artifact_generations(&ledger).unwrap();

        let mut release_input = debug_input;
        release_input.display_name = "Local release".into();
        release_input.argv = vec!["build".into(), "--release".into()];
        release_input.profile_label = "release".into();
        release_input.target_label = "release-target".into();
        release_input.artifact_paths[0].relative_path = "target/release".into();
        let release = manager.register_profile(release_input).unwrap();
        let mut policy = ArtifactBudgetPolicy {
            enabled: true,
            ..ArtifactBudgetPolicy::default()
        };
        policy.global_limits.maximum_allocated_bytes = Some(1);
        manager.set_policy(policy).unwrap();
        let removed = missing_artifact.clone();
        manager.set_before_registered_artifact_execution(move |plan| {
            if plan.items.iter().any(|item| item.proof.path == removed) && removed.exists() {
                std::fs::remove_dir_all(&removed).unwrap();
            }
        });

        let run = manager.start_run(&release.profile_id).unwrap();
        assert_eq!(
            wait_for_terminal(&manager, &run.run_id).state,
            BuildRunState::AnalysisFailed
        );
        let retained_ids = manager
            .storage
            .artifact_generations()
            .unwrap()
            .generations
            .into_iter()
            .map(|generation| generation.generation_id)
            .collect::<std::collections::HashSet<_>>();
        assert!(!retained_ids.contains(&debug_generation_id));
        assert!(retained_ids.contains(&failed_generation_id));
        assert!(!project_root.join("target/debug").exists());
        assert!(!missing_artifact.exists());

        let service = super::super::execution::CleanupService::new(root.join("app-data")).unwrap();
        let execution = service
            .history()
            .unwrap()
            .into_iter()
            .find(|entry| {
                entry
                    .items
                    .iter()
                    .any(|item| item.state == ItemState::Quarantined)
                    && entry
                        .items
                        .iter()
                        .any(|item| item.state == ItemState::Failed)
            })
            .unwrap();
        service.undo(&execution.execution_id).unwrap();
        assert!(project_root.join("target/debug/output.bin").exists());
        assert!(!missing_artifact.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    fn copy_fixture(source: &Path, destination: &Path) {
        std::fs::create_dir_all(destination).unwrap();
        for entry in std::fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            let target = destination.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_fixture(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), target).unwrap();
            }
        }
    }

    struct QuarantinedBudgetFixture {
        root: PathBuf,
        stale_bytes: Vec<u8>,
        protected_before: BTreeMap<PathBuf, Vec<u8>>,
        journal: super::super::storage::ExecutionJournal,
    }

    fn read_artifact_tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        fn visit(root: &Path, path: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for entry in std::fs::read_dir(path).unwrap() {
                let entry = entry.unwrap();
                let kind = entry.file_type().unwrap();
                assert!(
                    !kind.is_symlink(),
                    "disposable fixture must not contain links"
                );
                if kind.is_dir() {
                    visit(root, &entry.path(), files);
                } else {
                    assert!(kind.is_file());
                    files.insert(
                        entry.path().strip_prefix(root).unwrap().to_path_buf(),
                        std::fs::read(entry.path()).unwrap(),
                    );
                }
            }
        }
        let mut files = BTreeMap::new();
        visit(root, root, &mut files);
        files
    }

    fn quarantined_budget_fixture() -> QuarantinedBudgetFixture {
        let root = std::env::temp_dir().join(format!(
            "supa-diska-budget-enforcement-{}-{}",
            std::process::id(),
            getrandom::u64().unwrap()
        ));
        std::fs::create_dir(&root).unwrap();
        let root = std::fs::canonicalize(root).unwrap();
        copy_fixture(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/build-artifacts/rust"),
            &root,
        );
        let cargo = std::fs::canonicalize(
            std::env::var_os("CARGO").expect("Cargo must supply its executable for this test"),
        )
        .unwrap();
        let storage = CleanupStorage::open(root.join("app-data/cleanup")).unwrap();
        storage
            .write_project_roots(&[super::super::storage::ProjectRoot {
                id: "9".repeat(32),
                display_path: root.to_string_lossy().into_owned(),
                paused: false,
                added_at_unix_seconds: 1,
                last_scanned_at_unix_seconds: None,
            }])
            .unwrap();
        // Only the approval prompt is substituted; builds and filesystem mutations are native.
        let manager = BuildArtifactManager::with_components(
            storage,
            Arc::new(Mutex::new(())),
            Arc::new(TestApprover {
                approved: true,
                seen: Mutex::new(Vec::new()),
            }),
            Arc::new(NativeProcessRunner),
        );
        assert!(!manager.policy().unwrap().enabled);
        for label in ["release", "debug"] {
            let mut argv = vec!["build".into(), "--offline".into()];
            if label == "release" {
                argv.push("--release".into());
            }
            let mut artifact_paths = vec![RegisteredArtifactPath {
                relative_path: format!("target/{label}/artifact-budget-fixture.exe"),
                role: ArtifactRole::Generation,
            }];
            if label == "debug" {
                artifact_paths.extend([
                    RegisteredArtifactPath {
                        relative_path: "target/debug/deps".into(),
                        role: ArtifactRole::Dependency,
                    },
                    RegisteredArtifactPath {
                        relative_path: "target/debug/incremental".into(),
                        role: ArtifactRole::Incremental,
                    },
                ]);
            }
            let profile = manager
                .register_profile(RegisterBuildProfileInput {
                    root_id: "9".repeat(32),
                    display_name: label.into(),
                    ecosystem: BuildEcosystem::Rust,
                    executable: cargo.to_string_lossy().into_owned(),
                    argv,
                    working_directory: String::new(),
                    profile_label: label.into(),
                    toolchain_label: "test-toolchain".into(),
                    target_label: format!("{label}-target"),
                    rebuild_cost: RebuildCost::Low,
                    artifact_paths,
                })
                .unwrap();
            let run = manager.start_run(&profile.profile_id).unwrap();
            assert_eq!(
                wait_for_terminal(&manager, &run.run_id).state,
                BuildRunState::Succeeded
            );
        }

        let stale = root.join("target/release/artifact-budget-fixture.exe");
        let stale_bytes = std::fs::read(&stale).unwrap();
        assert!(!stale_bytes.is_empty());
        let protected_root = root.join("target/debug");
        let protected_before = read_artifact_tree(&protected_root);
        assert!(protected_before.contains_key(Path::new("artifact-budget-fixture.exe")));
        for directory in ["deps", "incremental"] {
            assert!(
                protected_before
                    .keys()
                    .any(|path| path.starts_with(directory))
            );
        }
        let ledger_before = manager.storage.artifact_generations().unwrap();
        let stale_id = ledger_before
            .generations
            .iter()
            .find(|generation| {
                generation.normalized_path == "target/release/artifact-budget-fixture.exe"
            })
            .unwrap()
            .generation_id
            .clone();
        assert!(manager.storage.executions().unwrap().is_empty());
        let mut policy = ArtifactBudgetPolicy {
            enabled: true,
            ..ArtifactBudgetPolicy::default()
        };
        policy.global_limits.maximum_allocated_bytes = Some(1);
        assert!(
            ledger_before
                .generations
                .iter()
                .map(|item| item.allocated_bytes)
                .sum::<u64>()
                > 1
        );

        // Public policy application performs analysis, selection, revalidation and journaled quarantine.
        let result = manager.set_policy(policy).unwrap();
        assert!(result.policy_saved);
        assert_eq!(
            result.analysis_status,
            ArtifactBudgetAnalysisStatus::Completed
        );
        assert!(!stale.exists());
        assert_eq!(read_artifact_tree(&protected_root), protected_before);
        let journals = manager.storage.executions().unwrap();
        assert_eq!(journals.len(), 1);
        let journal = &journals[0];
        assert_eq!(
            journal.disposition,
            super::super::storage::CleanupDisposition::Quarantine
        );
        assert_eq!(journal.items.len(), 1);
        assert_eq!(journal.items[0].state, ItemState::Quarantined);
        assert!(journal.items[0].failure.is_none());
        let quarantine = journal.items[0].quarantine_path.as_ref().unwrap();
        assert!(quarantine.starts_with(root.join("app-data/cleanup")));
        assert_eq!(std::fs::read(quarantine).unwrap(), stale_bytes);
        assert_eq!(journal.accounting.reclaimed_bytes, 0);
        let mut expected_ids = ledger_before
            .generations
            .iter()
            .filter(|generation| generation.generation_id != stale_id)
            .map(|generation| generation.generation_id.clone())
            .collect::<Vec<_>>();
        let mut retained_ids = manager
            .storage
            .artifact_generations()
            .unwrap()
            .generations
            .into_iter()
            .map(|generation| generation.generation_id)
            .collect::<Vec<_>>();
        expected_ids.sort();
        retained_ids.sort();
        assert_eq!(retained_ids, expected_ids);
        QuarantinedBudgetFixture {
            root,
            stale_bytes,
            protected_before,
            journal: journal.clone(),
        }
    }

    #[test]
    fn real_budget_enforcement_quarantines_only_stale_artifact_and_preserves_protected_bytes() {
        let fixture = quarantined_budget_fixture();
        std::fs::remove_dir_all(fixture.root).unwrap();
    }

    #[test]
    fn real_budget_undo_after_restart_restores_artifact_and_preserves_protected_bytes() {
        let fixture = quarantined_budget_fixture();
        let app_data = fixture.root.join("app-data");
        let original = fixture
            .root
            .join("target/release/artifact-budget-fixture.exe");
        let protected = fixture.root.join("target/debug");
        let quarantine = fixture.journal.items[0].quarantine_path.as_ref().unwrap();
        let service = super::super::execution::CleanupService::new(app_data.clone()).unwrap();
        let history = service.history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].execution_id, fixture.journal.execution_id);
        assert_eq!(history[0].items[0].state, ItemState::Quarantined);
        drop(service);

        // Reopen persisted plans and journals; no live manager or service state is reused.
        let service = super::super::execution::CleanupService::new(app_data.clone()).unwrap();
        assert!(!original.exists());
        assert_eq!(std::fs::read(quarantine).unwrap(), fixture.stale_bytes);
        assert_eq!(read_artifact_tree(&protected), fixture.protected_before);
        let restored = service.undo(&fixture.journal.execution_id).unwrap();
        assert_eq!(restored.items.len(), 1);
        assert_eq!(restored.items[0].state, ItemState::Restored);
        assert!(restored.items[0].failure.is_none());
        assert_eq!(std::fs::read(&original).unwrap(), fixture.stale_bytes);
        assert!(!quarantine.exists());
        assert_eq!(read_artifact_tree(&protected), fixture.protected_before);
        drop(service);

        let storage = CleanupStorage::open(app_data.join("cleanup")).unwrap();
        let persisted = storage
            .read_execution(&fixture.journal.execution_id)
            .unwrap();
        assert_eq!(persisted.items[0].state, ItemState::Restored);
        assert!(persisted.items[0].failure.is_none());
        drop(storage);
        std::fs::remove_dir_all(fixture.root).unwrap();
    }

    #[test]
    fn real_warm_cargo_build_keeps_debug_and_incremental_state() {
        let root = std::env::temp_dir().join(format!(
            "supa-diska-cargo-budget-{}-{}",
            std::process::id(),
            getrandom::u64().unwrap()
        ));
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/build-artifacts/rust");
        copy_fixture(&fixture, &root);
        let root = std::fs::canonicalize(root).unwrap();
        let cargo = std::env::var_os("CARGO")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute() && path.exists())
            .or_else(|| {
                std::env::var_os("USERPROFILE")
                    .map(PathBuf::from)
                    .map(|home| home.join(".cargo/bin/cargo.exe"))
                    .filter(|path| path.exists())
            })
            .map(|path| std::fs::canonicalize(path).unwrap())
            .expect("an absolute cargo.exe is required for this Windows integration test");
        let storage = CleanupStorage::open(root.join("app-data/cleanup")).unwrap();
        storage
            .write_project_roots(&[super::super::storage::ProjectRoot {
                id: "9".repeat(32),
                display_path: root.to_string_lossy().into_owned(),
                paused: false,
                added_at_unix_seconds: 1,
                last_scanned_at_unix_seconds: None,
            }])
            .unwrap();
        let approver = Arc::new(TestApprover {
            approved: true,
            seen: Mutex::new(Vec::new()),
        });
        let manager = BuildArtifactManager::with_components(
            storage,
            Arc::new(Mutex::new(())),
            approver,
            Arc::new(NativeProcessRunner),
        );
        let profile =
            |name: &str, argv: Vec<String>, target: &str, artifacts| RegisterBuildProfileInput {
                root_id: "9".repeat(32),
                display_name: name.into(),
                ecosystem: BuildEcosystem::Rust,
                executable: cargo.to_string_lossy().into_owned(),
                argv,
                working_directory: String::new(),
                profile_label: name.into(),
                toolchain_label: "test-toolchain".into(),
                target_label: target.into(),
                rebuild_cost: RebuildCost::Low,
                artifact_paths: artifacts,
            };
        let release = manager
            .register_profile(profile(
                "release",
                vec!["build".into(), "--release".into()],
                "release-target",
                vec![RegisteredArtifactPath {
                    relative_path: "target/release/artifact-budget-fixture.exe".into(),
                    role: ArtifactRole::Generation,
                }],
            ))
            .unwrap();
        let run = manager.start_run(&release.profile_id).unwrap();
        assert_eq!(
            wait_for_terminal(&manager, &run.run_id).state,
            BuildRunState::Succeeded
        );
        let debug = manager
            .register_profile(profile(
                "debug",
                vec!["build".into()],
                "debug-target",
                vec![
                    RegisteredArtifactPath {
                        relative_path: "target/debug/artifact-budget-fixture.exe".into(),
                        role: ArtifactRole::Generation,
                    },
                    RegisteredArtifactPath {
                        relative_path: "target/debug/deps".into(),
                        role: ArtifactRole::Dependency,
                    },
                    RegisteredArtifactPath {
                        relative_path: "target/debug/incremental".into(),
                        role: ArtifactRole::Incremental,
                    },
                ],
            ))
            .unwrap();
        let run = manager.start_run(&debug.profile_id).unwrap();
        assert_eq!(
            wait_for_terminal(&manager, &run.run_id).state,
            BuildRunState::Succeeded
        );
        let executable = root.join("target/debug/artifact-budget-fixture.exe");
        let incremental = root.join("target/debug/incremental");
        let executable_modified = std::fs::metadata(&executable).unwrap().modified().unwrap();
        let incremental_identity = WindowsFileSystem
            .metadata_no_follow(&incremental)
            .unwrap()
            .identity;
        let mut policy = ArtifactBudgetPolicy {
            enabled: true,
            ..ArtifactBudgetPolicy::default()
        };
        policy.global_limits.maximum_allocated_bytes = Some(1);
        manager.set_policy(policy).unwrap();
        assert!(
            !root
                .join("target/release/artifact-budget-fixture.exe")
                .exists()
        );
        assert!(executable.exists());
        assert!(incremental.exists());

        let run = manager.start_run(&debug.profile_id).unwrap();
        assert_eq!(
            wait_for_terminal(&manager, &run.run_id).state,
            BuildRunState::Succeeded
        );
        assert_eq!(
            std::fs::metadata(&executable).unwrap().modified().unwrap(),
            executable_modified,
            "a warm build must not rebuild the unchanged binary"
        );
        assert_eq!(
            WindowsFileSystem
                .metadata_no_follow(&incremental)
                .unwrap()
                .identity,
            incremental_identity
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    fn wait_for_file(path: &Path) {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !path.exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for {}",
                path.display()
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[derive(Debug)]
    struct FixtureProcess(OwnedHandle);

    impl FixtureProcess {
        fn open(pid_file: &Path) -> Self {
            use windows_sys::Win32::System::Threading::{
                OpenProcess, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
            };
            let pid = std::fs::read_to_string(pid_file).unwrap().parse().unwrap();
            // SAFETY: the PID comes from our disposable fixture; retain its handle before cancel.
            let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_TERMINATE, 0, pid) };
            assert!(
                !handle.is_null(),
                "must open live fixture process before cancellation"
            );
            // SAFETY: OpenProcess returned an owned handle.
            let process = Self(unsafe { OwnedHandle::from_raw_handle(handle) });
            assert!(
                !process.exited(),
                "fixture must be alive before cancellation"
            );
            process
        }

        fn exited(&self) -> bool {
            use windows_sys::Win32::{
                Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
                System::Threading::WaitForSingleObject,
            };
            // SAFETY: the retained handle is live; zero timeout cannot wait for a late exit.
            let result = unsafe { WaitForSingleObject(self.0.as_raw_handle() as HANDLE, 0) };
            assert!(
                result == WAIT_OBJECT_0 || result == WAIT_TIMEOUT,
                "process probe failed"
            );
            result == WAIT_OBJECT_0
        }
    }

    impl Drop for FixtureProcess {
        fn drop(&mut self) {
            use windows_sys::Win32::System::Threading::{TerminateProcess, WaitForSingleObject};
            // SAFETY: this owns a fixture-only handle. Cleanup cannot turn a failed probe into a pass.
            unsafe {
                TerminateProcess(self.0.as_raw_handle() as HANDLE, 1);
                WaitForSingleObject(self.0.as_raw_handle() as HANDLE, 2_000);
            }
        }
    }

    fn assert_native_tree_cancelled(executable: &Path, argv: &[String], root: &Path) {
        let ready = root.join("descendant.pid");
        let sentinel = root.join("descendant.sentinel");
        let cancelled = AtomicBool::new(false);
        thread::scope(|scope| {
            struct CancelOnDrop<'a>(&'a AtomicBool);
            impl Drop for CancelOnDrop<'_> {
                fn drop(&mut self) {
                    self.0.store(true, Ordering::Release);
                }
            }
            let _cancel_on_failure = CancelOnDrop(&cancelled);
            let (sender, receiver) = std::sync::mpsc::channel::<[FixtureProcess; 2]>();
            let cancelled = &cancelled;
            let run = scope.spawn(move || {
                let outcome = NativeProcessRunner.run(executable, argv, root, cancelled);
                // Handles were opened and sent before cancellation, avoiding PID reuse and
                // treating OpenProcess failure as success. Probe on this thread at runner return.
                let processes = receiver.recv().unwrap();
                let exited_at_return = processes.each_ref().map(FixtureProcess::exited);
                (outcome, exited_at_return, processes)
            });
            wait_for_file(&ready);
            let processes = [
                FixtureProcess::open(&ready.with_extension("parent.pid")),
                FixtureProcess::open(&ready),
            ];
            assert!(
                !sentinel.exists(),
                "cancellation must precede the delayed write"
            );
            sender.send(processes).unwrap();
            cancelled.store(true, Ordering::Release);
            let (outcome, exited_at_return, processes) = run.join().unwrap();
            assert_eq!(outcome.unwrap(), ProcessOutcome::Cancelled);
            assert_eq!(
                exited_at_return,
                [true, true],
                "process tree alive at Cancelled return"
            );
            // Observe beyond the fixture's write delay; do not remove its directory prematurely.
            thread::sleep(Duration::from_millis(1_600));
            assert!(!sentinel.exists(), "descendant wrote after cancellation");
            drop(processes);
        });
    }

    #[test]
    fn real_cargo_cancellation_stops_build_script_and_descendant() {
        let root = std::env::temp_dir().join(format!(
            "supa-diska-cargo-cancellation-{}-{}",
            std::process::id(),
            getrandom::u64().unwrap()
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"cancellation-fixture\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[lib]\npath = \"lib.rs\"\n[workspace]\n",
        ).unwrap();
        std::fs::write(root.join("lib.rs"), "").unwrap();
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/native-process-tree.rs"),
            root.join("build.rs"),
        )
        .unwrap();
        let cargo = PathBuf::from(env!("CARGO"));
        assert!(
            cargo.is_absolute() && cargo.exists(),
            "absolute Cargo executable required"
        );
        assert_native_tree_cancelled(
            &cargo,
            &[
                "build".into(),
                "--offline".into(),
                "--target-dir".into(),
                root.join("target").to_string_lossy().into_owned(),
            ],
            &root,
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn native_process_runner_cancels_the_entire_process_tree() {
        let root = std::env::temp_dir().join(format!(
            "supa-diska-process-tree-{}-{}",
            std::process::id(),
            getrandom::u64().unwrap()
        ));
        std::fs::create_dir(&root).unwrap();
        let source =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/native-process-tree.rs");
        let executable = root.join("native-process-tree.exe");
        let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
        assert!(
            Command::new(rustc)
                .arg(source)
                .args(["--edition", "2024", "-o"])
                .arg(&executable)
                .status()
                .unwrap()
                .success(),
            "fixture compilation failed"
        );

        assert_native_tree_cancelled(
            &executable,
            &[
                "parent".into(),
                root.join("descendant.pid").to_string_lossy().into_owned(),
                root.join("descendant.sentinel")
                    .to_string_lossy()
                    .into_owned(),
            ],
            &root,
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
