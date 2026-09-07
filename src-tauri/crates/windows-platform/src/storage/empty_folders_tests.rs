use super::{empty_folders::*, scans::StorageService};
use crate::{
    WindowsFileSystem,
    cleanup::{CleanupDisposition, CleanupService},
};
use cleanup_core::{storage::*, *};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("storage-empty-{}", super::opaque_id().unwrap()));
        for name in ["scan", "system", "documents", "app"] {
            std::fs::create_dir_all(p.join(name)).unwrap();
        }
        Self(WindowsFileSystem.canonicalize(&p).unwrap())
    }
    fn dir(&self, name: &str) -> PathBuf {
        let p = self.0.join("scan").join(name);
        std::fs::create_dir_all(&p).unwrap();
        p
    }
    fn scan(&self) -> (StorageService, StorageSelection) {
        let s = StorageService::new();
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
        let root = s.authorize_root(&self.0.join("scan"), &p).unwrap();
        let id = s
            .start_with(
                StorageModule::EmptyFolders,
                Some(&root),
                StorageLimits::default(),
                Some(p.clone()),
                move |ctx| discover(ctx, &p).map_err(Into::into),
            )
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        let page = loop {
            let (status, error) = s.status(&id).unwrap();
            assert!(error.is_none(), "{error:?}");
            assert_ne!(status.phase, StoragePhase::Failed);
            if let Ok(page) = s.page(&PageRequest {
                snapshot_id: id.clone(),
                module: StorageModule::EmptyFolders,
                collection: PageCollection::EmptyFolders,
                parent_id: None,
                cursor: None,
                page_size: 200,
            }) {
                break page;
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        };
        let ids = page
            .records
            .iter()
            .map(|r| {
                let StorageRecord::EmptyFolder(row) = r else {
                    panic!()
                };
                assert_ne!(PathBuf::from(&row.display_path), self.0.join("scan"));
                assert!(row.completeness.is_complete());
                let CandidateEligibility::Eligible { candidate_id } = &row.eligibility else {
                    panic!()
                };
                candidate_id.clone()
            })
            .collect();
        (
            s,
            StorageSelection {
                snapshot_id: id,
                module: StorageModule::EmptyFolders,
                candidate_ids: ids,
            },
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn empty_folders_native_nested_selection_execution_and_replay() {
    let f = Fixture::new();
    let parent = f
        .dir("parent/child/grandchild")
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned();
    let (s, mut all) = f.scan();
    assert_eq!(all.candidate_ids.len(), 3);
    let cleanup = CleanupService::new(f.0.join("plans")).unwrap();
    let evidence = s.resolve_selection(&all).unwrap();
    let parent_id = all.candidate_ids[evidence
        .iter()
        .position(|e| e.entry().canonical_path == parent)
        .unwrap()]
    .clone();
    let mut only_parent = all.clone();
    only_parent.candidate_ids = vec![parent_id];
    assert!(
        cleanup
            .create_storage_plan(&s, &only_parent, CleanupDisposition::Permanent)
            .is_err()
    );
    for e in &evidence {
        assert!(!plan_proof(e.clone()).unwrap().rule.default_selected);
    }
    all.candidate_ids.reverse(); // renderer ordering cannot control mutation ordering
    let plan = cleanup
        .create_storage_plan(&s, &all, CleanupDisposition::Permanent)
        .unwrap();
    assert!(cleanup.execute(&plan.plan_id).is_err());
    cleanup.execute_permanent(&plan.plan_id).unwrap();
    assert!(!parent.exists());
    assert!(f.0.join("scan").is_dir());
    std::fs::create_dir_all(&parent).unwrap();
    assert!(cleanup.execute_permanent(&plan.plan_id).is_err());
    assert!(parent.is_dir());
}

#[test]
fn empty_folders_native_late_child_survives_and_recycle_has_no_fallback() {
    for recycle in [false, true] {
        let f = Fixture::new();
        let dir = f.dir("empty");
        let (s, all) = f.scan();
        assert_eq!(all.candidate_ids.len(), 1);
        let cleanup = CleanupService::new(f.0.join("plans")).unwrap();
        let disposition = if recycle {
            CleanupDisposition::RecycleBin
        } else {
            CleanupDisposition::Permanent
        };
        let plan = cleanup.create_storage_plan(&s, &all, disposition).unwrap();
        if !recycle {
            std::fs::write(dir.join("late.txt"), b"must survive").unwrap();
        }
        if recycle {
            cleanup.execute(&plan.plan_id).unwrap();
        } else {
            cleanup.execute_permanent(&plan.plan_id).unwrap();
        }
        assert!(dir.exists());
        if !recycle {
            assert_eq!(
                std::fs::read(dir.join("late.txt")).unwrap(),
                b"must survive"
            );
        }
    }
}

#[test]
fn empty_folders_native_hidden_file_hidden_directory_system_and_junction_block_parents() {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_SYSTEM, SetFileAttributesW,
    };
    let f = Fixture::new();
    for (name, directory, attr) in [
        ("hidden-file", false, FILE_ATTRIBUTE_HIDDEN),
        ("hidden-dir", true, FILE_ATTRIBUTE_HIDDEN),
        ("system-dir", true, FILE_ATTRIBUTE_SYSTEM),
    ] {
        let p = f.dir(name).join("child");
        if directory {
            std::fs::create_dir(&p).unwrap();
        } else {
            std::fs::write(&p, b"").unwrap();
        }
        let wide: Vec<u16> = p.as_os_str().encode_wide().chain(Some(0)).collect();
        assert_ne!(unsafe { SetFileAttributesW(wide.as_ptr(), attr) }, 0);
    }
    f.dir("protected/.git");
    let unreadable = f.dir("unreadable/child");
    use std::os::windows::fs::OpenOptionsExt;
    // An incompatible native handle makes the child's metadata/enumeration unavailable.
    let locked = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS)
        .open(&unreadable)
        .unwrap();
    let target = f.0.join("target");
    std::fs::create_dir(&target).unwrap();
    let link = f.dir("junction").join("child");
    junction::create(&target, &link).unwrap();
    let (s, all) = f.scan();
    assert!(all.candidate_ids.is_empty());
    let cleanup = CleanupService::new(f.0.join("plans")).unwrap();
    assert!(
        cleanup
            .create_storage_plan(&s, &all, CleanupDisposition::Permanent)
            .is_err()
    );
    assert!(target.exists());
    junction::delete(&link).unwrap();
    drop(locked);
}
