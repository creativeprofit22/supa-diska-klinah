#![cfg(windows)]
//! Machine-independent regression guards from docs/performance.md ("Regression guards").
//! Runs in normal `cargo test`. It is the only test in this binary because handle counts are
//! process-wide and a concurrent test would perturb them.
use cleanup_core::storage::large_files::{FileCategory, FileSort};
use std::{
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};
use windows_platform::storage::{
    FileFilter, StorageLimits, StorageModule, StoragePhase, current_protection, large_files,
    scans::{JobError, StorageService},
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessHandleCount};

/// Generous bound; measured medians are ~20-50 ms on the reference machine.
const CANCEL_BOUND: Duration = Duration::from_secs(2);
/// Covers lazily created runtime handles (thread pool, events) that are not per-scan leaks.
const HANDLE_TOLERANCE: u32 = 16;
const CYCLES: usize = 6;

fn handles() -> u32 {
    let mut count = 0;
    // SAFETY: pseudo-handle for this process and a valid out pointer.
    assert_ne!(
        unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count) },
        0
    );
    count
}

struct Tree(PathBuf);
impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn synthetic_tree() -> Tree {
    let root = std::env::temp_dir().join(format!(
        "supa-diska-scan-guard-{}-{}",
        std::process::id(),
        getrandom::u64().unwrap()
    ));
    for directory in 0..40 {
        let dir = root.join(format!("d{directory:02}"));
        std::fs::create_dir_all(&dir).unwrap();
        for file in 0..100 {
            std::fs::write(dir.join(format!("f{file:03}.dat")), [directory as u8; 64]).unwrap();
        }
    }
    Tree(std::fs::canonicalize(root).unwrap())
}

/// Starts a large-files scan, cancels it after `cancel_after` visited entries, and returns
/// the cancel-to-terminal latency when the cancel landed before completion.
fn scan_and_cancel(service: &StorageService, root: &Path, cancel_after: usize) -> Option<Duration> {
    let protection = current_protection().unwrap();
    let root_id = service
        .authorize_root_for(root, StorageModule::LargeFiles, &protection)
        .unwrap();
    let policy = protection.clone();
    let id = service
        .start_with(
            StorageModule::LargeFiles,
            Some(&root_id),
            StorageLimits::default(),
            Some(protection),
            move |context| {
                large_files::discover(
                    context,
                    &policy,
                    FileFilter {
                        minimum_bytes: 0,
                        maximum_bytes: None,
                        extensions: Vec::new(),
                        category: FileCategory::Any,
                        sort: FileSort::Size,
                        descending: true,
                    },
                )
                .map_err(JobError::Storage)
            },
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut cancelled_at = None;
    let latency = loop {
        assert!(Instant::now() < deadline, "scan did not finish within 60 s");
        let (status, _) = service.status(&id).unwrap();
        if cancelled_at.is_none()
            && status.visited_entries >= cancel_after
            && service.cancel(&id).is_ok()
        {
            cancelled_at = Some(Instant::now());
        }
        match status.phase {
            StoragePhase::Cancelled => break cancelled_at.map(|at| at.elapsed()),
            StoragePhase::Complete | StoragePhase::Failed => break None,
            _ => thread::sleep(Duration::from_millis(1)),
        }
    };
    service.release(&id).unwrap();
    latency
}

#[test]
fn scan_cancellation_is_prompt_and_scan_cycles_do_not_leak_handles() {
    let tree = synthetic_tree();
    let service = StorageService::new();
    // Warm-up cycle: lets one-time runtime handles appear before the baseline sample.
    scan_and_cancel(&service, &tree.0, 200);
    let before = handles();

    let mut cancelled = 0;
    for _ in 0..CYCLES {
        if let Some(latency) = scan_and_cancel(&service, &tree.0, 200) {
            cancelled += 1;
            assert!(latency <= CANCEL_BOUND, "cancellation took {latency:?}");
        }
    }
    assert!(cancelled > 0, "no cycle was cancelled before completion");
    // Scan threads exit asynchronously after reaching a terminal phase.
    let settle = Instant::now() + Duration::from_secs(5);
    let mut after = handles();
    while after > before + HANDLE_TOLERANCE && Instant::now() < settle {
        thread::sleep(Duration::from_millis(50));
        after = handles();
    }
    assert!(
        after <= before + HANDLE_TOLERANCE,
        "handle count grew from {before} to {after} over {CYCLES} scan/cancel cycles"
    );
}
