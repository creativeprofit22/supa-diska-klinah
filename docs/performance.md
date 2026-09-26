# Performance: methodology, results and budgets

This page says how to measure compile speed, scan speed, resource use, cancellation and real disk reclamation on Windows. It records what was measured and which optimizations were kept or rejected, and why. Dated runs with execution IDs are in [performance verification](verification/performance.md).

Rule: **optimize only what a measurement shows**, and give every accepted change a before/after table and a regression guard.

## Methodology

- Every script writes canonical, sorted JSON to `.gg/perf-artifacts/<utc-stamp>-<kind>/` (gitignored). Each run records its run ID, git SHA and dirty flag; OS build, CPU, RAM and power plan; Node, pnpm, rustc and cargo versions; volume media and bus type; and whether Defender real-time protection is on.
- Scenarios repeat N times (default 5) after one discarded warm-up. We report **min, median and p90** (nearest rank, never interpolated). Budgets use the median, or p90 for latency.
- **Contention and sleep are recorded, not hidden.** A background monitor samples every 5 s for `cargo`/`rustc` processes outside the benchmark's own process tree. Compile runs also check before each iteration. Windows sleep/resume events (Kernel-Power 42/107) during a run go into `sleepEvents`. The budget checker marks machine-speed results from a run that slept as `invalid`.
- Benchmarks never touch developer state:
  - Compile runs build into an isolated, marked target (`.gg/perf-target/<label>`) and never into `src-tauri/target`, and never run `cargo clean`.
  - Edit scenarios only edit files that are clean in git. They restore the file in `finally` and fail closed if the SHA-256 differs afterwards.
  - Corpora, compile targets and disk payloads live in folders marked `.perf-fixture` (disk payloads use `%TEMP%\supa-diska-perf-disk-*`). Removal refuses unmarked or reparse-point folders and volume roots.
- Local compile runs assert that `CARGO_INCREMENTAL` is unset or `1` and that no `RUSTC_WRAPPER` is set.

## Hardware

Reference machine (the "machine class" for time budgets):

| Item | Value |
| --- | --- |
| CPU | Intel Core i7-8700 @ 3.20 GHz, 6 cores / 12 threads |
| RAM | 24 GiB |
| OS | Windows 10 Pro 22H2, build 19045; power plan Balanced |
| SSD volume (C:) | Kingston SUV400S37 240 GB, SATA SSD |
| HDD volume (E:, repository) | Toshiba DT01ACA100 1 TB, SATA 7200 rpm HDD |
| Defender real-time protection | On (never excluded automatically) |
| Toolchain | Node 24.19.0, pnpm 11.22.0, rustc/cargo 1.90.0 (pinned by `src-tauri/rust-toolchain.toml`; runs before 2026-09-25 15:06 UTC recorded the machine default 1.97.1 by mistake, because the version was read outside `src-tauri`, but their builds used 1.90.0) |

The repository, and therefore every compile benchmark, is on the **HDD**.

## Corpora

`scripts/perf/new-corpus.ps1` builds deterministic trees: the same size and seed always give the same names, sizes and bytes. Each writes `perf-manifest.json`, whose `fingerprint` is a SHA-256 over sorted `path|size` lines. `-Verify` checks a tree against its manifest. The medium corpus built on C: and on E: gave the identical fingerprint `2b379056…`.

| Size | Entries | Bytes | Contents |
| --- | --- | --- | --- |
| small | 8,946 | 58 MB | fan-out 4 / depth 3 tree of tiny files, 2 × 16 MiB large files, 20 duplicate groups, 50 empty folders, 10 Rust/Node projects with `target`/`node_modules` |
| medium | 199,913 (197,254 files, 2,659 dirs) | 1.04 GB | fan-out 6 / depth 4 tree (186,600 tiny files), 4 × 128 MiB, 200 × 3 duplicates, 500 empty folders, 50 projects × 200 artifact files |
| large | ≈ 1.1 M (not generated yet) | ≈ 3.5 GB | fan-out 8 / depth 5 tree, 8 × 256 MiB, 1,000 × 3 duplicates, 2,000 empty folders, 100 projects × 500 artifact files |

## How to run

All commands run from the repository root in Windows PowerShell 5.1 or later. The Node checks need Node 24.19.0 / pnpm 11.22.0 (see `AGENTS.md`).

```powershell
# Corpus (put it on the volume you want to measure)
powershell -File scripts/perf/new-corpus.ps1 -Size medium -CorpusParent C:\Users\me\supa-perf-corpus

# Compile: clean, no-op, one-line Rust edit (leaf + app crate), frontend edit, rebuild after cleanup
powershell -File scripts/perf/bench-compile.ps1 -Runs 5 -CleanRuns 3
#   evaluate a build option without committing it:
powershell -File scripts/perf/bench-compile.ps1 -Label lld -CargoConfig scripts\perf\trials\linker-rust-lld.toml

# Native runtime: throughput, first progress, cancellation, working set, handles, threads for 1/2/4 workers
powershell -File scripts/perf/bench-runtime.ps1 -Corpus C:\Users\me\supa-perf-corpus\medium -Runs 3 -Label ssd

# Built app: startup, 60 s idle, route-visit growth (needs a built debug exe; no running instance)
powershell -File scripts/perf/bench-app.ps1 -Runs 5
#   plus in-app scan responsiveness and UI cancellation (a person picks the corpus folder once):
powershell -File scripts/perf/bench-app.ps1 -Runs 1 -Interactive -CorpusRoot .gg\perf-corpus\medium

# Disk reclamation through the real CleanupService (-AllowRecycleBin adds the Recycle Bin scenario)
powershell -File scripts/perf/bench-disk.ps1 -Runs 3 -PayloadMiB 256

# Compare any results.json with the budgets
pnpm perf:budgets .gg/perf-artifacts/<run>/results.json
```

What each benchmark measures:

- **Runtime** (`src-tauri/crates/windows-platform/tests/perf_storage.rs`, `#[ignore]`, opted in with `SUPA_PERF_CORPUS`).
  - Runs the real large-files, disk-analyzer and empty-folders scans through `StorageService`, plus project-artifact discovery.
  - Each scan runs fully, then once with cancellation after 25 % of the corpus's entries.
  - Records: entries/s and bytes/s, time to first progress, cancel→terminal latency, peak working set, and handle/thread counts before, peak and after.
- **App** (`bench-app.ps1`). Starts the exe with a loopback-only WebView2 debug port and measures the main process plus its WebView2 child tree.
  - **Startup** runs from process start to `readyState === "complete"` with the app shell rendered. It includes up to about 50 ms of polling.
  - **Idle** samples 60 s of CPU time, working set, private bytes, handles and threads.
  - **Route growth** visits all 10 routes K times.
- **Disk** (`perf_disk_reclamation`, `#[ignore]`, opted in with `SUPA_PERF_DISK_MIB`). Each disposition gets its own deterministic payload in the Temporary cleanup scope. It records:
  - **selected** (plan);
  - **processed / quarantined / purged / occupied / app-reclaimed** (`ByteAccounting`);
  - **recycled** (sum of `logical_bytes` of items in the `Recycled` state, computed by the harness);
  - the **Recycle Bin size change** (`SHQueryRecycleBin`);
  - **externally reclaimed** bytes (the median of 3 `GetDiskFreeSpaceEx` samples before and after, with the min–max noise band).

  Quarantine is purged through the app's own purge path, and the purge is measured too. The Recycle Bin scenario undoes its own items and checks they are restored.

## Results

Medians of the reference run. The full min, median and p90 values are in the artifacts listed in [the verification page](verification/performance.md).

### Compile (repository on HDD, isolated target)

| Scenario | Median | p90 | Notes |
| --- | --- | --- | --- |
| Clean workspace build (`cargo build --workspace`) | 186.6 s | 190.8 s | 3 runs |
| Clean frontend (`tsc` + `vite build`) | 5.5 s + 1.2 s | 6.8 s + 1.3 s | |
| No-op rebuild | 0.79 s | 0.94 s | |
| One-line edit, leaf crate (`cleanup-core`) | 42.6 s | 50.3 s | Recompiles every dependent crate. The app crate alone takes about 37 s, and linking its binary about 9 s |
| One-line edit, app crate (`src-tauri/src/lib.rs`) | 43.7 s | 49.0 s | |
| Frontend one-line edit | 5.5 s `tsc` + 1.1 s Vite | 6.0 s + 1.2 s | |
| Rebuild after Generation cleanup (final exe/pdb removed, `deps`/`incremental` kept) | 0.79 s | 1.7 s | Relinks only. A whole-target delete equals the clean build above |

That baseline run overlapped with another project's `cargo test` on the same machine (recorded in its `contention` field). The trial baseline below ran without contention and was faster (clean 142 s, leaf edit 32 s).

### Native runtime (medium corpus, debug build, median of 3)

| Scan | SSD w1 | SSD w4 | HDD w1 | HDD w4 | Release SSD w1 / w4 |
| --- | --- | --- | --- | --- | --- |
| Large files (full walk + allocated size) | 44.5 s (4,491 e/s) | 43.2 s | 57.0 s | 57.2 s | 29.9 s / 30.3 s |
| Disk analyzer | 5.5 s | 5.5 s | 6.7 s | 6.6 s | 3.9 s / 3.8 s |
| Empty folders | 51.5 s | 51.1 s | 71.4 s | 71.2 s | 34.6 s / 35.5 s |
| Project discovery (rule pool) | 8.8 s | **6.2 s** | 11.8 s | **7.6 s** | 5.9 s / **3.5 s** |

| Responsiveness and resources (all scans, both volumes) | Observed |
| --- | --- |
| Time to first progress | 29–34 ms median |
| Cancellation latency (cancel at 25 %) | 21–50 ms median, p90 ≤ 66 ms |
| Peak working set of the test process | 9–14 MiB |
| Handle and thread growth per scan (after release) | 0 handles; at most 1 thread (a scan worker still retiring) |

Large-files, disk-analyzer and empty-folder scans do not change with the worker count. **Finding:** the storage walk is serial; it passes `workers` into `ScanLimits` but never spawns threads. Only project discovery uses a worker pool, and 4 workers beat the previous fixed 2 by 23 % (SSD) and 29 % (HDD). The walk costs about 220 µs per entry in debug and 150 µs in release, mostly per-entry metadata and allocated-size handle opens, not CPU. So a parallel walk is the most promising next optimization (see Limitations).

### Built app (debug build, 5 runs + warm-up, 60 s idle, 5 route cycles)

| Metric | Observed |
| --- | --- |
| Startup to rendered shell | median 679 ms, p90 695 ms (second series: 661 / 676 ms) |
| Process tree at ready | 8 processes, 320 MiB working set (30 MiB main process), about 3,135 handles |
| Idle CPU over 60 s | 0.05–0.07 % of the machine |
| Idle growth over 60 s | Working set +7 MiB; main process −2 handles; whole tree +65 to +73 handles |
| After 5 visits to all 10 routes | Tree working set +49 MiB, mostly on the first cycle (+35 MiB), then +1–7 MiB per cycle; main-process handles +11 to +13, flat after the second cycle |

**In-app scan (interactive, SSD medium corpus, large-files scan started and cancelled through the real UI commands):**

| Metric | Observed |
| --- | --- |
| First progress | 6.4 ms |
| Entries before cancel | 98,695 (cancelled at 50 % of the corpus, 34.5 s in) |
| Cancel → cancelled status | 21.7 ms |
| UI frames during the scan | 2,071; frame gap p50 16.7 ms, p99 16.8 ms, max 16.9 ms; 0 frames over 50 ms |

The window stayed at a steady 60 fps for the whole scan: the scan runs off the UI thread and status polling never blocked rendering.

The whole-tree handle count rises about 100 at around 20 s idle and then partly falls back, while the app's own process stays flat. That rise comes from WebView2's helper processes, not from an app leak. So the idle guard checks the main process, and tree growth is recorded for information only.

### Disk reclamation (64 MiB payload, SSD, `%TEMP%`, 3 runs, medians in bytes)

| Disposition | Selected | Quarantined | Purged | Recycled | Recycle Bin change | App-reclaimed | Externally reclaimed | Time |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Recycle Bin | 67,108,864 | 0 | 0 | 67,108,864 | +67,108,864 | 0 | −4,096 | 603 ms |
| Quarantine | 67,108,864 | 67,112,960 | 0 | 0 | 0 | 0 | −8,192 | 19 ms |
| … after purge | | | 67,112,960 | | | 67,112,960 | 67,112,960 | |
| Permanent | 67,108,864 | 0 | 67,112,960 | 0 | 0 | 67,112,960 | 67,108,864 | 29 ms |

- Recycle Bin and Quarantine keep the data on the same volume, so **they free nothing**.
- The Recycle Bin grew by exactly the recycled bytes. The benchmark restored its own items with the app's undo afterwards.
- Space is actually freed only by purge or Permanent deletion. The app's `reclaimed_bytes` agreed with the external free-space change to within one 4 KiB cluster, and the free-space noise band was ≤ 8 KiB.
- Quarantined and purged bytes are allocated size, so they are one cluster above the logical selected size.

## Budgets

`scripts/perf/budgets.json` holds explicit budgets, checked by `scripts/perf/check-budgets.mjs` (`pnpm perf:budgets <results.json>`).

- **Machine-speed budgets** (compile times, scan durations, startup) apply only when the run's CPU model and thread count match `machineClass`. Otherwise they are reported as `skipped`, never as passed.
- **Machine-independent budgets** apply everywhere:
  - cancellation p90 ≤ 500 ms;
  - peak working set ≤ 64 MiB;
  - handle growth ≤ 4 per scan;
  - idle CPU ≤ 1 %;
  - idle handle growth of the app's own process ≤ 20;
  - route-visit handle growth ≤ 100;
  - Permanent deletion and Quarantine purge must externally free space.
- Budgets leave about 25–50 % headroom over the reference medians.

CI-enforced guards (run in `pnpm check`, `pnpm test` and `cargo test`):

| Guard | Protects |
| --- | --- |
| `scripts/perf/check-build-config.mjs` | Fails if any committed Cargo profile or config disables incremental builds for dev/test; a committed `.cargo/config*` sets `rustc-wrapper`, `RUSTC_WRAPPER` or `CARGO_INCREMENTAL`; or any script runs `cargo clean` |
| `scripts/perf/perf-guards.test.mjs` | Unit tests for both checkers, plus validation of the committed budgets file |
| `windows-platform/tests/scan_guards.rs` | A walk over a synthetic tree cancels within 2 s, and 6 scan/cancel cycles grow the process handle count by at most 16 |
| `storage::scan_profile` tests | The worker resolver maps every profile/media pair and stays within `1..=4`; unknown media uses the HDD count; the settings file rejects unknown fields, profiles and schema versions |
| `large_files_commands.rs` IPC test | The scan-settings commands round-trip, reject unknown profiles and refuse foreign windows and origins |

## Accepted and rejected optimizations

### Accepted: scan concurrency profile (worker count per drive type)

The Settings page now has a **Scan speed** control: `Automatic` (default), `Solid-state drive` or `Hard drive`. It is saved in its own `scan-settings.json` (schema v1), so the journal and policy schemas are unchanged.

- `Automatic` asks Windows whether the scanned volume has a seek penalty (`IOCTL_STORAGE_QUERY_PROPERTY` / `StorageDeviceSeekPenaltyProperty`, opened with no access rights and no elevation). It correctly reported C: as SSD and E: as HDD on the reference machine.
- SSD → 4 workers. HDD or unknown → 2 workers, the previous fixed value.
- One pure resolver clamps every result to `1..=MAX_WORKERS (4)`. It replaces the hard-coded worker values at every storage scan command and at project-artifact discovery.

| Project discovery, medium corpus | Before (fixed 2 workers) | After (SSD profile: 4) |
| --- | --- | --- |
| SSD, debug | 8.08 s | 6.21 s (−23 %) |
| SSD, release | (w1: 5.89 s) | 3.50 s |
| HDD, debug, when forced to the SSD profile | 10.67 s | 7.56 s (−29 %) |

HDD stays at 2 even though 4 was faster in the table above. That HDD run read a warm file cache after the corpus had just been written, so it does not show real seek costs. Changing HDD behaviour waits for cold-cache evidence (see Limitations). Guards: resolver unit tests, the settings-file tests, the IPC test and the `runtime.project-discovery-full-w4` budget.

### Evaluated separately: linker, selective optimization, CI caching

Each option was measured against a baseline run in the same session, in its own isolated target, on the same one-line edits (leaf crate and a leaf module of the app crate). Values are median / min in seconds of 5 runs (2 for clean). Runs were paired so that both sides of a comparison saw the same background load: the first pair had no foreign builds, the second overlapped another project's builds. The first pair crossed one machine sleep, which stretched a single lld app-edit sample to 68 minutes. That sample is kept in the artifact; a median of 5 is not moved by it. Later runs record sleeps in `sleepEvents` automatically.

| Run | Clean | No-op | Leaf edit | App edit | After cleanup |
| --- | --- | --- | --- | --- | --- |
| Baseline, pair 1 | 142.3 / 140.8 | 0.8 / 0.6 | 32.3 / 30.4 | 27.2 / 25.1 | 0.8 / 0.7 |
| `rust-lld` linker, pair 1 | 138.2 / 136.7 | 0.9 / 0.8 | 28.7 / 23.2 | 29.6 / 23.1 | 1.6 / 1.4 |
| Baseline, pair 2 | 197.0 / 193.4 | 1.1 / 0.9 | 50.6 / 39.6 | 41.2 / 34.2 | 1.9 / 0.9 |
| `rust-lld` linker, pair 2 | 187.4 / 185.6 | 1.0 / 0.7 | 53.1 / 39.8 | 39.2 / 33.9 | 1.6 / 0.7 |
| Dependencies at `opt-level = 1`, pair 2 | 338.7 / 285.9 | 0.7 / 0.7 | 48.7 / 45.5 | 44.2 / 31.5 | 1.6 / 0.7 |

- **Linker (`rust-lld`): rejected.** Clean builds were 3–5 % faster in both pairs. One-line edits were faster in one pair and slower in the other, by about as much as the run-to-run spread. Linking the app binary takes about 9 s of a 30–50 s edit, so even a much faster linker has a small ceiling. Not a measured win, so nothing was committed. The trial file stays in `scripts/perf/trials/` for re-evaluation.
- **Selective optimization (`[profile.dev.package."*"] opt-level = 1`): rejected.** Clean builds became 1.7–2.4× slower (338.7 s against 142–197 s), and edits were no faster. Debug-build runtime (SSD, medium corpus, 4 workers, 2 runs): large-files 41.6 s against 43.2 s for the earlier baseline. A back-to-back control run of the unchanged build took 85.5 s for the same scan, so the gain is well inside the machine's run-to-run noise. The cost is certain and the benefit is not measured, so it was not committed.
- **Worker pools:** accepted for project discovery on SSD (above). The storage walk's `workers` value has no effect today (see Limitations).
- **CI caching:** not evaluated. The manual-only workflow compares no cache, `actions/cache` and sccache with `CARGO_INCREMENTAL=0`. The owner did not approve a push, so it has not run.

### Not changed: local incremental builds and sccache

Local development keeps Cargo's default incremental dev builds. sccache cannot cache incrementally compiled crates, so it is only evaluated for CI or clean builds with `CARGO_INCREMENTAL=0`. That is why the CI experiment workflow sets these variables per job and nothing is committed to `.cargo/config`. `check-build-config.mjs` enforces this.

## Limitations

- **Serial storage walk.** Large-files, disk-analyzer, empty-folders, duplicates, cleaner and browser scans walk one directory at a time, so the SSD/HDD profile only changes project discovery today. A parallel walk looks like the biggest remaining scan win: in release, the 4-worker rule pool was 1.7× faster on a similar per-entry workload. It was not built in this phase because the walk carries the link, identity and change-detection safety checks, so it needs its own design and review.
- **HDD numbers are warm-cache.** The HDD corpus was generated right before measuring, so reads came mostly from the file cache. Cold-cache HDD measurements, for example after a reboot, are still needed before HDD worker counts change.
- **Noise.** Two back-to-back debug runs of the same scan on the SSD differed by up to 2× (large-files 43 s against 85 s), so small runtime differences are not evidence. Budgets have wide headroom for this reason.
- **Contention.** The first compile baseline overlapped with another project's `cargo test` on the same machine. It is recorded in the artifact and superseded by the uncontended trial baseline for comparisons.
- **Not measured yet:** the large corpus. The interactive app scan ran once (one run, not a series).
- **App handle counts include WebView2.** The whole process tree's handle count moves by about ±100 on its own, so it is recorded but not budgeted. The app's own process is budgeted.
- **CI caching was not evaluated.** `.github/workflows/perf-ci-cache.yml` (manual dispatch only) is ready, but running it needs a push and the owner's approval.
- **Single machine.** All time budgets belong to one machine class. On other machines they report `skipped`.
- **Windows 11 and ARM64** were not measured.
