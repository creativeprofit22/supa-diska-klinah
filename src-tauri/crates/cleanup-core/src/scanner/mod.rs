mod direct;
mod project_artifacts;

use crate::{
    CancellationToken, CleanupRule, DiagnosticReason, DirectoryEntry, EntryKind, FileSystem,
    ProtectionPolicy, ReadDirControl, ScanDiagnostic, ScanError, ScanLimits, ScannerKind,
};
use std::{
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::SystemTime,
};

#[derive(Clone, Debug)]
pub(crate) struct CandidateDraft {
    pub rule: CleanupRule,
    pub scan_root: PathBuf,
    pub context_root: PathBuf,
    pub path: PathBuf,
    pub kind: EntryKind,
    pub identity: crate::FileIdentity,
    pub scanned_at: SystemTime,
}

pub(crate) trait Scanner: Send + Sync {
    fn discover(
        &self,
        rule: &CleanupRule,
        root: &Path,
        context: &TraversalContext<'_>,
    ) -> Result<Vec<CandidateDraft>, ScanError>;
}

pub(crate) struct ScannerRegistry;
impl ScannerRegistry {
    pub fn get(kind: ScannerKind) -> &'static dyn Scanner {
        static DIRECT: direct::DirectScanner = direct::DirectScanner;
        static PROJECT: project_artifacts::ProjectArtifactsScanner =
            project_artifacts::ProjectArtifactsScanner;
        match kind {
            ScannerKind::Direct => &DIRECT,
            ScannerKind::ProjectArtifacts => &PROJECT,
        }
    }
}

pub(crate) struct TraversalContext<'a> {
    pub fs: &'a dyn FileSystem,
    pub protection: &'a ProtectionPolicy,
    pub cancellation: &'a CancellationToken,
    pub limits: ScanLimits,
    pub now: SystemTime,
    visited: AtomicUsize,
    candidates: AtomicUsize,
    diagnostics: Mutex<Vec<ScanDiagnostic>>,
}

impl<'a> TraversalContext<'a> {
    pub fn new(
        fs: &'a dyn FileSystem,
        protection: &'a ProtectionPolicy,
        cancellation: &'a CancellationToken,
        limits: ScanLimits,
        now: SystemTime,
    ) -> Self {
        Self {
            fs,
            protection,
            cancellation,
            limits,
            now,
            visited: AtomicUsize::new(0),
            candidates: AtomicUsize::new(0),
            diagnostics: Mutex::new(Vec::new()),
        }
    }
    pub fn check_cancelled(&self) -> Result<(), ScanError> {
        if self.cancellation.is_cancelled() {
            Err(ScanError::Cancelled)
        } else {
            Ok(())
        }
    }
    pub fn read_dir(
        &self,
        rule_id: &str,
        path: &Path,
        expected_identity: crate::FileIdentity,
    ) -> Result<Option<Vec<DirectoryEntry>>, ScanError> {
        self.check_cancelled()?;
        let mut enumerated_entries = Vec::new();
        let mut cancelled = false;
        let mut limit_reached = false;
        let result = self.fs.read_dir(path, expected_identity, &mut |entry| {
            if self.cancellation.is_cancelled() {
                cancelled = true;
                return ReadDirControl::Stop;
            }
            if claim(&self.visited, self.limits.max_visited_entries) {
                enumerated_entries.push(entry);
                ReadDirControl::Continue
            } else {
                limit_reached = true;
                ReadDirControl::Stop
            }
        });
        if cancelled {
            return Err(ScanError::Cancelled);
        }
        if limit_reached {
            self.diagnostic(rule_id, path, DiagnosticReason::LimitReached);
            return Ok(None);
        }
        if let Err(error) = result {
            self.diagnostic(rule_id, path, diagnostic_for_error(error.kind));
            return Ok(None);
        }
        let mut entries = Vec::with_capacity(enumerated_entries.len());
        for entry in enumerated_entries {
            self.check_cancelled()?;
            let current = match self.fs.metadata_no_follow(&entry.path) {
                Ok(current) => current,
                Err(error) => {
                    self.diagnostic(rule_id, &entry.path, diagnostic_for_error(error.kind));
                    continue;
                }
            };
            if current.kind == EntryKind::LinkLike {
                self.diagnostic(rule_id, &entry.path, DiagnosticReason::LinkLike);
                continue;
            }
            if current.kind != entry.kind || current.identity != entry.identity {
                self.diagnostic(rule_id, &entry.path, DiagnosticReason::Changed);
                continue;
            }
            if current.identity.is_none() {
                self.diagnostic(rule_id, &entry.path, DiagnosticReason::MissingIdentity);
                continue;
            }
            entries.push(entry);
        }
        Ok(Some(entries))
    }
    pub fn push_candidate(&self, draft: CandidateDraft, output: &mut Vec<CandidateDraft>) {
        let count = self.candidates.fetch_add(1, Ordering::AcqRel) + 1;
        if count <= self.limits.max_candidates {
            output.push(draft);
        } else {
            self.diagnostic("", Path::new(""), DiagnosticReason::LimitReached);
        }
    }
    pub fn diagnostic(&self, rule_id: &str, path: &Path, reason: DiagnosticReason) {
        let mut diagnostics = self
            .diagnostics
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if diagnostics.len() < self.limits.max_diagnostics {
            diagnostics.push(ScanDiagnostic {
                rule_id: rule_id.into(),
                path: path.to_string_lossy().into_owned(),
                reason,
            });
        }
    }
    pub fn visited(&self) -> usize {
        self.visited
            .load(Ordering::Acquire)
            .min(self.limits.max_visited_entries)
    }
    pub fn candidate_count(&self) -> usize {
        self.candidates
            .load(Ordering::Acquire)
            .min(self.limits.max_candidates)
    }
    pub fn diagnostics(&self) -> Vec<ScanDiagnostic> {
        self.diagnostics
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

fn diagnostic_for_error(kind: crate::FsErrorKind) -> DiagnosticReason {
    match kind {
        crate::FsErrorKind::NotFound => DiagnosticReason::Disappeared,
        crate::FsErrorKind::Changed => DiagnosticReason::Changed,
        _ => DiagnosticReason::Unreadable,
    }
}

fn claim(counter: &AtomicUsize, limit: usize) -> bool {
    counter
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
            (current < limit).then_some(current + 1)
        })
        .is_ok()
}

pub(crate) fn excluded(
    rule: &CleanupRule,
    root: &Path,
    path: &Path,
    semantics: crate::PathSemantics,
) -> bool {
    let name = path.file_name().map(|name| name.to_string_lossy());
    if name.is_some_and(|name| {
        rule.excluded_names
            .iter()
            .any(|excluded| name.eq_ignore_ascii_case(excluded))
    }) {
        return true;
    }
    let relative = path.strip_prefix(root).unwrap_or(path);
    rule.excluded_paths
        .iter()
        .any(|excluded| semantics.equivalent(excluded, relative))
}

pub(crate) fn target_matches(rule: &CleanupRule, name: &str, kind: EntryKind) -> bool {
    rule.targets
        .iter()
        .any(|target| target.eq_ignore_ascii_case(name))
        && match rule.target_type {
            crate::TargetType::File => kind == EntryKind::File,
            crate::TargetType::Directory => kind == EntryKind::Directory,
            crate::TargetType::Either => matches!(kind, EntryKind::File | EntryKind::Directory),
        }
}

pub(crate) fn old_enough(
    rule: &CleanupRule,
    metadata: &crate::EntryMetadata,
    now: SystemTime,
) -> bool {
    rule.minimum_age_seconds == 0
        || metadata
            .modified
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age.as_secs() >= rule.minimum_age_seconds)
}
