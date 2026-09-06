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

const MAX_HISTORY: usize = 100;
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
        let service = Self {
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
        let limits = divided_project_limits(divisor);
        let mut records = Vec::new();
        let mut diagnostics = Vec::new();
        let mut scanned_ids = HashSet::new();
        for root in selected {
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
        let plan = CleanupPlan::new(
            plan_id.clone(),
            scan_id.to_owned(),
            now_seconds()?,
            disposition,
            items,
        )
        .map_err(map_storage_error)?;
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
        let current_rule = temporary_rule().map_err(|_| CleanupServiceError::ValidationFailed)?;
        let expected_root = temporary_root().map_err(|_| CleanupServiceError::ValidationFailed)?;
        if !plan_matches_current_scope(&self.file_system, &plan, &current_rule, &expected_root) {
            return Err(CleanupServiceError::ValidationFailed);
        }
        let execution_id = random_id()?;
        let started_at = now_seconds()?;
        let policy = self.storage.policy().map_err(map_storage_error)?;
        let purge_after = (plan.disposition == CleanupDisposition::Quarantine)
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
            if plan.disposition == CleanupDisposition::Quarantine {
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
        Ok(summary(&journal))
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
        Ok(summary(&journal))
    }

    pub fn history(&self) -> Result<Vec<CleanupExecutionSummary>, CleanupServiceError> {
        Ok(self
            .storage
            .executions()
            .map_err(map_storage_error)?
            .into_iter()
            .take(MAX_HISTORY)
            .map(|journal| summary(&journal))
            .collect())
    }

    pub fn policy(&self) -> Result<AutoCleanupPolicy, CleanupServiceError> {
        self.storage.policy().map_err(map_storage_error)
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
        for mut journal in self.storage.executions().map_err(map_storage_error)? {
            if journal.disposition != CleanupDisposition::Quarantine
                || journal
                    .purge_after
                    .is_none_or(|purge_after| purge_after > now)
            {
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

    fn reconcile_interrupted(&self) -> Result<(), CleanupServiceError> {
        for mut journal in self.storage.executions().map_err(map_storage_error)? {
            if journal.completed_at.is_some() {
                continue;
            }
            let plan = self
                .storage
                .read_plan(&journal.plan_id)
                .map_err(map_storage_error)?;
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
        CleanupPlanScope::Temporary => return Err(CleanupServiceError::ValidationFailed),
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
                .filter(|item| item.processed)
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
        state: item.state,
        logical_bytes: item.logical_bytes,
        failure: item.failure.clone(),
    }
}

fn summary(journal: &ExecutionJournal) -> CleanupExecutionSummary {
    CleanupExecutionSummary {
        execution_id: journal.execution_id.clone(),
        plan_id: journal.plan_id.clone(),
        disposition: journal.disposition,
        completed: journal.completed_at.is_some(),
        purge_after: journal.purge_after,
        items: journal.items.iter().map(item_outcome).collect(),
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
    let semantics = file_system.semantics();
    plan.items.iter().all(|item| {
        item.proof.rule == *current_rule
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
        let execution = service.history().unwrap().pop().unwrap();
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
