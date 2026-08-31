use crate::{
    ArtifactIntelligence, CleanupRule, DirectoryEntry, Entropy, EntryKind, EntryMetadata,
    FileIdentity, FileSystem, FsError, ProtectionPolicy, ReadDirControl, Risk, RuleCatalog,
    ScannerKind,
    scanner::{CandidateDraft, ScannerRegistry, TraversalContext},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::{Component, Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Default)]
pub struct CancellationToken(Arc<AtomicBool>);
impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ScanPhase {
    Discovering,
    Measuring,
    Finalizing,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    pub phase: ScanPhase,
    pub visited_entries: usize,
    pub candidates: usize,
    pub completed_jobs: usize,
    pub total_jobs: usize,
}

pub trait ProgressSink: Send + Sync {
    fn report(&self, event: ProgressEvent);
}
impl<F: Fn(ProgressEvent) + Send + Sync> ProgressSink for F {
    fn report(&self, event: ProgressEvent) {
        self(event);
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ScanLimits {
    pub max_workers: usize,
    pub max_visited_entries: usize,
    pub max_candidates: usize,
    pub max_diagnostics: usize,
    pub max_measurement_entries: usize,
}
impl Default for ScanLimits {
    fn default() -> Self {
        Self {
            max_workers: 4,
            max_visited_entries: 1_000_000,
            max_candidates: 100_000,
            max_diagnostics: 1_000,
            max_measurement_entries: 1_000_000,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewRecord {
    pub id: String,
    pub rule_id: String,
    pub display_path: String,
    pub kind: PreviewKind,
    pub bytes: u64,
    pub modified_unix_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact: Option<ArtifactIntelligence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk: Option<Risk>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_selected: Option<bool>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PreviewKind {
    File,
    Directory,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolvedCandidate {
    pub path: PathBuf,
    pub scan_root: PathBuf,
    pub context_root: PathBuf,
    pub rule: CleanupRule,
    pub identity: FileIdentity,
    pub kind: EntryKind,
    pub logical_bytes: u64,
    pub allocated_bytes: u64,
    pub scanned_at: SystemTime,
}
#[derive(Debug)]
pub struct ScanSnapshot {
    scan_id: String,
    records: Vec<PreviewRecord>,
    resolved: HashMap<String, ResolvedCandidate>,
}
impl ScanSnapshot {
    pub fn scan_id(&self) -> &str {
        &self.scan_id
    }
    pub fn records(&self) -> &[PreviewRecord] {
        &self.records
    }
    pub fn resolve(&self, id: &str) -> Option<&ResolvedCandidate> {
        self.resolved.get(id)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValidatedCandidate {
    pub logical_bytes: u64,
    pub allocated_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CandidateRejection {
    InvalidProof,
    OutsideRoot,
    LinkLike,
    TypeChanged,
    IdentityChanged,
    RuleMismatch,
    MarkerMissing,
    TooRecent,
    Protected,
    Active,
    Unreadable,
    LimitReached,
}

pub fn revalidate_candidate(
    fs: &dyn FileSystem,
    candidate: &ResolvedCandidate,
    protection: &ProtectionPolicy,
    now: SystemTime,
) -> Result<ValidatedCandidate, CandidateRejection> {
    let semantics = fs.semantics();
    if !candidate.path.is_absolute()
        || !candidate.scan_root.is_absolute()
        || !candidate.context_root.is_absolute()
        || semantics.equivalent(&candidate.scan_root, &candidate.path)
        || semantics.equivalent(&candidate.context_root, &candidate.path)
        || !semantics.contains(&candidate.scan_root, &candidate.path)
        || !semantics.contains(&candidate.context_root, &candidate.path)
    {
        return Err(CandidateRejection::OutsideRoot);
    }
    if protection.is_protected(&candidate.path) {
        return Err(CandidateRejection::Protected);
    }
    let scan_metadata = fs
        .metadata_no_follow(&candidate.scan_root)
        .map_err(|_| CandidateRejection::Unreadable)?;
    if scan_metadata.kind == EntryKind::LinkLike {
        return Err(CandidateRejection::LinkLike);
    }
    let relative = candidate
        .path
        .strip_prefix(&candidate.scan_root)
        .map_err(|_| CandidateRejection::OutsideRoot)?;
    let mut component_path = candidate.scan_root.clone();
    for component in relative.components() {
        component_path.push(component);
        let metadata = fs
            .metadata_no_follow(&component_path)
            .map_err(|_| CandidateRejection::Unreadable)?;
        if metadata.kind == EntryKind::LinkLike {
            return Err(CandidateRejection::LinkLike);
        }
    }
    let metadata = fs
        .metadata_no_follow(&candidate.path)
        .map_err(|_| CandidateRejection::Unreadable)?;
    if metadata.kind != candidate.kind {
        return Err(CandidateRejection::TypeChanged);
    }
    if metadata.identity != Some(candidate.identity) {
        return Err(CandidateRejection::IdentityChanged);
    }
    let canonical = fs
        .canonicalize(&candidate.path)
        .map_err(|_| CandidateRejection::Unreadable)?;
    if !semantics.equivalent(&canonical, &candidate.path)
        || protection.is_protected(&canonical)
        || !semantics.contains(&candidate.scan_root, &canonical)
        || !semantics.contains(&candidate.context_root, &canonical)
    {
        return Err(if protection.is_protected(&canonical) {
            CandidateRejection::Protected
        } else {
            CandidateRejection::OutsideRoot
        });
    }
    let name = candidate
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(CandidateRejection::RuleMismatch)?;
    if !crate::scanner::target_matches(&candidate.rule, name, metadata.kind)
        || crate::scanner::excluded(
            &candidate.rule,
            &candidate.scan_root,
            &candidate.path,
            semantics,
        )
    {
        return Err(CandidateRejection::RuleMismatch);
    }
    if !crate::scanner::old_enough(&candidate.rule, &metadata, now) {
        return Err(CandidateRejection::TooRecent);
    }
    if candidate.rule.scanner == crate::ScannerKind::ProjectArtifacts {
        let context_metadata = fs
            .metadata_no_follow(&candidate.context_root)
            .map_err(|_| CandidateRejection::MarkerMissing)?;
        let context_identity = context_metadata
            .identity
            .ok_or(CandidateRejection::MarkerMissing)?;
        if context_metadata.kind != EntryKind::Directory {
            return Err(CandidateRejection::MarkerMissing);
        }
        let mut names = HashSet::new();
        fs.read_dir(&candidate.context_root, context_identity, &mut |entry| {
            if entry.kind != EntryKind::LinkLike {
                names.insert(entry.name.to_ascii_lowercase());
            }
            ReadDirControl::Continue
        })
        .map_err(|_| CandidateRejection::MarkerMissing)?;
        if !candidate
            .rule
            .markers
            .all
            .iter()
            .all(|name| names.contains(&name.to_ascii_lowercase()))
            || (!candidate.rule.markers.any.is_empty()
                && !candidate
                    .rule
                    .markers
                    .any
                    .iter()
                    .any(|name| names.contains(&name.to_ascii_lowercase())))
        {
            return Err(CandidateRejection::MarkerMissing);
        }
    }
    fs.ensure_inactive(&candidate.path)
        .map_err(|_| CandidateRejection::Active)?;
    measure_candidate(fs, candidate)
}

fn measure_candidate(
    fs: &dyn FileSystem,
    candidate: &ResolvedCandidate,
) -> Result<ValidatedCandidate, CandidateRejection> {
    let metadata = fs
        .metadata_no_follow(&candidate.path)
        .map_err(|_| CandidateRejection::Unreadable)?;
    let mut result = ValidatedCandidate {
        logical_bytes: if metadata.kind == EntryKind::File {
            metadata.size
        } else {
            0
        },
        allocated_bytes: fs
            .allocated_size(&candidate.path, &metadata)
            .map_err(|_| CandidateRejection::Unreadable)?,
    };
    if metadata.kind != EntryKind::Directory {
        return Ok(result);
    }
    let mut directories = vec![(candidate.path.clone(), candidate.identity)];
    let mut visited = 0_usize;
    while let Some((directory, identity)) = directories.pop() {
        let mut failure = None;
        fs.read_dir(&directory, identity, &mut |entry| {
            visited = visited.saturating_add(1);
            if visited > 250_000 {
                failure = Some(CandidateRejection::LimitReached);
                return ReadDirControl::Stop;
            }
            let metadata = match fs.metadata_no_follow(&entry.path) {
                Ok(metadata) => metadata,
                Err(_) => {
                    failure = Some(CandidateRejection::Unreadable);
                    return ReadDirControl::Stop;
                }
            };
            if metadata.kind == EntryKind::LinkLike || metadata.kind != entry.kind {
                failure = Some(CandidateRejection::LinkLike);
                return ReadDirControl::Stop;
            }
            let Some(identity) = metadata.identity else {
                failure = Some(CandidateRejection::IdentityChanged);
                return ReadDirControl::Stop;
            };
            let allocated = match fs.allocated_size(&entry.path, &metadata) {
                Ok(value) => value,
                Err(_) => {
                    failure = Some(CandidateRejection::Unreadable);
                    return ReadDirControl::Stop;
                }
            };
            let Some(total) = result.allocated_bytes.checked_add(allocated) else {
                failure = Some(CandidateRejection::LimitReached);
                return ReadDirControl::Stop;
            };
            result.allocated_bytes = total;
            if metadata.kind == EntryKind::Directory {
                directories.push((entry.path, identity));
            } else if let Some(total) = result.logical_bytes.checked_add(metadata.size) {
                result.logical_bytes = total;
            } else {
                failure = Some(CandidateRejection::LimitReached);
                return ReadDirControl::Stop;
            }
            ReadDirControl::Continue
        })
        .map_err(|_| CandidateRejection::Unreadable)?;
        if let Some(failure) = failure {
            return Err(failure);
        }
    }
    Ok(result)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanDiagnostic {
    pub rule_id: String,
    pub path: String,
    pub reason: DiagnosticReason,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticReason {
    Unreadable,
    Disappeared,
    LinkLike,
    Loop,
    MissingIdentity,
    OutsideRoot,
    Protected,
    Changed,
    LimitReached,
}
#[derive(Debug)]
pub enum ScanError {
    InvalidInput(String),
    Filesystem(FsError),
    Entropy(FsError),
    Cancelled,
}
impl std::fmt::Display for ScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput(reason) => write!(f, "invalid scan input: {reason}"),
            Self::Filesystem(_) => f.write_str("scan filesystem validation failed"),
            Self::Entropy(_) => f.write_str("scan identifier generation failed"),
            Self::Cancelled => f.write_str("scan cancelled"),
        }
    }
}
impl std::error::Error for ScanError {}
#[derive(Debug)]
pub struct ScanResult {
    pub snapshot: ScanSnapshot,
    pub diagnostics: Vec<ScanDiagnostic>,
}

pub struct ScanRequest<'a> {
    pub catalog: &'a RuleCatalog,
    pub selected_rule_ids: &'a [String],
    pub root_bindings: &'a HashMap<String, PathBuf>,
    pub protection: &'a ProtectionPolicy,
    pub limits: ScanLimits,
    pub cancellation: CancellationToken,
    pub entropy: &'a dyn Entropy,
    pub progress: &'a dyn ProgressSink,
}

pub struct ScanEngine {
    fs: Arc<dyn FileSystem>,
}
impl ScanEngine {
    pub fn new(fs: Arc<dyn FileSystem>) -> Self {
        Self { fs }
    }

    pub fn scan(&self, request: ScanRequest<'_>) -> Result<ScanResult, ScanError> {
        let ScanRequest {
            catalog,
            selected_rule_ids,
            root_bindings,
            protection,
            limits,
            cancellation,
            entropy,
            progress,
        } = request;
        validate_limits(limits)?;
        cancellation_check(&cancellation)?;
        let selected = select_rules(catalog, selected_rule_ids)?;
        let jobs = self.build_jobs(&selected, root_bindings, protection)?;
        let total_jobs = jobs.len();
        let context = TraversalContext::new(
            self.fs.as_ref(),
            protection,
            &cancellation,
            limits,
            SystemTime::now(),
        );
        progress.report(ProgressEvent {
            phase: ScanPhase::Discovering,
            visited_entries: 0,
            candidates: 0,
            completed_jobs: 0,
            total_jobs,
        });
        let drafts = discover(&jobs, &context, limits.max_workers, progress)?;
        cancellation_check(&cancellation)?;
        progress.report(ProgressEvent {
            phase: ScanPhase::Measuring,
            visited_entries: context.visited(),
            candidates: drafts.len(),
            completed_jobs: total_jobs,
            total_jobs,
        });
        let mut diagnostics = context.diagnostics();
        let measured = self.measure(drafts, protection, limits, &cancellation, &mut diagnostics)?;
        cancellation_check(&cancellation)?;
        progress.report(ProgressEvent {
            phase: ScanPhase::Finalizing,
            visited_entries: context.visited(),
            candidates: measured.len(),
            completed_jobs: total_jobs,
            total_jobs,
        });
        Ok(ScanResult {
            snapshot: finalize(measured, self.fs.semantics(), entropy)?,
            diagnostics,
        })
    }

    fn build_jobs<'a>(
        &self,
        rules: &[&'a CleanupRule],
        bindings: &HashMap<String, PathBuf>,
        protection: &ProtectionPolicy,
    ) -> Result<Vec<Job<'a>>, ScanError> {
        for path in bindings.values() {
            if !path.is_absolute()
                || path
                    .components()
                    .any(|part| matches!(part, Component::ParentDir))
            {
                return Err(ScanError::InvalidInput(
                    "root bindings must be absolute and normalized".into(),
                ));
            }
        }
        for rule in rules {
            for root in &rule.roots {
                let binding = bindings.get(&root.binding).ok_or_else(|| {
                    ScanError::InvalidInput(format!("missing root binding {}", root.binding))
                })?;
                let requested = binding.join(&root.suffix);
                if !requested.is_absolute()
                    || requested
                        .components()
                        .any(|part| matches!(part, Component::ParentDir))
                {
                    return Err(ScanError::InvalidInput(
                        "resolved rule roots must be absolute and normalized".into(),
                    ));
                }
            }
        }
        let mut jobs = Vec::new();
        for rule in rules {
            for root in &rule.roots {
                let binding = bindings.get(&root.binding).ok_or_else(|| {
                    ScanError::InvalidInput(format!("missing root binding {}", root.binding))
                })?;
                let requested = binding.join(&root.suffix);
                let before = self
                    .fs
                    .metadata_no_follow(&requested)
                    .map_err(ScanError::Filesystem)?;
                if before.kind != EntryKind::Directory || before.identity.is_none() {
                    return Err(ScanError::InvalidInput(
                        "scan roots must be identity-bearing directories".into(),
                    ));
                }
                let canonical = self
                    .fs
                    .canonicalize(&requested)
                    .map_err(ScanError::Filesystem)?;
                let after = self
                    .fs
                    .metadata_no_follow(&canonical)
                    .map_err(ScanError::Filesystem)?;
                if after.kind != EntryKind::Directory
                    || after.identity != before.identity
                    || protection.is_protected(&canonical)
                {
                    return Err(ScanError::InvalidInput(
                        "scan root is link-like, changed, or protected".into(),
                    ));
                }
                jobs.push(Job {
                    rule,
                    root: canonical,
                });
            }
        }
        Ok(jobs)
    }

    fn measure(
        &self,
        drafts: Vec<CandidateDraft>,
        protection: &ProtectionPolicy,
        limits: ScanLimits,
        cancellation: &CancellationToken,
        diagnostics: &mut Vec<ScanDiagnostic>,
    ) -> Result<Vec<Measured>, ScanError> {
        let count = AtomicUsize::new(0);
        let mut output = Vec::new();
        for draft in drafts {
            cancellation_check(cancellation)?;
            if protection.is_protected(&draft.path) {
                push_diagnostic(diagnostics, limits, &draft, DiagnosticReason::Protected);
                continue;
            }
            let before = match self.fs.metadata_no_follow(&draft.path) {
                Ok(value) => value,
                Err(_) => {
                    push_diagnostic(diagnostics, limits, &draft, DiagnosticReason::Disappeared);
                    continue;
                }
            };
            let Some(identity) = before.identity else {
                push_diagnostic(
                    diagnostics,
                    limits,
                    &draft,
                    DiagnosticReason::MissingIdentity,
                );
                continue;
            };
            if identity != draft.identity {
                push_diagnostic(diagnostics, limits, &draft, DiagnosticReason::Changed);
                continue;
            }
            if before.kind == EntryKind::LinkLike || before.kind != draft.kind {
                push_diagnostic(diagnostics, limits, &draft, DiagnosticReason::LinkLike);
                continue;
            }
            let canonical = match self.fs.canonicalize(&draft.path) {
                Ok(value) => value,
                Err(_) => {
                    push_diagnostic(diagnostics, limits, &draft, DiagnosticReason::Disappeared);
                    continue;
                }
            };
            if !self.fs.semantics().contains(&draft.scan_root, &canonical) {
                push_diagnostic(diagnostics, limits, &draft, DiagnosticReason::OutsideRoot);
                continue;
            }
            if protection.is_protected(&canonical) {
                push_diagnostic(diagnostics, limits, &draft, DiagnosticReason::Protected);
                continue;
            }
            let canonical_metadata = match self.fs.metadata_no_follow(&canonical) {
                Ok(value) => value,
                Err(_) => {
                    push_diagnostic(diagnostics, limits, &draft, DiagnosticReason::Disappeared);
                    continue;
                }
            };
            if canonical_metadata.kind == EntryKind::LinkLike
                || canonical_metadata.identity != Some(identity)
            {
                push_diagnostic(diagnostics, limits, &draft, DiagnosticReason::Changed);
                continue;
            }
            let mut measurement = MeasurementContext {
                protection,
                limits,
                cancellation,
                count: &count,
                diagnostics,
            };
            let Some(measured_tree) = self.measure_path(&draft, &canonical, &mut measurement)?
            else {
                continue;
            };
            if let Err(error) = self.revalidate_measurement(&measured_tree, cancellation) {
                match error {
                    RevalidationError::Cancelled => return Err(ScanError::Cancelled),
                    RevalidationError::Changed(reason) => {
                        push_diagnostic(diagnostics, limits, &draft, reason);
                        continue;
                    }
                }
            }
            let after = match self.fs.metadata_no_follow(&canonical) {
                Ok(value) => value,
                Err(_) => {
                    push_diagnostic(diagnostics, limits, &draft, DiagnosticReason::Disappeared);
                    continue;
                }
            };
            if after.identity != Some(identity)
                || after.kind != before.kind
                || after.size != before.size
                || after.modified != before.modified
            {
                push_diagnostic(diagnostics, limits, &draft, DiagnosticReason::Changed);
                continue;
            }
            output.push(Measured {
                rule: draft.rule,
                scan_root: draft.scan_root,
                context_root: draft.context_root,
                path: canonical,
                identity,
                kind: before.kind,
                logical_bytes: measured_tree.logical_bytes,
                allocated_bytes: measured_tree.allocated_bytes,
                modified: before.modified,
                scanned_at: draft.scanned_at,
            });
        }
        Ok(output)
    }

    fn measure_path(
        &self,
        draft: &CandidateDraft,
        root: &Path,
        context: &mut MeasurementContext<'_>,
    ) -> Result<Option<MeasuredTree>, ScanError> {
        let protection = context.protection;
        let limits = context.limits;
        let cancellation = context.cancellation;
        let count = context.count;
        let diagnostics = &mut *context.diagnostics;
        if draft.kind == EntryKind::File {
            return match self.fs.metadata_no_follow(root) {
                Ok(metadata)
                    if metadata.kind == EntryKind::File
                        && metadata.identity == Some(draft.identity) =>
                {
                    Ok(Some(MeasuredTree {
                        logical_bytes: metadata.size,
                        allocated_bytes: self
                            .fs
                            .allocated_size(root, &metadata)
                            .map_err(ScanError::Filesystem)?,
                        entries: vec![MeasuredEntry {
                            path: root.to_path_buf(),
                            metadata,
                        }],
                        directories: Vec::new(),
                    }))
                }
                Ok(_) => {
                    push_diagnostic(diagnostics, limits, draft, DiagnosticReason::Changed);
                    Ok(None)
                }
                Err(error) => {
                    push_diagnostic(
                        diagnostics,
                        limits,
                        draft,
                        diagnostic_for_fs_error(error.kind),
                    );
                    Ok(None)
                }
            };
        }
        let root_metadata = self
            .fs
            .metadata_no_follow(root)
            .map_err(ScanError::Filesystem)?;
        let mut logical_bytes = 0_u64;
        let mut allocated_bytes = self
            .fs
            .allocated_size(root, &root_metadata)
            .map_err(ScanError::Filesystem)?;
        let mut stack = vec![(root.to_path_buf(), draft.identity)];
        let mut identities = HashSet::from([draft.identity]);
        let mut entries = Vec::new();
        let mut directories = Vec::new();
        while let Some((directory, directory_identity)) = stack.pop() {
            cancellation_check(cancellation)?;
            let mut stopped = None;
            let mut was_cancelled = false;
            let mut children = Vec::new();
            let result = self
                .fs
                .read_dir(&directory, directory_identity, &mut |entry| {
                    if cancellation.is_cancelled() {
                        was_cancelled = true;
                        return ReadDirControl::Stop;
                    }
                    if !claim(count, limits.max_measurement_entries) {
                        stopped = Some(DiagnosticReason::LimitReached);
                        return ReadDirControl::Stop;
                    }
                    if !self.fs.semantics().contains(root, &entry.path)
                        || protection.is_protected(&entry.path)
                    {
                        stopped = Some(DiagnosticReason::Protected);
                        return ReadDirControl::Stop;
                    }
                    let metadata = match self.fs.metadata_no_follow(&entry.path) {
                        Ok(value) => value,
                        Err(error) => {
                            push_path_diagnostic(
                                diagnostics,
                                limits,
                                draft,
                                &entry.path,
                                diagnostic_for_fs_error(error.kind),
                            );
                            stopped = Some(DiagnosticReason::Changed);
                            return ReadDirControl::Stop;
                        }
                    };
                    if metadata.kind == EntryKind::LinkLike {
                        push_path_diagnostic(
                            diagnostics,
                            limits,
                            draft,
                            &entry.path,
                            DiagnosticReason::LinkLike,
                        );
                        stopped = Some(DiagnosticReason::Changed);
                        return ReadDirControl::Stop;
                    }
                    if metadata.kind != entry.kind || metadata.identity != entry.identity {
                        push_path_diagnostic(
                            diagnostics,
                            limits,
                            draft,
                            &entry.path,
                            DiagnosticReason::Changed,
                        );
                        stopped = Some(DiagnosticReason::Changed);
                        return ReadDirControl::Stop;
                    }
                    let Some(identity) = metadata.identity else {
                        stopped = Some(DiagnosticReason::MissingIdentity);
                        return ReadDirControl::Stop;
                    };
                    if !identities.insert(identity) {
                        stopped = Some(DiagnosticReason::Loop);
                        return ReadDirControl::Stop;
                    }
                    children.push(entry.clone());
                    entries.push(MeasuredEntry {
                        path: entry.path.clone(),
                        metadata: metadata.clone(),
                    });
                    let entry_allocated = match self.fs.allocated_size(&entry.path, &metadata) {
                        Ok(value) => value,
                        Err(_) => {
                            stopped = Some(DiagnosticReason::Unreadable);
                            return ReadDirControl::Stop;
                        }
                    };
                    let Some(next_allocated) = allocated_bytes.checked_add(entry_allocated) else {
                        stopped = Some(DiagnosticReason::LimitReached);
                        return ReadDirControl::Stop;
                    };
                    allocated_bytes = next_allocated;
                    if metadata.kind == EntryKind::Directory {
                        stack.push((entry.path, identity));
                    } else if let Some(value) = logical_bytes.checked_add(metadata.size) {
                        logical_bytes = value;
                    } else {
                        stopped = Some(DiagnosticReason::LimitReached);
                        return ReadDirControl::Stop;
                    }
                    ReadDirControl::Continue
                });
            if was_cancelled {
                return Err(ScanError::Cancelled);
            }
            if let Some(reason) = stopped {
                push_diagnostic(diagnostics, limits, draft, reason);
                return Ok(None);
            }
            if let Err(error) = result {
                push_diagnostic(
                    diagnostics,
                    limits,
                    draft,
                    diagnostic_for_fs_error(error.kind),
                );
                return Ok(None);
            }
            directories.push(MeasuredDirectory {
                path: directory,
                identity: directory_identity,
                children,
            });
        }
        Ok(Some(MeasuredTree {
            logical_bytes,
            allocated_bytes,
            entries,
            directories,
        }))
    }

    fn revalidate_measurement(
        &self,
        tree: &MeasuredTree,
        cancellation: &CancellationToken,
    ) -> Result<(), RevalidationError> {
        for entry in &tree.entries {
            if cancellation.is_cancelled() {
                return Err(RevalidationError::Cancelled);
            }
            match self.fs.metadata_no_follow(&entry.path) {
                Ok(metadata) if metadata == entry.metadata => {}
                Ok(_) => return Err(RevalidationError::Changed(DiagnosticReason::Changed)),
                Err(error) => {
                    return Err(RevalidationError::Changed(diagnostic_for_fs_error(
                        error.kind,
                    )));
                }
            }
        }
        for directory in &tree.directories {
            if cancellation.is_cancelled() {
                return Err(RevalidationError::Cancelled);
            }
            let mut remaining = directory.children.clone();
            let mut changed = false;
            let mut cancelled = false;
            let result = self
                .fs
                .read_dir(&directory.path, directory.identity, &mut |entry| {
                    if cancellation.is_cancelled() {
                        cancelled = true;
                        return ReadDirControl::Stop;
                    }
                    let Some(index) = remaining.iter().position(|expected| {
                        self.fs.semantics().equivalent(&expected.path, &entry.path)
                            && expected.kind == entry.kind
                            && expected.identity == entry.identity
                    }) else {
                        changed = true;
                        return ReadDirControl::Stop;
                    };
                    remaining.swap_remove(index);
                    ReadDirControl::Continue
                });
            if cancelled {
                return Err(RevalidationError::Cancelled);
            }
            if changed {
                return Err(RevalidationError::Changed(DiagnosticReason::Changed));
            }
            if let Err(error) = result {
                return Err(RevalidationError::Changed(diagnostic_for_fs_error(
                    error.kind,
                )));
            }
            if !remaining.is_empty() {
                return Err(RevalidationError::Changed(DiagnosticReason::Disappeared));
            }
        }
        Ok(())
    }
}

struct MeasuredTree {
    logical_bytes: u64,
    allocated_bytes: u64,
    entries: Vec<MeasuredEntry>,
    directories: Vec<MeasuredDirectory>,
}

struct MeasuredEntry {
    path: PathBuf,
    metadata: EntryMetadata,
}

struct MeasuredDirectory {
    path: PathBuf,
    identity: FileIdentity,
    children: Vec<DirectoryEntry>,
}

enum RevalidationError {
    Cancelled,
    Changed(DiagnosticReason),
}

struct MeasurementContext<'a> {
    protection: &'a ProtectionPolicy,
    limits: ScanLimits,
    cancellation: &'a CancellationToken,
    count: &'a AtomicUsize,
    diagnostics: &'a mut Vec<ScanDiagnostic>,
}

struct Job<'a> {
    rule: &'a CleanupRule,
    root: PathBuf,
}
fn discover(
    jobs: &[Job<'_>],
    context: &TraversalContext<'_>,
    max_workers: usize,
    progress: &dyn ProgressSink,
) -> Result<Vec<CandidateDraft>, ScanError> {
    if jobs.is_empty() {
        return Ok(Vec::new());
    }
    let next = AtomicUsize::new(0);
    let output = Mutex::new(Vec::new());
    let (sender, receiver) = mpsc::channel();
    std::thread::scope(|scope| {
        for _ in 0..max_workers.min(jobs.len()) {
            let sender = sender.clone();
            let next = &next;
            let output = &output;
            scope.spawn(move || {
                loop {
                    let index = next.fetch_add(1, Ordering::AcqRel);
                    let Some(job) = jobs.get(index) else { break };
                    let result = ScannerRegistry::get(job.rule.scanner)
                        .discover(job.rule, &job.root, context);
                    if let Ok(drafts) = &result {
                        output
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .extend(drafts.iter().cloned());
                    }
                    if sender.send(result.map(|_| ())).is_err() {
                        break;
                    }
                }
            });
        }
        drop(sender);
        for (completed, result) in receiver.into_iter().enumerate() {
            result?;
            progress.report(ProgressEvent {
                phase: ScanPhase::Discovering,
                visited_entries: context.visited(),
                candidates: context.candidate_count(),
                completed_jobs: completed + 1,
                total_jobs: jobs.len(),
            });
        }
        Ok::<_, ScanError>(())
    })?;
    Ok(output
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner))
}

#[derive(Debug)]
struct Measured {
    rule: CleanupRule,
    scan_root: PathBuf,
    context_root: PathBuf,
    path: PathBuf,
    identity: FileIdentity,
    kind: EntryKind,
    logical_bytes: u64,
    allocated_bytes: u64,
    modified: Option<SystemTime>,
    scanned_at: SystemTime,
}
fn finalize(
    mut measured: Vec<Measured>,
    semantics: crate::PathSemantics,
    entropy: &dyn Entropy,
) -> Result<ScanSnapshot, ScanError> {
    measured.sort_by(|left, right| {
        path_depth(&left.path)
            .cmp(&path_depth(&right.path))
            .then_with(|| semantics.key(&left.path).cmp(&semantics.key(&right.path)))
            .then_with(|| left.rule.id.cmp(&right.rule.id))
    });
    let mut retained: Vec<Measured> = Vec::new();
    for candidate in measured {
        if !retained.iter().any(|parent| {
            semantics.equivalent(&parent.path, &candidate.path)
                || (parent.kind == EntryKind::Directory
                    && semantics.contains(&parent.path, &candidate.path))
        }) {
            retained.push(candidate);
        }
    }
    let mut used = HashSet::new();
    let scan_id = unique_id(entropy, &mut used)?;
    let mut records = Vec::with_capacity(retained.len());
    let mut resolved = HashMap::with_capacity(retained.len());
    for candidate in retained {
        let id = unique_id(entropy, &mut used)?;
        let project_artifact = candidate.rule.scanner == ScannerKind::ProjectArtifacts;
        records.push(PreviewRecord {
            id: id.clone(),
            rule_id: candidate.rule.id.clone(),
            display_path: candidate.path.to_string_lossy().into_owned(),
            kind: if candidate.kind == EntryKind::Directory {
                PreviewKind::Directory
            } else {
                PreviewKind::File
            },
            bytes: candidate.logical_bytes,
            modified_unix_seconds: candidate.modified.and_then(unix_seconds),
            project_name: project_artifact.then(|| project_name(&candidate.context_root)),
            project_path: project_artifact
                .then(|| candidate.context_root.to_string_lossy().into_owned()),
            artifact: candidate.rule.artifact,
            risk: project_artifact.then_some(candidate.rule.risk),
            default_selected: project_artifact.then_some(candidate.rule.default_selected),
        });
        resolved.insert(
            id,
            ResolvedCandidate {
                path: candidate.path,
                scan_root: candidate.scan_root,
                context_root: candidate.context_root,
                rule: candidate.rule,
                identity: candidate.identity,
                kind: candidate.kind,
                logical_bytes: candidate.logical_bytes,
                allocated_bytes: candidate.allocated_bytes,
                scanned_at: candidate.scanned_at,
            },
        );
    }
    Ok(ScanSnapshot {
        scan_id,
        records,
        resolved,
    })
}

fn project_name(path: &Path) -> String {
    let display = path.to_string_lossy();
    display
        .trim_end_matches(['\\', '/'])
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(&display)
        .to_owned()
}

fn select_rules<'a>(
    catalog: &'a RuleCatalog,
    selected: &[String],
) -> Result<Vec<&'a CleanupRule>, ScanError> {
    let mut requested = HashSet::new();
    for id in selected {
        if !requested.insert(id.as_str()) {
            return Err(ScanError::InvalidInput(
                "selected rule IDs must be unique".into(),
            ));
        }
    }
    let mut rules = Vec::new();
    for rule in catalog.rules() {
        if requested.remove(rule.id.as_str()) {
            if matches!(
                rule.lifecycle,
                crate::Lifecycle::Disabled | crate::Lifecycle::Deprecated
            ) {
                return Err(ScanError::InvalidInput(
                    "disabled or deprecated rules cannot be scanned".into(),
                ));
            }
            rules.push(rule);
        }
    }
    if !requested.is_empty() {
        return Err(ScanError::InvalidInput(
            "selected rule ID does not exist".into(),
        ));
    }
    Ok(rules)
}
fn validate_limits(limits: ScanLimits) -> Result<(), ScanError> {
    if limits.max_workers == 0
        || limits.max_visited_entries == 0
        || limits.max_candidates == 0
        || limits.max_diagnostics == 0
        || limits.max_measurement_entries == 0
    {
        Err(ScanError::InvalidInput(
            "scan limits must be positive".into(),
        ))
    } else {
        Ok(())
    }
}
fn cancellation_check(token: &CancellationToken) -> Result<(), ScanError> {
    if token.is_cancelled() {
        Err(ScanError::Cancelled)
    } else {
        Ok(())
    }
}
fn claim(counter: &AtomicUsize, limit: usize) -> bool {
    counter
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
            (current < limit).then_some(current + 1)
        })
        .is_ok()
}
fn diagnostic_for_fs_error(kind: crate::FsErrorKind) -> DiagnosticReason {
    match kind {
        crate::FsErrorKind::NotFound => DiagnosticReason::Disappeared,
        crate::FsErrorKind::Changed => DiagnosticReason::Changed,
        _ => DiagnosticReason::Unreadable,
    }
}

fn push_path_diagnostic(
    output: &mut Vec<ScanDiagnostic>,
    limits: ScanLimits,
    draft: &CandidateDraft,
    path: &Path,
    reason: DiagnosticReason,
) {
    if output.len() < limits.max_diagnostics {
        output.push(ScanDiagnostic {
            rule_id: draft.rule.id.clone(),
            path: path.to_string_lossy().into_owned(),
            reason,
        });
    }
}

fn push_diagnostic(
    output: &mut Vec<ScanDiagnostic>,
    limits: ScanLimits,
    draft: &CandidateDraft,
    reason: DiagnosticReason,
) {
    if output.len() < limits.max_diagnostics {
        output.push(ScanDiagnostic {
            rule_id: draft.rule.id.clone(),
            path: draft.path.to_string_lossy().into_owned(),
            reason,
        });
    }
}
fn unique_id(entropy: &dyn Entropy, used: &mut HashSet<String>) -> Result<String, ScanError> {
    for _ in 0..8 {
        let id = random_id(entropy)?;
        if used.insert(id.clone()) {
            return Ok(id);
        }
    }
    Err(ScanError::Entropy(FsError::new(
        crate::FsErrorKind::InvalidData,
        "entropy returned duplicate identifiers",
    )))
}
fn random_id(entropy: &dyn Entropy) -> Result<String, ScanError> {
    let mut bytes = [0_u8; 16];
    entropy.fill(&mut bytes).map_err(ScanError::Entropy)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}
fn unix_seconds(time: SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|value| value.as_secs())
}
fn path_depth(path: &Path) -> usize {
    path.components().count()
}
