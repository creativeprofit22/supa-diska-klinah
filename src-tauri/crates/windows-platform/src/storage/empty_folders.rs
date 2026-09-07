//! Bottom-up empty-folder adapter. Mutation remains owned by CleanupService.
use super::scans::ScanContext;
use cleanup_core::{FileSystem, ProtectionPolicy, storage::*};

pub fn discover(
    context: &mut ScanContext,
    protection: &ProtectionPolicy,
) -> Result<(), StorageError> {
    let root = context.root.clone().ok_or(StorageError::InvalidEvidence)?;
    let machine = super::protection::MachineRoots::resolve()?;
    let mut folders = empty_folders::EmptyFolders::default();
    context.phase(StoragePhase::Walking);
    context.walk(
        protection,
        &|path| {
            machine.excludes(path)
                || empty_folders::blocks_visibility(&crate::WindowsFileSystem, path)
        },
        &mut |_, event| folders.observe(&root, event),
    )?;
    context.phase(StoragePhase::Finalizing);
    for (index, (mut row, evidence)) in folders.finish().into_iter().enumerate() {
        if context.cancellation.is_cancelled() {
            break;
        }
        row.record_id = super::opaque_id()?;
        evidence.validate()?;
        context.add_candidate(row.record_id.clone(), evidence)?;
        row.eligibility = CandidateEligibility::Eligible {
            candidate_id: row.record_id.clone(),
        };
        context.push(
            StorageRecord::EmptyFolder(row),
            RecordOrder {
                numeric: index as u64,
                text: String::new(),
            },
        )?;
    }
    Ok(())
}

pub(crate) fn plan_proof(
    evidence: StorageEvidence,
) -> Result<cleanup_core::ResolvedCandidate, StorageError> {
    if !matches!(evidence, StorageEvidence::EmptyFolder { .. }) {
        return Err(StorageError::UnsupportedScope);
    }
    evidence.validate()?;
    // Reuse the explicit storage proof envelope, never Temporary authority.
    let mut file = evidence.entry().clone();
    file.kind = cleanup_core::EntryKind::File;
    let mut proof = super::large_files::plan_proof(StorageEvidence::UserSelectedFile {
        root: evidence.root().clone(),
        entry: file,
    })?;
    proof.kind = cleanup_core::EntryKind::Directory;
    proof.rule.id = "storage-empty-folders".into();
    proof.rule.provenance.source = "backend-complete-empty-subtree".into();
    proof.rule.target_type = cleanup_core::TargetType::Directory;
    proof.scope = cleanup_core::CandidateProofScope::Storage {
        evidence: Box::new(evidence),
    };
    Ok(proof)
}

pub(crate) fn validate_current(evidence: &StorageEvidence) -> bool {
    let path = &evidence.entry().canonical_path;
    super::protection::personal_path_allowed(&evidence.root().canonical_path)
        && super::protection::personal_path_allowed(path)
        && !empty_folders::blocks_visibility(&crate::WindowsFileSystem, path)
        && crate::WindowsFileSystem
            .metadata_no_follow(path)
            .is_ok_and(|m| m.identity == Some(evidence.entry().identity))
}
