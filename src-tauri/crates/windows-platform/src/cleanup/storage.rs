use cleanup_core::{
    ArtifactRole, BudgetLimits, CandidateProofScope, FileIdentity, GenerationState, RebuildCost,
    ResolvedCandidate,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use windows_sys::Win32::Storage::FileSystem::{
    MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
};

use super::{filesystem::wide, recycle::RecycleItem};

pub const MAX_ITEMS: usize = 1_000;
pub const MAX_PROJECT_ROOTS: usize = 32;
pub const MAX_BUILD_PROFILES: usize = 32;
pub const MAX_PROFILE_ARTIFACTS: usize = 16;
pub const MAX_PROFILE_ARGUMENTS: usize = 64;
pub const MAX_GENERATIONS: usize = 2_048;
const MAX_GENERATIONS_PER_PROFILE: usize = 256;
const MAX_GENERATIONS_PER_PROJECT: usize = 512;
const MAX_PROJECT_PATH_BYTES: usize = 4_096;
const MAX_STRING_BYTES: usize = 1_024;
const MAX_ARGUMENT_BYTES: usize = 4_096;
const MAX_RECORD_BYTES: u64 = 8 * 1024 * 1024;
const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CleanupDisposition {
    RecycleBin,
    Quarantine,
    Permanent,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanItem {
    pub item_id: String,
    pub proof: ResolvedCandidate,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum CleanupPlanScope {
    #[default]
    Temporary,
    BuildArtifact {
        root_id: String,
        profile_id: String,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CleanupPlan {
    pub schema_version: u32,
    pub plan_id: String,
    pub scan_id: String,
    pub created_at: u64,
    pub disposition: CleanupDisposition,
    #[serde(default)]
    pub scope: CleanupPlanScope,
    pub items: Vec<PlanItem>,
}

impl CleanupPlan {
    pub fn new(
        plan_id: String,
        scan_id: String,
        created_at: u64,
        disposition: CleanupDisposition,
        items: Vec<PlanItem>,
    ) -> Result<Self, StorageError> {
        Self::with_scope(
            plan_id,
            scan_id,
            created_at,
            disposition,
            CleanupPlanScope::Temporary,
            items,
        )
    }

    pub fn build_artifact(
        plan_id: String,
        scan_id: String,
        created_at: u64,
        root_id: String,
        profile_id: String,
        items: Vec<PlanItem>,
    ) -> Result<Self, StorageError> {
        Self::with_scope(
            plan_id,
            scan_id,
            created_at,
            CleanupDisposition::Quarantine,
            CleanupPlanScope::BuildArtifact {
                root_id,
                profile_id,
            },
            items,
        )
    }

    fn with_scope(
        plan_id: String,
        scan_id: String,
        created_at: u64,
        disposition: CleanupDisposition,
        scope: CleanupPlanScope,
        items: Vec<PlanItem>,
    ) -> Result<Self, StorageError> {
        let plan = Self {
            schema_version: SCHEMA_VERSION,
            plan_id,
            scan_id,
            created_at,
            disposition,
            scope,
            items,
        };
        plan.validate()?;
        Ok(plan)
    }

    pub fn validate(&self) -> Result<(), StorageError> {
        let scope_valid = match &self.scope {
            CleanupPlanScope::Temporary => self
                .items
                .iter()
                .all(|item| matches!(item.proof.scope, CandidateProofScope::Temporary)),
            CleanupPlanScope::BuildArtifact {
                root_id,
                profile_id,
            } => {
                valid_id(root_id)
                    && valid_id(profile_id)
                    && self.disposition == CleanupDisposition::Quarantine
                    && self.items.iter().all(|item| {
                        matches!(
                            &item.proof.scope,
                            CandidateProofScope::RegisteredBuildArtifact {
                                root_id: proof_root,
                                profile_id: proof_profile,
                                generation_id,
                            } if proof_root == root_id
                                && proof_profile == profile_id
                                && valid_id(generation_id)
                        )
                    })
            }
        };
        if self.schema_version != SCHEMA_VERSION
            || !valid_id(&self.plan_id)
            || !valid_id(&self.scan_id)
            || self.items.is_empty()
            || self.items.len() > MAX_ITEMS
            || !scope_valid
            || self.items.iter().any(|item| {
                !valid_id(&item.item_id)
                    || !item.proof.path.is_absolute()
                    || !item.proof.scan_root.is_absolute()
                    || !item.proof.context_root.is_absolute()
            })
        {
            return Err(StorageError::Invalid);
        }
        let mut ids = std::collections::HashSet::new();
        if !self.items.iter().all(|item| ids.insert(&item.item_id)) {
            return Err(StorageError::Invalid);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ItemState {
    Pending,
    Mutating,
    Recycled,
    Quarantined,
    Purged,
    Restored,
    Failed,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionItem {
    pub item_id: String,
    pub state: ItemState,
    pub logical_bytes: u64,
    pub processed: bool,
    pub occupied_bytes: u64,
    pub reclaimed_bytes: u64,
    pub quarantine_path: Option<PathBuf>,
    pub recycle_item: Option<RecycleItem>,
    pub failure: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ByteAccounting {
    pub selected_bytes: u64,
    pub processed_bytes: u64,
    pub failed_bytes: u64,
    pub quarantined_bytes: u64,
    pub purged_bytes: u64,
    pub occupied_bytes: u64,
    pub reclaimed_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionJournal {
    pub schema_version: u32,
    pub execution_id: String,
    pub plan_id: String,
    pub started_at: u64,
    pub completed_at: Option<u64>,
    pub disposition: CleanupDisposition,
    pub purge_after: Option<u64>,
    pub items: Vec<ExecutionItem>,
    pub accounting: ByteAccounting,
}

impl ExecutionJournal {
    pub fn validate(&self) -> Result<(), StorageError> {
        if self.schema_version != SCHEMA_VERSION
            || !valid_id(&self.execution_id)
            || !valid_id(&self.plan_id)
            || self.items.is_empty()
            || self.items.len() > MAX_ITEMS
            || self.items.iter().any(|item| !valid_id(&item.item_id))
        {
            Err(StorageError::Invalid)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutoCleanupPolicy {
    pub schema_version: u32,
    pub enabled: bool,
    pub grace_days: u16,
}

impl Default for AutoCleanupPolicy {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            enabled: false,
            grace_days: 7,
        }
    }
}

impl AutoCleanupPolicy {
    pub fn validate(&self) -> Result<(), StorageError> {
        if self.schema_version == SCHEMA_VERSION && (1..=30).contains(&self.grace_days) {
            Ok(())
        } else {
            Err(StorageError::Invalid)
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectRoot {
    pub id: String,
    pub display_path: String,
    pub paused: bool,
    pub added_at_unix_seconds: u64,
    pub last_scanned_at_unix_seconds: Option<u64>,
}

impl ProjectRoot {
    fn validate(&self) -> Result<(), StorageError> {
        let path = Path::new(&self.display_path);
        if !valid_id(&self.id)
            || self.display_path.is_empty()
            || self.display_path.len() > MAX_PROJECT_PATH_BYTES
            || self.display_path.chars().any(char::is_control)
            || !path.is_absolute()
        {
            Err(StorageError::Invalid)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProjectRootRegistry {
    schema_version: u32,
    roots: Vec<ProjectRoot>,
}

impl Default for ProjectRootRegistry {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            roots: Vec::new(),
        }
    }
}

impl ProjectRootRegistry {
    fn validate(&self) -> Result<(), StorageError> {
        let mut ids = std::collections::HashSet::new();
        if self.schema_version != SCHEMA_VERSION
            || self.roots.len() > MAX_PROJECT_ROOTS
            || self
                .roots
                .iter()
                .any(|root| root.validate().is_err() || !ids.insert(&root.id))
        {
            Err(StorageError::Invalid)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BuildEcosystem {
    Rust,
    Node,
    Generic,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegisteredArtifactPath {
    pub relative_path: String,
    pub role: ArtifactRole,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BuildProfile {
    pub profile_id: String,
    pub root_id: String,
    pub display_name: String,
    pub ecosystem: BuildEcosystem,
    pub executable: String,
    pub executable_identity: FileIdentity,
    pub argv: Vec<String>,
    pub working_directory: String,
    pub profile_label: String,
    pub toolchain_label: String,
    pub target_label: String,
    pub rebuild_cost: RebuildCost,
    pub artifact_paths: Vec<RegisteredArtifactPath>,
}

impl BuildProfile {
    pub fn validate(&self) -> Result<(), StorageError> {
        if !valid_id(&self.profile_id)
            || !valid_id(&self.root_id)
            || !valid_text(&self.display_name, MAX_STRING_BYTES)
            || !valid_text(&self.profile_label, MAX_STRING_BYTES)
            || !valid_text(&self.toolchain_label, MAX_STRING_BYTES)
            || !valid_text(&self.target_label, MAX_STRING_BYTES)
            || !valid_absolute_path(&self.executable)
            || !valid_absolute_path(&self.working_directory)
            || self.argv.len() > MAX_PROFILE_ARGUMENTS
            || self.argv.iter().any(|value| {
                value.len() > MAX_ARGUMENT_BYTES || value.chars().any(|character| character == '\0')
            })
            || self.artifact_paths.is_empty()
            || self.artifact_paths.len() > MAX_PROFILE_ARTIFACTS
        {
            return Err(StorageError::Invalid);
        }
        let mut paths = Vec::with_capacity(self.artifact_paths.len());
        for artifact in &self.artifact_paths {
            let path = normalize_relative(&artifact.relative_path).ok_or(StorageError::Invalid)?;
            if paths.iter().any(|other| {
                Path::new(&path).starts_with(Path::new(other))
                    || Path::new(other).starts_with(Path::new(&path))
            }) {
                return Err(StorageError::Invalid);
            }
            paths.push(path);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BuildProfileRegistry {
    schema_version: u32,
    profiles: Vec<BuildProfile>,
}

impl Default for BuildProfileRegistry {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            profiles: Vec::new(),
        }
    }
}
impl BuildProfileRegistry {
    fn validate(&self, roots: &[ProjectRoot]) -> Result<(), StorageError> {
        let root_ids = roots
            .iter()
            .map(|root| root.id.as_str())
            .collect::<std::collections::HashSet<_>>();
        let mut profile_ids = std::collections::HashSet::new();
        if self.schema_version != SCHEMA_VERSION
            || self.profiles.len() > MAX_BUILD_PROFILES
            || self.profiles.iter().any(|profile| {
                profile.validate().is_err()
                    || !root_ids.contains(profile.root_id.as_str())
                    || !profile_ids.insert(profile.profile_id.as_str())
            })
        {
            Err(StorageError::Invalid)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "mode", rename_all = "camelCase", deny_unknown_fields)]
pub enum ProjectBudgetOverride {
    Inherit,
    Disabled,
    Explicit { limits: BudgetLimits },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectBudgetPolicy {
    pub root_id: String,
    pub policy: ProjectBudgetOverride,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactBudgetPolicy {
    pub schema_version: u32,
    pub enabled: bool,
    pub global_limits: BudgetLimits,
    pub scheduled_analysis_interval_seconds: u64,
    pub stale_change_grace_seconds: u64,
    pub quarantine_grace_seconds: u64,
    pub project_overrides: Vec<ProjectBudgetPolicy>,
}

impl Default for ArtifactBudgetPolicy {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            enabled: false,
            global_limits: BudgetLimits::default(),
            scheduled_analysis_interval_seconds: 86_400,
            stale_change_grace_seconds: 86_400,
            quarantine_grace_seconds: 7 * 86_400,
            project_overrides: Vec::new(),
        }
    }
}

impl ArtifactBudgetPolicy {
    fn validate(&self, roots: &[ProjectRoot]) -> Result<(), StorageError> {
        let root_ids = roots
            .iter()
            .map(|root| root.id.as_str())
            .collect::<std::collections::HashSet<_>>();
        let mut overrides = std::collections::HashSet::new();
        if self.schema_version != SCHEMA_VERSION
            || !valid_limits(self.global_limits)
            || !(3_600..=7 * 86_400).contains(&self.scheduled_analysis_interval_seconds)
            || !(3_600..=30 * 86_400).contains(&self.stale_change_grace_seconds)
            || !(86_400..=30 * 86_400).contains(&self.quarantine_grace_seconds)
            || self.project_overrides.len() > MAX_PROJECT_ROOTS
            || self.project_overrides.iter().any(|entry| {
                !root_ids.contains(entry.root_id.as_str())
                    || !overrides.insert(entry.root_id.as_str())
                    || matches!(&entry.policy, ProjectBudgetOverride::Explicit { limits } if !valid_limits(*limits) || !limits.enabled())
            })
        {
            Err(StorageError::Invalid)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SuccessfulBuildStamp {
    pub profile_id: String,
    pub root_id: String,
    pub succeeded_at: u64,
    pub profile_label: String,
    pub toolchain_label: String,
    pub target_label: String,
    pub touched_generation_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactGenerationLedger {
    pub schema_version: u32,
    pub last_analysis_at: Option<u64>,
    #[serde(default)]
    pub last_successful_build: Option<SuccessfulBuildStamp>,
    pub generations: Vec<GenerationState>,
}

impl Default for ArtifactGenerationLedger {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            last_analysis_at: None,
            last_successful_build: None,
            generations: Vec::new(),
        }
    }
}
impl ArtifactGenerationLedger {
    fn validate(
        &self,
        roots: &[ProjectRoot],
        profiles: &[BuildProfile],
    ) -> Result<(), StorageError> {
        let roots = roots
            .iter()
            .map(|root| root.id.as_str())
            .collect::<std::collections::HashSet<_>>();
        let profiles = profiles
            .iter()
            .map(|profile| profile.profile_id.as_str())
            .collect::<std::collections::HashSet<_>>();
        let mut generation_ids = std::collections::HashSet::new();
        let mut per_profile = std::collections::HashMap::new();
        let mut per_project = std::collections::HashMap::new();
        let stamp_valid = self.last_successful_build.as_ref().is_none_or(|stamp| {
            profiles.contains(stamp.profile_id.as_str())
                && roots.contains(stamp.root_id.as_str())
                && valid_text(&stamp.profile_label, MAX_STRING_BYTES)
                && valid_text(&stamp.toolchain_label, MAX_STRING_BYTES)
                && valid_text(&stamp.target_label, MAX_STRING_BYTES)
                && stamp.touched_generation_ids.len() <= MAX_PROFILE_ARTIFACTS
                && {
                    let mut ids = std::collections::HashSet::new();
                    stamp
                        .touched_generation_ids
                        .iter()
                        .all(|id| valid_id(id) && ids.insert(id))
                }
        });
        if self.schema_version != SCHEMA_VERSION
            || self.generations.len() > MAX_GENERATIONS
            || !stamp_valid
        {
            return Err(StorageError::Invalid);
        }
        for generation in &self.generations {
            *per_profile
                .entry(generation.profile_id.as_str())
                .or_insert(0_usize) += 1;
            *per_project
                .entry(generation.root_id.as_str())
                .or_insert(0_usize) += 1;
            if !valid_id(&generation.generation_id)
                || !profiles.contains(generation.profile_id.as_str())
                || !roots.contains(generation.root_id.as_str())
                || !generation_ids.insert(generation.generation_id.as_str())
                || normalize_relative(&generation.normalized_path).is_none()
                || !valid_text(&generation.profile_label, MAX_STRING_BYTES)
                || !valid_text(&generation.toolchain_label, MAX_STRING_BYTES)
                || !valid_text(&generation.target_label, MAX_STRING_BYTES)
                || generation.active
                || per_profile[&generation.profile_id.as_str()] > MAX_GENERATIONS_PER_PROFILE
                || per_project[&generation.root_id.as_str()] > MAX_GENERATIONS_PER_PROJECT
            {
                return Err(StorageError::Invalid);
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageError {
    Io,
    Invalid,
    TooLarge,
    Exists,
}

#[derive(Clone, Debug)]
pub struct CleanupStorage {
    root: PathBuf,
    #[cfg(test)]
    write_failure_countdown: std::sync::Arc<std::sync::Mutex<Option<usize>>>,
}

impl CleanupStorage {
    pub fn open(root: PathBuf) -> Result<Self, StorageError> {
        fs::create_dir_all(root.join("plans")).map_err(|_| StorageError::Io)?;
        fs::create_dir_all(root.join("executions")).map_err(|_| StorageError::Io)?;
        fs::create_dir_all(root.join("quarantine")).map_err(|_| StorageError::Io)?;
        let root = fs::canonicalize(root).map_err(|_| StorageError::Io)?;
        Ok(Self {
            root,
            #[cfg(test)]
            write_failure_countdown: std::sync::Arc::new(std::sync::Mutex::new(None)),
        })
    }

    #[cfg(test)]
    pub(super) fn fail_write(&self, write_number: usize) {
        assert!(write_number > 0);
        *self.write_failure_countdown.lock().unwrap() = Some(write_number);
    }

    #[cfg(test)]
    fn check_write_failure(&self) -> Result<(), StorageError> {
        let mut countdown = self.write_failure_countdown.lock().unwrap();
        match countdown.as_mut() {
            Some(1) => {
                *countdown = None;
                Err(StorageError::Io)
            }
            Some(remaining) => {
                *remaining -= 1;
                Ok(())
            }
            None => Ok(()),
        }
    }

    pub fn create_plan(&self, plan: &CleanupPlan) -> Result<(), StorageError> {
        plan.validate()?;
        write_json(&self.id_path("plans", &plan.plan_id)?, plan, false)
    }

    pub fn read_plan(&self, plan_id: &str) -> Result<CleanupPlan, StorageError> {
        let plan: CleanupPlan = read_json(&self.id_path("plans", plan_id)?)?;
        plan.validate()?;
        if plan.plan_id != plan_id {
            return Err(StorageError::Invalid);
        }
        Ok(plan)
    }

    pub fn write_execution(&self, journal: &ExecutionJournal) -> Result<(), StorageError> {
        journal.validate()?;
        write_json(
            &self.id_path("executions", &journal.execution_id)?,
            journal,
            true,
        )
    }

    pub fn read_execution(&self, execution_id: &str) -> Result<ExecutionJournal, StorageError> {
        let journal: ExecutionJournal = read_json(&self.id_path("executions", execution_id)?)?;
        journal.validate()?;
        if journal.execution_id != execution_id {
            return Err(StorageError::Invalid);
        }
        Ok(journal)
    }

    pub fn reconcile(
        &self,
        plan: &CleanupPlan,
        journal: &mut ExecutionJournal,
    ) -> Result<(), StorageError> {
        plan.validate()?;
        journal.validate()?;
        if plan.plan_id != journal.plan_id || plan.items.len() != journal.items.len() {
            return Err(StorageError::Invalid);
        }
        for item in &mut journal.items {
            if item.state != ItemState::Mutating {
                continue;
            }
            let source = plan
                .items
                .iter()
                .find(|planned| planned.item_id == item.item_id)
                .ok_or(StorageError::Invalid)?
                .proof
                .path
                .as_path();
            item.state = if fs::symlink_metadata(source).is_ok() {
                ItemState::Pending
            } else if item
                .quarantine_path
                .as_ref()
                .is_some_and(|path| fs::symlink_metadata(path).is_ok())
            {
                ItemState::Quarantined
            } else {
                ItemState::Unknown
            };
        }
        self.write_execution(journal)
    }

    pub fn executions(&self) -> Result<Vec<ExecutionJournal>, StorageError> {
        let mut output = Vec::new();
        for entry in fs::read_dir(self.root.join("executions")).map_err(|_| StorageError::Io)? {
            let entry = entry.map_err(|_| StorageError::Io)?;
            if entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                let journal: ExecutionJournal = read_json(&entry.path())?;
                journal.validate()?;
                output.push(journal);
                if output.len() > MAX_ITEMS {
                    return Err(StorageError::TooLarge);
                }
            }
        }
        output.sort_by_key(|journal| std::cmp::Reverse(journal.started_at));
        Ok(output)
    }

    pub fn policy(&self) -> Result<AutoCleanupPolicy, StorageError> {
        let path = self.root.join("policy.json");
        if !path.exists() {
            return Ok(AutoCleanupPolicy::default());
        }
        let policy: AutoCleanupPolicy = read_json(&path)?;
        policy.validate()?;
        Ok(policy)
    }

    pub fn write_policy(&self, policy: &AutoCleanupPolicy) -> Result<(), StorageError> {
        policy.validate()?;
        write_json(&self.root.join("policy.json"), policy, true)
    }

    pub fn project_roots(&self) -> Result<Vec<ProjectRoot>, StorageError> {
        let path = self.root.join("project-roots.json");
        if !path.exists() {
            return Ok(Vec::new());
        }
        let registry: ProjectRootRegistry = read_json(&path)?;
        registry.validate()?;
        Ok(registry.roots)
    }

    pub fn write_project_roots(&self, roots: &[ProjectRoot]) -> Result<(), StorageError> {
        let registry = ProjectRootRegistry {
            schema_version: SCHEMA_VERSION,
            roots: roots.to_vec(),
        };
        registry.validate()?;
        let profile_path = self.root.join("build-profiles.json");
        if profile_path.exists() {
            let profiles: BuildProfileRegistry = read_json(&profile_path)?;
            profiles.validate(roots)?;
        }
        write_json(&self.root.join("project-roots.json"), &registry, true)
    }

    pub fn build_profiles(&self) -> Result<Vec<BuildProfile>, StorageError> {
        let path = self.root.join("build-profiles.json");
        if !path.exists() {
            return Ok(Vec::new());
        }
        let registry: BuildProfileRegistry = read_json(&path)?;
        registry.validate(&self.project_roots()?)?;
        Ok(registry.profiles)
    }

    pub fn write_build_profiles(&self, profiles: &[BuildProfile]) -> Result<(), StorageError> {
        let registry = BuildProfileRegistry {
            schema_version: SCHEMA_VERSION,
            profiles: profiles.to_vec(),
        };
        registry.validate(&self.project_roots()?)?;
        #[cfg(test)]
        self.check_write_failure()?;
        write_json(&self.root.join("build-profiles.json"), &registry, true)
    }

    pub fn artifact_budget_policy(&self) -> Result<ArtifactBudgetPolicy, StorageError> {
        let roots = self.project_roots()?;
        let path = self.root.join("artifact-budget-policy.json");
        if !path.exists() {
            return Ok(ArtifactBudgetPolicy::default());
        }
        let policy: ArtifactBudgetPolicy = read_json(&path)?;
        policy.validate(&roots)?;
        Ok(policy)
    }

    pub fn write_artifact_budget_policy(
        &self,
        policy: &ArtifactBudgetPolicy,
    ) -> Result<(), StorageError> {
        policy.validate(&self.project_roots()?)?;
        write_json(&self.root.join("artifact-budget-policy.json"), policy, true)
    }

    pub fn artifact_generations(&self) -> Result<ArtifactGenerationLedger, StorageError> {
        let path = self.root.join("artifact-generations.json");
        if !path.exists() {
            return Ok(ArtifactGenerationLedger::default());
        }
        let ledger: ArtifactGenerationLedger = read_json(&path)?;
        ledger.validate(&self.project_roots()?, &self.build_profiles()?)?;
        Ok(ledger)
    }

    pub fn write_artifact_generations(
        &self,
        ledger: &ArtifactGenerationLedger,
    ) -> Result<(), StorageError> {
        ledger.validate(&self.project_roots()?, &self.build_profiles()?)?;
        #[cfg(test)]
        self.check_write_failure()?;
        write_json(&self.root.join("artifact-generations.json"), ledger, true)
    }

    pub fn quarantine_directory(
        &self,
        execution_id: &str,
        item_id: &str,
    ) -> Result<PathBuf, StorageError> {
        if !valid_id(execution_id) || !valid_id(item_id) {
            return Err(StorageError::Invalid);
        }
        let directory = self.root.join("quarantine").join(execution_id);
        fs::create_dir_all(&directory).map_err(|_| StorageError::Io)?;
        Ok(directory.join(item_id))
    }

    fn id_path(&self, directory: &str, id: &str) -> Result<PathBuf, StorageError> {
        if !valid_id(id) {
            return Err(StorageError::Invalid);
        }
        Ok(self.root.join(directory).join(format!("{id}.json")))
    }
}

fn valid_id(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_text(value: &str, maximum_bytes: usize) -> bool {
    !value.is_empty() && value.len() <= maximum_bytes && !value.chars().any(char::is_control)
}

fn valid_absolute_path(value: &str) -> bool {
    value.len() <= MAX_PROJECT_PATH_BYTES
        && !value.chars().any(|character| character == '\0')
        && Path::new(value).is_absolute()
}

fn normalize_relative(value: &str) -> Option<String> {
    use std::path::Component;
    let path = Path::new(value);
    if value.is_empty() || value.len() > MAX_PROJECT_PATH_BYTES || path.is_absolute() {
        return None;
    }
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str()?.to_ascii_lowercase()),
            _ => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn valid_limits(limits: BudgetLimits) -> bool {
    limits.maximum_allocated_bytes != Some(0) && limits.maximum_age_seconds != Some(0)
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, StorageError> {
    let file = File::open(path).map_err(|_| StorageError::Io)?;
    if file.metadata().map_err(|_| StorageError::Io)?.len() > MAX_RECORD_BYTES {
        return Err(StorageError::TooLarge);
    }
    let mut bytes = Vec::new();
    file.take(MAX_RECORD_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| StorageError::Io)?;
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return Err(StorageError::TooLarge);
    }
    serde_json::from_slice(&bytes).map_err(|_| StorageError::Invalid)
}

fn write_json<T: Serialize>(path: &Path, value: &T, replace: bool) -> Result<(), StorageError> {
    let bytes = serde_json::to_vec(value).map_err(|_| StorageError::Invalid)?;
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return Err(StorageError::TooLarge);
    }
    if !replace && path.exists() {
        return Err(StorageError::Exists);
    }
    let parent = path.parent().ok_or(StorageError::Invalid)?;
    let mut nonce = [0_u8; 16];
    getrandom::fill(&mut nonce).map_err(|_| StorageError::Io)?;
    let temporary = parent.join(format!(
        ".tmp-{}",
        nonce
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|_| StorageError::Io)?;
    if file
        .write_all(&bytes)
        .and_then(|()| file.sync_all())
        .is_err()
    {
        let _ = fs::remove_file(&temporary);
        return Err(StorageError::Io);
    }
    let result = if replace {
        move_replace(&temporary, path)
    } else {
        fs::rename(&temporary, path).map_err(|_| StorageError::Io)
    };
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
        return result;
    }
    Ok(())
}

fn move_replace(source: &Path, destination: &Path) -> Result<(), StorageError> {
    let source = wide(source.as_os_str()).map_err(|_| StorageError::Invalid)?;
    let destination = wide(destination.as_os_str()).map_err(|_| StorageError::Invalid)?;
    // SAFETY: both paths are valid, NUL-terminated UTF-16 strings.
    if unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        Err(StorageError::Io)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "supa-diska-storage-{}-{}",
            std::process::id(),
            getrandom::u64().unwrap()
        ));
        fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn policy_is_atomic_persistent_and_bounded() {
        let root = temp();
        let storage = CleanupStorage::open(root.clone()).unwrap();
        assert_eq!(storage.policy().unwrap(), AutoCleanupPolicy::default());
        let policy = AutoCleanupPolicy {
            enabled: true,
            grace_days: 14,
            ..AutoCleanupPolicy::default()
        };
        storage.write_policy(&policy).unwrap();
        assert_eq!(
            CleanupStorage::open(root.clone())
                .unwrap()
                .policy()
                .unwrap(),
            policy
        );
        assert!(
            storage
                .write_policy(&AutoCleanupPolicy {
                    grace_days: 0,
                    ..policy
                })
                .is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn project_roots_are_atomic_persistent_and_bounded() {
        let root = temp();
        let storage = CleanupStorage::open(root.clone()).unwrap();
        assert!(storage.project_roots().unwrap().is_empty());
        let saved = ProjectRoot {
            id: "1".repeat(32),
            display_path: root.to_string_lossy().into_owned(),
            paused: false,
            added_at_unix_seconds: 10,
            last_scanned_at_unix_seconds: None,
        };
        storage
            .write_project_roots(std::slice::from_ref(&saved))
            .unwrap();
        assert_eq!(
            CleanupStorage::open(root.clone())
                .unwrap()
                .project_roots()
                .unwrap(),
            vec![saved.clone()]
        );
        assert!(
            storage
                .write_project_roots(&vec![saved; MAX_PROJECT_ROOTS + 1])
                .is_err()
        );
        fs::write(root.join("project-roots.json"), b"{bad").unwrap();
        assert_eq!(storage.project_roots(), Err(StorageError::Invalid));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn interrupted_mutations_reconcile_without_assuming_success() {
        use cleanup_core::{
            CatalogLimits, EntryKind, FileIdentity, ResolvedCandidate, load_catalog,
        };
        use std::{io::Cursor, time::SystemTime};

        let root = temp();
        let source = root.join("source");
        fs::create_dir(&source).unwrap();
        let quarantine = root.join("quarantined");
        let rule_json = r#"{"schemaVersion":1,"rules":[{"id":"cache","ruleVersion":1,"lifecycle":"stable","risk":"safe","provenance":{"source":"test","verifiedAt":"2026-08-30"},"defaultSelected":false,"scanner":"direct","roots":[{"binding":"temp","suffix":""}],"markers":{},"targets":["source"],"targetType":"directory","rootDepth":1}]}"#;
        let rule = load_catalog(Cursor::new(rule_json), CatalogLimits::default())
            .unwrap()
            .rules()[0]
            .clone();
        let item_id = "3".repeat(32);
        let plan = CleanupPlan::new(
            "1".repeat(32),
            "2".repeat(32),
            1,
            CleanupDisposition::Quarantine,
            vec![PlanItem {
                item_id: item_id.clone(),
                proof: ResolvedCandidate {
                    scope: CandidateProofScope::Temporary,
                    path: source.clone(),
                    scan_root: root.clone(),
                    context_root: root.clone(),
                    context_identity: None,
                    rule,
                    identity: FileIdentity { volume: 1, file: 1 },
                    kind: EntryKind::Directory,
                    logical_bytes: 0,
                    allocated_bytes: 0,
                    scanned_at: SystemTime::UNIX_EPOCH,
                },
            }],
        )
        .unwrap();
        let mut journal = ExecutionJournal {
            schema_version: 1,
            execution_id: "4".repeat(32),
            plan_id: plan.plan_id.clone(),
            started_at: 1,
            completed_at: None,
            disposition: CleanupDisposition::Quarantine,
            purge_after: None,
            items: vec![ExecutionItem {
                item_id,
                state: ItemState::Mutating,
                logical_bytes: 0,
                processed: false,
                occupied_bytes: 0,
                reclaimed_bytes: 0,
                quarantine_path: Some(quarantine.clone()),
                recycle_item: None,
                failure: None,
            }],
            accounting: ByteAccounting::default(),
        };
        let storage = CleanupStorage::open(root.join("records")).unwrap();
        storage.reconcile(&plan, &mut journal).unwrap();
        assert_eq!(journal.items[0].state, ItemState::Pending);

        fs::rename(&source, &quarantine).unwrap();
        journal.items[0].state = ItemState::Mutating;
        storage.reconcile(&plan, &mut journal).unwrap();
        assert_eq!(journal.items[0].state, ItemState::Quarantined);

        fs::remove_dir(&quarantine).unwrap();
        journal.items[0].state = ItemState::Mutating;
        storage.reconcile(&plan, &mut journal).unwrap();
        assert_eq!(journal.items[0].state, ItemState::Unknown);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn immutable_records_and_identifier_boundaries_fail_closed() {
        let root = temp();
        let storage = CleanupStorage::open(root.clone()).unwrap();
        assert_eq!(
            storage.read_plan("../escape").unwrap_err(),
            StorageError::Invalid
        );
        let plan = CleanupPlan::new(
            "1".repeat(32),
            "2".repeat(32),
            1,
            CleanupDisposition::RecycleBin,
            vec![],
        );
        assert_eq!(plan.unwrap_err(), StorageError::Invalid);
        fs::remove_dir_all(root).unwrap();
    }

    fn build_profile(root: &Path) -> BuildProfile {
        BuildProfile {
            profile_id: "a".repeat(32),
            root_id: "b".repeat(32),
            display_name: "Debug build".into(),
            ecosystem: BuildEcosystem::Rust,
            executable: root.join("cargo.exe").to_string_lossy().into_owned(),
            executable_identity: FileIdentity { volume: 1, file: 2 },
            argv: vec!["build".into(), "--profile".into(), "debug local".into()],
            working_directory: root.to_string_lossy().into_owned(),
            profile_label: "debug".into(),
            toolchain_label: "stable".into(),
            target_label: "x86_64-pc-windows-msvc".into(),
            rebuild_cost: RebuildCost::Low,
            artifact_paths: vec![RegisteredArtifactPath {
                relative_path: "target/debug".into(),
                role: ArtifactRole::Generation,
            }],
        }
    }

    #[test]
    fn artifact_records_round_trip_atomically_and_reject_stale_references() {
        let root = temp();
        let storage = CleanupStorage::open(root.clone()).unwrap();
        let project = ProjectRoot {
            id: "b".repeat(32),
            display_path: root.to_string_lossy().into_owned(),
            paused: false,
            added_at_unix_seconds: 1,
            last_scanned_at_unix_seconds: None,
        };
        storage
            .write_project_roots(std::slice::from_ref(&project))
            .unwrap();
        let profile = build_profile(&root);
        storage
            .write_build_profiles(std::slice::from_ref(&profile))
            .unwrap();
        assert_eq!(storage.build_profiles().unwrap(), vec![profile.clone()]);

        let mut policy = ArtifactBudgetPolicy {
            enabled: true,
            ..ArtifactBudgetPolicy::default()
        };
        policy.global_limits.maximum_allocated_bytes = Some(1024);
        policy.project_overrides.push(ProjectBudgetPolicy {
            root_id: project.id.clone(),
            policy: ProjectBudgetOverride::Explicit {
                limits: BudgetLimits {
                    maximum_allocated_bytes: None,
                    maximum_age_seconds: Some(86_400),
                },
            },
        });
        storage.write_artifact_budget_policy(&policy).unwrap();
        assert_eq!(storage.artifact_budget_policy().unwrap(), policy);

        let generation = GenerationState {
            generation_id: "c".repeat(32),
            profile_id: profile.profile_id.clone(),
            root_id: project.id.clone(),
            normalized_path: "target/debug".into(),
            allocated_bytes: 10,
            last_successful_touch: Some(2),
            last_external_change: None,
            profile_label: profile.profile_label.clone(),
            toolchain_label: profile.toolchain_label.clone(),
            target_label: profile.target_label.clone(),
            rebuild_cost: profile.rebuild_cost,
            owned: true,
            identity: Some(FileIdentity { volume: 1, file: 3 }),
            observed_modified_at_unix_nanos: Some(1),
            role: ArtifactRole::Generation,
            active: false,
            readable: true,
            ambiguous: false,
            touched_by_latest_success: true,
        };
        let ledger = ArtifactGenerationLedger {
            schema_version: SCHEMA_VERSION,
            last_analysis_at: Some(2),
            last_successful_build: None,
            generations: vec![generation],
        };
        storage.write_artifact_generations(&ledger).unwrap();
        assert_eq!(storage.artifact_generations().unwrap().generations.len(), 1);
        assert_eq!(storage.write_project_roots(&[]), Err(StorageError::Invalid));

        fs::write(
            root.join("build-profiles.json"),
            br#"{"schemaVersion":1,"profiles":[],"unexpected":true}"#,
        )
        .unwrap();
        assert_eq!(storage.build_profiles(), Err(StorageError::Invalid));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn artifact_budget_policy_requires_an_enabled_explicit_limit() {
        let root = temp();
        let storage = CleanupStorage::open(root.clone()).unwrap();
        let project = ProjectRoot {
            id: "b".repeat(32),
            display_path: root.to_string_lossy().into_owned(),
            paused: false,
            added_at_unix_seconds: 1,
            last_scanned_at_unix_seconds: None,
        };
        storage
            .write_project_roots(std::slice::from_ref(&project))
            .unwrap();

        let mut policy = ArtifactBudgetPolicy::default();
        policy.project_overrides.push(ProjectBudgetPolicy {
            root_id: project.id,
            policy: ProjectBudgetOverride::Explicit {
                limits: BudgetLimits::default(),
            },
        });
        assert_eq!(
            storage.write_artifact_budget_policy(&policy),
            Err(StorageError::Invalid)
        );

        policy.project_overrides[0].policy = ProjectBudgetOverride::Explicit {
            limits: BudgetLimits {
                maximum_allocated_bytes: Some(1024),
                maximum_age_seconds: None,
            },
        };
        storage.write_artifact_budget_policy(&policy).unwrap();

        policy.project_overrides[0].policy = ProjectBudgetOverride::Explicit {
            limits: BudgetLimits {
                maximum_allocated_bytes: None,
                maximum_age_seconds: Some(86_400),
            },
        };
        storage.write_artifact_budget_policy(&policy).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn artifact_records_reject_runtime_paths_duplicates_zero_limits_and_corruption() {
        let root = temp();
        let storage = CleanupStorage::open(root.clone()).unwrap();
        storage
            .write_project_roots(&[ProjectRoot {
                id: "b".repeat(32),
                display_path: root.to_string_lossy().into_owned(),
                paused: false,
                added_at_unix_seconds: 1,
                last_scanned_at_unix_seconds: None,
            }])
            .unwrap();
        let mut profile = build_profile(&root);
        profile.artifact_paths.push(RegisteredArtifactPath {
            relative_path: "target\\debug".into(),
            role: ArtifactRole::Generation,
        });
        assert_eq!(
            storage.write_build_profiles(&[profile]),
            Err(StorageError::Invalid)
        );
        let mut profile = build_profile(&root);
        profile.artifact_paths[0].relative_path = "../source".into();
        assert_eq!(
            storage.write_build_profiles(&[profile]),
            Err(StorageError::Invalid)
        );
        let mut policy = ArtifactBudgetPolicy::default();
        policy.global_limits.maximum_allocated_bytes = Some(0);
        assert_eq!(
            storage.write_artifact_budget_policy(&policy),
            Err(StorageError::Invalid)
        );
        fs::write(root.join("artifact-generations.json"), b"{bad").unwrap();
        assert_eq!(storage.artifact_generations(), Err(StorageError::Invalid));
        fs::remove_dir_all(root).unwrap();
    }
}
