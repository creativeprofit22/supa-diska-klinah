use crate::history::{HistoryCursor, HistoryKind, HistoryPage, HistoryRequest};
use crate::storage::scan_profile::{
    SCAN_SETTINGS_SCHEMA_VERSION, ScanProfile, ScanSettings, resolve_workers, seek_penalty,
};
use cleanup_core::{
    ArtifactRole, CandidateProofScope, FileSystem, PreviewRecord, ProtectionPolicy, ScanDiagnostic,
    ScanLimits, ScanSnapshot, revalidate_candidate,
};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use super::{
    build_artifacts::{
        ArtifactBudgetPreview, BuildArtifactError, BuildArtifactManager, BuildRun,
        RegisterBuildProfileInput, SetArtifactBudgetPolicyResult,
    },
    filesystem::WindowsFileSystem,
    preview::{
        CleanupPreview, PROJECT_DISCOVERY_LIMITS, PrivateCleanupScan, current_protection,
        discover_project_artifacts_with_limits, scan_temporary_caches, temporary_root,
        temporary_rule, validate_project_root,
    },
    recycle::{RecycleBin, WindowsRecycleBin, recycle_exact, restore_exact},
    storage::{
        ArtifactBudgetPolicy, AutoCleanupPolicy, BuildProfile, ByteAccounting, CleanupDisposition,
        CleanupPlan, CleanupPlanScope, CleanupStorage, ExecutionItem, ExecutionJournal, ItemState,
        MAX_ITEMS, MAX_PROJECT_ROOTS, PlanItem, ProjectRoot, StorageError,
    },
};

const MAX_MUTATION_ENTRIES: usize = 250_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CleanupServiceError {
    InvalidInput,
    NotFound,
    Conflict,
    DuplicateRoot,
    RootPaused,
    RootLimitReached,
    ValidationFailed,
    RecoveryVolumeUnsupported,
    PersistenceFailed,
    OperationFailed,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupPlanSummary {
    pub plan_id: String,
    pub disposition: CleanupDisposition,
    pub selected_count: usize,
    pub selected_bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupExecutionSummary {
    pub execution_id: String,
    pub plan_id: String,
    pub disposition: CleanupDisposition,
    pub completed: bool,
    pub purge_after: Option<u64>,
    pub items: Vec<CleanupItemOutcome>,
    pub accounting: ByteAccounting,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupItemOutcome {
    pub item_id: String,
    /// Display-only, bounded text from the retained immutable plan, never path authority.
    pub display_path: Option<String>,
    pub state: ItemState,
    pub logical_bytes: u64,
    pub failure: Option<String>,
}

#[derive(Debug)]
pub(super) struct RegisteredArtifactExecutionOutcome {
    pub quarantined_generation_ids: Vec<String>,
    pub failed_items: Vec<CleanupItemOutcome>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectArtifactScan {
    pub roots: Vec<ProjectRoot>,
    pub records: Vec<PreviewRecord>,
    pub diagnostics: Vec<ScanDiagnostic>,
    pub scanned_at_unix_seconds: u64,
}

struct RetainedScan {
    snapshot: ScanSnapshot,
    _protection: ProtectionPolicy,
}

pub struct CleanupService {
    storage: CleanupStorage,
    file_system: WindowsFileSystem,
    recycle_bin: Arc<dyn RecycleBin>,
    scans: Mutex<HashMap<String, RetainedScan>>,
    writer: Arc<Mutex<()>>,
    artifact_builds: BuildArtifactManager,
    vendor_jobs: Result<
        crate::storage::vendor_uninstall::VendorJobManager,
        crate::storage::vendor_uninstall::VendorJobError,
    >,
}

impl CleanupService {
    pub fn new(app_data: PathBuf) -> Result<Self, CleanupServiceError> {
        Self::with_recycle_bin(app_data, Arc::new(WindowsRecycleBin))
    }

    fn with_recycle_bin(
        app_data: PathBuf,
        recycle_bin: Arc<dyn RecycleBin>,
    ) -> Result<Self, CleanupServiceError> {
        let storage = CleanupStorage::open(app_data.join("cleanup")).map_err(map_storage_error)?;
        let writer = Arc::new(Mutex::new(()));
        let artifact_builds = BuildArtifactManager::new(storage.clone(), Arc::clone(&writer));
        let vendor_jobs = crate::storage::vendor_uninstall::VendorJobManager::new(
            storage.clone(),
            Arc::clone(&writer),
        );
        // A corrupt vendor ledger disables only vendor operations, never legacy recovery/undo.
        let service = Self {
            vendor_jobs,
            storage,
            file_system: WindowsFileSystem,
            recycle_bin,
            scans: Mutex::new(HashMap::new()),
            writer,
            artifact_builds,
        };
        service.reconcile_interrupted()?;
        Ok(service)
    }

    pub fn vendor_jobs(
        &self,
    ) -> Result<
        &crate::storage::vendor_uninstall::VendorJobManager,
        crate::storage::vendor_uninstall::VendorJobError,
    > {
        self.vendor_jobs.as_ref().map_err(Clone::clone)
    }

    pub fn build_profiles(&self) -> Result<Vec<BuildProfile>, BuildArtifactError> {
        self.artifact_builds.profiles()
    }

    pub fn register_build_profile(
        &self,
        input: RegisterBuildProfileInput,
    ) -> Result<BuildProfile, BuildArtifactError> {
        self.artifact_builds.register_profile(input)
    }

    pub fn remove_build_profile(&self, profile_id: &str) -> Result<(), BuildArtifactError> {
        self.artifact_builds.remove_profile(profile_id)
    }

    pub fn start_build_run(&self, profile_id: &str) -> Result<BuildRun, BuildArtifactError> {
        self.artifact_builds.start_run(profile_id)
    }

    pub fn active_build_run(&self) -> Result<Option<BuildRun>, BuildArtifactError> {
        self.artifact_builds.active_run()
    }

    pub fn build_run(&self, run_id: &str) -> Result<BuildRun, BuildArtifactError> {
        self.artifact_builds.run(run_id)
    }

    pub fn cancel_build_run(&self, run_id: &str) -> Result<BuildRun, BuildArtifactError> {
        self.artifact_builds.cancel_run(run_id)
    }

    pub fn artifact_budget_policy(&self) -> Result<ArtifactBudgetPolicy, BuildArtifactError> {
        self.artifact_builds.policy()
    }

    pub fn set_artifact_budget_policy(
        &self,
        policy: ArtifactBudgetPolicy,
    ) -> Result<SetArtifactBudgetPolicyResult, BuildArtifactError> {
        self.artifact_builds.set_policy(policy)
    }

    pub fn preview_artifact_budget(&self) -> Result<ArtifactBudgetPreview, BuildArtifactError> {
        self.artifact_builds.preview_budget()
    }

    pub fn analyze_due_build_artifacts(&self) -> Result<bool, BuildArtifactError> {
        self.artifact_builds.analyze_due_external()
    }

    pub fn preview(&self) -> Result<CleanupPreview, CleanupServiceError> {
        let PrivateCleanupScan {
            preview,
            snapshot,
            protection,
        } = scan_temporary_caches().map_err(|_| CleanupServiceError::OperationFailed)?;
        let mut scans = self
            .scans
            .lock()
            .map_err(|_| CleanupServiceError::Conflict)?;
        if scans.len() >= 4 {
            scans.clear();
        }
        scans.insert(
            preview.scan_id.clone(),
            RetainedScan {
                snapshot,
                _protection: protection,
            },
        );
        Ok(preview)
    }

    pub fn list_project_roots(&self) -> Result<Vec<ProjectRoot>, CleanupServiceError> {
        let mut roots = self.storage.project_roots().map_err(map_storage_error)?;
        sort_project_roots(&mut roots, self.file_system.semantics());
        Ok(roots)
    }

    pub fn add_project_root(&self, path: &str) -> Result<Vec<ProjectRoot>, CleanupServiceError> {
        let _writer = self
            .writer
            .lock()
            .map_err(|_| CleanupServiceError::Conflict)?;
        let protection = current_protection().map_err(|_| CleanupServiceError::ValidationFailed)?;
        let canonical = validate_project_root(&self.file_system, &protection, path)
            .map_err(|_| CleanupServiceError::InvalidInput)?;
        let mut roots = self.storage.project_roots().map_err(map_storage_error)?;
        if roots.len() >= MAX_PROJECT_ROOTS {
            return Err(CleanupServiceError::RootLimitReached);
        }
        if roots.iter().any(|root| {
            self.file_system
                .semantics()
                .equivalent(Path::new(&root.display_path), &canonical)
        }) {
            return Err(CleanupServiceError::DuplicateRoot);
        }
        roots.push(ProjectRoot {
            id: random_id()?,
            display_path: canonical.to_string_lossy().into_owned(),
            paused: false,
            added_at_unix_seconds: now_seconds()?,
            last_scanned_at_unix_seconds: None,
        });
        sort_project_roots(&mut roots, self.file_system.semantics());
        self.storage
            .write_project_roots(&roots)
            .map_err(map_storage_error)?;
        Ok(roots)
    }

    pub fn set_project_root_paused(
        &self,
        id: &str,
        paused: bool,
    ) -> Result<Vec<ProjectRoot>, CleanupServiceError> {
        if !valid_id(id) {
            return Err(CleanupServiceError::InvalidInput);
        }
        let _writer = self
            .writer
            .lock()
            .map_err(|_| CleanupServiceError::Conflict)?;
        let mut roots = self.storage.project_roots().map_err(map_storage_error)?;
        let root = roots
            .iter_mut()
            .find(|root| root.id == id)
            .ok_or(CleanupServiceError::NotFound)?;
        root.paused = paused;
        sort_project_roots(&mut roots, self.file_system.semantics());
        self.storage
            .write_project_roots(&roots)
            .map_err(map_storage_error)?;
        Ok(roots)
    }

    pub fn remove_project_root(&self, id: &str) -> Result<Vec<ProjectRoot>, CleanupServiceError> {
        if !valid_id(id) {
            return Err(CleanupServiceError::InvalidInput);
        }
        let _writer = self
            .writer
            .lock()
            .map_err(|_| CleanupServiceError::Conflict)?;
        let mut roots = self.storage.project_roots().map_err(map_storage_error)?;
        let before = roots.len();
        roots.retain(|root| root.id != id);
        if roots.len() == before {
            return Err(CleanupServiceError::NotFound);
        }
        self.storage
            .write_project_roots(&roots)
            .map_err(map_storage_error)?;
        Ok(roots)
    }

    pub fn discover_project_artifacts(
        &self,
        root_id: Option<&str>,
    ) -> Result<ProjectArtifactScan, CleanupServiceError> {
        if root_id.is_some_and(|id| !valid_id(id)) {
            return Err(CleanupServiceError::InvalidInput);
        }
        let _writer = self
            .writer
            .lock()
            .map_err(|_| CleanupServiceError::Conflict)?;
        let mut roots = self.storage.project_roots().map_err(map_storage_error)?;
        let mut selected: Vec<ProjectRoot> = match root_id {
            Some(id) => vec![
                roots
                    .iter()
                    .find(|root| root.id == id)
                    .cloned()
                    .ok_or(CleanupServiceError::NotFound)?,
            ],
            None => roots.iter().filter(|root| !root.paused).cloned().collect(),
        };
        if selected.iter().any(|root| root.paused) {
            return Err(CleanupServiceError::RootPaused);
        }
        sort_project_roots(&mut selected, self.file_system.semantics());
        if root_id.is_none() {
            selected = collapse_project_roots(selected, self.file_system.semantics());
        }
        let protection = current_protection().map_err(|_| CleanupServiceError::ValidationFailed)?;
        let scanned_at = now_seconds()?;
        let divisor = selected.len().max(1);
        let base_limits = divided_project_limits(divisor);
        let profile = self.scan_profile();
        let mut records = Vec::new();
        let mut diagnostics = Vec::new();
        let mut scanned_ids = HashSet::new();
        for root in selected {
            let root_seek_penalty = match profile {
                ScanProfile::Auto => seek_penalty(Path::new(&root.display_path)),
                ScanProfile::Ssd | ScanProfile::Hdd => None,
            };
            let limits = project_root_limits(base_limits, profile, root_seek_penalty);
            let discovery = discover_project_artifacts_with_limits(
                Arc::new(WindowsFileSystem),
                PathBuf::from(&root.display_path),
                protection.clone(),
                &ServiceEntropy,
                limits,
            )
            .map_err(|_| CleanupServiceError::OperationFailed)?;
            records.extend(discovery.records);
            diagnostics.extend(discovery.diagnostics);
            scanned_ids.insert(root.id);
        }
        records.sort_by(|left, right| {
            self.file_system
                .semantics()
                .key(Path::new(&left.display_path))
                .cmp(
                    &self
                        .file_system
                        .semantics()
                        .key(Path::new(&right.display_path)),
                )
                .then_with(|| left.rule_id.cmp(&right.rule_id))
        });
        records.dedup_by(|left, right| {
            self.file_system.semantics().equivalent(
                Path::new(&left.display_path),
                Path::new(&right.display_path),
            )
        });
        diagnostics.truncate(PROJECT_DISCOVERY_LIMITS.max_diagnostics);
        for root in &mut roots {
            if scanned_ids.contains(&root.id) {
                root.last_scanned_at_unix_seconds = Some(scanned_at);
            }
        }
        sort_project_roots(&mut roots, self.file_system.semantics());
        self.storage
            .write_project_roots(&roots)
            .map_err(map_storage_error)?;
        Ok(ProjectArtifactScan {
            roots,
            records,
            diagnostics,
            scanned_at_unix_seconds: scanned_at,
        })
    }

    /// Backend-only bridge from retained storage IDs to the existing immutable plan/journal
    /// flow. Recycle remains identity-required-recycle-unsupported (no pathname fallback).
    pub fn create_storage_plan(
        &self,
        service: &crate::storage::scans::StorageService,
        selection: &cleanup_core::storage::StorageSelection,
        disposition: CleanupDisposition,
    ) -> Result<CleanupPlanSummary, CleanupServiceError> {
        use cleanup_core::storage::StorageModule;
        if !matches!(
            selection.module,
            StorageModule::Cleaner
                | StorageModule::Browser
                | StorageModule::LargeFiles
                | StorageModule::Duplicates
                | StorageModule::EmptyFolders
        ) {
            return Err(CleanupServiceError::ValidationFailed);
        }
        let _writer = self
            .writer
            .lock()
            .map_err(|_| CleanupServiceError::Conflict)?;
        let evidence = service.resolve_selection(selection).map_err(|error| {
            use crate::storage::scans::JobError;
            use cleanup_core::storage::StorageError;
            // Keep refusal reasons distinct: a live snapshot that rejects the selected
            // evidence (for example every copy of a duplicate group) is not "unavailable".
            match error {
                JobError::Storage(StorageError::SnapshotUnavailable) => {
                    CleanupServiceError::NotFound
                }
                JobError::Storage(StorageError::InvalidRequest) => {
                    CleanupServiceError::InvalidInput
                }
                JobError::Busy => CleanupServiceError::Conflict,
                JobError::Storage(_) => CleanupServiceError::ValidationFailed,
                JobError::Native(_) | JobError::WorkerFailed => {
                    CleanupServiceError::OperationFailed
                }
            }
        })?;
        let root = evidence
            .first()
            .ok_or(CleanupServiceError::InvalidInput)?
            .root()
            .clone();
        let mut items = selection
            .candidate_ids
            .iter()
            .zip(evidence)
            .map(|(id, evidence)| {
                let proof = if selection.module == StorageModule::EmptyFolders {
                    crate::storage::empty_folders::plan_proof(evidence)
                } else if selection.module == StorageModule::Duplicates {
                    crate::storage::duplicates::plan_proof(evidence)
                } else if selection.module == StorageModule::LargeFiles {
                    crate::storage::large_files::plan_proof(evidence)
                } else {
                    crate::storage::cleaner::plan_proof(evidence)
                }
                .map_err(|_| CleanupServiceError::ValidationFailed)?;
                Ok(PlanItem {
                    item_id: id.clone(),
                    proof,
                })
            })
            .collect::<Result<Vec<_>, CleanupServiceError>>()?;
        if selection.module == StorageModule::EmptyFolders {
            items.sort_by_key(|item| std::cmp::Reverse(item.proof.path.components().count()));
        }
        let selected_bytes = items
            .iter()
            .try_fold(0u64, |sum, item| sum.checked_add(item.proof.logical_bytes))
            .ok_or(CleanupServiceError::InvalidInput)?;
        let plan_id = random_id()?;
        let plan = CleanupPlan::with_scope(
            plan_id.clone(),
            selection.snapshot_id.clone(),
            now_seconds()?,
            disposition,
            CleanupPlanScope::Storage {
                module: selection.module,
                root,
            },
            items,
        )
        .map_err(map_storage_error)?;
        self.validate_current_scope(&plan)?;
        self.validate_storage_recovery_support(&plan)?;
        self.storage.create_plan(&plan).map_err(map_storage_error)?;
        Ok(CleanupPlanSummary {
            plan_id,
            disposition,
            selected_count: plan.items.len(),
            selected_bytes,
        })
    }

    pub fn create_plan(
        &self,
        scan_id: &str,
        candidate_ids: &[String],
        disposition: CleanupDisposition,
    ) -> Result<CleanupPlanSummary, CleanupServiceError> {
        if !valid_id(scan_id)
            || candidate_ids.is_empty()
            || candidate_ids.len() > MAX_ITEMS
            || candidate_ids.iter().any(|id| !valid_id(id))
        {
            return Err(CleanupServiceError::InvalidInput);
        }
        let _writer = self
            .writer
            .lock()
            .map_err(|_| CleanupServiceError::Conflict)?;
        let scans = self
            .scans
            .lock()
            .map_err(|_| CleanupServiceError::Conflict)?;
        let scan = scans.get(scan_id).ok_or(CleanupServiceError::NotFound)?;
        let mut selected = HashSet::new();
        let mut items = Vec::with_capacity(candidate_ids.len());
        for candidate_id in candidate_ids {
            if !selected.insert(candidate_id) {
                return Err(CleanupServiceError::InvalidInput);
            }
            let proof = scan
                .snapshot
                .resolve(candidate_id)
                .ok_or(CleanupServiceError::NotFound)?
                .clone();
            items.push(PlanItem {
                item_id: candidate_id.clone(),
                proof,
            });
        }
        let selected_bytes = items
            .iter()
            .try_fold(0_u64, |total, item| {
                total.checked_add(item.proof.logical_bytes)
            })
            .ok_or(CleanupServiceError::InvalidInput)?;
        let plan_id = random_id()?;
        let scope = match &items[0].proof.scope {
            CandidateProofScope::Temporary => CleanupPlanScope::Temporary,
            CandidateProofScope::Storage { evidence } => CleanupPlanScope::Storage {
                module: evidence.module(),
                root: evidence.root().clone(),
            },
            _ => return Err(CleanupServiceError::ValidationFailed),
        };
        if matches!(
            scope,
            CleanupPlanScope::Storage {
                module: cleanup_core::storage::StorageModule::EmptyFolders,
                ..
            }
        ) {
            items.sort_by_key(|item| std::cmp::Reverse(item.proof.path.components().count()));
        }
        let plan = if matches!(scope, CleanupPlanScope::Temporary) {
            CleanupPlan::new(
                plan_id.clone(),
                scan_id.to_owned(),
                now_seconds()?,
                disposition,
                items,
            )
        } else {
            CleanupPlan::with_scope(
                plan_id.clone(),
                scan_id.to_owned(),
                now_seconds()?,
                disposition,
                scope,
                items,
            )
        }
        .map_err(map_storage_error)?;
        self.validate_current_scope(&plan)?;
        self.storage.create_plan(&plan).map_err(map_storage_error)?;
        Ok(CleanupPlanSummary {
            plan_id,
            disposition,
            selected_count: plan.items.len(),
            selected_bytes,
        })
    }

    pub fn execute(&self, plan_id: &str) -> Result<CleanupExecutionSummary, CleanupServiceError> {
        self.execute_inner(plan_id, false)
    }

    pub fn execute_permanent(
        &self,
        plan_id: &str,
    ) -> Result<CleanupExecutionSummary, CleanupServiceError> {
        self.execute_inner(plan_id, true)
    }

    /// `owner` must be the HWND obtained by the native app, never a frontend argument.
    pub fn execute_permanent_confirmed(
        &self,
        plan_id: &str,
        owner: isize,
    ) -> Result<CleanupExecutionSummary, CleanupServiceError> {
        self.confirm_permanent_with(plan_id, owner, |owner, text| {
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                IDYES, MB_DEFBUTTON2, MB_ICONWARNING, MB_YESNO, MessageBoxW,
            };
            let text: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
            let title: Vec<u16> = "Confirm permanent cleanup"
                .encode_utf16()
                .chain(Some(0))
                .collect();
            // SAFETY: app-owned HWND and live, NUL-terminated bounded UTF-16 buffers.
            let result = unsafe {
                MessageBoxW(
                    owner as _,
                    text.as_ptr(),
                    title.as_ptr(),
                    MB_YESNO | MB_DEFBUTTON2 | MB_ICONWARNING,
                )
            };
            if result == 0 {
                Err(CleanupServiceError::OperationFailed)
            } else {
                Ok(result == IDYES)
            }
        })
    }

    fn confirm_permanent_with(
        &self,
        plan_id: &str,
        owner: isize,
        confirm: impl FnOnce(isize, &str) -> Result<bool, CleanupServiceError>,
    ) -> Result<CleanupExecutionSummary, CleanupServiceError> {
        if owner == 0 {
            return Err(CleanupServiceError::InvalidInput);
        }
        let text = {
            let _writer = self
                .writer
                .lock()
                .map_err(|_| CleanupServiceError::Conflict)?;
            let plan = self.storage.read_plan(plan_id).map_err(map_storage_error)?;
            if plan.disposition != CleanupDisposition::Permanent {
                return Err(CleanupServiceError::InvalidInput);
            }
            if self
                .storage
                .has_execution_for_plan(&plan.plan_id)
                .map_err(map_storage_error)?
            {
                return Err(CleanupServiceError::Conflict);
            }
            self.validate_current_scope(&plan)?;
            let bytes = checked_sum(plan.items.iter().map(|p| p.proof.logical_bytes))?;
            let mut text = format!(
                "Permanently delete {} selected items ({} logical bytes)?\nNo recovery is available.\nPlan: {}\n",
                plan.items.len(),
                bytes,
                plan.plan_id
            );
            // Bounded escaped preview; count/bytes cover the full immutable native selection.
            for item in plan.items.iter().take(8) {
                let path: String = item
                    .proof
                    .path
                    .to_string_lossy()
                    .chars()
                    .take(180)
                    .flat_map(char::escape_default)
                    .collect();
                text.push_str(&format!("\n{}: {}", item.item_id, path));
            }
            if plan.items.len() > 8 {
                text.push_str("\nAdditional selected items omitted.");
            }
            text
        };
        if !confirm(owner, &text)? {
            return Err(CleanupServiceError::OperationFailed);
        }
        self.execute_permanent(plan_id)
    }

    /// Native identities only; review is advisory, so execution must check again.
    fn validate_storage_recovery_support(
        &self,
        plan: &CleanupPlan,
    ) -> Result<(), CleanupServiceError> {
        if plan.disposition == CleanupDisposition::Quarantine
            && matches!(plan.scope, CleanupPlanScope::Storage { .. })
        {
            for item in &plan.items {
                use cleanup_core::FsErrorKind;
                match self
                    .file_system
                    .same_volume(&item.proof.path, self.storage.native_root())
                {
                    Ok(true) => {}
                    Ok(false) => return Err(CleanupServiceError::RecoveryVolumeUnsupported),
                    // An unreachable item is recorded per item by the execution guard; a
                    // cross-volume handle rename would still fail closed for that item.
                    Err(error)
                        if matches!(
                            error.kind,
                            FsErrorKind::NotFound
                                | FsErrorKind::InUse
                                | FsErrorKind::PermissionDenied
                        ) => {}
                    Err(_) => return Err(CleanupServiceError::ValidationFailed),
                }
            }
        }
        Ok(())
    }

    fn execute_inner(
        &self,
        plan_id: &str,
        permanent_command: bool,
    ) -> Result<CleanupExecutionSummary, CleanupServiceError> {
        let _writer = self
            .writer
            .lock()
            .map_err(|_| CleanupServiceError::Conflict)?;
        let plan = self.storage.read_plan(plan_id).map_err(map_storage_error)?;
        if (plan.disposition == CleanupDisposition::Permanent) != permanent_command {
            return Err(CleanupServiceError::InvalidInput);
        }
        if self
            .storage
            .has_execution_for_plan(&plan.plan_id)
            .map_err(map_storage_error)?
        {
            return Err(CleanupServiceError::Conflict);
        }
        self.validate_scope(&plan, true)?;
        self.validate_storage_recovery_support(&plan)?;
        // Pin every duplicate group before its first mutation; never reopen members
        // while DELETE handles are held. Keepers/ancestors survive journal completion.
        let mut duplicate_members = std::collections::HashMap::new();
        let mut duplicate_keepers = std::collections::HashMap::new();
        for (index, item) in plan.items.iter().enumerate() {
            if let CandidateProofScope::Storage { evidence } = &item.proof.scope
                && let cleanup_core::storage::StorageEvidence::DuplicateMember {
                    root,
                    entry,
                    keeper,
                } = evidence.as_ref()
            {
                let key = keeper.group_id.clone();
                if !duplicate_keepers.contains_key(&key) {
                    let guard = self
                        .file_system
                        .guard_entry(&keeper.keeper_root, &keeper.keeper, true)
                        .map_err(|_| CleanupServiceError::ValidationFailed)?;
                    duplicate_keepers.insert(key.clone(), guard);
                }
                let member = self
                    .file_system
                    .guard_duplicate_member(root, entry)
                    .map_err(|_| CleanupServiceError::ValidationFailed)?;
                crate::storage::duplicates::verify(
                    &duplicate_keepers[&key],
                    &member,
                    &keeper.full_sha256,
                    &cleanup_core::CancellationToken::default(),
                )
                .map_err(|_| CleanupServiceError::ValidationFailed)?;
                duplicate_members.insert(index, member);
            }
        }
        let execution_id = random_id()?;
        let started_at = now_seconds()?;
        let policy = self.storage.policy().map_err(map_storage_error)?;
        let purge_after = (plan.disposition == CleanupDisposition::Quarantine
            && !matches!(plan.scope, CleanupPlanScope::Storage { .. }))
        .then(|| started_at.saturating_add(u64::from(policy.grace_days) * 86_400));
        let selected_bytes = checked_sum(plan.items.iter().map(|item| item.proof.logical_bytes))?;
        let mut journal = ExecutionJournal {
            schema_version: 1,
            execution_id,
            plan_id: plan.plan_id.clone(),
            started_at,
            completed_at: None,
            disposition: plan.disposition,
            purge_after,
            items: plan
                .items
                .iter()
                .map(|item| ExecutionItem {
                    item_id: item.item_id.clone(),
                    state: ItemState::Pending,
                    logical_bytes: item.proof.logical_bytes,
                    processed: false,
                    occupied_bytes: 0,
                    reclaimed_bytes: 0,
                    quarantine_path: None,
                    recycle_item: None,
                    failure: None,
                })
                .collect(),
            accounting: ByteAccounting {
                selected_bytes,
                ..ByteAccounting::default()
            },
        };
        self.storage
            .write_execution(&journal)
            .map_err(map_storage_error)?;

        for index in 0..journal.items.len() {
            journal.items[index].state = ItemState::Mutating;
            if plan.disposition == CleanupDisposition::Quarantine
                && matches!(plan.scope, CleanupPlanScope::Storage { .. })
            {
                journal.items[index].quarantine_path = Some(
                    self.storage
                        .expected_quarantine_path(
                            &journal.execution_id,
                            &journal.items[index].item_id,
                        )
                        .map_err(map_storage_error)?,
                );
                journal.items[index].occupied_bytes = plan.items[index].proof.allocated_bytes;
            } else if plan.disposition == CleanupDisposition::Quarantine {
                journal.items[index].quarantine_path = Some(
                    self.storage
                        .quarantine_directory(&journal.execution_id, &journal.items[index].item_id)
                        .map_err(map_storage_error)?,
                );
            }
            self.storage
                .write_execution(&journal)
                .map_err(map_storage_error)?;
            let planned = &plan.items[index];
            let protection = match current_protection() {
                Ok(protection) => protection,
                Err(_) => {
                    fail_item(&mut journal.items[index], "protection-unavailable");
                    persist_accounting(&self.storage, &mut journal)?;
                    continue;
                }
            };
            if let CandidateProofScope::Storage { evidence } = &planned.proof.scope {
                // Keep Kudu's per-item reasons for files that vanished, are locked, or are
                // access-denied; everything else stays a generic revalidation refusal.
                let guard = if matches!(
                    evidence.as_ref(),
                    cleanup_core::storage::StorageEvidence::DuplicateMember { .. }
                ) {
                    duplicate_members
                        .remove(&index)
                        .ok_or("storage-revalidation-rejected")
                } else {
                    self.storage_entry_guard(evidence, &protection)
                };
                let result = guard.and_then(|guard| match plan.disposition {
                    CleanupDisposition::Permanent => {
                        guard.remove().map_err(|_| "permanent-remove-failed")
                    }
                    CleanupDisposition::Quarantine => {
                        // Simplification ceiling: files only, same-volume handle rename;
                        // never copy/delete fallback, shell recycle, or recursive removal.
                        guard
                            .rename_to_recovery(
                                journal.items[index]
                                    .quarantine_path
                                    .as_ref()
                                    .ok_or("quarantine-unavailable")?,
                            )
                            .map_err(|_| "quarantine-move-failed")
                    }
                    CleanupDisposition::RecycleBin => {
                        super::recycle::reject_identity_required(&guard)
                            .map_err(|_| "identity-required-recycle-unsupported")
                    }
                });
                match result {
                    Ok(()) => {
                        journal.items[index].processed = true;
                        journal.items[index].state =
                            if plan.disposition == CleanupDisposition::Quarantine {
                                ItemState::Quarantined
                            } else {
                                ItemState::Purged
                            };
                    }
                    Err(reason) => fail_item(&mut journal.items[index], reason),
                }
                persist_accounting(&self.storage, &mut journal)?;
                continue;
            }
            let measured = match revalidate_candidate(
                &self.file_system,
                &planned.proof,
                &protection,
                SystemTime::now(),
            ) {
                Ok(measured) => measured,
                Err(_) => {
                    fail_item(&mut journal.items[index], "revalidation-rejected");
                    persist_accounting(&self.storage, &mut journal)?;
                    continue;
                }
            };
            journal.items[index].processed = true;
            journal.items[index].logical_bytes = measured.logical_bytes;
            journal.items[index].occupied_bytes = measured.allocated_bytes;
            if let Err(reason) = self.mutate_item(&plan, index, &mut journal) {
                fail_item(&mut journal.items[index], reason);
            }
            persist_accounting(&self.storage, &mut journal)?;
        }
        journal.completed_at = Some(now_seconds()?);
        persist_accounting(&self.storage, &mut journal)?;
        // Explicit lifetime: keeper and ancestor guards survive every selected
        // member removal and the durable group/plan completion write.
        drop(duplicate_keepers);
        Ok(summary(&journal, &self.storage))
    }

    fn mutate_item(
        &self,
        plan: &CleanupPlan,
        index: usize,
        journal: &mut ExecutionJournal,
    ) -> Result<(), &'static str> {
        let planned = &plan.items[index];
        let item = &mut journal.items[index];
        match plan.disposition {
            CleanupDisposition::RecycleBin => {
                let recycled = recycle_exact(self.recycle_bin.as_ref(), &planned.proof.path)
                    .map_err(|_| "recycle-failed")?;
                item.recycle_item = Some(recycled);
                item.state = ItemState::Recycled;
            }
            CleanupDisposition::Quarantine => {
                let destination = item
                    .quarantine_path
                    .clone()
                    .ok_or("quarantine-unavailable")?;
                let directory = destination
                    .parent()
                    .ok_or("quarantine-unavailable")?
                    .to_path_buf();
                if destination.exists() {
                    return Err("quarantine-collision");
                }
                if self
                    .file_system
                    .same_volume(&planned.proof.path, &directory)
                    .map_err(|_| "volume-check-failed")?
                {
                    fs::rename(&planned.proof.path, &destination)
                        .map_err(|_| "quarantine-move-failed")?;
                } else {
                    let staging = directory.join(format!(".{}.staging", item.item_id));
                    self.file_system
                        .copy_tree_no_follow(
                            &planned.proof.path,
                            &staging,
                            MAX_MUTATION_ENTRIES,
                            item.logical_bytes,
                        )
                        .map_err(|_| "quarantine-copy-failed")?;
                    fs::rename(&staging, &destination).map_err(|_| "quarantine-publish-failed")?;
                    let protection = current_protection().map_err(|_| "protection-unavailable")?;
                    revalidate_candidate(
                        &self.file_system,
                        &planned.proof,
                        &protection,
                        SystemTime::now(),
                    )
                    .map_err(|_| "source-revalidation-failed")?;
                    self.file_system
                        .remove_tree_no_follow(&planned.proof.path, MAX_MUTATION_ENTRIES)
                        .map_err(|_| "source-remove-failed")?;
                }
                item.state = ItemState::Quarantined;
            }
            CleanupDisposition::Permanent => {
                let parent = planned.proof.path.parent().ok_or("invalid-parent")?;
                let before = self
                    .file_system
                    .free_space(parent)
                    .map_err(|_| "space-sample-failed")?;
                self.file_system
                    .remove_tree_no_follow(&planned.proof.path, MAX_MUTATION_ENTRIES)
                    .map_err(|_| "permanent-remove-failed")?;
                let after = self
                    .file_system
                    .free_space(parent)
                    .map_err(|_| "space-sample-failed")?;
                item.reclaimed_bytes = after.saturating_sub(before).min(item.occupied_bytes);
                item.state = ItemState::Purged;
            }
        }
        Ok(())
    }

    pub fn undo(&self, execution_id: &str) -> Result<CleanupExecutionSummary, CleanupServiceError> {
        let _writer = self
            .writer
            .lock()
            .map_err(|_| CleanupServiceError::Conflict)?;
        let mut journal = self
            .storage
            .read_execution(execution_id)
            .map_err(map_storage_error)?;
        let plan = self
            .storage
            .read_plan(&journal.plan_id)
            .map_err(map_storage_error)?;
        if matches!(plan.scope, CleanupPlanScope::Storage { .. }) {
            return self.undo_storage(&plan, &mut journal);
        }
        for index in 0..journal.items.len() {
            {
                let item = &mut journal.items[index];
                match item.state {
                    ItemState::Recycled => {
                        let recycled = item
                            .recycle_item
                            .as_ref()
                            .ok_or(CleanupServiceError::ValidationFailed)?;
                        if restore_exact(self.recycle_bin.as_ref(), recycled).is_ok() {
                            item.state = ItemState::Restored;
                        } else {
                            item.failure = Some("restore-failed".to_owned());
                        }
                    }
                    ItemState::Quarantined => {
                        let source = &plan.items[index].proof.path;
                        let quarantined = item
                            .quarantine_path
                            .as_ref()
                            .ok_or(CleanupServiceError::ValidationFailed)?;
                        if source.exists() {
                            item.failure = Some("restore-collision".to_owned());
                        } else if fs::rename(quarantined, source).is_ok() {
                            item.state = ItemState::Restored;
                        } else {
                            let staging = source.with_extension("cleanup-restore-staging");
                            let restored = self
                                .file_system
                                .copy_tree_no_follow(
                                    quarantined,
                                    &staging,
                                    MAX_MUTATION_ENTRIES,
                                    item.logical_bytes,
                                )
                                .and_then(|_| {
                                    fs::rename(&staging, source)
                                        .map_err(cleanup_core::FsError::from)
                                })
                                .and_then(|_| {
                                    self.file_system
                                        .remove_tree_no_follow(quarantined, MAX_MUTATION_ENTRIES)
                                });
                            if restored.is_ok() {
                                item.state = ItemState::Restored;
                            } else {
                                item.failure = Some("restore-failed".to_owned());
                            }
                        }
                    }
                    _ => {}
                }
            }
            persist_accounting(&self.storage, &mut journal)?;
        }
        Ok(summary(&journal, &self.storage))
    }

    pub fn history_page(
        &self,
        request: HistoryRequest,
    ) -> Result<HistoryPage<CleanupExecutionSummary>, CleanupServiceError> {
        let (cursor, limit) = request
            .validate(HistoryKind::Cleanup)
            .map_err(|_| CleanupServiceError::InvalidInput)?;
        let _writer = self
            .writer
            .lock()
            .map_err(|_| CleanupServiceError::Conflict)?;
        let journals = self
            .storage
            .execution_page(
                cursor
                    .as_ref()
                    .map(|cursor| (cursor.timestamp, cursor.id.as_str())),
                limit,
            )
            .map_err(map_storage_error)?;
        // An exact full page may lead to an empty terminal page; do not enumerate twice.
        let next_cursor = if journals.len() == limit {
            journals
                .last()
                .map(|journal| {
                    HistoryCursor::encode(
                        HistoryKind::Cleanup,
                        journal.started_at,
                        &journal.execution_id,
                    )
                })
                .transpose()
                .map_err(|_| CleanupServiceError::PersistenceFailed)?
        } else {
            None
        };
        Ok(HistoryPage {
            records: journals
                .iter()
                .map(|journal| summary(journal, &self.storage))
                .collect(),
            next_cursor,
        })
    }

    pub fn policy(&self) -> Result<AutoCleanupPolicy, CleanupServiceError> {
        self.storage.policy().map_err(map_storage_error)
    }

    /// Effective scan settings, matching what `scan_profile` hands to scans. Stored
    /// content that is corrupt, oversized, or from another schema reads as the default
    /// (`auto`) so the UI can show it and a save can replace the bad file. I/O failures
    /// still surface so a transient error stays retryable rather than masked.
    pub fn scan_settings(&self) -> Result<ScanSettings, CleanupServiceError> {
        match self.storage.scan_settings() {
            Ok(settings) => Ok(settings),
            Err(StorageError::Invalid | StorageError::TooLarge) => Ok(ScanSettings::default()),
            Err(error) => Err(map_storage_error(error)),
        }
    }

    /// Profile used to resolve scan workers. An unreadable settings file falls back to
    /// `auto`, whose unknown-media path is the conservative HDD count.
    pub fn scan_profile(&self) -> ScanProfile {
        self.storage
            .scan_settings()
            .map(|settings| settings.profile)
            .unwrap_or_default()
    }

    pub fn set_scan_profile(
        &self,
        profile: ScanProfile,
    ) -> Result<ScanSettings, CleanupServiceError> {
        let settings = ScanSettings {
            schema_version: SCAN_SETTINGS_SCHEMA_VERSION,
            profile,
        };
        let _writer = self
            .writer
            .lock()
            .map_err(|_| CleanupServiceError::Conflict)?;
        self.storage
            .write_scan_settings(&settings)
            .map_err(map_storage_error)?;
        Ok(settings)
    }

    pub fn set_policy(
        &self,
        enabled: bool,
        grace_days: u16,
    ) -> Result<AutoCleanupPolicy, CleanupServiceError> {
        let policy = AutoCleanupPolicy {
            schema_version: 1,
            enabled,
            grace_days,
        };
        {
            let _writer = self
                .writer
                .lock()
                .map_err(|_| CleanupServiceError::Conflict)?;
            self.storage
                .write_policy(&policy)
                .map_err(map_storage_error)?;
        }
        if enabled {
            self.run_maintenance()?;
        }
        Ok(policy)
    }

    pub fn run_maintenance(&self) -> Result<(), CleanupServiceError> {
        if !self.policy()?.enabled {
            return Ok(());
        }
        let preview = self.preview()?;
        if !preview.records.is_empty() {
            let candidate_ids: Vec<String> = preview
                .records
                .iter()
                .map(|record| record.id.clone())
                .collect();
            let plan = self.create_plan(
                &preview.scan_id,
                &candidate_ids,
                CleanupDisposition::Quarantine,
            )?;
            self.execute(&plan.plan_id)?;
        }
        self.purge_due()
    }

    pub fn purge_due(&self) -> Result<(), CleanupServiceError> {
        let _writer = self
            .writer
            .lock()
            .map_err(|_| CleanupServiceError::Conflict)?;
        let policy = self.storage.policy().map_err(map_storage_error)?;
        if !policy.enabled {
            return Ok(());
        }
        let now = now_seconds()?;
        for record in self.storage.execution_records() {
            let mut journal = record.map_err(map_storage_error)?;
            if journal.disposition != CleanupDisposition::Quarantine
                || journal
                    .purge_after
                    .is_none_or(|purge_after| purge_after > now)
            {
                continue;
            }
            // Manual Storage recovery is never fed to the legacy path-based purge engine.
            let plan = self
                .storage
                .read_plan(&journal.plan_id)
                .map_err(map_storage_error)?;
            if matches!(plan.scope, CleanupPlanScope::Storage { .. }) {
                continue;
            }
            for item in &mut journal.items {
                if item.state != ItemState::Quarantined {
                    continue;
                }
                let Some(path) = item.quarantine_path.as_ref() else {
                    item.state = ItemState::Unknown;
                    continue;
                };
                let parent = path.parent().ok_or(CleanupServiceError::ValidationFailed)?;
                let before = self
                    .file_system
                    .free_space(parent)
                    .map_err(|_| CleanupServiceError::OperationFailed)?;
                if self
                    .file_system
                    .remove_tree_no_follow(path, MAX_MUTATION_ENTRIES)
                    .is_ok()
                {
                    let after = self
                        .file_system
                        .free_space(parent)
                        .map_err(|_| CleanupServiceError::OperationFailed)?;
                    item.reclaimed_bytes = after.saturating_sub(before).min(item.occupied_bytes);
                    item.state = ItemState::Purged;
                } else {
                    item.failure = Some("purge-failed".to_owned());
                }
            }
            persist_accounting(&self.storage, &mut journal)?;
        }
        Ok(())
    }

    /// Shared creation/execution/recovery gate. Unimplemented evidence resolvers deny authority.
    fn validate_current_scope(&self, plan: &CleanupPlan) -> Result<(), CleanupServiceError> {
        self.validate_scope(plan, false)
    }

    /// `tolerate_item_access`: at execution only, a non-duplicate item that is missing, in use
    /// or access-denied does not refuse the whole plan. The per-item guard reopens it and
    /// records that reason, like Kudu; every other check stays plan-wide and fails closed.
    fn validate_scope(
        &self,
        plan: &CleanupPlan,
        tolerate_item_access: bool,
    ) -> Result<(), CleanupServiceError> {
        plan.validate().map_err(map_storage_error)?;
        if matches!(plan.scope, CleanupPlanScope::Storage { .. }) {
            let protection =
                current_protection().map_err(|_| CleanupServiceError::ValidationFailed)?;
            let mut group_keepers = std::collections::HashMap::new();
            for item in &plan.items {
                if let CandidateProofScope::Storage { evidence } = &item.proof.scope
                    && let cleanup_core::storage::StorageEvidence::DuplicateMember {
                        keeper, ..
                    } = evidence.as_ref()
                    && (group_keepers
                        .insert(&keeper.group_id, keeper)
                        .is_some_and(|previous| previous != keeper)
                        || plan
                            .items
                            .iter()
                            .any(|i| i.proof.identity == keeper.keeper.identity))
                {
                    return Err(CleanupServiceError::ValidationFailed);
                }
                let CandidateProofScope::Storage { evidence } = &item.proof.scope else {
                    return Err(CleanupServiceError::ValidationFailed);
                };
                match self.storage_entry_guard(evidence, &protection) {
                    Ok(_) => {}
                    Err("not-found" | "in-use" | "permission-denied")
                        if tolerate_item_access
                            && !matches!(
                                evidence.as_ref(),
                                cleanup_core::storage::StorageEvidence::DuplicateMember { .. }
                            ) => {}
                    Err(_) => return Err(CleanupServiceError::ValidationFailed),
                }
            }
            return Ok(());
        }
        let rule = temporary_rule().map_err(|_| CleanupServiceError::ValidationFailed)?;
        let root = temporary_root().map_err(|_| CleanupServiceError::ValidationFailed)?;
        if plan_matches_current_scope(&self.file_system, plan, &rule, &root) {
            Ok(())
        } else {
            Err(CleanupServiceError::ValidationFailed)
        }
    }

    /// Revalidate one storage entry and bind its identity guard. The error is the per-item
    /// failure reason: `not-found`, `in-use` or `permission-denied` when the entry itself
    /// cannot be opened, otherwise `storage-revalidation-rejected`.
    fn storage_entry_guard(
        &self,
        evidence: &cleanup_core::storage::StorageEvidence,
        protection: &ProtectionPolicy,
    ) -> Result<super::filesystem::IdentityGuard, &'static str> {
        use cleanup_core::storage::StorageEvidence;
        const REJECTED: &str = "storage-revalidation-rejected";
        evidence.validate().map_err(|_| REJECTED)?;
        let scope_valid = match evidence {
            StorageEvidence::UserSelectedFile { root, entry } => {
                crate::storage::protection::personal_path_allowed(&root.canonical_path)
                    && crate::storage::protection::personal_path_allowed(&entry.canonical_path)
            }
            StorageEvidence::DuplicateMember {
                root,
                entry,
                keeper,
            } => {
                crate::storage::protection::personal_path_allowed(&root.canonical_path)
                    && crate::storage::protection::personal_path_allowed(&entry.canonical_path)
                    && crate::storage::protection::personal_path_allowed(
                        &keeper.keeper_root.canonical_path,
                    )
                    && crate::storage::protection::personal_path_allowed(
                        &keeper.keeper.canonical_path,
                    )
                    && !protection.is_protected(&keeper.keeper.canonical_path)
                    && !protection.is_repository_metadata(&keeper.keeper.canonical_path)
                    && self
                        .file_system
                        .guard_entry(&keeper.keeper_root, &keeper.keeper, true)
                        .is_ok()
            }
            StorageEvidence::EmptyFolder { .. } => {
                crate::storage::empty_folders::validate_current(evidence)
            }
            StorageEvidence::CatalogTarget { .. } => {
                crate::storage::cleaner::validate_current(evidence, protection).is_ok()
            }
            StorageEvidence::BrowserCache { .. } => {
                crate::storage::browser::validate_current(evidence).is_ok()
            }
            _ => false,
        };
        if !scope_valid
            || protection.is_protected(&evidence.entry().canonical_path)
            || protection.is_repository_metadata(&evidence.entry().canonical_path)
        {
            return Err(REJECTED);
        }
        self.file_system
            .guard_entry(evidence.root(), evidence.entry(), false)
            .map_err(|error| match error.kind {
                cleanup_core::FsErrorKind::NotFound => "not-found",
                cleanup_core::FsErrorKind::InUse => "in-use",
                cleanup_core::FsErrorKind::PermissionDenied => "permission-denied",
                _ => REJECTED,
            })
    }

    fn storage_recovery_guard(
        &self,
        plan: &CleanupPlan,
        journal: &ExecutionJournal,
        index: usize,
    ) -> Result<super::filesystem::IdentityGuard, CleanupServiceError> {
        let item = &journal.items[index];
        let planned = plan
            .items
            .iter()
            .find(|p| p.item_id == item.item_id)
            .ok_or(CleanupServiceError::ValidationFailed)?;
        let CandidateProofScope::Storage { evidence } = &planned.proof.scope else {
            return Err(CleanupServiceError::ValidationFailed);
        };
        let expected = self
            .storage
            .expected_quarantine_path(&journal.execution_id, &item.item_id)
            .map_err(map_storage_error)?;
        if plan.disposition != CleanupDisposition::Quarantine
            || journal.disposition != plan.disposition
            || item.quarantine_path.as_ref() != Some(&expected)
        {
            return Err(CleanupServiceError::ValidationFailed);
        }
        self.file_system
            .guard_relocated_file(&expected, evidence.entry())
            .map_err(|_| CleanupServiceError::ValidationFailed)
    }

    fn undo_storage(
        &self,
        plan: &CleanupPlan,
        journal: &mut ExecutionJournal,
    ) -> Result<CleanupExecutionSummary, CleanupServiceError> {
        for index in 0..journal.items.len() {
            if journal.items[index].state != ItemState::Quarantined {
                continue;
            }
            let planned = plan
                .items
                .iter()
                .find(|p| p.item_id == journal.items[index].item_id)
                .ok_or(CleanupServiceError::ValidationFailed)?;
            let CandidateProofScope::Storage { evidence } = &planned.proof.scope else {
                return Err(CleanupServiceError::ValidationFailed);
            };
            let protection =
                current_protection().map_err(|_| CleanupServiceError::ValidationFailed)?;
            let root = &evidence.root().canonical_path;
            let original = &evidence.entry().canonical_path;
            if protection.is_protected(original)
                || protection.is_repository_metadata(original)
                || protection.is_protected(root)
                || protection.is_repository_metadata(root)
                || (matches!(
                    evidence.as_ref(),
                    cleanup_core::storage::StorageEvidence::BrowserCache { .. }
                ) && crate::storage::browser::validate_current(evidence).is_err())
                || (matches!(
                    evidence.as_ref(),
                    cleanup_core::storage::StorageEvidence::UserSelectedFile { .. }
                        | cleanup_core::storage::StorageEvidence::DuplicateMember { .. }
                ) && (!crate::storage::protection::personal_path_allowed(root)
                    || !crate::storage::protection::personal_path_allowed(original)))
            {
                journal.items[index].failure = Some("restore-protection-rejected".into());
                persist_accounting(&self.storage, journal)?;
                continue;
            }
            let guard = match self.storage_recovery_guard(plan, journal, index) {
                Ok(guard) => guard,
                Err(_) => {
                    journal.items[index].state = ItemState::Unknown;
                    journal.items[index].failure = Some("recovery-identity-unproven".into());
                    persist_accounting(&self.storage, journal)?;
                    continue;
                }
            };
            // Write intent before rename. Restart only proves recovery identity, never absence.
            journal.completed_at = None;
            journal.items[index].state = ItemState::Mutating;
            persist_accounting(&self.storage, journal)?;
            if guard.rename_to_original(evidence.root(), original).is_ok() {
                journal.items[index].state = ItemState::Restored;
                journal.items[index].failure = None;
            } else {
                journal.items[index].state = ItemState::Quarantined;
                journal.items[index].failure = Some("restore-rejected".into());
            }
            persist_accounting(&self.storage, journal)?;
        }
        journal.completed_at = Some(now_seconds()?);
        persist_accounting(&self.storage, journal)?;
        Ok(summary(journal, &self.storage))
    }

    fn reconcile_interrupted(&self) -> Result<(), CleanupServiceError> {
        for record in self.storage.execution_records() {
            let mut journal = record.map_err(map_storage_error)?;
            if journal.completed_at.is_some() {
                continue;
            }
            let plan = self
                .storage
                .read_plan(&journal.plan_id)
                .map_err(map_storage_error)?;
            if matches!(plan.scope, CleanupPlanScope::Storage { .. }) {
                let valid = self.validate_current_scope(&plan).is_ok();
                let recoverable: Vec<bool> = (0..journal.items.len())
                    .map(|index| self.storage_recovery_guard(&plan, &journal, index).is_ok())
                    .collect();
                for (index, item) in journal.items.iter_mut().enumerate() {
                    if matches!(item.state, ItemState::Pending | ItemState::Mutating) {
                        if plan.disposition == CleanupDisposition::Quarantine {
                            if let Some(planned) =
                                plan.items.iter().find(|p| p.item_id == item.item_id)
                            {
                                item.occupied_bytes =
                                    item.occupied_bytes.max(planned.proof.allocated_bytes);
                            }
                            if recoverable[index] {
                                item.state = ItemState::Quarantined;
                                item.processed = true;
                                item.failure = None;
                                continue;
                            }
                        }
                        item.state = ItemState::Unknown;
                        item.failure = Some(
                            if valid {
                                "interrupted-outcome-unknown"
                            } else {
                                "interrupted-scope-stale"
                            }
                            .into(),
                        );
                    }
                }
                journal.completed_at = Some(now_seconds()?);
                persist_accounting(&self.storage, &mut journal)?;
                continue;
            }
            self.storage
                .reconcile(&plan, &mut journal)
                .map_err(map_storage_error)?;
        }
        Ok(())
    }
}

pub(super) fn execute_registered_artifact_plan(
    storage: &CleanupStorage,
    file_system: &WindowsFileSystem,
    plan: &CleanupPlan,
    quarantine_grace_seconds: u64,
) -> Result<RegisteredArtifactExecutionOutcome, CleanupServiceError> {
    plan.validate().map_err(map_storage_error)?;
    let (root_id, profile_id) = match &plan.scope {
        CleanupPlanScope::BuildArtifact {
            root_id,
            profile_id,
        } => (root_id, profile_id),
        CleanupPlanScope::Temporary | CleanupPlanScope::Storage { .. } => {
            return Err(CleanupServiceError::ValidationFailed);
        }
    };
    let root = storage
        .project_roots()
        .map_err(map_storage_error)?
        .into_iter()
        .find(|root| root.id == *root_id)
        .ok_or(CleanupServiceError::ValidationFailed)?;
    let profile = storage
        .build_profiles()
        .map_err(map_storage_error)?
        .into_iter()
        .find(|profile| profile.profile_id == *profile_id && profile.root_id == *root_id)
        .ok_or(CleanupServiceError::ValidationFailed)?;
    let ledger = storage.artifact_generations().map_err(map_storage_error)?;
    for item in &plan.items {
        let generation_id = match &item.proof.scope {
            CandidateProofScope::RegisteredBuildArtifact {
                root_id: proof_root,
                profile_id: proof_profile,
                generation_id,
            } if proof_root == root_id && proof_profile == profile_id => generation_id,
            _ => return Err(CleanupServiceError::ValidationFailed),
        };
        let generation = ledger
            .generations
            .iter()
            .find(|generation| {
                generation.generation_id == *generation_id
                    && generation.root_id == *root_id
                    && generation.profile_id == *profile_id
                    && generation.owned
                    && generation.role == ArtifactRole::Generation
                    && generation.identity == Some(item.proof.identity)
            })
            .ok_or(CleanupServiceError::ValidationFailed)?;
        let registered = profile.artifact_paths.iter().any(|artifact| {
            artifact.role == ArtifactRole::Generation
                && artifact
                    .relative_path
                    .replace('\\', "/")
                    .eq_ignore_ascii_case(&generation.normalized_path)
        });
        let expected = Path::new(&root.display_path).join(&generation.normalized_path);
        if !registered
            || !file_system
                .semantics()
                .equivalent(&expected, &item.proof.path)
            || !file_system
                .semantics()
                .equivalent(Path::new(&root.display_path), &item.proof.context_root)
        {
            return Err(CleanupServiceError::ValidationFailed);
        }
    }
    storage.create_plan(plan).map_err(map_storage_error)?;
    let execution_id = random_id()?;
    let started_at = now_seconds()?;
    let mut journal = ExecutionJournal {
        schema_version: 1,
        execution_id,
        plan_id: plan.plan_id.clone(),
        started_at,
        completed_at: None,
        disposition: CleanupDisposition::Quarantine,
        purge_after: Some(started_at.saturating_add(quarantine_grace_seconds)),
        items: plan
            .items
            .iter()
            .map(|item| ExecutionItem {
                item_id: item.item_id.clone(),
                state: ItemState::Pending,
                logical_bytes: item.proof.logical_bytes,
                processed: false,
                occupied_bytes: 0,
                reclaimed_bytes: 0,
                quarantine_path: None,
                recycle_item: None,
                failure: None,
            })
            .collect(),
        accounting: ByteAccounting {
            selected_bytes: checked_sum(plan.items.iter().map(|item| item.proof.logical_bytes))?,
            ..ByteAccounting::default()
        },
    };
    storage
        .write_execution(&journal)
        .map_err(map_storage_error)?;
    let mut quarantined = Vec::new();
    for index in 0..journal.items.len() {
        journal.items[index].state = ItemState::Mutating;
        journal.items[index].quarantine_path = Some(
            storage
                .quarantine_directory(&journal.execution_id, &journal.items[index].item_id)
                .map_err(map_storage_error)?,
        );
        storage
            .write_execution(&journal)
            .map_err(map_storage_error)?;
        let protection = match current_protection() {
            Ok(value) => value,
            Err(_) => {
                fail_item(&mut journal.items[index], "protection-unavailable");
                persist_accounting(storage, &mut journal)?;
                continue;
            }
        };
        let measured = match revalidate_candidate(
            file_system,
            &plan.items[index].proof,
            &protection,
            SystemTime::now(),
        ) {
            Ok(value) => value,
            Err(_) => {
                fail_item(&mut journal.items[index], "revalidation-rejected");
                persist_accounting(storage, &mut journal)?;
                continue;
            }
        };
        journal.items[index].processed = true;
        journal.items[index].logical_bytes = measured.logical_bytes;
        journal.items[index].occupied_bytes = measured.allocated_bytes;
        if let Err(reason) =
            quarantine_registered_item(storage, file_system, plan, index, &mut journal, &protection)
        {
            fail_item(&mut journal.items[index], reason);
        } else if let CandidateProofScope::RegisteredBuildArtifact { generation_id, .. } =
            &plan.items[index].proof.scope
        {
            quarantined.push(generation_id.clone());
        }
        persist_accounting(storage, &mut journal)?;
    }
    journal.completed_at = Some(now_seconds()?);
    persist_accounting(storage, &mut journal)?;
    let failed_items = journal
        .items
        .iter()
        .filter(|item| item.state == ItemState::Failed)
        .map(item_outcome)
        .collect();
    Ok(RegisteredArtifactExecutionOutcome {
        quarantined_generation_ids: quarantined,
        failed_items,
    })
}

fn quarantine_registered_item(
    _storage: &CleanupStorage,
    file_system: &WindowsFileSystem,
    plan: &CleanupPlan,
    index: usize,
    journal: &mut ExecutionJournal,
    protection: &ProtectionPolicy,
) -> Result<(), &'static str> {
    let planned = &plan.items[index];
    let item = &mut journal.items[index];
    let destination = item
        .quarantine_path
        .clone()
        .ok_or("quarantine-unavailable")?;
    let directory = destination
        .parent()
        .ok_or("quarantine-unavailable")?
        .to_path_buf();
    if destination.exists() {
        return Err("quarantine-collision");
    }
    revalidate_candidate(file_system, &planned.proof, protection, SystemTime::now())
        .map_err(|_| "source-revalidation-failed")?;
    if file_system
        .same_volume(&planned.proof.path, &directory)
        .map_err(|_| "volume-check-failed")?
    {
        fs::rename(&planned.proof.path, &destination).map_err(|_| "quarantine-move-failed")?;
    } else {
        let staging = directory.join(format!(".{}.staging", item.item_id));
        file_system
            .copy_tree_no_follow(
                &planned.proof.path,
                &staging,
                MAX_MUTATION_ENTRIES,
                item.logical_bytes,
            )
            .map_err(|_| "quarantine-copy-failed")?;
        fs::rename(&staging, &destination).map_err(|_| "quarantine-publish-failed")?;
        revalidate_candidate(file_system, &planned.proof, protection, SystemTime::now())
            .map_err(|_| "source-revalidation-failed")?;
        file_system
            .remove_tree_no_follow(&planned.proof.path, MAX_MUTATION_ENTRIES)
            .map_err(|_| "source-remove-failed")?;
    }
    item.state = ItemState::Quarantined;
    Ok(())
}

fn fail_item(item: &mut ExecutionItem, reason: &str) {
    item.state = ItemState::Failed;
    item.failure = Some(reason.to_owned());
}

fn persist_accounting(
    storage: &CleanupStorage,
    journal: &mut ExecutionJournal,
) -> Result<(), CleanupServiceError> {
    let selected_bytes = journal.accounting.selected_bytes;
    journal.accounting = ByteAccounting {
        selected_bytes,
        processed_bytes: checked_sum(
            journal
                .items
                .iter()
                .filter(|item| item.processed)
                .map(|item| item.logical_bytes),
        )?,
        failed_bytes: checked_sum(
            journal
                .items
                .iter()
                .filter(|item| matches!(item.state, ItemState::Failed | ItemState::Unknown))
                .map(|item| item.logical_bytes),
        )?,
        quarantined_bytes: checked_sum(
            journal
                .items
                .iter()
                .filter(|item| item.state == ItemState::Quarantined)
                .map(|item| item.occupied_bytes),
        )?,
        purged_bytes: checked_sum(
            journal
                .items
                .iter()
                .filter(|item| item.state == ItemState::Purged)
                .map(|item| item.occupied_bytes),
        )?,
        occupied_bytes: checked_sum(
            journal
                .items
                .iter()
                .filter(|item| {
                    item.processed
                        || (item.state == ItemState::Unknown && item.quarantine_path.is_some())
                })
                .map(|item| item.occupied_bytes),
        )?,
        reclaimed_bytes: checked_sum(journal.items.iter().map(|item| item.reclaimed_bytes))?,
    };
    storage.write_execution(journal).map_err(map_storage_error)
}

fn checked_sum(mut values: impl Iterator<Item = u64>) -> Result<u64, CleanupServiceError> {
    values.try_fold(0_u64, |total, value| {
        total
            .checked_add(value)
            .ok_or(CleanupServiceError::ValidationFailed)
    })
}

fn item_outcome(item: &ExecutionItem) -> CleanupItemOutcome {
    CleanupItemOutcome {
        item_id: item.item_id.clone(),
        display_path: None,
        state: item.state,
        logical_bytes: item.logical_bytes,
        failure: item.failure.clone(),
    }
}

fn outcome_display_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    let mut chars = text.chars();
    let mut display: String = chars.by_ref().take(1024).map(|c| {
        if c.is_control() || matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}') { '\u{fffd}' } else { c }
    }).collect();
    if chars.next().is_some() {
        display.push('…');
    }
    display
}

fn summary(journal: &ExecutionJournal, storage: &CleanupStorage) -> CleanupExecutionSummary {
    // Plans already persist independently of scans. Missing legacy/corrupt metadata must
    // not hide journal outcomes or grant any filesystem authority to the frontend.
    let plan = storage.read_plan(&journal.plan_id).ok();
    let paths: HashMap<_, _> = plan
        .as_ref()
        .map(|plan| {
            plan.items
                .iter()
                .map(|item| (item.item_id.as_str(), &item.proof.path))
                .collect()
        })
        .unwrap_or_default();
    CleanupExecutionSummary {
        execution_id: journal.execution_id.clone(),
        plan_id: journal.plan_id.clone(),
        disposition: journal.disposition,
        completed: journal.completed_at.is_some(),
        purge_after: journal.purge_after,
        items: journal
            .items
            .iter()
            .map(|item| {
                let mut outcome = item_outcome(item);
                outcome.display_path = paths
                    .get(item.item_id.as_str())
                    .map(|path| outcome_display_path(path));
                outcome
            })
            .collect(),
        accounting: journal.accounting.clone(),
    }
}

fn sort_project_roots(roots: &mut [ProjectRoot], semantics: cleanup_core::PathSemantics) {
    roots.sort_by(|left, right| {
        semantics
            .key(Path::new(&left.display_path))
            .cmp(&semantics.key(Path::new(&right.display_path)))
            .then_with(|| left.id.cmp(&right.id))
    });
}

fn collapse_project_roots(
    roots: Vec<ProjectRoot>,
    semantics: cleanup_core::PathSemantics,
) -> Vec<ProjectRoot> {
    let mut by_depth = roots;
    by_depth.sort_by(|left, right| {
        Path::new(&left.display_path)
            .components()
            .count()
            .cmp(&Path::new(&right.display_path).components().count())
            .then_with(|| {
                semantics
                    .key(Path::new(&left.display_path))
                    .cmp(&semantics.key(Path::new(&right.display_path)))
            })
    });
    let mut retained: Vec<ProjectRoot> = Vec::new();
    for root in by_depth {
        if !retained.iter().any(|parent| {
            semantics.contains(
                Path::new(&parent.display_path),
                Path::new(&root.display_path),
            )
        }) {
            retained.push(root);
        }
    }
    retained
}

fn divided_project_limits(divisor: usize) -> ScanLimits {
    ScanLimits {
        max_workers: PROJECT_DISCOVERY_LIMITS.max_workers,
        max_visited_entries: (PROJECT_DISCOVERY_LIMITS.max_visited_entries / divisor).max(1),
        max_candidates: (PROJECT_DISCOVERY_LIMITS.max_candidates / divisor).max(1),
        max_diagnostics: (PROJECT_DISCOVERY_LIMITS.max_diagnostics / divisor).max(1),
        max_measurement_entries: (PROJECT_DISCOVERY_LIMITS.max_measurement_entries / divisor)
            .max(1),
    }
}

/// Per-root discovery limits: the divided budgets with the saved profile's worker count.
/// `seek_penalty` is only consulted for `ScanProfile::Auto` (see `workers_for_path`).
fn project_root_limits(
    base: ScanLimits,
    profile: ScanProfile,
    seek_penalty: Option<bool>,
) -> ScanLimits {
    ScanLimits {
        max_workers: resolve_workers(profile, seek_penalty),
        ..base
    }
}

struct ServiceEntropy;

impl cleanup_core::Entropy for ServiceEntropy {
    fn fill(&self, bytes: &mut [u8]) -> Result<(), cleanup_core::FsError> {
        getrandom::fill(bytes).map_err(|_| {
            cleanup_core::FsError::new(
                cleanup_core::FsErrorKind::Other,
                "system entropy unavailable",
            )
        })
    }
}

fn valid_id(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn plan_matches_current_scope(
    file_system: &dyn FileSystem,
    plan: &CleanupPlan,
    current_rule: &cleanup_core::CleanupRule,
    expected_root: &Path,
) -> bool {
    if !matches!(plan.scope, CleanupPlanScope::Temporary) {
        return false;
    }
    let semantics = file_system.semantics();
    plan.items.iter().all(|item| {
        matches!(item.proof.scope, CandidateProofScope::Temporary)
            && item.proof.rule == *current_rule
            && semantics.equivalent(&item.proof.scan_root, expected_root)
            && semantics.equivalent(&item.proof.context_root, expected_root)
    })
}

fn random_id() -> Result<String, CleanupServiceError> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| CleanupServiceError::OperationFailed)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn now_seconds() -> Result<u64, CleanupServiceError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| CleanupServiceError::OperationFailed)
}

fn map_storage_error(error: StorageError) -> CleanupServiceError {
    match error {
        StorageError::Invalid | StorageError::TooLarge => CleanupServiceError::ValidationFailed,
        StorageError::Exists => CleanupServiceError::Conflict,
        StorageError::Io => CleanupServiceError::PersistenceFailed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cleanup::storage::{
        ArtifactGenerationLedger, BuildEcosystem, RegisteredArtifactPath,
    };
    use cleanup_core::{
        ArtifactRole, CandidateProofScope, GenerationState, Lifecycle, Markers, Provenance,
        RebuildCost, Risk, RuleRoot, ScannerKind, TargetType, snapshot_registered_path,
    };

    #[test]
    fn outcome_display_is_bounded_and_removes_control_and_direction_overrides() {
        let text = format!("C:\\fixture\\\n\u{202e}{}", "a".repeat(2000));
        let display = outcome_display_path(Path::new(&text));
        assert_eq!(display.chars().count(), 1025);
        assert!(display.ends_with('…'));
        assert!(!display.contains('\n'));
        assert!(!display.contains('\u{202e}'));
        assert!(display.contains('\u{fffd}'));
    }

    fn storage_file_fixture() -> (PathBuf, PathBuf, CleanupStorage, CleanupPlan, PathBuf) {
        use cleanup_core::storage::{
            ObservedEntry, RootAuthorization, StorageEvidence, StorageModule,
        };
        let (fixture, app_data, storage, mut plan, _) = registered_execution_fixture(&["artifact"]);
        let path = plan.items[0].proof.path.join("payload.bin");
        let fs = WindowsFileSystem;
        let meta = fs.metadata_no_follow(&path).unwrap();
        let root_path = plan.items[0].proof.context_root.clone();
        let root = RootAuthorization {
            snapshot_id: plan.scan_id.clone(),
            root_id: "d".repeat(32),
            canonical_path: root_path.clone(),
            identity: fs.metadata_no_follow(&root_path).unwrap().identity.unwrap(),
        };
        let entry = ObservedEntry {
            canonical_path: path.clone(),
            identity: meta.identity.unwrap(),
            kind: meta.kind,
            logical_bytes: meta.size,
            allocated_bytes: Some(fs.allocated_size(&path, &meta).unwrap()),
            modified_unix_nanos: meta
                .modified
                .unwrap()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64,
        };
        plan.schema_version = 2;
        plan.disposition = CleanupDisposition::Permanent;
        plan.scope = CleanupPlanScope::Storage {
            module: StorageModule::LargeFiles,
            root: root.clone(),
        };
        let proof = &mut plan.items[0].proof;
        proof.path = path.clone();
        proof.scan_root = root_path;
        proof.identity = entry.identity;
        proof.kind = entry.kind;
        proof.logical_bytes = entry.logical_bytes;
        proof.allocated_bytes = entry.allocated_bytes.unwrap();
        proof.scope = CandidateProofScope::Storage {
            evidence: Box::new(StorageEvidence::UserSelectedFile { root, entry }),
        };
        storage.create_plan(&plan).unwrap();
        (fixture, app_data, storage, plan, path)
    }

    /// Adds a second user-selected file beside the fixture file, as its own plan item.
    fn add_storage_file_item(plan: &mut CleanupPlan, name: &str) -> PathBuf {
        use cleanup_core::storage::StorageEvidence;
        let mut item = plan.items[0].clone();
        let path = item.proof.path.with_file_name(name);
        fs::write(&path, b"second item").unwrap();
        let fs_native = WindowsFileSystem;
        let meta = fs_native.metadata_no_follow(&path).unwrap();
        let CandidateProofScope::Storage { evidence } = &item.proof.scope else {
            unreachable!()
        };
        let StorageEvidence::UserSelectedFile { root, entry } = evidence.as_ref().clone() else {
            unreachable!()
        };
        let entry = cleanup_core::storage::ObservedEntry {
            canonical_path: path.clone(),
            identity: meta.identity.unwrap(),
            logical_bytes: meta.size,
            allocated_bytes: Some(fs_native.allocated_size(&path, &meta).unwrap()),
            modified_unix_nanos: meta
                .modified
                .unwrap()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64,
            ..entry
        };
        item.item_id = "e".repeat(32);
        item.proof.path = path.clone();
        item.proof.identity = entry.identity;
        item.proof.logical_bytes = entry.logical_bytes;
        item.proof.allocated_bytes = entry.allocated_bytes.unwrap();
        item.proof.scope = CandidateProofScope::Storage {
            evidence: Box::new(StorageEvidence::UserSelectedFile { root, entry }),
        };
        plan.items.push(item);
        path
    }

    // Kudu reports a locked, missing or access-denied file per item and still cleans the
    // rest. The execution gate must not refuse the whole plan for one such item.
    #[test]
    fn storage_execution_reports_in_use_and_not_found_per_item_and_cleans_the_rest() {
        use std::os::windows::fs::OpenOptionsExt;
        let cases = [
            CleanupDisposition::Quarantine,
            CleanupDisposition::Permanent,
        ]
        .into_iter()
        .flat_map(|disposition| [(disposition, false), (disposition, true)]);
        for (case, (disposition, missing)) in cases.enumerate() {
            {
                let (fixture, app_data, storage, mut plan, blocked) = storage_file_fixture();
                let other = add_storage_file_item(&mut plan, "other.bin");
                plan.plan_id = format!("{:032x}", 0x500 + case);
                plan.disposition = disposition;
                storage.create_plan(&plan).unwrap();
                let bytes = fs::read(&blocked).unwrap();
                // No sharing: every later open of the file fails with a sharing violation.
                let lock = (!missing).then(|| {
                    fs::OpenOptions::new()
                        .read(true)
                        .share_mode(0)
                        .open(&blocked)
                        .unwrap()
                });
                if missing {
                    fs::remove_file(&blocked).unwrap();
                }
                let service = CleanupService::new(app_data).unwrap();
                let result = match disposition {
                    CleanupDisposition::Permanent => service.execute_permanent(&plan.plan_id),
                    _ => service.execute(&plan.plan_id),
                }
                .unwrap();
                drop(lock);
                let blocked_outcome = &result.items[0];
                assert_eq!(blocked_outcome.state, ItemState::Failed);
                assert_eq!(
                    blocked_outcome.failure.as_deref(),
                    Some(if missing { "not-found" } else { "in-use" })
                );
                if !missing {
                    assert_eq!(fs::read(&blocked).unwrap(), bytes, "locked file untouched");
                }
                assert_eq!(
                    result.items[1].state,
                    if disposition == CleanupDisposition::Quarantine {
                        ItemState::Quarantined
                    } else {
                        ItemState::Purged
                    }
                );
                assert!(!other.exists());
                drop(service);
                fs::remove_dir_all(fixture).unwrap();
            }
        }
    }

    #[test]
    fn older_page_undo_restores_real_quarantine_bytes_after_restart() {
        let (fixture, app_data, storage, _, _) = storage_file_fixture();
        let mut fixtures = Vec::new();
        let mut artifacts = Vec::new();
        let service = CleanupService::new(app_data.clone()).unwrap();
        let mut ids = Vec::new();
        let mut payloads = Vec::new();
        for index in 0..21 {
            let (source_fixture, _, _, mut plan, payload) = storage_file_fixture();
            fixtures.push(source_fixture);
            artifacts.push(payload.clone());
            plan.plan_id = format!("{:032x}", index + 1000);
            plan.disposition = CleanupDisposition::Quarantine;
            storage.create_plan(&plan).unwrap();
            payloads.push(fs::read(&payload).unwrap());
            let result = service.execute(&plan.plan_id).unwrap();
            assert_eq!(result.items[0].state, ItemState::Quarantined);
            assert!(!payload.exists());
            let mut journal = storage.read_execution(&result.execution_id).unwrap();
            let recovery = journal.items[0].quarantine_path.as_ref().unwrap();
            assert_eq!(fs::read(recovery).unwrap(), payloads[index]);
            // Deterministic immutable ordering, independent of the fixture's wall-clock speed.
            journal.started_at = index as u64 + 1;
            storage.write_execution(&journal).unwrap();
            ids.push(result.execution_id);
        }
        let first = service.history_page(HistoryRequest::default()).unwrap();
        assert_eq!(first.records.len(), 20);
        assert_eq!(
            first
                .records
                .iter()
                .map(|row| &row.execution_id)
                .collect::<Vec<_>>(),
            ids[1..].iter().rev().collect::<Vec<_>>()
        );
        assert!(
            first
                .records
                .iter()
                .all(|row| row.items[0].state == ItemState::Quarantined)
        );
        drop(service);
        let service = CleanupService::new(app_data).unwrap();
        let older = service
            .history_page(HistoryRequest {
                cursor: first.next_cursor,
                limit: None,
            })
            .unwrap();
        assert_eq!(older.records.len(), 1);
        assert!(older.next_cursor.is_none());
        let oldest_id = &older.records[0].execution_id;
        assert_eq!(oldest_id, &ids[0]);
        assert_eq!(older.records[0].items[0].state, ItemState::Quarantined);
        let restored = service.undo(oldest_id).unwrap();
        assert_eq!(&restored.execution_id, oldest_id);
        assert_eq!(restored.items[0].state, ItemState::Restored);
        assert_eq!(fs::read(&artifacts[0]).unwrap(), payloads[0]);
        for (index, id) in ids.iter().enumerate().skip(1) {
            assert!(!artifacts[index].exists());
            let journal = storage.read_execution(id).unwrap();
            assert_eq!(journal.items[0].state, ItemState::Quarantined);
            assert_eq!(
                fs::read(journal.items[0].quarantine_path.as_ref().unwrap()).unwrap(),
                payloads[index]
            );
        }
        drop(service);
        fs::remove_dir_all(fixture).unwrap();
        for source_fixture in fixtures {
            fs::remove_dir_all(source_fixture).unwrap();
        }
    }

    #[test]
    fn retained_history_regression_reaches_more_than_one_hundred_rows() {
        let (fixture, app_data, storage, _, _) = storage_file_fixture();
        let mut journal: ExecutionJournal =
            serde_json::from_str(include_str!("fixtures/journal-v1-mutating.json")).unwrap();
        journal.completed_at = Some(1);
        for index in 0..121 {
            journal.execution_id = format!("{index:032x}");
            journal.started_at = index / 3;
            storage.write_execution(&journal).unwrap();
        }
        let service = CleanupService::new(app_data).unwrap();
        let mut cursor = None;
        let mut seen = Vec::new();
        loop {
            let page = service
                .history_page(HistoryRequest {
                    cursor,
                    limit: None,
                })
                .unwrap();
            assert!(page.records.len() <= 20);
            seen.extend(page.records.into_iter().map(|record| record.execution_id));
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
        let expected: Vec<_> = (0..121)
            .rev()
            .map(|index| format!("{index:032x}"))
            .collect();
        assert_eq!(seen, expected);
        drop(service);
        fs::remove_dir_all(fixture).unwrap();
    }

    #[test]
    fn history_page_validates_boundaries_and_reports_invalid_journals() {
        let (fixture, app_data, storage, _, _) = storage_file_fixture();
        let service = CleanupService::new(app_data.clone()).unwrap();
        assert!(
            service
                .history_page(HistoryRequest::default())
                .unwrap()
                .records
                .is_empty()
        );
        for request in [
            HistoryRequest {
                cursor: None,
                limit: Some(0),
            },
            HistoryRequest {
                cursor: None,
                limit: Some(101),
            },
            HistoryRequest {
                cursor: Some(" ".repeat(257)),
                limit: None,
            },
            HistoryRequest {
                cursor: Some(
                    HistoryCursor::encode(HistoryKind::Vendor, 0, &"a".repeat(32)).unwrap(),
                ),
                limit: None,
            },
        ] {
            assert_eq!(
                service.history_page(request).unwrap_err(),
                CleanupServiceError::InvalidInput
            );
        }
        for id in ["a", "../invalid", "0123456789abcdef0123456789abcdeg"] {
            let cursor =
                serde_json::json!({"version": 1, "kind": "cleanup", "timestamp": 0, "id": id})
                    .to_string();
            assert_eq!(
                service
                    .history_page(HistoryRequest {
                        cursor: Some(cursor),
                        limit: None
                    })
                    .unwrap_err(),
                CleanupServiceError::InvalidInput
            );
        }
        let mut journal: ExecutionJournal =
            serde_json::from_str(include_str!("fixtures/journal-v1-mutating.json")).unwrap();
        journal.completed_at = Some(1);
        for index in 0..4 {
            journal.execution_id = format!("{index:032x}");
            journal.started_at = 1;
            storage.write_execution(&journal).unwrap();
        }
        let first = service
            .history_page(HistoryRequest {
                cursor: None,
                limit: Some(2),
            })
            .unwrap();
        assert_eq!(first.records[0].execution_id, format!("{:032x}", 3));
        // Changing an outcome and inserting a newer key must not shift the older boundary.
        journal.completed_at = Some(2);
        storage.write_execution(&journal).unwrap();
        journal.execution_id = format!("{:032x}", 4);
        journal.started_at = 2;
        storage.write_execution(&journal).unwrap();
        drop(service);
        let service = CleanupService::new(app_data.clone()).unwrap();
        let older = service
            .history_page(HistoryRequest {
                cursor: first.next_cursor,
                limit: Some(2),
            })
            .unwrap();
        assert_eq!(
            older
                .records
                .iter()
                .map(|record| record.execution_id.clone())
                .collect::<Vec<_>>(),
            vec![format!("{:032x}", 1), format!("{:032x}", 0)]
        );
        assert!(older.next_cursor.is_some());
        let terminal = service
            .history_page(HistoryRequest {
                cursor: older.next_cursor,
                limit: Some(2),
            })
            .unwrap();
        assert!(terminal.records.is_empty());
        assert!(terminal.next_cursor.is_none());
        assert_eq!(
            service
                .history_page(HistoryRequest::default())
                .unwrap()
                .records[0]
                .execution_id,
            journal.execution_id
        );
        fs::write(
            app_data
                .join("cleanup")
                .join("executions")
                .join(format!("{}.json", journal.execution_id)),
            b"invalid journal",
        )
        .unwrap();
        assert!(service.history_page(HistoryRequest::default()).is_err());
        drop(service);
        fs::remove_dir_all(fixture).unwrap();
    }

    #[test]
    fn lifetime_journals_do_not_block_enumeration_or_startup() {
        let (fixture, app_data, storage, _, path) = storage_file_fixture();
        let mut journal: ExecutionJournal =
            serde_json::from_str(include_str!("fixtures/journal-v1-mutating.json")).unwrap();
        journal.completed_at = Some(1);
        for index in 0..=MAX_ITEMS {
            journal.execution_id = format!("{index:032x}");
            storage.write_execution(&journal).unwrap();
        }
        let enumeration = storage
            .execution_records()
            .try_fold(0, |count, record| record.map(|_| count + 1));
        let startup = CleanupService::new(app_data).map(|_| ());
        assert!(path.exists());
        fs::remove_dir_all(fixture).unwrap();
        assert_eq!(
            (enumeration, startup),
            (Ok(MAX_ITEMS + 1), Ok(())),
            "1001 valid retained records must enumerate and allow startup"
        );
    }

    #[test]
    fn lifetime_journals_preserve_interrupted_recovery_replay_and_history_pages() {
        let (fixture, app_data, storage, permanent, path) = storage_file_fixture();
        let mut plan = permanent.clone();
        plan.plan_id = "e".repeat(32);
        plan.disposition = CleanupDisposition::Quarantine;
        storage.create_plan(&plan).unwrap();
        let payload = fs::read(&path).unwrap();
        let mut journal: ExecutionJournal =
            serde_json::from_str(include_str!("fixtures/journal-v1-mutating.json")).unwrap();
        journal.plan_id = plan.plan_id.clone();
        journal.disposition = CleanupDisposition::Quarantine;
        journal.purge_after = None;
        journal.items[0].item_id = plan.items[0].item_id.clone();
        // Every page contains interrupted records. Only the final journal has provable recovery.
        for index in 0..=MAX_ITEMS {
            journal.execution_id = format!("{index:032x}");
            journal.started_at = (index / 3) as u64; // exercise timestamp ties
            storage.write_execution(&journal).unwrap();
        }
        let recovery_id = journal.execution_id.clone();
        let recovery = storage
            .quarantine_directory(&recovery_id, &journal.items[0].item_id)
            .unwrap();
        fs::rename(&path, &recovery).unwrap(); // disposable fixture, no cleanup execution
        journal.items[0].quarantine_path = Some(recovery.clone());
        storage.write_execution(&journal).unwrap();
        let service = CleanupService::new(app_data.clone()).unwrap();
        let mut seen = HashSet::new();
        for record in storage.execution_records() {
            let record = record.unwrap();
            assert!(seen.insert(record.execution_id.clone()));
            assert!(record.completed_at.is_some());
            assert_eq!(
                record.items[0].state,
                if record.execution_id == recovery_id {
                    ItemState::Quarantined
                } else {
                    ItemState::Unknown
                }
            );
        }
        assert_eq!(seen.len(), MAX_ITEMS + 1);
        assert_eq!(fs::read(&recovery).unwrap(), payload);
        assert!(!path.exists());
        assert_eq!(
            service.execute(&plan.plan_id).unwrap_err(),
            CleanupServiceError::Conflict
        );
        assert!(!storage.has_execution_for_plan(&permanent.plan_id).unwrap());
        // Native permanent confirmation and execute both reject an already-used plan.
        journal.execution_id = "f".repeat(32);
        journal.plan_id = permanent.plan_id.clone();
        journal.disposition = CleanupDisposition::Permanent;
        journal.completed_at = Some(1);
        journal.items[0].state = ItemState::Unknown;
        journal.items[0].quarantine_path = None;
        storage.write_execution(&journal).unwrap();
        assert_eq!(
            service.execute_permanent(&permanent.plan_id).unwrap_err(),
            CleanupServiceError::Conflict
        );
        assert_eq!(
            service
                .confirm_permanent_with(&permanent.plan_id, 1, |_, _| panic!("replay prompted"))
                .unwrap_err(),
            CleanupServiceError::Conflict
        );

        let mut before: Option<(u64, String)> = None;
        let mut paged = HashSet::new();
        loop {
            let page = storage
                .execution_page(
                    before.as_ref().map(|(time, id)| (*time, id.as_str())),
                    crate::cleanup::storage::MAX_EXECUTION_PAGE,
                )
                .unwrap();
            assert!(page.len() <= crate::cleanup::storage::MAX_EXECUTION_PAGE);
            if page.is_empty() {
                break;
            }
            for record in &page {
                let key = (record.started_at, record.execution_id.clone());
                assert!(before.as_ref().is_none_or(|before| key < *before));
                assert!(paged.insert(record.execution_id.clone()));
                before = Some(key);
            }
        }
        assert_eq!(paged.len(), MAX_ITEMS + 2);
        assert!(seen.is_subset(&paged));
        assert_eq!(
            service
                .history_page(HistoryRequest {
                    cursor: None,
                    limit: Some(crate::cleanup::storage::MAX_EXECUTION_PAGE)
                })
                .unwrap()
                .records
                .len(),
            crate::cleanup::storage::MAX_EXECUTION_PAGE
        );
        // Enabled maintenance must traverse all records without purging uncertain/manual recovery.
        storage
            .write_policy(&AutoCleanupPolicy {
                enabled: true,
                ..Default::default()
            })
            .unwrap();
        service.purge_due().unwrap();
        assert_eq!(fs::read(&recovery).unwrap(), payload);
        drop(service);
        let service = CleanupService::new(app_data).unwrap();
        assert_eq!(
            service.undo(&recovery_id).unwrap().items[0].state,
            ItemState::Restored
        );
        assert_eq!(fs::read(&path).unwrap(), payload);
        assert_eq!(
            storage
                .execution_records()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
                .len(),
            MAX_ITEMS + 2
        );
        drop(service);
        fs::remove_dir_all(fixture).unwrap();
    }

    #[test]
    fn storage_confirmation_is_native_bounded_and_denial_never_mutates() {
        let (fixture, app_data, storage, plan, path) = storage_file_fixture();
        let service = CleanupService::new(app_data).unwrap();
        for id in ["invalid", &"f".repeat(32)] {
            assert!(
                service
                    .confirm_permanent_with(id, 1, |_, _| panic!("invalid plan prompted"))
                    .is_err()
            );
        }
        assert!(
            service
                .confirm_permanent_with(&plan.plan_id, 0, |_, _| panic!("zero owner prompted"))
                .is_err()
        );
        assert!(
            service
                .confirm_permanent_with(&plan.plan_id, 1, |_, text| {
                    assert!(text.contains(&plan.plan_id));
                    assert!(text.contains(&plan.items[0].item_id));
                    assert!(text.len() < 20000);
                    Ok(false)
                })
                .is_err()
        );
        assert!(
            service
                .confirm_permanent_with(&plan.plan_id, 1, |_, _| Err(
                    CleanupServiceError::OperationFailed
                ))
                .is_err()
        );
        assert!(path.exists());
        assert!(storage.executions().unwrap().is_empty());
        assert_eq!(
            service
                .confirm_permanent_with(&plan.plan_id, 1, |_, _| Ok(true))
                .unwrap()
                .items[0]
                .state,
            ItemState::Purged
        );
        assert!(!path.exists());
        drop(service);
        fs::remove_dir_all(fixture).unwrap();
    }

    #[test]
    fn storage_quarantine_identity_restart_collision_and_undo() {
        let (fixture, app_data, storage, mut plan, path) = storage_file_fixture();
        plan.plan_id = "e".repeat(32);
        plan.disposition = CleanupDisposition::Quarantine;
        storage.create_plan(&plan).unwrap();
        let expected = storage
            .expected_quarantine_path(&"f".repeat(32), &plan.items[0].item_id)
            .unwrap();
        assert!(!expected.parent().unwrap().exists());
        assert!(
            storage
                .expected_quarantine_path("../bad", &plan.items[0].item_id)
                .is_err()
        );
        let service = CleanupService::new(app_data.clone()).unwrap();
        let result = service.execute(&plan.plan_id).unwrap();
        assert_eq!(result.items[0].state, ItemState::Quarantined);
        assert!(result.accounting.occupied_bytes > 0);
        assert!(!path.exists());
        let mut journal = storage.read_execution(&result.execution_id).unwrap();
        let recovery = journal.items[0].quarantine_path.clone().unwrap();
        assert_eq!(
            WindowsFileSystem
                .metadata_no_follow(&recovery)
                .unwrap()
                .identity,
            Some(plan.items[0].proof.identity)
        );
        journal.completed_at = None;
        journal.items[0].state = ItemState::Mutating;
        storage.write_execution(&journal).unwrap();
        drop(service);
        let service = CleanupService::new(app_data.clone()).unwrap();
        assert_eq!(
            storage.read_execution(&result.execution_id).unwrap().items[0].state,
            ItemState::Quarantined
        );
        fs::write(&path, b"collision").unwrap();
        assert_eq!(
            service.undo(&result.execution_id).unwrap().items[0].state,
            ItemState::Quarantined
        );
        assert_eq!(fs::read(&path).unwrap(), b"collision");
        assert!(recovery.exists());
        fs::remove_file(&path).unwrap();
        // A post-rename journal failure must not turn absence into proof of restore.
        service.storage.fail_write(2);
        assert!(service.undo(&result.execution_id).is_err());
        assert_eq!(
            WindowsFileSystem
                .metadata_no_follow(&path)
                .unwrap()
                .identity,
            Some(plan.items[0].proof.identity)
        );
        drop(service);
        let service = CleanupService::new(app_data).unwrap();
        let interrupted = storage.read_execution(&result.execution_id).unwrap();
        assert_eq!(interrupted.items[0].state, ItemState::Unknown);
        assert!(interrupted.accounting.occupied_bytes > 0);
        assert_eq!(
            service.undo(&result.execution_id).unwrap().items[0].state,
            ItemState::Unknown
        );
        drop(service);
        fs::remove_dir_all(fixture).unwrap();
    }

    #[test]
    fn storage_recovery_success_and_intent_failure_preserve_identity() {
        let (fixture, app_data, storage, mut plan, path) = storage_file_fixture();
        plan.plan_id = "e".repeat(32);
        plan.disposition = CleanupDisposition::Quarantine;
        storage.create_plan(&plan).unwrap();
        let service = CleanupService::new(app_data).unwrap();
        service.storage.fail_write(2);
        assert_eq!(
            service.execute(&plan.plan_id).unwrap_err(),
            CleanupServiceError::PersistenceFailed
        );
        assert_eq!(
            WindowsFileSystem
                .metadata_no_follow(&path)
                .unwrap()
                .identity,
            Some(plan.items[0].proof.identity)
        );
        plan.plan_id = "f".repeat(32);
        storage.create_plan(&plan).unwrap();
        let result = service.execute(&plan.plan_id).unwrap();
        assert_eq!(result.items[0].state, ItemState::Quarantined);
        let recovered = service.undo(&result.execution_id).unwrap();
        assert_eq!(recovered.items[0].state, ItemState::Restored);
        assert_eq!(
            WindowsFileSystem
                .metadata_no_follow(&path)
                .unwrap()
                .identity,
            Some(plan.items[0].proof.identity)
        );
        assert_eq!(
            service.undo(&result.execution_id).unwrap().items[0].state,
            ItemState::Restored
        );
        assert_eq!(
            service.execute(&plan.plan_id).unwrap_err(),
            CleanupServiceError::Conflict
        );
        drop(service);
        fs::remove_dir_all(fixture).unwrap();
    }

    #[test]
    fn storage_recovery_rejects_untrusted_paths_and_modified_identity() {
        let (fixture, app_data, storage, mut plan, path) = storage_file_fixture();
        plan.plan_id = "e".repeat(32);
        plan.disposition = CleanupDisposition::Quarantine;
        storage.create_plan(&plan).unwrap();
        let service = CleanupService::new(app_data.clone()).unwrap();
        let result = service.execute(&plan.plan_id).unwrap();
        assert_eq!(result.items[0].state, ItemState::Quarantined);
        let mut journal = storage.read_execution(&result.execution_id).unwrap();
        let recovery = journal.items[0].quarantine_path.clone().unwrap();
        let arbitrary = fixture.join("arbitrary.bin");
        fs::write(&arbitrary, b"must survive").unwrap();
        journal.items[0].quarantine_path = Some(arbitrary.clone());
        storage.write_execution(&journal).unwrap();
        assert_eq!(
            service.undo(&result.execution_id).unwrap().items[0].state,
            ItemState::Unknown
        );
        assert_eq!(fs::read(&arbitrary).unwrap(), b"must survive");
        assert!(recovery.exists());
        assert!(!path.exists());
        journal.items[0].quarantine_path = Some(recovery.clone());
        journal.items[0].state = ItemState::Quarantined;
        storage.write_execution(&journal).unwrap();
        // Original root replacement must not authorize restoration to the replacement.
        let root = plan.items[0].proof.scan_root.clone();
        let old_root = root.with_extension("original-root");
        fs::rename(&root, &old_root).unwrap();
        fs::create_dir(&root).unwrap();
        assert_eq!(
            service.undo(&result.execution_id).unwrap().items[0].state,
            ItemState::Quarantined
        );
        assert!(recovery.exists());
        fs::remove_dir(&root).unwrap();
        fs::rename(&old_root, &root).unwrap();
        fs::write(&recovery, b"changed recovery bytes").unwrap();
        journal.completed_at = None;
        journal.items[0].state = ItemState::Mutating;
        storage.write_execution(&journal).unwrap();
        drop(service);
        let service = CleanupService::new(app_data).unwrap();
        let unknown = storage.read_execution(&result.execution_id).unwrap();
        assert_eq!(unknown.items[0].state, ItemState::Unknown);
        assert!(unknown.accounting.occupied_bytes > 0);
        assert_eq!(fs::read(&recovery).unwrap(), b"changed recovery bytes");
        assert_eq!(
            service.undo(&result.execution_id).unwrap().items[0].state,
            ItemState::Unknown
        );
        assert!(!path.exists());
        drop(service);
        fs::remove_dir_all(fixture).unwrap();
    }

    #[test]
    fn storage_mutating_journal_write_failure_preserves_source() {
        let (fixture, app_data, storage, plan, path) = storage_file_fixture();
        let before = fs::read(&path).unwrap();
        let identity = WindowsFileSystem
            .metadata_no_follow(&path)
            .unwrap()
            .identity;
        let service = CleanupService::new(app_data.clone()).unwrap();
        // Pending persists; the second write (Mutating) must fail before acquiring/deleting target.
        service.storage.fail_write(2);
        assert_eq!(
            service.execute_permanent(&plan.plan_id).unwrap_err(),
            CleanupServiceError::PersistenceFailed
        );
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(
            WindowsFileSystem
                .metadata_no_follow(&path)
                .unwrap()
                .identity,
            identity
        );
        let journals = storage.executions().unwrap();
        assert_eq!(journals.len(), 1);
        assert_eq!(journals[0].items[0].state, ItemState::Pending);
        assert!(journals[0].completed_at.is_none());
        drop(service);
        let restarted = CleanupService::new(app_data).unwrap();
        assert_eq!(
            restarted.execute_permanent(&plan.plan_id).unwrap_err(),
            CleanupServiceError::Conflict
        );
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(
            WindowsFileSystem
                .metadata_no_follow(&path)
                .unwrap()
                .identity,
            identity
        );
        drop(restarted);
        fs::remove_dir_all(fixture).unwrap();
    }

    // Undo is idempotent by design: repeating it reports the same restored outcome and never
    // touches the restored file or anything later written at that path.
    #[test]
    fn storage_repeat_undo_is_idempotent_and_never_mutates_again() {
        let (fixture, app_data, storage, mut plan, path) = storage_file_fixture();
        plan.plan_id = "b".repeat(32);
        plan.disposition = CleanupDisposition::Quarantine;
        storage.create_plan(&plan).unwrap();
        let bytes = fs::read(&path).unwrap();
        let service = CleanupService::new(app_data).unwrap();
        let result = service.execute(&plan.plan_id).unwrap();
        assert_eq!(result.items[0].state, ItemState::Quarantined);
        let first = service.undo(&result.execution_id).unwrap();
        assert_eq!(first.items[0].state, ItemState::Restored);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        let restored = WindowsFileSystem
            .metadata_no_follow(&path)
            .unwrap()
            .identity;
        let second = service.undo(&result.execution_id).unwrap();
        assert_eq!(second.items[0].state, ItemState::Restored);
        assert_eq!(
            WindowsFileSystem
                .metadata_no_follow(&path)
                .unwrap()
                .identity,
            restored
        );
        fs::write(&path, b"newer user data").unwrap();
        assert_eq!(
            service.undo(&result.execution_id).unwrap().items[0].state,
            ItemState::Restored
        );
        assert_eq!(fs::read(&path).unwrap(), b"newer user data");
        drop(service);
        fs::remove_dir_all(fixture).unwrap();
    }

    // A plan is created from snapshot evidence; replacing the file afterwards (same path and
    // bytes, new file identity) must be refused when the plan executes, for both dispositions.
    #[test]
    fn storage_execution_refuses_file_replaced_after_plan_creation() {
        for disposition in [
            CleanupDisposition::Quarantine,
            CleanupDisposition::Permanent,
        ] {
            let (fixture, app_data, storage, mut plan, path) = storage_file_fixture();
            plan.plan_id = "a".repeat(32);
            plan.disposition = disposition;
            storage.create_plan(&plan).unwrap();
            let bytes = fs::read(&path).unwrap();
            fs::remove_file(&path).unwrap();
            fs::write(&path, &bytes).unwrap();
            let replacement = WindowsFileSystem
                .metadata_no_follow(&path)
                .unwrap()
                .identity;
            assert_ne!(replacement, Some(plan.items[0].proof.identity));
            let service = CleanupService::new(app_data).unwrap();
            let outcome = match disposition {
                CleanupDisposition::Permanent => service.execute_permanent(&plan.plan_id),
                _ => service.execute(&plan.plan_id),
            };
            if let Ok(summary) = &outcome {
                assert!(summary.items.iter().all(|item| !matches!(
                    item.state,
                    ItemState::Quarantined | ItemState::Purged | ItemState::Recycled
                )));
            }
            assert_eq!(fs::read(&path).unwrap(), bytes);
            assert_eq!(
                WindowsFileSystem
                    .metadata_no_follow(&path)
                    .unwrap()
                    .identity,
                replacement
            );
            drop(service);
            fs::remove_dir_all(fixture).unwrap();
        }
    }

    #[test]
    fn storage_permanent_replay_and_interrupted_recovery_never_repeat_mutation() {
        let (fixture, app_data, storage, plan, path) = storage_file_fixture();
        let service = CleanupService::new(app_data.clone()).unwrap();
        service.validate_current_scope(&plan).unwrap();
        let mut recycle = plan.clone();
        recycle.plan_id = "e".repeat(32);
        recycle.disposition = CleanupDisposition::RecycleBin;
        storage.create_plan(&recycle).unwrap();
        let rejected = service.execute(&recycle.plan_id).unwrap();
        assert_eq!(rejected.items[0].state, ItemState::Failed);
        assert!(path.exists());
        assert_eq!(
            service.execute(&plan.plan_id).unwrap_err(),
            CleanupServiceError::InvalidInput
        );
        let result = service.execute_permanent(&plan.plan_id).unwrap();
        assert_eq!(result.items[0].state, ItemState::Purged);
        fs::write(&path, b"replacement survives").unwrap();
        assert_eq!(
            service.execute_permanent(&plan.plan_id).unwrap_err(),
            CleanupServiceError::Conflict
        );
        let mut journal = storage.read_execution(&result.execution_id).unwrap();
        journal.completed_at = None;
        journal.items[0].state = ItemState::Mutating;
        storage.write_execution(&journal).unwrap();
        drop(service);
        let restarted = CleanupService::new(app_data).unwrap();
        let recovered = storage.read_execution(&result.execution_id).unwrap();
        assert_eq!(recovered.items[0].state, ItemState::Unknown);
        assert_eq!(
            restarted.execute_permanent(&plan.plan_id).unwrap_err(),
            CleanupServiceError::Conflict
        );
        assert_eq!(fs::read(path).unwrap(), b"replacement survives");
        drop(restarted);
        fs::remove_dir_all(fixture).unwrap();
    }

    fn project_test_directory() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "supa-diska-project-roots-{}-{}",
            std::process::id(),
            getrandom::u64().unwrap()
        ));
        std::fs::create_dir(&path).unwrap();
        path
    }

    fn registered_execution_fixture(
        relative_paths: &[&str],
    ) -> (
        PathBuf,
        PathBuf,
        CleanupStorage,
        CleanupPlan,
        Vec<(String, PathBuf)>,
    ) {
        let root = project_test_directory();
        let app_data = root.join("app-data");
        let project = root.join("project");
        std::fs::create_dir(&project).unwrap();
        let project = std::fs::canonicalize(project).unwrap();
        let storage = CleanupStorage::open(app_data.join("cleanup")).unwrap();
        let root_id = "a".repeat(32);
        let profile_id = "b".repeat(32);
        storage
            .write_project_roots(&[ProjectRoot {
                id: root_id.clone(),
                display_path: project.to_string_lossy().into_owned(),
                paused: false,
                added_at_unix_seconds: 1,
                last_scanned_at_unix_seconds: None,
            }])
            .unwrap();
        storage
            .write_build_profiles(&[BuildProfile {
                profile_id: profile_id.clone(),
                root_id: root_id.clone(),
                display_name: "Fixture build".into(),
                ecosystem: BuildEcosystem::Rust,
                executable: project.join("builder.exe").to_string_lossy().into_owned(),
                executable_identity: cleanup_core::FileIdentity { volume: 1, file: 1 },
                argv: vec!["build".into()],
                working_directory: project.to_string_lossy().into_owned(),
                profile_label: "fixture".into(),
                toolchain_label: "stable".into(),
                target_label: "test".into(),
                rebuild_cost: RebuildCost::Low,
                artifact_paths: relative_paths
                    .iter()
                    .map(|path| RegisteredArtifactPath {
                        relative_path: (*path).into(),
                        role: ArtifactRole::Generation,
                    })
                    .collect(),
            }])
            .unwrap();

        let file_system = WindowsFileSystem;
        let context_identity = file_system
            .metadata_no_follow(&project)
            .unwrap()
            .identity
            .unwrap();
        let mut generations = Vec::new();
        let mut items = Vec::new();
        let mut artifacts = Vec::new();
        for (index, relative_path) in relative_paths.iter().enumerate() {
            let relative = PathBuf::from(relative_path);
            let path = project.join(&relative);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(path.join("payload.bin"), format!("payload-{index}")).unwrap();
            let snapshot = snapshot_registered_path(&file_system, &project, &relative).unwrap();
            let generation_id = format!("{:032x}", index + 1);
            generations.push(GenerationState {
                generation_id: generation_id.clone(),
                profile_id: profile_id.clone(),
                root_id: root_id.clone(),
                normalized_path: relative_path.replace('\\', "/"),
                allocated_bytes: snapshot.allocated_bytes,
                last_successful_touch: Some(1),
                last_external_change: None,
                profile_label: "fixture".into(),
                toolchain_label: "stable".into(),
                target_label: "test".into(),
                rebuild_cost: RebuildCost::Low,
                owned: true,
                identity: Some(snapshot.identity),
                observed_modified_at_unix_nanos: snapshot.modified_at_unix_nanos,
                role: ArtifactRole::Generation,
                active: false,
                readable: true,
                ambiguous: false,
                touched_by_latest_success: false,
            });
            let target = path.file_name().unwrap().to_string_lossy().into_owned();
            items.push(PlanItem {
                item_id: format!("{:032x}", index + 100),
                proof: cleanup_core::ResolvedCandidate {
                    scope: CandidateProofScope::RegisteredBuildArtifact {
                        root_id: root_id.clone(),
                        profile_id: profile_id.clone(),
                        generation_id: generation_id.clone(),
                    },
                    path: path.clone(),
                    scan_root: path.parent().unwrap().to_path_buf(),
                    context_root: project.clone(),
                    context_identity: Some(context_identity),
                    rule: cleanup_core::CleanupRule {
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
                        target_type: TargetType::Directory,
                        root_depth: 0,
                        project_depth: None,
                        target_depth: None,
                        minimum_age_seconds: 0,
                        excluded_names: Vec::new(),
                        excluded_paths: Vec::new(),
                    },
                    identity: snapshot.identity,
                    kind: snapshot.kind,
                    logical_bytes: snapshot.allocated_bytes,
                    allocated_bytes: snapshot.allocated_bytes,
                    scanned_at: SystemTime::now(),
                },
            });
            artifacts.push((generation_id, path));
        }
        storage
            .write_artifact_generations(&ArtifactGenerationLedger {
                schema_version: 1,
                last_analysis_at: Some(1),
                last_successful_build: None,
                generations,
            })
            .unwrap();
        let plan = CleanupPlan::build_artifact(
            "c".repeat(32),
            "d".repeat(32),
            1,
            root_id,
            profile_id,
            items,
        )
        .unwrap();
        (root, app_data, storage, plan, artifacts)
    }

    #[test]
    fn registered_artifact_execution_reports_all_failed_items() {
        let (root, _app_data, storage, plan, artifacts) =
            registered_execution_fixture(&["target/one", "target/two"]);
        for (_, path) in &artifacts {
            std::fs::remove_dir_all(path).unwrap();
        }

        let outcome =
            execute_registered_artifact_plan(&storage, &WindowsFileSystem, &plan, 60).unwrap();

        assert!(outcome.quarantined_generation_ids.is_empty());
        assert_eq!(outcome.failed_items.len(), 2);
        assert!(outcome.failed_items.iter().all(|item| {
            item.state == ItemState::Failed
                && item.failure.as_deref() == Some("revalidation-rejected")
        }));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn corrupt_vendor_journal_does_not_disable_legacy_history_or_undo() {
        for corrupt in [
            b"not JSON".as_slice(),
            br#"{"schema_version":999,"journals":[]}"#.as_slice(),
        ] {
            let (root, app_data, storage, plan, artifacts) =
                registered_execution_fixture(&["target/one"]);
            execute_registered_artifact_plan(&storage, &WindowsFileSystem, &plan, 60).unwrap();
            let before = serde_json::to_vec(&storage.executions().unwrap()).unwrap();
            let vendor_path = app_data.join("cleanup/vendor-jobs.json");
            fs::write(&vendor_path, corrupt).unwrap();
            let service = CleanupService::new(app_data).unwrap();
            assert!(matches!(
                service.vendor_jobs(),
                Err(crate::storage::vendor_uninstall::VendorJobError::Storage)
            ));
            assert_eq!(fs::read(&vendor_path).unwrap(), corrupt);
            assert_eq!(
                serde_json::to_vec(&storage.executions().unwrap()).unwrap(),
                before
            );
            let execution = service
                .history_page(HistoryRequest {
                    cursor: None,
                    limit: Some(crate::cleanup::storage::MAX_EXECUTION_PAGE),
                })
                .unwrap()
                .records
                .pop()
                .unwrap();
            service.undo(&execution.execution_id).unwrap();
            assert!(artifacts[0].1.exists());
            assert_eq!(fs::read(&vendor_path).unwrap(), corrupt);
            drop(service);
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn registered_artifact_execution_reports_partial_failure_and_keeps_undo() {
        let (root, app_data, storage, plan, artifacts) =
            registered_execution_fixture(&["target/one", "target/two"]);
        std::fs::remove_dir_all(&artifacts[1].1).unwrap();

        let outcome =
            execute_registered_artifact_plan(&storage, &WindowsFileSystem, &plan, 60).unwrap();

        assert_eq!(
            outcome.quarantined_generation_ids,
            vec![artifacts[0].0.clone()]
        );
        assert_eq!(outcome.failed_items.len(), 1);
        assert_eq!(outcome.failed_items[0].item_id, plan.items[1].item_id);
        assert!(!artifacts[0].1.exists());
        let service = CleanupService::new(app_data).unwrap();
        let execution = service
            .history_page(HistoryRequest {
                cursor: None,
                limit: Some(crate::cleanup::storage::MAX_EXECUTION_PAGE),
            })
            .unwrap()
            .records
            .pop()
            .unwrap();
        service.undo(&execution.execution_id).unwrap();
        assert!(artifacts[0].1.exists());
        assert!(!artifacts[1].1.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn project_root_crud_scans_by_id_and_never_retains_cleanup_snapshots() {
        let root = project_test_directory();
        let app_data = root.join("app-data");
        let project = root.join("project");
        std::fs::create_dir(&project).unwrap();
        std::fs::write(project.join("package.json"), b"{}").unwrap();
        std::fs::create_dir(project.join("node_modules")).unwrap();
        std::fs::write(project.join("node_modules/dependency.bin"), b"dependency").unwrap();
        let service = CleanupService::new(app_data.clone()).unwrap();

        let roots = service
            .add_project_root(&format!("  {}  ", project.display()))
            .unwrap();
        let id = roots[0].id.clone();
        assert_eq!(
            service.add_project_root(&project.to_string_lossy().to_ascii_uppercase()),
            Err(CleanupServiceError::DuplicateRoot)
        );
        assert!(service.set_project_root_paused(&id, true).unwrap()[0].paused);
        assert!(matches!(
            service.discover_project_artifacts(Some(&id)),
            Err(CleanupServiceError::RootPaused)
        ));
        service.set_project_root_paused(&id, false).unwrap();
        let scan = service.discover_project_artifacts(Some(&id)).unwrap();
        assert_eq!(scan.records.len(), 1);
        assert_eq!(scan.records[0].default_selected, Some(false));
        assert_eq!(
            scan.roots[0].last_scanned_at_unix_seconds,
            Some(scan.scanned_at_unix_seconds)
        );
        assert!(service.scans.lock().unwrap().is_empty());
        assert_eq!(
            CleanupService::new(app_data)
                .unwrap()
                .list_project_roots()
                .unwrap(),
            scan.roots
        );
        assert!(service.remove_project_root(&id).unwrap().is_empty());
        assert!(
            project.exists(),
            "removing a root must never remove project files"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn all_root_scan_collapses_children_but_one_root_scan_uses_the_exact_id() {
        let root = project_test_directory();
        let parent = root.join("parent");
        let child = parent.join("child");
        std::fs::create_dir_all(child.join("node_modules")).unwrap();
        std::fs::write(child.join("package.json"), b"{}").unwrap();
        let service = CleanupService::new(root.join("app-data")).unwrap();
        let parent_id = service.add_project_root(parent.to_str().unwrap()).unwrap()[0]
            .id
            .clone();
        let child_id = service
            .add_project_root(child.to_str().unwrap())
            .unwrap()
            .into_iter()
            .find(|saved| saved.display_path.ends_with("child"))
            .unwrap()
            .id;

        let all = service.discover_project_artifacts(None).unwrap();
        assert_eq!(all.records.len(), 1);
        assert!(
            all.roots
                .iter()
                .find(|saved| saved.id == parent_id)
                .unwrap()
                .last_scanned_at_unix_seconds
                .is_some()
        );
        assert!(
            all.roots
                .iter()
                .find(|saved| saved.id == child_id)
                .unwrap()
                .last_scanned_at_unix_seconds
                .is_none()
        );
        let child_scan = service.discover_project_artifacts(Some(&child_id)).unwrap();
        assert_eq!(child_scan.records.len(), 1);
        assert!(
            child_scan
                .roots
                .iter()
                .find(|saved| saved.id == child_id)
                .unwrap()
                .last_scanned_at_unix_seconds
                .is_some()
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "moves only owned disposable fixtures through the real Windows Recycle Bin"]
    fn disposable_recycle_quarantine_purge_and_permanent_drill() {
        for orphan in WindowsRecycleBin
            .list()
            .unwrap()
            .into_iter()
            .filter(|item| {
                item.original_path().components().any(|component| {
                    component
                        .as_os_str()
                        .to_string_lossy()
                        .starts_with("supa-diska-destructive-drill-")
                })
            })
        {
            let orphan_root = orphan
                .original_path()
                .parent()
                .and_then(Path::parent)
                .map(Path::to_path_buf);
            let _ = restore_exact(&WindowsRecycleBin, &orphan);
            if let Some(orphan_root) = orphan_root {
                let _ = std::fs::remove_dir_all(orphan_root);
            }
        }
        let root = std::env::temp_dir().join(format!(
            "supa-diska-destructive-drill-{}-{}",
            std::process::id(),
            getrandom::u64().unwrap()
        ));
        let app_data = root.join("app-data");
        std::fs::create_dir_all(&app_data).unwrap();
        let candidate = |name: &str| {
            let path = root.join(name).join("cache");
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(path.join("payload"), name).unwrap();
            std::fs::canonicalize(path).unwrap()
        };
        let select = |service: &CleanupService, path: &Path| {
            let preview = service.preview().unwrap();
            let id = preview
                .records
                .iter()
                .find(|record| Path::new(&record.display_path) == path)
                .unwrap()
                .id
                .clone();
            (preview.scan_id, id)
        };

        let service = CleanupService::new(app_data.clone()).unwrap();
        let recycled_path = candidate("recycle");
        let (scan_id, id) = select(&service, &recycled_path);
        let plan = service
            .create_plan(&scan_id, &[id], CleanupDisposition::RecycleBin)
            .unwrap();
        let recycled = service.execute(&plan.plan_id).unwrap();
        assert!(!recycled_path.exists());
        service.undo(&recycled.execution_id).unwrap();
        assert!(recycled_path.exists());

        let quarantined_path = candidate("quarantine");
        let (scan_id, id) = select(&service, &quarantined_path);
        let plan = service
            .create_plan(&scan_id, &[id], CleanupDisposition::Quarantine)
            .unwrap();
        let quarantined = service.execute(&plan.plan_id).unwrap();
        assert!(!quarantined_path.exists());
        drop(service);
        let service = CleanupService::new(app_data.clone()).unwrap();
        service.undo(&quarantined.execution_id).unwrap();
        assert!(quarantined_path.exists());

        let purge_path = candidate("purge");
        let (scan_id, id) = select(&service, &purge_path);
        let plan = service
            .create_plan(&scan_id, &[id], CleanupDisposition::Quarantine)
            .unwrap();
        let purged = service.execute(&plan.plan_id).unwrap();
        let mut journal = service
            .storage
            .read_execution(&purged.execution_id)
            .unwrap();
        journal.purge_after = Some(0);
        service.storage.write_execution(&journal).unwrap();
        service
            .storage
            .write_policy(&AutoCleanupPolicy {
                schema_version: 1,
                enabled: true,
                grace_days: 1,
            })
            .unwrap();
        service.purge_due().unwrap();
        assert!(
            service
                .storage
                .read_execution(&purged.execution_id)
                .unwrap()
                .items
                .iter()
                .all(|item| item.state == ItemState::Purged)
        );

        let permanent_path = candidate("permanent");
        let (scan_id, id) = select(&service, &permanent_path);
        let plan = service
            .create_plan(&scan_id, &[id], CleanupDisposition::Permanent)
            .unwrap();
        service.execute_permanent(&plan.plan_id).unwrap();
        assert!(!permanent_path.exists());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn persisted_plan_cannot_expand_the_fixed_scan_root() {
        let expected_root = PathBuf::from(r"C:\Temp");
        let plan = CleanupPlan::new(
            "1".repeat(32),
            "2".repeat(32),
            1,
            CleanupDisposition::RecycleBin,
            vec![PlanItem {
                item_id: "3".repeat(32),
                proof: cleanup_core::ResolvedCandidate {
                    scope: cleanup_core::CandidateProofScope::Temporary,
                    path: PathBuf::from(r"C:\Other\cache"),
                    scan_root: PathBuf::from(r"C:\Other"),
                    context_root: PathBuf::from(r"C:\Other"),
                    context_identity: None,
                    rule: temporary_rule().unwrap(),
                    identity: cleanup_core::FileIdentity { volume: 1, file: 1 },
                    kind: cleanup_core::EntryKind::Directory,
                    logical_bytes: 0,
                    allocated_bytes: 0,
                    scanned_at: SystemTime::UNIX_EPOCH,
                },
            }],
        )
        .unwrap();

        assert!(!plan_matches_current_scope(
            &WindowsFileSystem,
            &plan,
            &temporary_rule().unwrap(),
            &expected_root,
        ));
    }

    #[test]
    fn automatic_policy_defaults_disabled_and_persists_grace() {
        let root = std::env::temp_dir().join(format!(
            "supa-diska-service-{}-{}",
            std::process::id(),
            getrandom::u64().unwrap()
        ));
        std::fs::create_dir(&root).unwrap();
        let service = CleanupService::new(root.clone()).unwrap();
        assert_eq!(service.policy().unwrap(), AutoCleanupPolicy::default());
        service.set_policy(false, 14).unwrap();
        drop(service);
        let reopened = CleanupService::new(root.clone()).unwrap();
        assert_eq!(reopened.policy().unwrap().grace_days, 14);
        assert!(!reopened.policy().unwrap().enabled);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn project_root_limits_apply_the_profile_workers_and_keep_divided_budgets() {
        use crate::storage::scan_profile::{HDD_WORKERS, SSD_WORKERS};
        let table = [
            (ScanProfile::Ssd, None, SSD_WORKERS),
            (ScanProfile::Hdd, None, HDD_WORKERS),
            (ScanProfile::Auto, Some(false), SSD_WORKERS),
            (ScanProfile::Auto, None, HDD_WORKERS),
        ];
        assert_eq!((SSD_WORKERS, HDD_WORKERS), (4, 2));
        for divisor in [1, 3] {
            let base = divided_project_limits(divisor);
            for (profile, seek_penalty, expected) in table {
                let limits = project_root_limits(base, profile, seek_penalty);
                let context = format!("{profile:?} {seek_penalty:?} /{divisor}");
                assert_eq!(limits.max_workers, expected, "{context}");
                assert_eq!(
                    limits.max_visited_entries, base.max_visited_entries,
                    "{context}"
                );
                assert_eq!(limits.max_candidates, base.max_candidates, "{context}");
                assert_eq!(limits.max_diagnostics, base.max_diagnostics, "{context}");
                assert_eq!(
                    limits.max_measurement_entries, base.max_measurement_entries,
                    "{context}"
                );
            }
        }
    }

    #[test]
    fn scan_settings_and_profile_persist_and_report_auto_when_corrupt() {
        let root = std::env::temp_dir().join(format!(
            "supa-diska-scan-profile-{}-{}",
            std::process::id(),
            getrandom::u64().unwrap()
        ));
        std::fs::create_dir(&root).unwrap();
        let service = CleanupService::new(root.clone()).unwrap();
        assert_eq!(service.scan_profile(), ScanProfile::Auto);
        service.set_scan_profile(ScanProfile::Ssd).unwrap();
        assert_eq!(service.scan_profile(), ScanProfile::Ssd);
        drop(service);

        let reopened = CleanupService::new(root.clone()).unwrap();
        assert_eq!(reopened.scan_profile(), ScanProfile::Ssd);
        std::fs::write(root.join("cleanup").join("scan-settings.json"), b"{").unwrap();
        assert_eq!(reopened.scan_settings().unwrap(), ScanSettings::default());
        assert_eq!(reopened.scan_profile(), ScanProfile::Auto);

        let settings_path = root.join("cleanup").join("scan-settings.json");
        for bytes in [
            &br#"{"schemaVersion":2,"profile":"ssd"}"#[..],
            br#"{"schemaVersion":1,"profile":"turbo"}"#,
        ] {
            std::fs::write(&settings_path, bytes).unwrap();
            assert_eq!(reopened.scan_settings().unwrap(), ScanSettings::default());
            assert_eq!(reopened.scan_profile(), ScanProfile::Auto);
        }

        // Saving a valid profile replaces the corrupt file.
        reopened.set_scan_profile(ScanProfile::Hdd).unwrap();
        assert_eq!(reopened.scan_settings().unwrap().profile, ScanProfile::Hdd);
        assert_eq!(reopened.scan_profile(), ScanProfile::Hdd);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn accounting_separates_failed_recoverable_and_reclaimed_bytes() {
        let root = std::env::temp_dir().join(format!(
            "supa-diska-accounting-{}-{}",
            std::process::id(),
            getrandom::u64().unwrap()
        ));
        let storage = CleanupStorage::open(root.clone()).unwrap();
        let mut journal = ExecutionJournal {
            schema_version: 1,
            execution_id: "1".repeat(32),
            plan_id: "2".repeat(32),
            started_at: 1,
            completed_at: None,
            disposition: CleanupDisposition::Quarantine,
            purge_after: Some(2),
            items: vec![
                ExecutionItem {
                    item_id: "3".repeat(32),
                    state: ItemState::Failed,
                    logical_bytes: 10,
                    processed: false,
                    occupied_bytes: 0,
                    reclaimed_bytes: 0,
                    quarantine_path: None,
                    recycle_item: None,
                    failure: Some("rejected".to_owned()),
                },
                ExecutionItem {
                    item_id: "4".repeat(32),
                    state: ItemState::Quarantined,
                    logical_bytes: 20,
                    processed: true,
                    occupied_bytes: 24,
                    reclaimed_bytes: 0,
                    quarantine_path: Some(root.join("payload")),
                    recycle_item: None,
                    failure: None,
                },
            ],
            accounting: ByteAccounting {
                selected_bytes: 30,
                ..ByteAccounting::default()
            },
        };

        persist_accounting(&storage, &mut journal).unwrap();

        assert_eq!(journal.accounting.selected_bytes, 30);
        assert_eq!(journal.accounting.processed_bytes, 20);
        assert_eq!(journal.accounting.failed_bytes, 10);
        assert_eq!(journal.accounting.quarantined_bytes, 24);
        assert_eq!(journal.accounting.occupied_bytes, 24);
        assert_eq!(journal.accounting.reclaimed_bytes, 0);
        std::fs::remove_dir_all(root).unwrap();
    }
}
