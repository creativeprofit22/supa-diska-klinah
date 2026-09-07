//! Storage traversal is an adapter over the existing scanner TraversalContext.
//! Events are borrowed/streamed; callers own retention and never receive deletion authority.
use super::{Completeness, PartialReason, RootAuthorization, StorageError, StorageLimits};
use crate::{
    CancellationToken, DiagnosticReason, EntryKind, EntryMetadata, FileIdentity, FileSystem,
    ProgressEvent, ProgressSink, ProtectionPolicy, ReadDirControl, ScanLimits, ScanPhase,
    is_local_storage_path, scanner::TraversalContext,
};
use std::{collections::HashSet, path::Path, time::SystemTime};

pub enum WalkEvent<'a> {
    Entry {
        path: &'a Path,
        metadata: &'a EntryMetadata,
        depth: u16,
    },
    /// Emitted for every entered directory including the root. Skips/limits propagate
    /// to all ancestors; consumers must never infer emptiness from missing Entry events.
    LeaveDirectory {
        path: &'a Path,
        depth: u16,
        completeness: &'a Completeness,
    },
    Blocked {
        path: &'a Path,
        reason: PartialReason,
    },
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WalkControl {
    Continue,
    Stop,
}
pub struct WalkReport {
    pub visited_entries: usize,
    pub completeness: Completeness,
    pub diagnostics: Vec<crate::ScanDiagnostic>,
}

pub struct WalkPolicy<'a> {
    pub protection: &'a ProtectionPolicy,
    pub excluded: &'a dyn Fn(&Path) -> bool,
}

pub fn walk(
    fs: &dyn FileSystem,
    root: &RootAuthorization,
    policy: WalkPolicy<'_>,
    cancellation: &CancellationToken,
    limits: StorageLimits,
    progress: &dyn ProgressSink,
    visitor: &mut dyn FnMut(WalkEvent<'_>) -> WalkControl,
) -> Result<WalkReport, StorageError> {
    let WalkPolicy {
        protection,
        excluded,
    } = policy;
    root.validate()?;
    limits.validate()?;
    let context = TraversalContext::new(
        fs,
        protection,
        cancellation,
        ScanLimits {
            max_workers: limits.workers,
            max_visited_entries: limits.visited_entries,
            max_candidates: limits.retained_records,
            max_diagnostics: limits.diagnostics,
            max_measurement_entries: limits.visited_entries,
        },
        SystemTime::now(),
    );
    // Validate every ancestor without following links, and bind the canonical root again.
    for ancestor in root
        .canonical_path
        .ancestors()
        .filter(|p| !p.as_os_str().is_empty())
    {
        let metadata = fs
            .metadata_no_follow(ancestor)
            .map_err(|_| StorageError::InvalidEvidence)?;
        if metadata.kind != EntryKind::Directory || metadata.identity.is_none() {
            return Err(StorageError::InvalidEvidence);
        }
    }
    let canonical = fs
        .canonicalize(&root.canonical_path)
        .map_err(|_| StorageError::InvalidEvidence)?;
    let metadata = fs
        .metadata_no_follow(&canonical)
        .map_err(|_| StorageError::InvalidEvidence)?;
    if !fs.semantics().equivalent(&canonical, &root.canonical_path)
        || metadata.identity != Some(root.identity)
        || metadata.kind != EntryKind::Directory
        || protection.is_protected(&canonical)
        || excluded(&canonical)
    {
        return Err(StorageError::InvalidEvidence);
    }
    let mut state = WalkState {
        context: &context,
        limits,
        progress,
        excluded,
        visitor,
        directories: HashSet::new(),
        stopped: false,
    };
    state.directories.insert(root.identity);
    let completeness = state.directory(&canonical, root.identity, 0);
    progress.report(ProgressEvent {
        phase: ScanPhase::Finalizing,
        visited_entries: context.visited(),
        candidates: 0,
        completed_jobs: 1,
        total_jobs: 1,
    });
    Ok(WalkReport {
        visited_entries: context.visited(),
        completeness,
        diagnostics: context.diagnostics(),
    })
}
struct WalkState<'a, 'fs> {
    context: &'a TraversalContext<'fs>,
    limits: StorageLimits,
    progress: &'a dyn ProgressSink,
    excluded: &'a dyn Fn(&Path) -> bool,
    visitor: &'a mut dyn FnMut(WalkEvent<'_>) -> WalkControl,
    directories: HashSet<FileIdentity>,
    stopped: bool,
}
impl WalkState<'_, '_> {
    fn blocked(&mut self, path: &Path, reason: PartialReason, completeness: &mut Completeness) {
        completeness.mark(reason);
        if (self.visitor)(WalkEvent::Blocked { path, reason }) == WalkControl::Stop {
            self.stopped = true;
        }
    }
    fn directory(&mut self, path: &Path, identity: FileIdentity, depth: u16) -> Completeness {
        let mut completeness = Completeness::default();
        let context = self.context;
        if context.cancellation.is_cancelled() {
            completeness.mark(PartialReason::Cancelled);
        } else {
            let result = context.visit_dir("storage", path, identity, &mut |result| {
                if self.stopped {
                    completeness.mark(PartialReason::RecordLimit);
                    return ReadDirControl::Stop;
                }
                if context.diagnostic_count() >= self.limits.diagnostics {
                    self.blocked(path, PartialReason::DiagnosticLimit, &mut completeness);
                    self.stopped = true;
                    return ReadDirControl::Stop;
                }
                let (entry, metadata) = match result {
                    Ok(entry) => entry,
                    Err(diagnostic) => {
                        let reason = match diagnostic.reason {
                            DiagnosticReason::LimitReached => PartialReason::EntryLimit,
                            DiagnosticReason::LinkLike => PartialReason::LinkLike,
                            DiagnosticReason::Changed | DiagnosticReason::Disappeared => {
                                PartialReason::Changed
                            }
                            DiagnosticReason::MissingIdentity => PartialReason::MissingIdentity,
                            _ => PartialReason::Unreadable,
                        };
                        self.blocked(Path::new(&diagnostic.path), reason, &mut completeness);
                        if reason == PartialReason::EntryLimit {
                            self.stopped = true;
                        }
                        return if self.stopped {
                            ReadDirControl::Stop
                        } else {
                            ReadDirControl::Continue
                        };
                    }
                };
                let canonical = context.fs.canonicalize(&entry.path);
                let reason = if !is_local_storage_path(&entry.path)
                    || !entry
                        .path
                        .parent()
                        .is_some_and(|parent| context.fs.semantics().equivalent(parent, path))
                {
                    Some(PartialReason::Changed)
                } else if context.protection.is_protected(&entry.path) {
                    Some(PartialReason::Protected)
                } else if (self.excluded)(&entry.path) {
                    Some(PartialReason::Excluded)
                } else if !canonical
                    .as_ref()
                    .is_ok_and(|p| context.fs.semantics().equivalent(p, &entry.path))
                {
                    Some(PartialReason::Changed)
                } else if depth >= self.limits.depth {
                    Some(PartialReason::DepthLimit)
                } else {
                    None
                };
                if let Some(reason) = reason {
                    context.diagnostic("storage", &entry.path, DiagnosticReason::Changed);
                    self.blocked(&entry.path, reason, &mut completeness);
                } else {
                    if (self.visitor)(WalkEvent::Entry {
                        path: &entry.path,
                        metadata: &metadata,
                        depth: depth + 1,
                    }) == WalkControl::Stop
                    {
                        self.stopped = true;
                        completeness.mark(PartialReason::RecordLimit);
                    } else if metadata.kind == EntryKind::Directory {
                        let identity = metadata
                            .identity
                            .expect("TraversalContext requires identity");
                        if self.directories.len() >= self.limits.retained_records {
                            self.stopped = true;
                            self.blocked(
                                &entry.path,
                                PartialReason::RecordLimit,
                                &mut completeness,
                            );
                        } else if !self.directories.insert(identity) {
                            self.blocked(&entry.path, PartialReason::Changed, &mut completeness);
                        } else {
                            let child = self.directory(&entry.path, identity, depth + 1);
                            for reason in child.reasons {
                                completeness.mark(reason);
                            }
                        }
                    }
                }
                self.progress.report(ProgressEvent {
                    phase: ScanPhase::Discovering,
                    visited_entries: context.visited(),
                    candidates: 0,
                    completed_jobs: 0,
                    total_jobs: 1,
                });
                if self.stopped {
                    ReadDirControl::Stop
                } else {
                    ReadDirControl::Continue
                }
            });
            if result.is_err() {
                completeness.mark(PartialReason::Cancelled);
            }
            if context.cancellation.is_cancelled() {
                completeness.mark(PartialReason::Cancelled);
            }
        }
        if (self.visitor)(WalkEvent::LeaveDirectory {
            path,
            depth,
            completeness: &completeness,
        }) == WalkControl::Stop
        {
            self.stopped = true;
            completeness.mark(PartialReason::RecordLimit);
        }
        completeness
    }
}
