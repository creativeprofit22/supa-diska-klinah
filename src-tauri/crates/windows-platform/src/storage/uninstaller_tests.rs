use super::scans::{Clock, JobError, StorageService};
use super::uninstaller::*;
use cleanup_core::storage::*;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use windows_sys::Win32::System::Registry::{REG_DWORD, REG_SZ};

#[derive(Default)]
struct Time(AtomicU64);
impl Clock for Time {
    fn now(&self) -> Duration {
        Duration::from_secs(self.0.load(Ordering::Acquire))
    }
}
struct Registry {
    count: usize,
    reads: AtomicUsize,
    block: AtomicBool,
    entered: AtomicBool,
}
fn text_value(s: &str) -> RegistryValue {
    RegistryValue {
        kind: REG_SZ,
        bytes: s
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect(),
    }
}
impl RegistryReader for Registry {
    fn keys(&self, hive: Hive, view: View) -> Result<Vec<String>, InventoryError> {
        Ok(if hive == Hive::Machine && view == View::Native64 {
            (0..self.count).map(|i| format!("program-{i:04}")).collect()
        } else {
            Vec::new()
        })
    }
    fn value(
        &self,
        key: &RegistryLocation,
        name: &str,
    ) -> Result<Option<RegistryValue>, InventoryError> {
        self.reads.fetch_add(1, Ordering::AcqRel);
        if key.subkey == "program-0031" && name == "Publisher" && self.block.load(Ordering::Acquire)
        {
            self.entered.store(true, Ordering::Release);
            until(|| !self.block.load(Ordering::Acquire));
        }
        Ok(match name {
            "DisplayName" => Some(text_value(&key.subkey)),
            "EstimatedSize" => Some(RegistryValue {
                kind: REG_DWORD,
                bytes: key.subkey[8..]
                    .parse::<u32>()
                    .unwrap()
                    .to_le_bytes()
                    .to_vec(),
            }),
            _ => None,
        })
    }
}
fn fixture(count: usize) -> Arc<Registry> {
    Arc::new(Registry {
        count,
        reads: AtomicUsize::new(0),
        block: AtomicBool::new(false),
        entered: AtomicBool::new(false),
    })
}
fn until(mut condition: impl FnMut() -> bool) {
    let start = Instant::now();
    while !condition() {
        assert!(start.elapsed() < Duration::from_secs(15));
        std::thread::park_timeout(Duration::from_millis(2));
    }
}
fn finish(service: &StorageService, id: &str) -> StorageStatus {
    until(|| {
        matches!(
            service.status(id).unwrap().0.phase,
            StoragePhase::Complete | StoragePhase::Cancelled | StoragePhase::Failed
        )
    });
    service.status(id).unwrap().0
}
fn request(id: &str) -> PageRequest {
    PageRequest {
        snapshot_id: id.into(),
        module: StorageModule::Uninstaller,
        collection: PageCollection::Programs,
        parent_id: None,
        cursor: None,
        page_size: 200,
    }
}
fn ids(page: &StoragePage) -> Vec<String> {
    page.records
        .iter()
        .map(|r| match r {
            StorageRecord::Program(p) => p.program_id.clone(),
            _ => panic!("not program"),
        })
        .collect()
}

#[test]
fn immutable_program_pages_filter_sort_cursor_and_expiring_authority() {
    let time = Arc::new(Time::default());
    let service = StorageService::with_clock(time.clone());
    let registry = fixture(450);
    let id = service
        .start_programs_with(
            StorageLimits::default(),
            ProgramQuery::default(),
            registry.clone(),
        )
        .unwrap();
    let status = finish(&service, &id);
    assert_eq!(status.visited_entries, 450);
    assert_eq!(status.retained_records, 450);
    assert!(status.completeness.is_complete());
    let mut query = request(&id);
    let first = service.page(&query).unwrap();
    assert_eq!(first.records.len(), 200);
    assert_eq!(ids(&first), ids(&service.page(&query).unwrap()));
    let first_id = ids(&first)[0].clone();
    assert_eq!(
        service
            .resolve_program(&id, &first_id)
            .unwrap()
            .display
            .name,
        "program-0000"
    );
    assert!(service.resolve_program(&id, &"f".repeat(32)).is_err());
    query.cursor = first.next_cursor.clone();
    let second = service.page(&query).unwrap();
    assert_eq!(second.records.len(), 200);
    query.cursor = second.next_cursor.clone();
    let third = service.page(&query).unwrap();
    assert_eq!(third.records.len(), 50);
    assert!(third.next_cursor.is_none());
    let all: std::collections::HashSet<_> = ids(&first)
        .into_iter()
        .chain(ids(&second))
        .chain(ids(&third))
        .collect();
    assert_eq!(all.len(), 450);
    query.page_size = 201;
    assert!(service.page(&query).is_err());
    query.page_size = 200;
    query.parent_id = Some("e".repeat(32));
    assert!(service.page(&query).is_err());
    query.parent_id = None;
    query.module = StorageModule::Drives;
    assert!(service.page(&query).is_err());
    let other = service
        .start_programs_with(
            StorageLimits::default(),
            ProgramQuery {
                name_contains: "program-00".into(),
                largest_first: true,
            },
            registry,
        )
        .unwrap();
    finish(&service, &other);
    query = request(&other);
    query.cursor = first.next_cursor.clone();
    assert!(service.page(&query).is_err());
    query.cursor = None;
    let filtered = service.page(&query).unwrap();
    assert_eq!(filtered.records.len(), 100);
    let StorageRecord::Program(highest) = &filtered.records[0] else {
        unreachable!()
    };
    assert_eq!(highest.name, "program-0099");
    assert_eq!(ids(&first), ids(&service.page(&request(&id)).unwrap()));
    assert!(service.resolve_program(&other, &first_id).is_err());
    time.0.store(SNAPSHOT_IDLE_SECONDS + 1, Ordering::Release);
    assert!(service.page(&request(&id)).is_err());
    assert!(service.resolve_program(&id, &first_id).is_err());
    assert!(
        service
            .start_programs_with(
                StorageLimits::default(),
                ProgramQuery {
                    name_contains: "x".repeat(129),
                    largest_first: false
                },
                fixture(1)
            )
            .is_err()
    );
}

#[test]
fn cancellation_during_metadata_has_truthful_progress_and_no_authority() {
    let service = StorageService::new();
    let registry = fixture(450);
    registry.block.store(true, Ordering::Release);
    let id = service
        .start_programs_with(
            StorageLimits::default(),
            ProgramQuery::default(),
            registry.clone(),
        )
        .unwrap();
    until(|| registry.entered.load(Ordering::Acquire));
    let status = service.status(&id).unwrap().0;
    assert_eq!(status.visited_entries, 32);
    assert_eq!(status.retained_records, 31);
    assert_eq!(status.phase, StoragePhase::Walking);
    assert!(matches!(
        service.start_programs_with(
            StorageLimits::default(),
            ProgramQuery::default(),
            fixture(1)
        ),
        Err(JobError::Busy)
    ));
    assert!(matches!(
        service.start_with(
            StorageModule::Drives,
            None,
            StorageLimits::default(),
            None,
            |_| Ok(())
        ),
        Err(JobError::Busy)
    ));
    service.cancel(&id).unwrap();
    let reads = registry.reads.load(Ordering::Acquire);
    registry.block.store(false, Ordering::Release);
    let status = finish(&service, &id);
    assert_eq!(status.phase, StoragePhase::Cancelled);
    assert!(
        status
            .completeness
            .reasons
            .contains(&PartialReason::Cancelled)
    );
    assert_eq!(registry.reads.load(Ordering::Acquire), reads);
    let page = service.page(&request(&id)).unwrap();
    assert_eq!(page.records.len(), 31);
    assert!(service.resolve_program(&id, &ids(&page)[0]).is_err());
    let second = service
        .start_programs_with(
            StorageLimits::default(),
            ProgramQuery::default(),
            fixture(1),
        )
        .unwrap();
    finish(&service, &second);
    let page = service.page(&request(&second)).unwrap();
    service.release(&second).unwrap();
    assert!(service.resolve_program(&second, &ids(&page)[0]).is_err());
}

#[test]
fn program_scan_caps_are_backend_caps_and_never_cleanup_evidence() {
    let service = StorageService::new();
    for (limits, reason, expected) in [
        (
            StorageLimits {
                visited_entries: 20,
                ..StorageLimits::default()
            },
            PartialReason::EntryLimit,
            20,
        ),
        (
            StorageLimits {
                retained_records: 15,
                ..StorageLimits::default()
            },
            PartialReason::RecordLimit,
            15,
        ),
    ] {
        let id = service
            .start_programs_with(limits, ProgramQuery::default(), fixture(450))
            .unwrap();
        let status = finish(&service, &id);
        assert!(status.completeness.reasons.contains(&reason));
        let page = service.page(&request(&id)).unwrap();
        assert_eq!(page.records.len(), expected);
        for record in page.records {
            let StorageRecord::Program(program) = record else {
                unreachable!()
            };
            assert_eq!(program.leftover_support, "unsupportedUnknownOwnership");
        }
    }
}
