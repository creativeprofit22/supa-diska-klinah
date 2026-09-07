use super::*;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc,
};
#[derive(Default)]
struct TestClock(AtomicU64);
impl Clock for TestClock {
    fn now(&self) -> Duration {
        Duration::from_secs(self.0.load(Ordering::SeqCst))
    }
}
#[test]
fn completion_after_maintenance_clock_sample_is_not_expired() {
    let clock = Arc::new(TestClock(AtomicU64::new(1)));
    let service = StorageService::with_clock(clock);
    let id = rows(&service, 1, 1);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let mut state = service.state.lock().unwrap();
        if state.active.as_ref().is_none_or(|a| a.worker.is_finished()) {
            state.maintain(Duration::ZERO);
            assert!(state.completed.iter().any(|c| c.status.snapshot_id == id));
            break;
        }
        assert!(Instant::now() < deadline);
        drop(state);
        thread::yield_now();
    }
    service.page(&request(&id)).unwrap();
}
fn wait(service: &StorageService, id: &str) -> StorageStatus {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        // Wait for actual publication, not merely the worker's last progress update.
        {
            let mut state = service.state.lock().unwrap();
            state.maintain(service.clock.now());
            if let Some(c) = state.completed.iter().find(|c| c.status.snapshot_id == id) {
                return c.status.clone();
            }
        }
        assert!(Instant::now() < deadline, "worker did not finish");
        thread::yield_now();
    }
}
fn request(id: &str) -> PageRequest {
    PageRequest {
        snapshot_id: id.into(),
        module: StorageModule::Drives,
        collection: PageCollection::Drives,
        parent_id: None,
        cursor: None,
        page_size: 1,
    }
}
fn rows(service: &StorageService, count: usize, limit: usize) -> String {
    service
        .start_with(
            StorageModule::Drives,
            None,
            StorageLimits {
                retained_records: limit,
                ..Default::default()
            },
            None,
            move |ctx| {
                for n in (0..count).rev() {
                    let record = StorageRecord::Drive(analysis::DriveSummary {
                        drive_id: opaque_id()?,
                        label: n.to_string(),
                        filesystem: "fixture".into(),
                        total_bytes: 10,
                        free_bytes: 3,
                        used_bytes: 7,
                        system: Some(false),
                    });
                    if let Err(e) = ctx.push(
                        record,
                        RecordOrder {
                            numeric: n as u64,
                            text: String::new(),
                        },
                    ) {
                        if e == StorageError::LimitReached {
                            break;
                        }
                        return Err(e.into());
                    }
                }
                Ok(())
            },
        )
        .unwrap()
}
#[test]
fn paging_bounds_cursors_release_and_two_snapshot_retention() {
    let service = StorageService::new();
    let first = rows(&service, 3, 2);
    assert_eq!(wait(&service, &first).retained_records, 2);
    let page = service.page(&request(&first)).unwrap();
    assert!(
        page.completeness
            .reasons
            .contains(&PartialReason::RecordLimit)
    );
    assert_eq!(page.retained_total, 2);
    assert!(matches!(&page.records[0], StorageRecord::Drive(d) if d.label == "1"));
    let mut next = request(&first);
    next.cursor = page.next_cursor.clone();
    assert_eq!(service.page(&next).unwrap().records.len(), 1);
    assert_eq!(
        serde_json::to_string(&service.page(&next).unwrap()).unwrap(),
        serde_json::to_string(&service.page(&next).unwrap()).unwrap()
    );
    next.cursor = Some("f".repeat(32));
    assert!(service.page(&next).is_err());
    next = request(&first);
    next.page_size = 201;
    assert!(service.page(&next).is_err());
    next = request(&first);
    next.module = StorageModule::LargeFiles;
    assert!(service.page(&next).is_err());
    next = request(&first);
    next.parent_id = Some("a".repeat(32));
    assert!(service.page(&next).is_err());
    let second = rows(&service, 2, 2);
    wait(&service, &second);
    next = request(&second);
    next.cursor = page.next_cursor;
    assert!(service.page(&next).is_err());
    let third = rows(&service, 1, 1);
    wait(&service, &third);
    assert!(service.status(&first).is_err());
    assert!(service.page(&request(&first)).is_err());
    assert_eq!(service.state.lock().unwrap().completed.len(), 2);
    service.release(&second).unwrap();
    assert!(service.page(&request(&second)).is_err());
    assert!(service.release(&second).is_err());
    let bad = StorageLimits {
        workers: 5,
        ..Default::default()
    };
    assert!(service.start_drives(bad).is_err());
    assert!(
        service
            .start_drives(StorageLimits {
                retained_records: MAX_RECORDS + 1,
                ..Default::default()
            })
            .is_err()
    );
}
#[test]
fn inactivity_status_refresh_and_exact_expiry() {
    let clock = Arc::new(TestClock::default());
    let service = StorageService::with_clock(clock.clone());
    let id = rows(&service, 2, 2);
    wait(&service, &id);
    clock.0.store(599, Ordering::SeqCst);
    service.status(&id).unwrap();
    clock.0.store(1198, Ordering::SeqCst);
    service.page(&request(&id)).unwrap();
    clock.0.store(1798, Ordering::SeqCst);
    assert!(service.status(&id).is_err());
    assert!(service.page(&request(&id)).is_err());
    assert!(service.state.lock().unwrap().completed.is_empty());
}
#[test]
fn overlap_cancel_release_do_not_free_slot_before_worker_exit() {
    let service = StorageService::new();
    let (entered, ready) = mpsc::channel();
    let (go, gate) = mpsc::channel();
    let id = service
        .start_with(
            StorageModule::Drives,
            None,
            Default::default(),
            None,
            move |ctx| {
                entered.send(()).unwrap();
                gate.recv().unwrap();
                assert!(ctx.cancellation.is_cancelled());
                Ok(())
            },
        )
        .unwrap();
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(matches!(
        service.start_drives(Default::default()),
        Err(JobError::Busy)
    ));
    // Status/cancel/release return while the callback is blocked: no service lock is held.
    service.status(&id).unwrap();
    service.cancel(&id).unwrap();
    service.release(&id).unwrap();
    assert!(service.status(&id).is_err());
    assert!(matches!(
        service.start_drives(Default::default()),
        Err(JobError::Busy)
    ));
    go.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let mut state = service.state.lock().unwrap();
        state.maintain(service.clock.now());
        if state.active.is_none() {
            break;
        }
        assert!(Instant::now() < deadline);
        drop(state);
        thread::yield_now();
    }
    assert!(service.page(&request(&id)).is_err());
    let id = service
        .start_with(
            StorageModule::Drives,
            None,
            Default::default(),
            None,
            |ctx| {
                ctx.cancellation.cancel();
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(wait(&service, &id).phase, StoragePhase::Cancelled);
    assert!(
        service
            .page(&request(&id))
            .unwrap()
            .completeness
            .reasons
            .contains(&PartialReason::Cancelled)
    );
}
#[test]
fn drop_joins_cooperative_worker_and_failure_is_explicit() {
    let service = StorageService::new();
    let (tx, rx) = mpsc::channel();
    let (dropped, observed) = mpsc::channel();
    service
        .start_with(
            StorageModule::Drives,
            None,
            Default::default(),
            None,
            move |ctx| {
                tx.send(()).unwrap();
                while !ctx.cancellation.is_cancelled() {
                    thread::yield_now();
                }
                dropped.send(()).unwrap();
                Ok(())
            },
        )
        .unwrap();
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    drop(service);
    observed.try_recv().unwrap();
    let service = StorageService::new();
    let id = service
        .start_with(
            StorageModule::Drives,
            None,
            Default::default(),
            None,
            |_| Err(JobError::Native("fixture native error".into())),
        )
        .unwrap();
    assert_eq!(wait(&service, &id).phase, StoragePhase::Failed);
    assert!(
        service
            .status(&id)
            .unwrap()
            .1
            .unwrap()
            .contains("fixture native error")
    );
    assert!(service.page(&request(&id)).is_err());
    let id = service
        .start_with(
            StorageModule::Drives,
            None,
            Default::default(),
            None,
            |_| panic!("fixture panic"),
        )
        .unwrap();
    assert_eq!(wait(&service, &id).phase, StoragePhase::Failed);
}
struct Fixture {
    base: std::path::PathBuf,
    root: std::path::PathBuf,
    policy: ProtectionPolicy,
}
impl Fixture {
    fn new() -> Self {
        let base = std::env::temp_dir().join(format!("storage-job-{}", opaque_id().unwrap()));
        let root = base.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let paths: Vec<_> = ["system", "durable", "configured"]
            .iter()
            .map(|n| {
                let p = base.join(n);
                std::fs::create_dir(&p).unwrap();
                p
            })
            .collect();
        let inputs = cleanup_core::ProtectionInputs::new(
            vec![paths[0].clone()],
            vec![paths[1].clone()],
            vec![paths[2].clone()],
        )
        .unwrap();
        let policy = ProtectionPolicy::compile(&WindowsFileSystem, inputs).unwrap();
        Self { base, root, policy }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.base).unwrap();
    }
}
#[test]
fn root_registry_is_bounded_consumed_snapshot_bound_and_revalidated() {
    let f = Fixture::new();
    let clock = Arc::new(TestClock::default());
    let service = StorageService::with_clock(clock.clone());
    let root = service.authorize_root(&f.root, &f.policy).unwrap();
    let spare = service.authorize_root(&f.root, &f.policy).unwrap();
    assert!(service.authorize_root(&f.root, &f.policy).is_err());
    service.release(&spare).unwrap();
    let id = service
        .start_with(
            StorageModule::EmptyFolders,
            Some(&root),
            Default::default(),
            Some(f.policy.clone()),
            |_| Ok(()),
        )
        .unwrap();
    wait(&service, &id);
    assert_eq!(
        service
            .resolve_root(&id, &root, &f.policy)
            .unwrap()
            .snapshot_id,
        id
    );
    assert!(
        service
            .resolve_root(&"a".repeat(32), &root, &f.policy)
            .is_err()
    );
    assert!(service.resolve_root(&id, &spare, &f.policy).is_err());
    assert!(
        service
            .start_with(
                StorageModule::EmptyFolders,
                Some(&root),
                Default::default(),
                Some(f.policy.clone()),
                |_| Ok(())
            )
            .is_err()
    );
    let pending = service.authorize_root(&f.root, &f.policy).unwrap();
    std::fs::rename(&f.root, f.base.join("old")).unwrap();
    std::fs::create_dir(&f.root).unwrap();
    assert!(service.resolve_root(&id, &root, &f.policy).is_err());
    let stale = service
        .start_with(
            StorageModule::EmptyFolders,
            Some(&pending),
            Default::default(),
            Some(f.policy.clone()),
            |_| panic!("stale root runner must not run"),
        )
        .unwrap();
    assert_eq!(wait(&service, &stale).phase, StoragePhase::Failed);
    let expired = service.authorize_root(&f.root, &f.policy).unwrap();
    clock.0.store(600, Ordering::SeqCst);
    assert!(
        service
            .start_with(
                StorageModule::EmptyFolders,
                Some(&expired),
                Default::default(),
                Some(f.policy.clone()),
                |_| Ok(())
            )
            .is_err()
    );
    for path in [r"\\server\share", r"\\.\C:\", r"C:\root:stream"] {
        assert!(service.authorize_root(Path::new(path), &f.policy).is_err());
    }
    assert!(
        service
            .authorize_root(&f.base.join("system"), &f.policy)
            .is_err()
    );
    let link = f.base.join("link");
    junction::create(&f.root, &link).unwrap();
    assert!(service.authorize_root(&link, &f.policy).is_err());
    junction::delete(link).unwrap();
}
#[test]
fn shared_walk_progress_and_entry_cancellation() {
    let f = Fixture::new();
    for n in 0..4 {
        std::fs::write(f.root.join(n.to_string()), b"test").unwrap();
    }
    let service = StorageService::new();
    let root = service.authorize_root(&f.root, &f.policy).unwrap();
    let policy = f.policy.clone();
    let id = service
        .start_with(
            StorageModule::EmptyFolders,
            Some(&root),
            Default::default(),
            Some(policy.clone()),
            move |ctx| {
                let report = ctx.walk(&policy, &|_| false, &mut |ctx, event| {
                    if matches!(event, walk::WalkEvent::Entry { .. }) {
                        ctx.cancellation.cancel();
                    }
                    walk::WalkControl::Continue
                })?;
                assert!(report.visited_entries <= 2);
                Ok(())
            },
        )
        .unwrap();
    let status = wait(&service, &id);
    assert_eq!(status.phase, StoragePhase::Cancelled);
    assert!(status.visited_entries > 0);
    assert!(
        status
            .completeness
            .reasons
            .contains(&PartialReason::Cancelled)
    );
}
#[test]
fn system_identity_failure_retains_capacity_and_denies_authority() {
    let service = StorageService::new();
    let id = service
        .start_drives_with_system(
            StorageLimits {
                diagnostics: 1,
                ..Default::default()
            },
            || Err(std::io::Error::other("injected system identity failure")),
        )
        .unwrap();
    let status = wait(&service, &id);
    assert_eq!(status.phase, StoragePhase::Complete);
    assert!(service.status(&id).unwrap().1.is_none());
    assert!(
        status
            .completeness
            .reasons
            .contains(&PartialReason::Unreadable)
    );
    let mut request = request(&id);
    request.page_size = MAX_PAGE_SIZE;
    let page = service.page(&request).unwrap();
    assert!(
        !page.records.is_empty(),
        "readable native capacities must survive"
    );
    assert!(
        page.completeness
            .reasons
            .contains(&PartialReason::Unreadable)
    );
    let issues = service.drive_issues(&id).unwrap();
    assert_eq!(issues.len(), 1);
    assert!(
        issues[0].mount.is_none(),
        "identity warning is inventory-wide"
    );
    let protected = std::env::current_dir().unwrap();
    let protection = ProtectionPolicy::compile(
        &crate::WindowsFileSystem,
        cleanup_core::ProtectionInputs::new(
            vec![protected.clone()],
            vec![protected.parent().unwrap().to_owned()],
            vec![protected.parent().unwrap().parent().unwrap().to_owned()],
        )
        .unwrap(),
    )
    .unwrap();
    for record in page.records {
        let StorageRecord::Drive(drive) = record else {
            panic!()
        };
        assert!(drive.total_bytes > 0);
        assert_eq!(drive.total_bytes - drive.free_bytes, drive.used_bytes);
        assert_eq!(
            serde_json::to_value(&drive).unwrap()["system"],
            serde_json::Value::Null
        );
        assert!(service.resolve_drive(&id, &drive.drive_id).is_err());
        assert!(
            service
                .resolve_root(&id, &drive.drive_id, &protection)
                .is_err()
        );
    }
}

#[test]
fn native_drive_runner_pages_and_bounds_authorizations_and_reports_failures() {
    let service = StorageService::new();
    let id = service.start_drives(Default::default()).unwrap();
    let status = wait(&service, &id);
    assert_eq!(status.phase, StoragePhase::Complete);
    let mut req = request(&id);
    req.page_size = MAX_PAGE_SIZE;
    let page = service.page(&req).unwrap();
    assert!(!page.records.is_empty());
    for record in page.records {
        let StorageRecord::Drive(drive) = record else {
            panic!()
        };
        let current = service.resolve_drive(&id, &drive.drive_id).unwrap();
        assert_eq!(current.total_bytes - current.free_bytes, current.used_bytes);
    }
    let issues = service.drive_issues(&id).unwrap();
    if !issues.is_empty() {
        assert!(
            status
                .completeness
                .reasons
                .contains(&PartialReason::Unreadable)
        );
    }
    for issue in &issues {
        assert!(!issue.error.is_empty());
        eprintln!(
            "explicit native drive outcome {:?}: {}",
            issue.mount, issue.error
        );
    }
    let state = service.state.lock().unwrap();
    let done = state
        .completed
        .iter()
        .find(|c| c.status.snapshot_id == id)
        .unwrap();
    assert!(done.drives.len() <= status.retained_records);
    assert!(done.drives.len() <= 26);
    drop(state);
    service.release(&id).unwrap();
    assert!(service.drive_issues(&id).is_err());
    let id = service
        .start_drives(StorageLimits {
            retained_records: 1,
            diagnostics: 1,
            ..Default::default()
        })
        .unwrap();
    wait(&service, &id);
    let state = service.state.lock().unwrap();
    let done = state
        .completed
        .iter()
        .find(|c| c.status.snapshot_id == id)
        .unwrap();
    assert!(done.drives.len() <= 1);
    assert!(done.drive_issues.len() <= 1);
}
