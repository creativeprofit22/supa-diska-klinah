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
    pub context_identity: crate::FileIdentity,
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
    diagnostic_count: AtomicUsize,
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
            diagnostic_count: AtomicUsize::new(0),
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
        let mut enumerated = Vec::new();
        let complete = self.enumerate_dir(rule_id, path, expected_identity, &mut |entry| {
            if let Ok(entry) = entry {
                enumerated.push(entry);
            }
            ReadDirControl::Continue
        })?;
        if !complete {
            return Ok(None);
        }
        // Legacy scanners validate after the enumeration handle closes. Preserve their
        // existing operation/worker bounds rather than nesting metadata calls in read_dir.
        let mut entries = Vec::with_capacity(enumerated.len());
        for entry in enumerated {
            self.check_cancelled()?;
            if let Ok((entry, _)) = self.validate_entry(rule_id, entry) {
                entries.push(entry);
            }
        }
        Ok(Some(entries))
    }

    /// Every skipped entry is reported; storage must not mistake a failed child for absence.
    pub fn visit_dir(
        &self,
        rule_id: &str,
        path: &Path,
        expected_identity: crate::FileIdentity,
        visitor: &mut dyn FnMut(
            Result<(DirectoryEntry, crate::EntryMetadata), ScanDiagnostic>,
        ) -> ReadDirControl,
    ) -> Result<bool, ScanError> {
        self.enumerate_dir(rule_id, path, expected_identity, &mut |entry| {
            visitor(entry.and_then(|entry| self.validate_entry(rule_id, entry)))
        })
    }
    fn issue(&self, rule_id: &str, path: &Path, reason: DiagnosticReason) -> ScanDiagnostic {
        self.diagnostic(rule_id, path, reason);
        ScanDiagnostic {
            rule_id: rule_id.into(),
            path: path.to_string_lossy().into_owned(),
            reason,
        }
    }
    fn validate_entry(
        &self,
        rule_id: &str,
        entry: DirectoryEntry,
    ) -> Result<(DirectoryEntry, crate::EntryMetadata), ScanDiagnostic> {
        let current = self
            .fs
            .metadata_no_follow(&entry.path)
            .map_err(|error| self.issue(rule_id, &entry.path, diagnostic_for_error(error.kind)))?;
        let reason = if current.kind == EntryKind::LinkLike {
            Some(DiagnosticReason::LinkLike)
        } else if current.kind != entry.kind || current.identity != entry.identity {
            Some(DiagnosticReason::Changed)
        } else if current.identity.is_none() {
            Some(DiagnosticReason::MissingIdentity)
        } else {
            None
        };
        if let Some(reason) = reason {
            return Err(self.issue(rule_id, &entry.path, reason));
        }
        Ok((entry, current))
    }
    fn enumerate_dir(
        &self,
        rule_id: &str,
        path: &Path,
        expected_identity: crate::FileIdentity,
        visitor: &mut dyn FnMut(Result<DirectoryEntry, ScanDiagnostic>) -> ReadDirControl,
    ) -> Result<bool, ScanError> {
        self.check_cancelled()?;
        let mut interrupted = false;
        let result = self.fs.read_dir(path, expected_identity, &mut |entry| {
            if self.cancellation.is_cancelled() {
                interrupted = true;
                return ReadDirControl::Stop;
            }
            if !claim(&self.visited, self.limits.max_visited_entries) {
                visitor(Err(self.issue(
                    rule_id,
                    path,
                    DiagnosticReason::LimitReached,
                )));
                interrupted = true;
                return ReadDirControl::Stop;
            }
            let control = visitor(Ok(entry));
            if control == ReadDirControl::Stop {
                interrupted = true;
            }
            control
        });
        self.check_cancelled()?;
        if let Err(error) = result {
            visitor(Err(self.issue(
                rule_id,
                path,
                diagnostic_for_error(error.kind),
            )));
            interrupted = true;
        }
        Ok(!interrupted)
    }
    pub fn push_candidate(&self, draft: CandidateDraft, output: &mut Vec<CandidateDraft>) {
        let count = self.candidates.fetch_add(1, Ordering::AcqRel) + 1;
        if count <= self.limits.max_candidates {
            output.push(draft);
        } else {
            self.diagnostic("", Path::new(""), DiagnosticReason::LimitReached);
        }
    }
    pub fn diagnostic_count(&self) -> usize {
        self.diagnostic_count.load(Ordering::Acquire)
    }
    pub fn diagnostic(&self, rule_id: &str, path: &Path, reason: DiagnosticReason) {
        self.diagnostic_count.fetch_add(1, Ordering::AcqRel);
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

pub(crate) fn marker_matches(
    rule: &CleanupRule,
    names: &std::collections::HashSet<String>,
) -> bool {
    rule.markers
        .all
        .iter()
        .all(|marker| names.contains(&marker.to_ascii_lowercase()))
        && (rule.markers.any.is_empty()
            || rule
                .markers
                .any
                .iter()
                .any(|marker| names.contains(&marker.to_ascii_lowercase())))
        && (rule.markers.any_suffix.is_empty()
            || rule.markers.any_suffix.iter().any(|suffix| {
                names
                    .iter()
                    .any(|name| name.ends_with(&suffix.to_ascii_lowercase()))
            }))
}

pub(crate) fn target_matches(rule: &CleanupRule, name: &str, kind: EntryKind) -> bool {
    (rule
        .targets
        .iter()
        .any(|target| target.eq_ignore_ascii_case(name))
        || rule
            .target_prefixes
            .iter()
            .any(|prefix| name.len() > prefix.len() && starts_with_ignore_ascii_case(name, prefix))
        || rule
            .target_suffixes
            .iter()
            .any(|suffix| name.len() > suffix.len() && ends_with_ignore_ascii_case(name, suffix)))
        && match rule.target_type {
            crate::TargetType::File => kind == EntryKind::File,
            crate::TargetType::Directory => kind == EntryKind::Directory,
            crate::TargetType::Either => matches!(kind, EntryKind::File | EntryKind::Directory),
        }
}

fn starts_with_ignore_ascii_case(value: &str, prefix: &str) -> bool {
    value
        .get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

fn ends_with_ignore_ascii_case(value: &str, suffix: &str) -> bool {
    value
        .get(value.len().saturating_sub(suffix.len())..)
        .is_some_and(|tail| tail.eq_ignore_ascii_case(suffix))
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
