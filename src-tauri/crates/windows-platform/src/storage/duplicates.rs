//! Guarded SHA-256 discovery. Hashes are evidence, never deletion authority.
use super::scans::ScanContext;
use crate::{WindowsFileSystem, cleanup::IdentityGuard};
use cleanup_core::{CancellationToken, FileSystem, ProtectionPolicy, storage::*};
use sha2::{Digest, Sha256};

const CHUNK: usize = 64 * 1024;

pub fn hash(
    guard: &IdentityGuard,
    limit: u64,
    cancel: &CancellationToken,
) -> Result<[u8; 32], StorageError> {
    hash_chunks(limit, cancel, |buffer, offset| {
        guard
            .read_at(buffer, offset)
            .map_err(|_| StorageError::InvalidEvidence)
    })
}

pub(super) fn hash_chunks(
    limit: u64,
    cancel: &CancellationToken,
    mut read_at: impl FnMut(&mut [u8], u64) -> Result<usize, StorageError>,
) -> Result<[u8; 32], StorageError> {
    let mut hash = Sha256::new();
    let mut buffer = [0; CHUNK];
    let mut offset = 0;
    loop {
        if cancel.is_cancelled() {
            return Err(StorageError::SnapshotUnavailable);
        }
        let count = (limit.saturating_sub(offset)).min(CHUNK as u64) as usize;
        if count == 0 {
            break;
        }
        let read = read_at(&mut buffer[..count], offset)?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
        offset += read as u64;
    }
    Ok(hash.finalize().into())
}

fn discovery_hash(
    context: &ScanContext,
    guard: &IdentityGuard,
    limit: u64,
) -> Result<[u8; 32], StorageError> {
    let mut pending = 0_u64;
    let mut last = std::time::Instant::now();
    let result = hash_chunks(limit, &context.cancellation, |buffer, offset| {
        let n = guard
            .read_at(buffer, offset)
            .map_err(|_| StorageError::InvalidEvidence)?;
        pending = pending.saturating_add(n as u64);
        // Count each chunk, but avoid taking the status mutex on every read.
        if pending >= 1024 * 1024 || last.elapsed() >= std::time::Duration::from_millis(100) {
            context.hash_progress(pending, false);
            pending = 0;
            last = std::time::Instant::now();
        }
        Ok(n)
    });
    context.hash_progress(pending, result.is_ok());
    result
}

/// Both handles stay pinned throughout digest AND byte comparison. SHA equality
/// alone (even a freshly calculated digest) is deliberately insufficient.
pub fn verify(
    keeper: &IdentityGuard,
    member: &IdentityGuard,
    expected: &[u8; 32],
    cancel: &CancellationToken,
) -> Result<(), StorageError> {
    if &hash(keeper, u64::MAX, cancel)? != expected || &hash(member, u64::MAX, cancel)? != expected
    {
        return Err(StorageError::InvalidEvidence);
    }
    let mut a = [0; CHUNK];
    let mut b = [0; CHUNK];
    let mut offset = 0;
    loop {
        if cancel.is_cancelled() {
            return Err(StorageError::SnapshotUnavailable);
        }
        let n = keeper
            .read_at(&mut a, offset)
            .map_err(|_| StorageError::InvalidEvidence)?;
        let m = member
            .read_at(&mut b, offset)
            .map_err(|_| StorageError::InvalidEvidence)?;
        if n != m || a[..n] != b[..m] {
            return Err(StorageError::InvalidEvidence);
        }
        if n == 0 {
            return Ok(());
        }
        offset += n as u64;
    }
}

pub fn discover(
    context: &mut ScanContext,
    protection: &ProtectionPolicy,
    filter: large_files::FileFilter,
) -> Result<(), StorageError> {
    let root = context.root.clone().ok_or(StorageError::InvalidEvidence)?;
    let mut files = large_files::LargeFiles::new(filter, context.limits.retained_records)?;
    let machine = super::protection::MachineRoots::resolve()?;
    context.phase(StoragePhase::Walking);
    context.walk(protection, &|p| machine.excludes(p), &mut |_, event| {
        let allocated = match &event {
            walk::WalkEvent::Entry { path, metadata, .. } => {
                WindowsFileSystem.allocated_size(path, metadata).ok()
            }
            _ => None,
        };
        files.observe(event, allocated)
    })?;
    context.phase(StoragePhase::Grouping);
    let groups = duplicates::size_groups(
        files
            .finish()
            .into_iter()
            .filter_map(|(r, e)| e.filter(|e| e.allocated_bytes.is_some()).map(|e| (r, e)))
            .collect(),
    );
    let mut partial = Vec::new();
    context.phase(StoragePhase::PartialHash);
    for group in groups {
        let mut hashed = Vec::new();
        for file in group {
            if context.cancellation.is_cancelled() {
                return Ok(());
            }
            match WindowsFileSystem
                .guard_entry(&root, &file.1, true)
                .ok()
                .and_then(|g| discovery_hash(context, &g, 4096).ok())
            {
                Some(digest) => hashed.push((file, digest)),
                None => context.mark_partial(PartialReason::Unreadable),
            }
        }
        partial.extend(
            duplicates::digest_groups(hashed)
                .into_iter()
                .map(|(_, g)| g),
        );
    }
    context.phase(StoragePhase::FullHash);
    let mut full = Vec::new();
    for group in partial {
        let mut hashed = Vec::new();
        for file in group {
            if context.cancellation.is_cancelled() {
                return Ok(());
            }
            match WindowsFileSystem
                .guard_entry(&root, &file.1, true)
                .ok()
                .and_then(|g| discovery_hash(context, &g, u64::MAX).ok())
            {
                Some(digest) => hashed.push((file, digest)),
                None => context.mark_partial(PartialReason::Unreadable),
            }
        }
        full.extend(duplicates::digest_groups(hashed));
    }
    context.phase(StoragePhase::Finalizing);
    let mut retained = 0;
    for (digest, group) in full {
        if context.cancellation.is_cancelled() {
            return Ok(());
        }
        if retained + group.len() + 1 > context.limits.retained_records {
            context.mark_partial(PartialReason::RecordLimit);
            break;
        }
        retained += group.len() + 1;
        let id = super::opaque_id()?;
        context.push(
            StorageRecord::DuplicateGroup(duplicates::DuplicateGroup {
                group_id: id.clone(),
                member_count: group.len(),
                independent_copies: group.len(),
                bytes_per_copy: group[0].1.logical_bytes, // This is a complete retained equivalence group, not a claim
                // that the entire scan is complete. Every group/member page also
                // carries the snapshot's partial reasons via StoragePage.
                completeness: Completeness::default(),
            }),
            RecordOrder {
                numeric: retained as u64,
                text: String::new(),
            },
        )?;
        for (index, (mut row, entry)) in group.iter().cloned().enumerate() {
            row.record_id = super::opaque_id()?;
            let evidence = StorageEvidence::DuplicateMember {
                root: root.clone(),
                entry,
                keeper: KeeperEvidence {
                    group_id: id.clone(),
                    keeper_root: root.clone(),
                    keeper: group[if index == 0 { 1 } else { 0 }].1.clone(),
                    full_sha256: digest,
                    independent_copies: group.len() as u32,
                },
            };
            context.add_candidate(row.record_id.clone(), evidence)?;
            row.eligibility = CandidateEligibility::Eligible {
                candidate_id: row.record_id.clone(),
            };
            context.push(
                StorageRecord::DuplicateMember(duplicates::DuplicateMember {
                    group_id: id.clone(),
                    file: row,
                }),
                RecordOrder {
                    numeric: index as u64,
                    text: String::new(),
                },
            )?;
        }
    }
    Ok(())
}

pub(crate) fn plan_proof(
    evidence: StorageEvidence,
) -> Result<cleanup_core::ResolvedCandidate, StorageError> {
    if !matches!(evidence, StorageEvidence::DuplicateMember { .. }) {
        return Err(StorageError::InvalidEvidence);
    }
    evidence.validate()?;
    let mut proof = super::large_files::plan_proof(StorageEvidence::UserSelectedFile {
        root: evidence.root().clone(),
        entry: evidence.entry().clone(),
    })?;
    proof.rule.id = "storage-duplicates".into();
    proof.rule.provenance.source = "backend-duplicate-retention".into();
    proof.scope = cleanup_core::CandidateProofScope::Storage {
        evidence: Box::new(evidence),
    };
    Ok(proof)
}
