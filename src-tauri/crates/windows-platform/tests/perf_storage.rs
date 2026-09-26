#![cfg(windows)]
//! Opt-in runtime benchmarks over a generated corpus (scripts/perf/new-corpus.ps1).
//!
//! Run: `SUPA_PERF_CORPUS=<dir> cargo test -p windows-platform --test perf_storage -- --ignored --nocapture`
//! Optional: SUPA_PERF_RUNS (default 3), SUPA_PERF_WORKERS (default "1,2,4"),
//! SUPA_PERF_CANCEL_FRACTION (default 0.25), SUPA_PERF_OUT (JSON-lines file to append).
//! Every scenario runs sequentially in one test so measurements never overlap.
use cleanup_core::storage::large_files::{FileCategory, FileSort};
use std::{
    io::Write,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};
use windows_platform::{
    cleanup::discover_project_artifacts_with_workers,
    storage::{
        FileFilter, StorageLimits, StorageModule, StoragePhase, current_protection, disk_analyzer,
        empty_folders, large_files,
        scans::{JobError, StorageService},
    },
};

mod process {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First,
                Thread32Next,
            },
            ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
            Threading::{GetCurrentProcess, GetCurrentProcessId, GetProcessHandleCount},
        },
    };

    #[derive(Clone, Copy, Debug, Default)]
    pub struct Sample {
        pub working_set: u64,
        pub handles: u32,
        pub threads: u32,
    }

    pub fn working_set() -> u64 {
        // SAFETY: pseudo-handle for this process; the counters struct is sized by `cb`.
        unsafe {
            let mut counters: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
            let cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
            if K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, cb) == 0 {
                return 0;
            }
            counters.WorkingSetSize as u64
        }
    }

    pub fn handles() -> u32 {
        let mut count = 0_u32;
        // SAFETY: pseudo-handle for this process and a valid out pointer.
        unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count) };
        count
    }

    pub fn threads() -> u32 {
        // SAFETY: snapshot handle is closed on every path; entry size is initialised.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return 0;
            }
            let pid = GetCurrentProcessId();
            let mut entry: THREADENTRY32 = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
            let mut count = 0;
            let mut ok = Thread32First(snapshot, &mut entry);
            while ok != 0 {
                if entry.th32OwnerProcessID == pid {
                    count += 1;
                }
                ok = Thread32Next(snapshot, &mut entry);
            }
            CloseHandle(snapshot);
            count
        }
    }

    pub fn sample() -> Sample {
        Sample {
            working_set: working_set(),
            handles: handles(),
            threads: threads(),
        }
    }
}

#[derive(Default)]
struct Peak {
    working_set: u64,
    handles: u32,
    threads: u32,
}
impl Peak {
    fn observe(&mut self, sample: process::Sample) {
        self.working_set = self.working_set.max(sample.working_set);
        self.handles = self.handles.max(sample.handles);
        self.threads = self.threads.max(sample.threads);
    }
}

fn env_or<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn worker_counts() -> Vec<usize> {
    std::env::var("SUPA_PERF_WORKERS")
        .unwrap_or_else(|_| "1,2,4".into())
        .split(',')
        .filter_map(|part| part.trim().parse().ok())
        .filter(|workers| (1..=cleanup_core::storage::MAX_WORKERS).contains(workers))
        .collect()
}

fn corpus_bytes(root: &Path) -> Option<u64> {
    let text = std::fs::read_to_string(root.join("perf-manifest.json")).ok()?;
    let manifest: serde_json::Value =
        serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    manifest["counts"]["bytes"].as_u64()
}

fn emit(record: serde_json::Value) {
    // serde_json maps are BTreeMaps here, so keys serialize sorted (deterministic).
    let line = record.to_string();
    println!("PERF {line}");
    if let Ok(path) = std::env::var("SUPA_PERF_OUT") {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("open SUPA_PERF_OUT");
        writeln!(file, "{line}").expect("write SUPA_PERF_OUT");
    }
}

#[derive(Clone, Copy, Debug)]
enum Scenario {
    LargeFiles,
    DiskAnalyzer,
    EmptyFolders,
}
impl Scenario {
    fn name(self) -> &'static str {
        match self {
            Self::LargeFiles => "large-files",
            Self::DiskAnalyzer => "disk-analyzer",
            Self::EmptyFolders => "empty-folders",
        }
    }
    fn module(self) -> StorageModule {
        match self {
            Self::LargeFiles => StorageModule::LargeFiles,
            Self::DiskAnalyzer => StorageModule::DiskAnalyzer,
            Self::EmptyFolders => StorageModule::EmptyFolders,
        }
    }
}

fn start(service: &StorageService, scenario: Scenario, root: &Path, workers: usize) -> String {
    let protection = current_protection().expect("protection policy");
    let root_id = service
        .authorize_root_for(root, scenario.module(), &protection)
        .expect("authorize corpus root");
    let limits = StorageLimits {
        workers,
        ..Default::default()
    };
    let policy = protection.clone();
    service
        .start_with(
            scenario.module(),
            Some(&root_id),
            limits,
            Some(protection),
            move |context| {
                match scenario {
                    Scenario::LargeFiles => large_files::discover(
                        context,
                        &policy,
                        FileFilter {
                            minimum_bytes: 1 << 20,
                            maximum_bytes: None,
                            extensions: Vec::new(),
                            category: FileCategory::Any,
                            sort: FileSort::Size,
                            descending: true,
                        },
                    ),
                    Scenario::DiskAnalyzer => disk_analyzer::discover(context, &policy, 4),
                    Scenario::EmptyFolders => empty_folders::discover(context, &policy),
                }
                .map_err(JobError::Storage)
            },
        )
        .expect("start scan")
}

struct Observation {
    elapsed: Duration,
    first_progress: Option<Duration>,
    cancel_latency: Option<Duration>,
    completed_before_cancel: bool,
    visited: usize,
    phase: StoragePhase,
    peak: Peak,
}

fn observe(service: &StorageService, id: &str, cancel_after: Option<usize>) -> Observation {
    let started = Instant::now();
    let mut first_progress = None;
    let mut cancelled_at = None;
    let mut peak = Peak::default();
    let mut last_sample = Instant::now() - Duration::from_secs(1);
    loop {
        let (status, _) = service.status(id).expect("status");
        if last_sample.elapsed() >= Duration::from_millis(10) {
            peak.observe(process::sample());
            last_sample = Instant::now();
        }
        if first_progress.is_none() && status.visited_entries > 0 {
            first_progress = Some(started.elapsed());
        }
        if let Some(threshold) = cancel_after
            && cancelled_at.is_none()
            && status.visited_entries >= threshold
            && service.cancel(id).is_ok()
        {
            cancelled_at = Some(Instant::now());
        }
        if matches!(
            status.phase,
            StoragePhase::Complete | StoragePhase::Cancelled | StoragePhase::Failed
        ) {
            peak.observe(process::sample());
            return Observation {
                elapsed: started.elapsed(),
                first_progress,
                cancel_latency: cancelled_at.map(|at| at.elapsed()),
                completed_before_cancel: cancel_after.is_some()
                    && status.phase != StoragePhase::Cancelled,
                visited: status.visited_entries,
                phase: status.phase,
                peak,
            };
        }
        thread::sleep(Duration::from_millis(1));
    }
}

fn ms(duration: Duration) -> f64 {
    (duration.as_secs_f64() * 1000.0 * 10.0).round() / 10.0
}

fn run_storage(
    service: &StorageService,
    scenario: Scenario,
    root: &Path,
    workers: usize,
    run: usize,
    cancel_after: Option<usize>,
    bytes: Option<u64>,
) -> usize {
    let before = process::sample();
    let id = start(service, scenario, root, workers);
    let observation = observe(service, &id, cancel_after);
    service.release(&id).expect("release snapshot");
    let after = process::sample();
    let seconds = observation.elapsed.as_secs_f64().max(1e-9);
    let kind = if cancel_after.is_some() {
        "cancel"
    } else {
        "full"
    };
    emit(serde_json::json!({
        "kind": kind,
        "scenario": scenario.name(),
        "workers": workers,
        "run": run,
        "phase": format!("{:?}", observation.phase),
        "elapsedMs": ms(observation.elapsed),
        "visitedEntries": observation.visited,
        "entriesPerSec": (observation.visited as f64 / seconds).round(),
        "bytesPerSec": if cancel_after.is_none() { bytes.map(|b| (b as f64 / seconds).round()) } else { None },
        "firstProgressMs": observation.first_progress.map(ms),
        "cancelLatencyMs": observation.cancel_latency.map(ms),
        "completedBeforeCancel": observation.completed_before_cancel,
        "workingSetBefore": before.working_set,
        "workingSetPeak": observation.peak.working_set,
        "workingSetAfter": after.working_set,
        "handlesBefore": before.handles,
        "handlesPeak": observation.peak.handles,
        "handlesAfter": after.handles,
        "threadsBefore": before.threads,
        "threadsPeak": observation.peak.threads,
        "threadsAfter": after.threads,
    }));
    observation.visited
}

fn run_project_discovery(root: &Path, workers: usize, run: usize) {
    let before = process::sample();
    let root_text = root.to_string_lossy().into_owned();
    let started = Instant::now();
    let worker =
        thread::spawn(move || discover_project_artifacts_with_workers(&root_text, workers));
    let mut peak = Peak::default();
    while !worker.is_finished() {
        peak.observe(process::sample());
        thread::sleep(Duration::from_millis(10));
    }
    let elapsed = started.elapsed();
    let discovery = worker.join().expect("join").expect("project discovery");
    let after = process::sample();
    emit(serde_json::json!({
        "kind": "full",
        "scenario": "project-discovery",
        "workers": workers,
        "run": run,
        "elapsedMs": ms(elapsed),
        "records": discovery.records.len(),
        "diagnostics": discovery.diagnostics.len(),
        "workingSetBefore": before.working_set,
        "workingSetPeak": peak.working_set,
        "workingSetAfter": after.working_set,
        "handlesBefore": before.handles,
        "handlesPeak": peak.handles,
        "handlesAfter": after.handles,
        "threadsBefore": before.threads,
        "threadsPeak": peak.threads,
        "threadsAfter": after.threads,
    }));
}

#[test]
#[ignore = "opt-in benchmark; set SUPA_PERF_CORPUS"]
fn perf_storage_suite() {
    let Some(root) = std::env::var_os("SUPA_PERF_CORPUS").map(PathBuf::from) else {
        eprintln!("SUPA_PERF_CORPUS is unset; skipping perf_storage_suite");
        return;
    };
    assert!(
        root.join(".perf-fixture").is_file(),
        "corpus must be a marked perf fixture"
    );
    let root = std::fs::canonicalize(&root).expect("canonical corpus");
    let runs: usize = env_or("SUPA_PERF_RUNS", 3).max(1);
    let fraction: f64 = env_or("SUPA_PERF_CANCEL_FRACTION", 0.25_f64).clamp(0.01, 0.95);
    let workers = worker_counts();
    assert!(!workers.is_empty(), "no valid worker counts");
    let bytes = corpus_bytes(&root);

    let baseline = process::sample();
    let service = StorageService::new();
    for scenario in [
        Scenario::LargeFiles,
        Scenario::DiskAnalyzer,
        Scenario::EmptyFolders,
    ] {
        // Discarded warm-up so every measured run sees the same warm file cache.
        let total = run_storage(&service, scenario, &root, workers[0], 0, None, bytes);
        for &count in &workers {
            for run in 1..=runs {
                run_storage(&service, scenario, &root, count, run, None, bytes);
            }
        }
        let threshold = ((total as f64) * fraction).max(1.0) as usize;
        for &count in &workers {
            for run in 1..=runs {
                run_storage(
                    &service,
                    scenario,
                    &root,
                    count,
                    run,
                    Some(threshold),
                    bytes,
                );
            }
        }
    }
    run_project_discovery(&root, workers[0], 0);
    for &count in &workers {
        for run in 1..=runs {
            run_project_discovery(&root, count, run);
        }
    }
    drop(service);
    thread::sleep(Duration::from_millis(200));
    let end = process::sample();
    emit(serde_json::json!({
        "kind": "process",
        "scenario": "suite",
        "handlesStart": baseline.handles,
        "handlesEnd": end.handles,
        "threadsStart": baseline.threads,
        "threadsEnd": end.threads,
        "workingSetStart": baseline.working_set,
        "workingSetEnd": end.working_set,
    }));
}
