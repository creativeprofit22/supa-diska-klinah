use crate::{EntryKind, FileIdentity, FileSystem, ReadDirControl};
use serde::{Deserialize, Serialize};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, HashMap, HashSet},
    path::{Component, Path},
    time::{SystemTime, UNIX_EPOCH},
};

pub const MAX_ARTIFACT_SNAPSHOT_ENTRIES: usize = 250_000;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ArtifactRole {
    Generation,
    Dependency,
    Incremental,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RebuildCost {
    Low,
    Medium,
    High,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GenerationState {
    pub generation_id: String,
    pub profile_id: String,
    pub root_id: String,
    pub normalized_path: String,
    pub allocated_bytes: u64,
    pub last_successful_touch: Option<u64>,
    pub last_external_change: Option<u64>,
    pub profile_label: String,
    pub toolchain_label: String,
    pub target_label: String,
    pub rebuild_cost: RebuildCost,
    pub owned: bool,
    pub identity: Option<FileIdentity>,
    #[serde(default)]
    pub observed_modified_at_unix_nanos: Option<u64>,
    pub role: ArtifactRole,
    pub active: bool,
    pub readable: bool,
    pub ambiguous: bool,
    pub touched_by_latest_success: bool,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BudgetLimits {
    pub maximum_allocated_bytes: Option<u64>,
    pub maximum_age_seconds: Option<u64>,
}

impl BudgetLimits {
    pub fn enabled(self) -> bool {
        self.maximum_allocated_bytes.is_some() || self.maximum_age_seconds.is_some()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtectedArtifactPath {
    pub root_id: String,
    pub normalized_path: String,
    pub role: ArtifactRole,
}

#[derive(Clone, Debug, Default)]
pub struct BudgetContext {
    pub global_limits: BudgetLimits,
    pub project_limits: HashMap<String, BudgetLimits>,
    pub protected_artifact_paths: Vec<ProtectedArtifactPath>,
    pub current_successful_profile_id: Option<String>,
    pub current_toolchain_label: Option<String>,
    pub current_target_label: Option<String>,
    pub stale_grace_seconds: u64,
    pub now: u64,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProtectionReason {
    Unowned,
    NonGeneration,
    Active,
    CurrentProfile,
    CurrentTarget,
    Dependency,
    Incremental,
    LatestSuccessfulBuild,
    RecentExternalChange,
    Unreadable,
    Ambiguous,
    MissingIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtectedGeneration {
    pub generation_id: String,
    pub root_id: String,
    pub allocated_bytes: u64,
    pub reasons: Vec<ProtectionReason>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetDecision {
    pub selected_generation_ids: Vec<String>,
    pub protected: Vec<ProtectedGeneration>,
    pub current_allocated_bytes: u64,
    pub projected_allocated_bytes: u64,
    pub quarantine_bytes: u64,
    pub unsatisfied_protected_byte_floor: Option<u64>,
    pub project_current_bytes: BTreeMap<String, u64>,
    pub project_projected_bytes: BTreeMap<String, u64>,
    pub project_protected_byte_floors: BTreeMap<String, u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BudgetError {
    Overflow,
    InvalidLimit,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactSnapshot {
    pub normalized_relative_path: String,
    pub identity: FileIdentity,
    pub kind: EntryKind,
    pub allocated_bytes: u64,
    pub modified_at_unix_nanos: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotError {
    InvalidPath,
    OutsideRoot,
    LinkLike,
    Unreadable,
    MissingIdentity,
    LimitReached,
    Overflow,
}

pub fn snapshot_registered_path(
    fs: &dyn FileSystem,
    root: &Path,
    relative_path: &Path,
) -> Result<ArtifactSnapshot, SnapshotError> {
    let normalized_relative_path = normalize_relative_path(relative_path)?;
    let root_metadata = fs
        .metadata_no_follow(root)
        .map_err(|_| SnapshotError::Unreadable)?;
    if root_metadata.kind != EntryKind::Directory {
        return Err(SnapshotError::InvalidPath);
    }
    if root_metadata.identity.is_none() {
        return Err(SnapshotError::MissingIdentity);
    }
    let candidate = root.join(relative_path);
    let semantics = fs.semantics();
    if !root.is_absolute()
        || !semantics.contains(root, &candidate)
        || semantics.equivalent(root, &candidate)
    {
        return Err(SnapshotError::OutsideRoot);
    }
    let canonical_root = fs
        .canonicalize(root)
        .map_err(|_| SnapshotError::Unreadable)?;
    let canonical_candidate = fs
        .canonicalize(&candidate)
        .map_err(|_| SnapshotError::Unreadable)?;
    if !semantics.equivalent(&canonical_root, root)
        || !semantics.equivalent(&canonical_candidate, &candidate)
        || !semantics.contains(&canonical_root, &canonical_candidate)
    {
        return Err(SnapshotError::OutsideRoot);
    }
    let metadata = fs
        .metadata_no_follow(&candidate)
        .map_err(|_| SnapshotError::Unreadable)?;
    if metadata.kind == EntryKind::LinkLike {
        return Err(SnapshotError::LinkLike);
    }
    let identity = metadata.identity.ok_or(SnapshotError::MissingIdentity)?;
    let mut allocated_bytes = fs
        .allocated_size(&candidate, &metadata)
        .map_err(|_| SnapshotError::Unreadable)?;
    let mut modified_at_unix_nanos = system_time_nanos(metadata.modified);
    if metadata.kind == EntryKind::Directory {
        let mut directories = vec![(candidate.clone(), identity)];
        let mut visited = 0_usize;
        while let Some((directory, directory_identity)) = directories.pop() {
            let mut failure = None;
            fs.read_dir(&directory, directory_identity, &mut |entry| {
                visited = visited.saturating_add(1);
                if visited > MAX_ARTIFACT_SNAPSHOT_ENTRIES {
                    failure = Some(SnapshotError::LimitReached);
                    return ReadDirControl::Stop;
                }
                let child = match fs.metadata_no_follow(&entry.path) {
                    Ok(value) => value,
                    Err(_) => {
                        failure = Some(SnapshotError::Unreadable);
                        return ReadDirControl::Stop;
                    }
                };
                if child.kind == EntryKind::LinkLike || child.kind != entry.kind {
                    failure = Some(SnapshotError::LinkLike);
                    return ReadDirControl::Stop;
                }
                let Some(child_identity) = child.identity else {
                    failure = Some(SnapshotError::MissingIdentity);
                    return ReadDirControl::Stop;
                };
                let child_bytes = match fs.allocated_size(&entry.path, &child) {
                    Ok(value) => value,
                    Err(_) => {
                        failure = Some(SnapshotError::Unreadable);
                        return ReadDirControl::Stop;
                    }
                };
                let Some(total) = allocated_bytes.checked_add(child_bytes) else {
                    failure = Some(SnapshotError::Overflow);
                    return ReadDirControl::Stop;
                };
                allocated_bytes = total;
                modified_at_unix_nanos =
                    modified_at_unix_nanos.max(system_time_nanos(child.modified));
                if child.kind == EntryKind::Directory {
                    directories.push((entry.path, child_identity));
                }
                ReadDirControl::Continue
            })
            .map_err(|_| SnapshotError::Unreadable)?;
            if let Some(error) = failure {
                return Err(error);
            }
        }
    }
    Ok(ArtifactSnapshot {
        normalized_relative_path,
        identity,
        kind: metadata.kind,
        allocated_bytes,
        modified_at_unix_nanos,
    })
}

pub fn select_artifact_generations(
    generations: &[GenerationState],
    context: &BudgetContext,
) -> Result<BudgetDecision, BudgetError> {
    validate_limits(context.global_limits)?;
    for limits in context.project_limits.values().copied() {
        validate_limits(limits)?;
    }

    let mut current_allocated_bytes = 0_u64;
    let mut project_current_bytes = BTreeMap::new();
    let mut protected = Vec::new();
    let mut eligible = Vec::new();
    for generation in generations {
        current_allocated_bytes = checked_add(current_allocated_bytes, generation.allocated_bytes)?;
        checked_map_add(
            &mut project_current_bytes,
            &generation.root_id,
            generation.allocated_bytes,
        )?;
        let reasons = protection_reasons(generation, generations, context);
        if reasons.is_empty() {
            eligible.push(generation);
        } else {
            protected.push(ProtectedGeneration {
                generation_id: generation.generation_id.clone(),
                root_id: generation.root_id.clone(),
                allocated_bytes: generation.allocated_bytes,
                reasons,
            });
        }
    }
    protected.sort_by(|left, right| left.generation_id.cmp(&right.generation_id));

    eligible.sort_by(|left, right| rank(left, right, context));
    let mut selected = HashSet::new();
    for generation in &eligible {
        let limits = context
            .project_limits
            .get(&generation.root_id)
            .copied()
            .unwrap_or_default();
        let age_limit = limits
            .maximum_age_seconds
            .or(context.global_limits.maximum_age_seconds);
        if age_limit.is_some_and(|limit| {
            generation
                .last_successful_touch
                .is_some_and(|touch| context.now.saturating_sub(touch) > limit)
        }) {
            selected.insert(generation.generation_id.as_str());
        }
    }

    let mut project_projected_bytes = project_current_bytes.clone();
    subtract_selected(&mut project_projected_bytes, &eligible, &selected)?;
    let mut roots = context.project_limits.keys().collect::<Vec<_>>();
    roots.sort_unstable();
    for root_id in roots {
        let Some(limit) = context.project_limits[root_id].maximum_allocated_bytes else {
            continue;
        };
        for generation in eligible.iter().filter(|item| &item.root_id == root_id) {
            if project_projected_bytes
                .get(root_id)
                .copied()
                .unwrap_or_default()
                <= limit
            {
                break;
            }
            if selected.insert(generation.generation_id.as_str()) {
                checked_map_sub(
                    &mut project_projected_bytes,
                    root_id,
                    generation.allocated_bytes,
                )?;
            }
        }
    }

    let mut projected_allocated_bytes = current_allocated_bytes;
    for generation in &eligible {
        if selected.contains(generation.generation_id.as_str()) {
            projected_allocated_bytes = projected_allocated_bytes
                .checked_sub(generation.allocated_bytes)
                .ok_or(BudgetError::Overflow)?;
        }
    }
    if let Some(limit) = context.global_limits.maximum_allocated_bytes {
        for generation in &eligible {
            if projected_allocated_bytes <= limit {
                break;
            }
            if selected.insert(generation.generation_id.as_str()) {
                projected_allocated_bytes = projected_allocated_bytes
                    .checked_sub(generation.allocated_bytes)
                    .ok_or(BudgetError::Overflow)?;
                checked_map_sub(
                    &mut project_projected_bytes,
                    &generation.root_id,
                    generation.allocated_bytes,
                )?;
            }
        }
    }

    let selected_generation_ids = eligible
        .iter()
        .filter(|generation| selected.contains(generation.generation_id.as_str()))
        .map(|generation| generation.generation_id.clone())
        .collect::<Vec<_>>();
    let quarantine_bytes = current_allocated_bytes
        .checked_sub(projected_allocated_bytes)
        .ok_or(BudgetError::Overflow)?;
    let mut project_protected_byte_floors = BTreeMap::new();
    for row in &protected {
        checked_map_add(
            &mut project_protected_byte_floors,
            &row.root_id,
            row.allocated_bytes,
        )?;
    }
    let protected_floor = protected.iter().try_fold(0_u64, |total, row| {
        total
            .checked_add(row.allocated_bytes)
            .ok_or(BudgetError::Overflow)
    })?;
    let global_unsatisfied = context
        .global_limits
        .maximum_allocated_bytes
        .filter(|limit| projected_allocated_bytes > *limit)
        .map(|_| protected_floor);

    Ok(BudgetDecision {
        selected_generation_ids,
        protected,
        current_allocated_bytes,
        projected_allocated_bytes,
        quarantine_bytes,
        unsatisfied_protected_byte_floor: global_unsatisfied,
        project_current_bytes,
        project_projected_bytes,
        project_protected_byte_floors,
    })
}

fn protection_reasons(
    generation: &GenerationState,
    generations: &[GenerationState],
    context: &BudgetContext,
) -> Vec<ProtectionReason> {
    let mut reasons = Vec::new();
    if !generation.owned {
        reasons.push(ProtectionReason::Unowned);
    }
    match generation.role {
        ArtifactRole::Generation => {}
        ArtifactRole::Dependency => reasons.push(ProtectionReason::Dependency),
        ArtifactRole::Incremental => reasons.push(ProtectionReason::Incremental),
    }
    if generation.role == ArtifactRole::Generation {
        let protected_roles = generations
            .iter()
            .map(|protected| {
                (
                    protected.root_id.as_str(),
                    protected.normalized_path.as_str(),
                    protected.role,
                )
            })
            .chain(context.protected_artifact_paths.iter().map(|protected| {
                (
                    protected.root_id.as_str(),
                    protected.normalized_path.as_str(),
                    protected.role,
                )
            }));
        for (_, _, role) in protected_roles.filter(|(root_id, path, role)| {
            *root_id == generation.root_id
                && matches!(role, ArtifactRole::Dependency | ArtifactRole::Incremental)
                && Path::new(path).starts_with(Path::new(&generation.normalized_path))
        }) {
            reasons.push(match role {
                ArtifactRole::Dependency => ProtectionReason::Dependency,
                ArtifactRole::Incremental => ProtectionReason::Incremental,
                ArtifactRole::Generation => unreachable!(),
            });
        }
    }
    if generation.role != ArtifactRole::Generation {
        reasons.push(ProtectionReason::NonGeneration);
    }
    if generation.active {
        reasons.push(ProtectionReason::Active);
    }
    if context
        .current_successful_profile_id
        .as_deref()
        .is_some_and(|value| value == generation.profile_id)
    {
        reasons.push(ProtectionReason::CurrentProfile);
    }
    if context
        .current_target_label
        .as_deref()
        .is_some_and(|value| value == generation.target_label)
    {
        reasons.push(ProtectionReason::CurrentTarget);
    }
    if generation.touched_by_latest_success {
        reasons.push(ProtectionReason::LatestSuccessfulBuild);
    }
    if generation
        .last_external_change
        .is_some_and(|changed| context.now.saturating_sub(changed) < context.stale_grace_seconds)
    {
        reasons.push(ProtectionReason::RecentExternalChange);
    }
    if !generation.readable {
        reasons.push(ProtectionReason::Unreadable);
    }
    if generation.ambiguous {
        reasons.push(ProtectionReason::Ambiguous);
    }
    if generation.identity.is_none() {
        reasons.push(ProtectionReason::MissingIdentity);
    }
    reasons.sort_unstable();
    reasons.dedup();
    reasons
}

fn rank(left: &GenerationState, right: &GenerationState, context: &BudgetContext) -> Ordering {
    left.last_successful_touch
        .unwrap_or(0)
        .cmp(&right.last_successful_touch.unwrap_or(0))
        .then_with(|| {
            let left_current = context
                .current_toolchain_label
                .as_deref()
                .is_some_and(|value| value == left.toolchain_label)
                || context
                    .current_successful_profile_id
                    .as_deref()
                    .is_some_and(|value| value == left.profile_id);
            let right_current = context
                .current_toolchain_label
                .as_deref()
                .is_some_and(|value| value == right.toolchain_label)
                || context
                    .current_successful_profile_id
                    .as_deref()
                    .is_some_and(|value| value == right.profile_id);
            left_current.cmp(&right_current)
        })
        .then_with(|| right.allocated_bytes.cmp(&left.allocated_bytes))
        .then_with(|| left.rebuild_cost.cmp(&right.rebuild_cost))
        .then_with(|| left.normalized_path.cmp(&right.normalized_path))
        .then_with(|| left.generation_id.cmp(&right.generation_id))
}

fn validate_limits(limits: BudgetLimits) -> Result<(), BudgetError> {
    if limits.maximum_allocated_bytes == Some(0) || limits.maximum_age_seconds == Some(0) {
        Err(BudgetError::InvalidLimit)
    } else {
        Ok(())
    }
}

fn normalize_relative_path(path: &Path) -> Result<String, SnapshotError> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err(SnapshotError::InvalidPath);
    }
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => parts.push(
                part.to_str()
                    .filter(|value| !value.is_empty())
                    .ok_or(SnapshotError::InvalidPath)?,
            ),
            _ => return Err(SnapshotError::InvalidPath),
        }
    }
    if parts.is_empty() {
        Err(SnapshotError::InvalidPath)
    } else {
        Ok(parts.join("/"))
    }
}

fn system_time_nanos(value: Option<SystemTime>) -> Option<u64> {
    value.and_then(|time| {
        time.duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|value| u64::try_from(value.as_nanos()).ok())
    })
}

fn checked_add(left: u64, right: u64) -> Result<u64, BudgetError> {
    left.checked_add(right).ok_or(BudgetError::Overflow)
}

fn checked_map_add(
    values: &mut BTreeMap<String, u64>,
    key: &str,
    amount: u64,
) -> Result<(), BudgetError> {
    let value = checked_add(values.get(key).copied().unwrap_or_default(), amount)?;
    values.insert(key.to_owned(), value);
    Ok(())
}

fn checked_map_sub(
    values: &mut BTreeMap<String, u64>,
    key: &str,
    amount: u64,
) -> Result<(), BudgetError> {
    let value = values
        .get(key)
        .copied()
        .unwrap_or_default()
        .checked_sub(amount)
        .ok_or(BudgetError::Overflow)?;
    values.insert(key.to_owned(), value);
    Ok(())
}

fn subtract_selected(
    totals: &mut BTreeMap<String, u64>,
    eligible: &[&GenerationState],
    selected: &HashSet<&str>,
) -> Result<(), BudgetError> {
    for generation in eligible {
        if selected.contains(generation.generation_id.as_str()) {
            checked_map_sub(totals, &generation.root_id, generation.allocated_bytes)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generation(id: &str, root: &str, age: u64, bytes: u64) -> GenerationState {
        GenerationState {
            generation_id: id.into(),
            profile_id: format!("profile-{id}"),
            root_id: root.into(),
            normalized_path: id.into(),
            allocated_bytes: bytes,
            last_successful_touch: Some(age),
            last_external_change: None,
            profile_label: id.into(),
            toolchain_label: id.into(),
            target_label: id.into(),
            rebuild_cost: RebuildCost::Medium,
            owned: true,
            identity: Some(FileIdentity {
                volume: 1,
                file: bytes,
            }),
            observed_modified_at_unix_nanos: Some(1),
            role: ArtifactRole::Generation,
            active: false,
            readable: true,
            ambiguous: false,
            touched_by_latest_success: false,
        }
    }

    fn context() -> BudgetContext {
        BudgetContext {
            now: 1_000,
            stale_grace_seconds: 100,
            ..BudgetContext::default()
        }
    }

    #[test]
    fn age_only_selects_every_expired_generation() {
        let mut context = context();
        context.global_limits.maximum_age_seconds = Some(100);
        let decision = select_artifact_generations(
            &[
                generation("old", "a", 800, 5),
                generation("new", "a", 950, 5),
            ],
            &context,
        )
        .unwrap();
        assert_eq!(decision.selected_generation_ids, ["old"]);
    }

    #[test]
    fn project_then_global_size_selects_minimum_ranked_set() {
        let mut context = context();
        context.project_limits.insert(
            "a".into(),
            BudgetLimits {
                maximum_allocated_bytes: Some(10),
                maximum_age_seconds: None,
            },
        );
        context.global_limits.maximum_allocated_bytes = Some(15);
        let decision = select_artifact_generations(
            &[
                generation("old-a", "a", 100, 10),
                generation("new-a", "a", 200, 10),
                generation("only-b", "b", 300, 10),
            ],
            &context,
        )
        .unwrap();
        assert_eq!(decision.selected_generation_ids, ["old-a", "new-a"]);
        assert_eq!(decision.projected_allocated_bytes, 10);
    }

    #[test]
    fn deterministic_ties_prefer_larger_then_lower_rebuild_cost() {
        let mut context = context();
        context.global_limits.maximum_allocated_bytes = Some(10);
        let mut low = generation("low", "a", 100, 10);
        low.rebuild_cost = RebuildCost::Low;
        let mut high = generation("high", "a", 100, 10);
        high.rebuild_cost = RebuildCost::High;
        let large = generation("large", "a", 100, 20);
        let decision = select_artifact_generations(&[low, high, large], &context).unwrap();
        assert_eq!(decision.selected_generation_ids, ["large", "low"]);
    }

    #[test]
    fn disabled_limits_select_nothing() {
        let decision =
            select_artifact_generations(&[generation("a", "a", 1, 10)], &context()).unwrap();
        assert!(decision.selected_generation_ids.is_empty());
    }

    #[test]
    fn protected_floor_is_reported_without_broadening_eligibility() {
        let mut context = context();
        context.global_limits.maximum_allocated_bytes = Some(1);
        let mut protected = generation("current", "a", 1, 10);
        protected.touched_by_latest_success = true;
        let decision = select_artifact_generations(&[protected], &context).unwrap();
        assert!(decision.selected_generation_ids.is_empty());
        assert_eq!(decision.unsatisfied_protected_byte_floor, Some(10));
    }

    #[test]
    fn overflow_is_rejected() {
        let result = select_artifact_generations(
            &[
                generation("a", "a", 1, u64::MAX),
                generation("b", "a", 1, 1),
            ],
            &context(),
        );
        assert_eq!(result, Err(BudgetError::Overflow));
    }

    #[test]
    fn dependencies_incremental_and_recent_external_changes_are_protected() {
        let context = context();
        let mut dependency = generation("dep", "a", 1, 1);
        dependency.role = ArtifactRole::Dependency;
        let mut incremental = generation("inc", "a", 1, 1);
        incremental.role = ArtifactRole::Incremental;
        let mut external = generation("external", "a", 1, 1);
        external.last_external_change = Some(950);
        let decision =
            select_artifact_generations(&[dependency, incremental, external], &context).unwrap();
        assert_eq!(decision.protected.len(), 3);
        assert!(decision.selected_generation_ids.is_empty());
    }

    #[test]
    fn generation_containing_protected_build_state_is_not_selected() {
        let mut context = context();
        context.global_limits.maximum_allocated_bytes = Some(1);
        let mut parent = generation("parent", "a", 1, 30);
        parent.normalized_path = "target/debug".into();
        let mut dependency = generation("dependency", "a", 1, 10);
        dependency.normalized_path = "target/debug/deps".into();
        dependency.role = ArtifactRole::Dependency;
        let mut incremental = generation("incremental", "a", 1, 10);
        incremental.normalized_path = "target/debug/incremental".into();
        incremental.role = ArtifactRole::Incremental;

        let decision =
            select_artifact_generations(&[parent, dependency, incremental], &context).unwrap();

        assert!(decision.selected_generation_ids.is_empty());
        let parent = decision
            .protected
            .iter()
            .find(|item| item.generation_id == "parent")
            .unwrap();
        assert!(parent.reasons.contains(&ProtectionReason::Dependency));
        assert!(parent.reasons.contains(&ProtectionReason::Incremental));
    }

    #[test]
    fn unobserved_registered_build_state_also_protects_its_parent() {
        let mut context = context();
        context.global_limits.maximum_allocated_bytes = Some(1);
        context.protected_artifact_paths = vec![
            ProtectedArtifactPath {
                root_id: "a".into(),
                normalized_path: "target/debug/deps".into(),
                role: ArtifactRole::Dependency,
            },
            ProtectedArtifactPath {
                root_id: "a".into(),
                normalized_path: "target/debug/incremental".into(),
                role: ArtifactRole::Incremental,
            },
        ];
        let mut parent = generation("parent", "a", 1, 30);
        parent.normalized_path = "target/debug".into();

        let decision = select_artifact_generations(&[parent], &context).unwrap();

        assert!(decision.selected_generation_ids.is_empty());
        assert_eq!(
            decision.protected[0].reasons,
            vec![ProtectionReason::Dependency, ProtectionReason::Incremental]
        );
    }
    #[test]
    fn current_owned_and_uncertain_states_never_become_budget_candidates() {
        let mut context = context();
        context.global_limits.maximum_allocated_bytes = Some(1);
        context.current_successful_profile_id = Some("current-profile".into());
        context.current_target_label = Some("current-target".into());
        let mut current_profile = generation("profile", "a", 1, 10);
        current_profile.profile_id = "current-profile".into();
        let mut current_target = generation("target", "a", 1, 10);
        current_target.target_label = "current-target".into();
        let mut latest = generation("latest", "a", 1, 10);
        latest.touched_by_latest_success = true;
        let mut unowned = generation("unowned", "a", 1, 10);
        unowned.owned = false;
        let mut active = generation("active", "a", 1, 10);
        active.active = true;
        let mut unreadable = generation("unreadable", "a", 1, 10);
        unreadable.readable = false;
        let mut ambiguous = generation("ambiguous", "a", 1, 10);
        ambiguous.ambiguous = true;
        let mut missing_identity = generation("missing", "a", 1, 10);
        missing_identity.identity = None;
        let decision = select_artifact_generations(
            &[
                current_profile,
                current_target,
                latest,
                unowned,
                active,
                unreadable,
                ambiguous,
                missing_identity,
            ],
            &context,
        )
        .unwrap();
        assert_eq!(decision.protected.len(), 8);
        assert!(decision.selected_generation_ids.is_empty());
        assert_eq!(decision.unsatisfied_protected_byte_floor, Some(80));
    }
}
