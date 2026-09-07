use super::{duplicates::*, scans::StorageService};
use crate::{
    WindowsFileSystem,
    cleanup::{CleanupDisposition, CleanupService},
};
use cleanup_core::{FileSystem, ProtectionInputs, ProtectionPolicy, storage::*};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "storage-duplicates-{}",
            super::opaque_id().unwrap()
        ));
        for name in ["scan", "system", "documents", "app"] {
            std::fs::create_dir_all(root.join(name)).unwrap();
        }
        for name in ["a", "b", "c"] {
            std::fs::write(root.join("scan").join(name), vec![42; 150_000]).unwrap();
        }
        Self(WindowsFileSystem.canonicalize(&root).unwrap())
    }
    fn scan(&self) -> (StorageService, StorageSelection) {
        let service = StorageService::new();
        let p = ProtectionPolicy::compile(
            &WindowsFileSystem,
            ProtectionInputs::new(
                vec![self.0.join("system")],
                vec![self.0.join("documents")],
                vec![self.0.join("app")],
            )
            .unwrap(),
        )
        .unwrap();
        let root = service.authorize_root(&self.0.join("scan"), &p).unwrap();
        let id = service
            .start_with(
                StorageModule::Duplicates,
                Some(&root),
                StorageLimits::default(),
                Some(p.clone()),
                move |ctx| {
                    discover(
                        ctx,
                        &p,
                        large_files::FileFilter {
                            minimum_bytes: 0,
                            ..Default::default()
                        },
                    )
                    .map_err(Into::into)
                },
            )
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let (status, error) = service.status(&id).unwrap();
            assert!(error.is_none(), "{error:?}");
            if status.phase == StoragePhase::Complete {
                assert_eq!(status.completed_hashes, 6);
                assert_eq!(status.hashed_bytes, 3 * (4096 + 150_000));
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        let group = service
            .page(&PageRequest {
                snapshot_id: id.clone(),
                module: StorageModule::Duplicates,
                collection: PageCollection::DuplicateGroups,
                parent_id: None,
                cursor: None,
                page_size: 1,
            })
            .unwrap();
        let StorageRecord::DuplicateGroup(group) = &group.records[0] else {
            panic!()
        };
        assert_eq!(group.independent_copies, 3);
        let page = service
            .page(&PageRequest {
                snapshot_id: id.clone(),
                module: StorageModule::Duplicates,
                collection: PageCollection::DuplicateMembers,
                parent_id: Some(group.group_id.clone()),
                cursor: None,
                page_size: 200,
            })
            .unwrap();
        let ids = page
            .records
            .iter()
            .map(|r| {
                let StorageRecord::DuplicateMember(m) = r else {
                    panic!()
                };
                let CandidateEligibility::Eligible { candidate_id } = &m.file.eligibility else {
                    panic!()
                };
                candidate_id.clone()
            })
            .collect();
        (
            service,
            StorageSelection {
                snapshot_id: id,
                module: StorageModule::Duplicates,
                candidate_ids: ids,
            },
        )
    }
    fn cleanup(&self) -> CleanupService {
        CleanupService::new(self.0.join("plans")).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn duplicate_all_copies_hardlinks_keeper_change_and_overlapping_plans() {
    let f = Fixture::new();
    std::fs::hard_link(f.0.join("scan/a"), f.0.join("scan/alias")).unwrap();
    let (service, all) = f.scan();
    let cleanup = f.cleanup();
    assert!(
        cleanup
            .create_storage_plan(&service, &all, CleanupDisposition::Permanent)
            .is_err()
    );
    let mut selection = all.clone();
    selection.candidate_ids.truncate(2);
    let proofs = service.resolve_selection(&selection).unwrap();
    let StorageEvidence::DuplicateMember { keeper, .. } = &proofs[0] else {
        panic!()
    };
    assert!(
        proofs
            .iter()
            .all(|e| e.entry().identity != keeper.keeper.identity)
    );
    let first = cleanup
        .create_storage_plan(&service, &selection, CleanupDisposition::Permanent)
        .unwrap();
    let mut other = all;
    other.candidate_ids = other.candidate_ids[2..].to_vec();
    let second = cleanup
        .create_storage_plan(&service, &other, CleanupDisposition::Permanent)
        .unwrap();
    cleanup.execute_permanent(&first.plan_id).unwrap();
    assert!(keeper.keeper.canonical_path.exists());
    drop(cleanup);
    let cleanup = f.cleanup(); // Reopen persisted plans after the earlier cleanup.
    assert!(cleanup.execute_permanent(&second.plan_id).is_err());
    assert!(keeper.keeper.canonical_path.exists());
}

#[test]
fn duplicate_vanished_keeper_same_size_rewrite_and_existing_writer_reject() {
    use std::os::windows::fs::OpenOptionsExt;
    for mode in 0..4 {
        let f = Fixture::new();
        let (service, mut selection) = f.scan();
        selection.candidate_ids.truncate(1);
        let proofs = service.resolve_selection(&selection).unwrap();
        let StorageEvidence::DuplicateMember { keeper, entry, .. } = &proofs[0] else {
            panic!()
        };
        let cleanup = f.cleanup();
        let plan = cleanup
            .create_storage_plan(&service, &selection, CleanupDisposition::Permanent)
            .unwrap();
        let mut writer = None;
        if mode == 0 {
            std::fs::remove_file(&keeper.keeper.canonical_path).unwrap();
        }
        if mode == 1 || mode == 3 {
            let rewritten = if mode == 1 { &keeper.keeper } else { entry };
            let before = std::fs::metadata(&rewritten.canonical_path)
                .unwrap()
                .modified()
                .unwrap();
            std::fs::write(
                &rewritten.canonical_path,
                vec![43; rewritten.logical_bytes as usize],
            )
            .unwrap();
            std::fs::File::options()
                .write(true)
                .open(&rewritten.canonical_path)
                .unwrap()
                .set_modified(before)
                .unwrap();
        }
        if mode == 2 {
            writer = Some(
                std::fs::OpenOptions::new()
                    .write(true)
                    .share_mode(7)
                    .open(&keeper.keeper.canonical_path)
                    .unwrap(),
            );
        }
        assert!(cleanup.execute_permanent(&plan.plan_id).is_err());
        assert!(entry.canonical_path.exists());
        drop(writer);
    }
}

#[test]
fn duplicate_hash_cancellation_and_byte_verification() {
    let f = Fixture::new();
    let (service, mut selection) = f.scan();
    selection.candidate_ids.truncate(1);
    let proofs = service.resolve_selection(&selection).unwrap();
    let StorageEvidence::DuplicateMember {
        root,
        entry,
        keeper,
    } = &proofs[0]
    else {
        panic!()
    };
    let k = WindowsFileSystem
        .guard_entry(root, &keeper.keeper, true)
        .unwrap();
    let m = WindowsFileSystem
        .guard_duplicate_member(root, entry)
        .unwrap();
    let cancel = cleanup_core::CancellationToken::default();
    verify(&k, &m, &keeper.full_sha256, &cancel).unwrap();
    let mut reads = 0;
    let result = hash_chunks(u64::MAX, &cancel, |buffer, offset| {
        reads += 1;
        assert_eq!(offset, 0);
        let read = k.read_at(buffer, offset).unwrap();
        assert_eq!(read, 64 * 1024);
        cancel.cancel(); // Deterministic cancellation after a real native chunk read.
        Ok(read)
    });
    assert_eq!(result, Err(StorageError::SnapshotUnavailable));
    assert_eq!(reads, 1, "cancellation must prevent the second read");
    assert!(hash(&k, u64::MAX, &cancel).is_err());
    assert!(verify(&k, &m, &keeper.full_sha256, &cancel).is_err());
}
