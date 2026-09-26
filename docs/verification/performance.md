# Performance verification runs

Dated evidence for [performance](../performance.md). Artifacts are in `.gg/perf-artifacts/<run-id>/`, which is local and gitignored. Each holds `results.json` (canonical JSON, including environment, contention and sleep events), plus build logs, Cargo `--timings` HTML or JSON-lines records.

Execution IDs identify the harness command that produced each run. Successful runs do not close any manual acceptance gate. Harness blockers are recorded in [storage parity](storage-parity.md).

## 2026-09-25 UTC: reference machine (i7-8700, Windows 10 22H2 19045)

| Run ID | Kind | What it shows | Notes |
| --- | --- | --- | --- |
| `20260925T085326Z-compile-smoke` | compile | Harness smoke (1 run); source restore verified clean in git | |
| `20260925T090730Z-app` | app | Startup 714 ms, idle 10 s smoke, 1 route cycle | Smoke settings; a 60 s idle / 5-run series is still to run |
| `20260925T091551Z-disk` | disk | Quarantine ≈ 0 freed until purge; purge and Permanent free 16 MiB externally | Recycle Bin not run (no consent) |
| `20260925T091822Z-compile-baseline` | compile | Clean 186.6 s, no-op 0.79 s, leaf/app edits 42.6/43.7 s, frontend 5.5 + 1.1 s, after cleanup 0.79 s | Contention: another project's `cargo test` during iterations |
| `20260925T101748Z-runtime-ssd-medium` | runtime | SSD sweep, workers 1/2/4 × 4 scans, full + cancel | No contention |
| `20260925T104134Z-runtime-hdd-medium` | runtime | HDD sweep, same corpus fingerprint | Warm file cache |
| `20260925T112135Z-runtime-ssd-medium-release` | runtime | Release w1/w4: walk-bound scans unchanged, project discovery 5.9 → 3.5 s | |
| `20260925T114152Z-compile-trial-baseline` | compile | Trial pair 1 baseline | Recorded before sleep detection was added |
| `20260925T120145Z-compile-trial-lld` | compile | `rust-lld` pair 1 | One sample crossed a 68-minute machine sleep (Kernel-Power 42/107 at about 12:55–14:03 local) |
| `20260925T132843Z-compile-trial-deps-opt1` | compile | deps `opt-level=1`, first attempt | Crossed sleeps; superseded by `-r2` |
| `20260925T140451Z-compile-trial-deps-opt1-r2` | compile | deps `opt-level=1`, pair 2 | Contention present |
| `20260925T144513Z-compile-trial-lld-r2` | compile | `rust-lld`, pair 2 | Contention present |
| `20260925T152151Z-compile-trial-baseline-r2` | compile | Baseline, pair 2 | Contention present |
| `20260925T155259Z-runtime-ssd-deps-opt1` | runtime | Debug runtime with deps `opt-level=1`, w4 | |
| `20260925T160048Z-runtime-ssd-dev-control` | runtime | Unchanged debug build, back-to-back control | Up to 2× slower than earlier identical runs: noise band |
| `20260925T163449Z-disk` | disk | 3 runs × 64 MiB with `-AllowRecycleBin` (owner-approved): Recycle Bin +64 MiB and 0 freed; Quarantine 0 freed until purge; purge and Permanent free 64 MiB | Recycle Bin items restored by the app's undo |
| `20260925T170339Z-app` | app | 5-run startup median 661 ms; 60 s idle; 5 route cycles | Failed the first idle guard on whole-tree handles (+65); diagnosed as WebView2 helper churn, main process −2 |
| `20260925T181708Z-app` | app | Interactive in-app scan with the owner's folder pick: first progress 6.4 ms, cancel 21.7 ms, 2,071 frames, max gap 16.9 ms, 0 long frames | 1 run |
| `20260925T170605Z-app` | app | Repeat on the rebuilt app: startup median 679 ms, idle CPU 0.07 %, main-process idle handles −2, route cycles +13 main handles | All app budgets pass |

Other checks from the same day:

- **Seek-penalty detection** (`perf_seek_penalty_matches_known_media`, `SUPA_PERF_SEEK_EXPECT="C:\=ssd;E:\=hdd"`): C: returned `Some(false)` and E: returned `Some(true)`, matching `Get-PhysicalDisk`. No elevation was needed.
- **Corpus determinism:** medium corpora generated independently on C: and E: have the identical fingerprint `2b379056fb5f113c3eadae67bb7d6272306fa4d14ce37e74bba5d8c0e7097bd8`.
- **Budgets:** `pnpm perf:budgets` passed against the compile baseline, the SSD runtime sweep, the app smoke run and the disk run.

## Not yet run

- A repeated series of interactive in-app scan runs. Only one interactive run exists: it succeeded as execution `4aa9063e-7ead-4268-a87e-cd0ecd62e94d`, after two attempts that got no folder pick (`012decb3-ed9e-4c1e-a2af-43883f81adb4`, `cbbec0c0-821f-451b-9af2-bab7a15d8d10`). The script now reports a missed pick as NOT MEASURED instead of blank values.
- The large corpus, and cold-cache HDD runs.
- The CI caching workflow (no push approved).
