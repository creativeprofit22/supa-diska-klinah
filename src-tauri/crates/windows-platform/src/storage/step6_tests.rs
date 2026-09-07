use super::{disk_analyzer, large_files, scans::StorageService};
use cleanup_core::{FileSystem, ProtectionInputs, ProtectionPolicy, storage::*};
use std::{
    path::PathBuf,
    time::{Duration, Instant, UNIX_EPOCH},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("storage-step6-{}", super::opaque_id().unwrap()));
        for name in ["scan", "system", "documents", "app"] {
            std::fs::create_dir_all(path.join(name)).unwrap();
        }
        Self(crate::WindowsFileSystem.canonicalize(&path).unwrap())
    }
    fn file(&self, name: &str, size: usize, seconds: u64) -> PathBuf {
        let path = self.0.join("scan").join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, vec![42; size]).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(UNIX_EPOCH + Duration::from_secs(seconds))
            .unwrap();
        path
    }
    fn protection(&self) -> ProtectionPolicy {
        ProtectionPolicy::compile(
            &crate::WindowsFileSystem,
            ProtectionInputs::new(
                vec![self.0.join("system")],
                vec![self.0.join("documents")],
                vec![self.0.join("app")],
            )
            .unwrap(),
        )
        .unwrap()
    }
    fn scan(
        &self,
        module: StorageModule,
        limits: StorageLimits,
        filter: cleanup_core::storage::large_files::FileFilter,
    ) -> (StorageService, String) {
        let service = StorageService::new();
        let p = self.protection();
        let root = service.authorize_root(&self.0.join("scan"), &p).unwrap();
        let id = service
            .start_with(module, Some(&root), limits, Some(p.clone()), move |ctx| {
                match module {
                    StorageModule::DiskAnalyzer => disk_analyzer::discover(ctx, &p, 1),
                    StorageModule::LargeFiles => large_files::discover(ctx, &p, filter),
                    _ => unreachable!(),
                }
                .map_err(Into::into)
            })
            .unwrap();
        wait(&service, &id);
        (service, id)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn wait(service: &StorageService, id: &str) -> StorageStatus {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let (status, error) = service.status(id).unwrap();
        assert!(error.is_none(), "{error:?}");
        if matches!(
            status.phase,
            StoragePhase::Complete | StoragePhase::Cancelled
        ) {
            return status;
        }
        assert_ne!(status.phase, StoragePhase::Failed);
        assert!(Instant::now() < deadline, "scan timed out");
        std::thread::yield_now();
    }
}
fn page(
    service: &StorageService,
    id: &str,
    module: StorageModule,
    collection: PageCollection,
    parent: Option<String>,
) -> StoragePage {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(page) = service.page(&PageRequest {
            snapshot_id: id.into(),
            module,
            collection,
            parent_id: parent.clone(),
            cursor: None,
            page_size: 100,
        }) {
            return page;
        }
        assert!(Instant::now() < deadline, "snapshot timed out");
        std::thread::yield_now();
    }
}
fn filter() -> cleanup_core::storage::large_files::FileFilter {
    cleanup_core::storage::large_files::FileFilter {
        minimum_bytes: 1,
        ..Default::default()
    }
}
#[test]
fn step6_native_deep_totals_hardlinks_extensions_and_parent_paging() {
    let f = Fixture::new();
    let one = f.file("a/deeper/still/deep/one.TXT", 10, 100);
    std::fs::hard_link(&one, f.0.join("scan/a/alias.txt")).unwrap();
    f.file("b/movie.mp4", 30, 200);
    std::fs::hard_link(&one, f.0.join("scan/b/cross.txt")).unwrap();
    let allocation = crate::WindowsFileSystem
        .allocated_size(
            &one,
            &crate::WindowsFileSystem.metadata_no_follow(&one).unwrap(),
        )
        .unwrap();
    let (s, id) = f.scan(
        StorageModule::DiskAnalyzer,
        StorageLimits::default(),
        filter(),
    );
    let roots = page(
        &s,
        &id,
        StorageModule::DiskAnalyzer,
        PageCollection::Tree,
        None,
    );
    assert!(roots.completeness.is_complete());
    assert_eq!(roots.records.len(), 1);
    let StorageRecord::Directory(root) = &roots.records[0] else {
        panic!()
    };
    assert_eq!(
        (
            root.logical_bytes,
            root.independent_files,
            root.hard_link_entries
        ),
        (40, 2, 2)
    );
    assert!(root.allocated_bytes.is_some());
    let children = page(
        &s,
        &id,
        StorageModule::DiskAnalyzer,
        PageCollection::Tree,
        Some(root.node_id.clone()),
    );
    assert_eq!(children.records.len(), 2);
    let StorageRecord::Directory(a) = &children.records[0] else {
        panic!()
    };
    assert_eq!(
        (a.logical_bytes, a.allocated_bytes, a.hard_link_entries),
        (10, Some(allocation), 1)
    );
    assert!(
        page(
            &s,
            &id,
            StorageModule::DiskAnalyzer,
            PageCollection::Tree,
            Some(a.node_id.clone())
        )
        .records
        .is_empty()
    );
    let types = page(
        &s,
        &id,
        StorageModule::DiskAnalyzer,
        PageCollection::Extensions,
        None,
    );
    assert!(types.records.iter().any(|r| matches!(r, StorageRecord::Extension(t) if t.extension == "txt" && t.file_count == 1 && t.logical_bytes == 10)));
    assert!(
        s.resolve_selection(&StorageSelection {
            snapshot_id: id,
            module: StorageModule::DiskAnalyzer,
            candidate_ids: vec![root.node_id.clone()]
        })
        .is_err()
    );
}
#[test]
fn step6_native_files_filter_sort_evidence_and_plan_bridge() {
    use cleanup_core::storage::large_files::{FileCategory, FileSort};
    let f = Fixture::new();
    f.file("z.TXT", 20, 100);
    f.file("a.txt", 10, 300);
    f.file("b.mp4", 30, 200);
    for (sort, descending, first) in [
        (FileSort::Size, true, "z.TXT"),
        (FileSort::Modified, true, "a.txt"),
        (FileSort::Path, true, "z.TXT"),
        (FileSort::Path, false, "a.txt"),
    ] {
        let (s, id) = f.scan(
            StorageModule::LargeFiles,
            StorageLimits::default(),
            cleanup_core::storage::large_files::FileFilter {
                minimum_bytes: 10,
                maximum_bytes: Some(20),
                extensions: vec!["txt".into()],
                category: FileCategory::Documents,
                sort,
                descending,
            },
        );
        let rows = page(
            &s,
            &id,
            StorageModule::LargeFiles,
            PageCollection::Files,
            None,
        );
        assert!(rows.completeness.is_complete());
        assert_eq!(rows.records.len(), 2);
        let StorageRecord::File(row) = &rows.records[0] else {
            panic!()
        };
        assert!(row.display_path.ends_with(first));
        let CandidateEligibility::Eligible { candidate_id } = &row.eligibility else {
            panic!()
        };
        let selection = StorageSelection {
            snapshot_id: id.clone(),
            module: StorageModule::LargeFiles,
            candidate_ids: vec![candidate_id.clone()],
        };
        let evidence = s.resolve_selection(&selection).unwrap().remove(0);
        assert_eq!(evidence.root().snapshot_id, id);
        let proof = large_files::plan_proof(evidence).unwrap();
        assert!(!proof.rule.default_selected);
        let cleanup = crate::cleanup::CleanupService::new(
            f.0.join(format!("plans-{}", super::opaque_id().unwrap())),
        )
        .unwrap();
        assert!(
            cleanup
                .create_storage_plan(
                    &s,
                    &selection,
                    crate::cleanup::CleanupDisposition::Permanent
                )
                .is_ok()
        );
        let mut wrong = selection.clone();
        wrong.module = StorageModule::Cleaner;
        assert!(
            cleanup
                .create_storage_plan(&s, &wrong, crate::cleanup::CleanupDisposition::Permanent)
                .is_err()
        );
        std::fs::File::options()
            .write(true)
            .open(&proof.path)
            .unwrap()
            .set_modified(UNIX_EPOCH + Duration::from_secs(999))
            .unwrap();
        assert!(
            cleanup
                .create_storage_plan(
                    &s,
                    &selection,
                    crate::cleanup::CleanupDisposition::Permanent
                )
                .is_err()
        );
        // Restore fixture timestamps for the next sort assertion.
        std::fs::File::options()
            .write(true)
            .open(f.0.join("scan/z.TXT"))
            .unwrap()
            .set_modified(UNIX_EPOCH + Duration::from_secs(100))
            .unwrap();
        std::fs::File::options()
            .write(true)
            .open(f.0.join("scan/a.txt"))
            .unwrap()
            .set_modified(UNIX_EPOCH + Duration::from_secs(300))
            .unwrap();
    }
}
#[test]
fn step6_native_partial_limits_locked_file_and_cancel() {
    use std::os::windows::fs::OpenOptionsExt;
    let f = Fixture::new();
    f.file("a.txt", 10, 100);
    f.file("b.txt", 20, 200);
    f.file("deep/deeper/c.txt", 30, 300);
    for (module, limits, reason) in [
        (
            StorageModule::LargeFiles,
            StorageLimits {
                retained_records: 1,
                ..Default::default()
            },
            PartialReason::RecordLimit,
        ),
        (
            StorageModule::DiskAnalyzer,
            StorageLimits {
                depth: 1,
                ..Default::default()
            },
            PartialReason::DepthLimit,
        ),
        (
            StorageModule::DiskAnalyzer,
            StorageLimits {
                retained_records: 2,
                ..Default::default()
            },
            PartialReason::RecordLimit,
        ),
    ] {
        let (s, id) = f.scan(module, limits, filter());
        assert!(wait(&s, &id).completeness.reasons.contains(&reason));
    }
    // Zero-access file metadata remains readable despite a file sharing lock.
    // Deny directory listing instead, so the shared walker encounters a real
    // unreadable subtree without requiring administrator ACL changes.
    let locked = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS)
        .open(f.0.join("scan/deep"))
        .unwrap();
    let (s, id) = f.scan(
        StorageModule::DiskAnalyzer,
        StorageLimits::default(),
        filter(),
    );
    let rows = page(
        &s,
        &id,
        StorageModule::DiskAnalyzer,
        PageCollection::Tree,
        None,
    );
    let StorageRecord::Directory(root) = &rows.records[0] else {
        panic!()
    };
    assert!(!root.completeness.is_complete());
    assert!(!rows.completeness.is_complete());
    drop(locked);
    for module in [StorageModule::DiskAnalyzer, StorageModule::LargeFiles] {
        let s = StorageService::new();
        let p = f.protection();
        let root = s.authorize_root(&f.0.join("scan"), &p).unwrap();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (go_tx, go_rx) = std::sync::mpsc::channel();
        let id = s
            .start_with(
                module,
                Some(&root),
                StorageLimits::default(),
                Some(p.clone()),
                move |ctx| {
                    ready_tx.send(()).unwrap();
                    go_rx.recv().unwrap();
                    if module == StorageModule::DiskAnalyzer {
                        disk_analyzer::discover(ctx, &p, 1)
                    } else {
                        large_files::discover(ctx, &p, filter())
                    }
                    .map_err(Into::into)
                },
            )
            .unwrap();
        ready_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        s.cancel(&id).unwrap();
        go_tx.send(()).unwrap();
        let status = wait(&s, &id);
        assert_eq!(status.phase, StoragePhase::Cancelled);
        assert!(
            status
                .completeness
                .reasons
                .contains(&PartialReason::Cancelled)
        );
        assert_eq!(status.retained_records, 0);
    }
}
#[test]
fn step6_native_machine_roots_and_existing_protection_are_excluded() {
    use super::known_folders::{KnownFolder, KnownFolderResolver, NativeKnownFolders};
    let f = Fixture::new();
    let p = f.protection();
    let s = StorageService::new();
    for folder in [
        KnownFolder::ProgramFiles,
        KnownFolder::ProgramFilesX86,
        KnownFolder::ProgramData,
    ] {
        let root = NativeKnownFolders.resolve(folder).unwrap();
        assert!(!super::protection::personal_path_allowed(&root));
        assert!(!super::protection::personal_path_allowed(
            &root.join("never-created-fixture.txt")
        ));
        assert!(s.authorize_root(&root, &p).is_err());
    }
    for root in ["system", "documents", "app"] {
        assert!(s.authorize_root(&f.0.join(root), &p).is_err());
    }
    for path in [
        r"Z:\PROGRAM FILES\app\file",
        r"Z:\ProgramData\service",
        r"Z:\System Volume Information",
        r"Z:\$Recycle.Bin",
    ] {
        assert!(!super::protection::personal_path_allowed(
            std::path::Path::new(path)
        ));
    }
    assert!(super::protection::personal_path_allowed(&f.0.join("scan")));
}
