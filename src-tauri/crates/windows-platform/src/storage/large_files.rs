//! Personal-file discovery only; candidates use central identity-bound validation.
use super::scans::ScanContext;
use cleanup_core::{EntryKind, FileSystem, ProtectionPolicy, storage::*};

pub fn discover(
    context: &mut ScanContext,
    protection: &ProtectionPolicy,
    filter: large_files::FileFilter,
) -> Result<(), StorageError> {
    let root = context.root.clone().ok_or(StorageError::InvalidEvidence)?;
    let mut files = large_files::LargeFiles::new(filter, context.limits.retained_records)?;
    context.phase(StoragePhase::Walking);
    let machine = super::protection::MachineRoots::resolve()?;
    context.walk(
        protection,
        &|path| machine.excludes(path),
        &mut |_, event| {
            let allocated = match &event {
                walk::WalkEvent::Entry { path, metadata, .. }
                    if metadata.kind == EntryKind::File =>
                {
                    crate::WindowsFileSystem.allocated_size(path, metadata).ok()
                }
                _ => None,
            };
            files.observe(event, allocated)
        },
    )?;
    context.phase(StoragePhase::Finalizing);
    for (index, (mut row, entry)) in files.finish().into_iter().enumerate() {
        if context.cancellation.is_cancelled() {
            return Ok(());
        }
        row.record_id = super::opaque_id()?;
        if let Some(entry) = entry.filter(|e| e.allocated_bytes.is_some()) {
            let evidence = StorageEvidence::UserSelectedFile {
                root: root.clone(),
                entry,
            };
            if evidence.validate().is_ok() {
                context.add_candidate(row.record_id.clone(), evidence)?;
                row.eligibility = CandidateEligibility::Eligible {
                    candidate_id: row.record_id.clone(),
                };
            }
        }
        context.push(
            StorageRecord::File(row),
            RecordOrder {
                numeric: index as u64,
                text: String::new(),
            },
        )?;
    }
    Ok(())
}

/// Pure proof construction is not authorization. The common plan validator
/// revalidates the native root and file and retains guards at mutation time.
pub(crate) fn plan_proof(
    evidence: StorageEvidence,
) -> Result<cleanup_core::ResolvedCandidate, StorageError> {
    use cleanup_core::*;
    if !matches!(evidence, StorageEvidence::UserSelectedFile { .. }) {
        return Err(StorageError::UnsupportedScope);
    }
    evidence.validate()?;
    let entry = evidence.entry();
    let root = evidence.root();
    Ok(ResolvedCandidate {
        path: entry.canonical_path.clone(),
        scan_root: root.canonical_path.clone(),
        context_root: root.canonical_path.clone(),
        context_identity: Some(root.identity),
        identity: entry.identity,
        kind: entry.kind,
        logical_bytes: entry.logical_bytes,
        allocated_bytes: entry.allocated_bytes.ok_or(StorageError::InvalidEvidence)?,
        scanned_at: std::time::SystemTime::now(),
        rule: CleanupRule {
            id: "storage-large-files".into(),
            rule_version: 1,
            lifecycle: Lifecycle::Stable,
            risk: Risk::HighImpact,
            provenance: Provenance {
                source: "backend-user-selected-file".into(),
                verified_at: String::new(),
            },
            default_selected: false,
            artifact: None,
            scanner: ScannerKind::Direct,
            roots: vec![RuleRoot {
                binding: "snapshot-root".into(),
                suffix: Default::default(),
            }],
            markers: Markers::default(),
            targets: vec![],
            target_prefixes: vec![],
            target_suffixes: vec![],
            target_type: TargetType::File,
            root_depth: 0,
            project_depth: None,
            target_depth: None,
            minimum_age_seconds: 0,
            excluded_names: vec![],
            excluded_paths: vec![],
        },
        scope: CandidateProofScope::Storage {
            evidence: Box::new(evidence),
        },
    })
}
