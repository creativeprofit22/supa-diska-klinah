# Build artifact budgets

Build artifact budgets are opt-in. They coordinate one explicitly approved native build, observe exact registered outputs, and quarantine only stale generations that the coordinator previously observed that build touch successfully. Any automatic `cargo clean` is forbidden; the coordinator also never infers build success from watcher silence or turns project discovery results into deletion authority.

## Registering a profile

A saved project root is required. Registration accepts:

- one absolute canonical local `.exe`;
- a separate ordered argument array;
- one relative working directory contained by the saved root;
- profile, toolchain, target, ecosystem, and rebuild-cost labels;
- up to 16 relative artifact paths typed as `Generation`, `Dependency`, or `Incremental`.

Windows displays the executable, every argument, working directory, and artifact path in a native confirmation before anything persists. Declining stores nothing. Registered paths must not duplicate, contain, or be contained by another path in the same profile. Shell strings, PATH lookup, runtime argument overrides, environment overrides, `.bat`, `.cmd`, `.ps1`, shells, and script hosts are rejected. The executable's Windows file identity is saved and checked before every run; replacement requires registration again.

### Rust example

Choose the absolute canonical `cargo.exe` and enter arguments as separate rows:

```text
build
--profile
dev
--target
x86_64-pc-windows-msvc
```

Register the exact generation output separately from protected state, for example:

```text
target/x86_64-pc-windows-msvc/debug/my-app.exe  Generation
target/x86_64-pc-windows-msvc/debug/deps        Dependency
target/x86_64-pc-windows-msvc/debug/incremental Incremental
```

Never configure ordinary post-build `cargo clean`. Cargo keeps incremental work products under `target` to make later builds faster. Whole-target cleanup discards current dependencies and incremental state instead of selecting stale observed generations.

### Node example

Use an absolute canonical `node.exe`. Pass a contained script path and its options as separate arguments:

```text
tools/build.mjs
--mode
production
```

Do not register `npm.cmd`, a shell, or command-line text. The script must exit with the real build status and must not daemonize.

### Generic example

Compile a small native `.exe` wrapper that launches the real build with fixed internal behavior, waits for it, returns its exit status, and never daemonizes. Register that wrapper and each explicit argument. Script wrappers are not accepted.

## Run lifecycle

Only one coordinated build runs globally. Standard input and child output are disconnected; output and command lines do not cross IPC. The UI receives fixed state and an exit code only. The read-only `get_active_build_run` command takes no input and returns the global active `BuildRun` snapshot or `null` when idle, including after restart. Run and profile IDs remain opaque. The active slot remains authoritative for busy checks, including the brief terminal-state handoff before it is cleared; a snapshot grants no new launch or cleanup authority.

The order is fixed:

1. Revalidate the saved root, working directory, executable path, and executable identity.
2. Snapshot every registered path without following links or reading file contents.
3. Launch the saved executable directly with the immutable argument array.
4. On exit code zero, complete all post-build snapshots.
5. Recheck cancellation and atomically commit the success stamp.
6. Preview the configured budgets.
7. Rebuild exact candidate proofs, revalidate live identity and containment, then quarantine selected generations.

A nonzero exit, cancellation, incomplete snapshot, changed profile, changed executable, unreadable path, or validation failure commits no new success stamp and authorizes no cleanup. Build-created partial files remain untouched. Cancellation terminates the private Windows Job Object containing the launched child and its descendants. The child starts suspended, is assigned before resuming, and retains direct executable/argv launching. Assignment failure aborts launch. Kill-on-close provides fallback termination; the normal cancellation path explicitly terminates the job and waits for its active-process count to reach zero before returning `Cancelled`. Containment or accounting errors return failure instead of `Cancelled`.

A failure after the success stamp but before mutation reports analysis failure and leaves artifacts in place. If enforcement partially succeeds, moved generation IDs are pruned, the remaining failures keep the run in analysis failure, and every completed move remains journaled for undo. A restart never upgrades an interrupted in-memory run to success.

## Ownership and protection

Only a registered `Generation` path changed during a successful coordinated build becomes owned. Pre-existing or externally discovered paths remain unowned. These states are always protected:

- unowned or non-generation paths;
- active, unreadable, ambiguous, missing-identity, link-like, or identity-changed paths;
- the current successful profile and target;
- anything touched by the latest successful build;
- dependency and incremental paths;
- paths changed externally within the stale-change grace period.

Every selected path receives a fresh exact proof bound to its saved root, profile, generation, Windows identity, and final path component. Execution reloads the live profile, root, and ledger before mutation. It then reuses the cleanup journal, containment, activity, reparse, identity, quarantine, undo, and delayed-purge checks. Failed items remain in the ledger and are never reported as successful enforcement.

## Budget precedence and ranking

The global policy and each project override can independently provide a maximum allocated size, maximum age, both, or neither. `Use global limits` applies global defaults to that project. `Disable for this project` removes its project limit; the global total still applies. An explicit override replaces that project's inherited limits.

Selection is deterministic:

1. Remove every protected row from eligibility.
2. Select every eligible generation older than its enabled age limit.
3. Select the minimum additional ranked set needed for each project size limit.
4. Select the minimum additional ranked set needed for the global size limit.
5. Rank by oldest successful touch, non-current toolchain or profile, larger allocated size, lower rebuild cost, then normalized path and opaque ID.

If protected bytes alone exceed a limit, selection stops at that protected floor. The preview reports the unmet limit; protection is never broadened into deletion eligibility.

## Scheduled external analysis

When enforcement is enabled, startup performs one due check and an hourly timer asks whether analysis is due. The default analysis interval is 24 hours. Only exact registered paths are inspected.

A new or changed external path receives an external-change timestamp, remains unowned unless an earlier coordinated build owned the same unchanged identity, and receives the default 24-hour stale grace. Repeated observations never create a success stamp, current profile, current target, or ownership. Watcher silence, process absence, file age, and a completed scan are not build success.

Scheduled enforcement requires at least one coordinated success stamp. It may quarantine only an old previously owned generation whose identity remains unchanged and whose external grace elapsed.

## Quarantine, undo, and purge

Automatic eviction always uses recoverable quarantine. The default recovery period is seven days. Each move is journaled before mutation, supports exact undo before purge, and uses the existing staged cross-volume copy and source-revalidation sequence. Successfully moved generations are pruned even if another item fails; any item failure reports `analysisFailed`. Existing maintenance physically removes quarantined bytes only after the configured deadline.

Forgetting a profile removes future launch and budget authority. It does not delete artifacts. Disabling the global policy stops scheduled analysis and automatic mutation; manual preview remains available.

## Native cancellation verification

Verified on Windows on 2026-09-05 without changing the existing production Job Object implementation:

```text
cargo test --manifest-path src-tauri/Cargo.toml -p windows-platform native_process_runner_cancels_the_entire_process_tree -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml -p windows-platform real_cargo_cancellation_stops_build_script_and_descendant -- --nocapture
```

Both tests passed. The disposable native fixture publishes readiness from its running descendant, which would write a sentinel after 1.5 seconds. Each test opens and retains live parent and descendant process handles before cancellation. On the runner thread, immediately after `NativeProcessRunner::run` returns, zero-timeout handle probes must show both processes already terminated; there is no post-return exit grace period. The test also observes beyond the sentinel delay before removing its temporary directory. The Cargo case builds a dependency-free temporary package offline and cancels while its real build script and that script's descendant are running.

The previous test passed too, but allowed a two-second post-return exit wait and treated any failure to open the descendant as success. Those weaknesses are removed. The existing production code's pre-resume assignment and zero-active-process check explain the strengthened tests passing; no production fix was needed for this reproduced path.

Remaining verification limits: these tests exercise the native runner, not the webview IPC end to end. They do not inject Job Object assignment/accounting failures, force incompatible outer-job restrictions, cancel during an active compiler/linker invocation, or exercise fallback kill-on-close during abnormal unwinding. No surviving descendant was observed in the tested native or Cargo cancellation paths.

## Troubleshooting

| State | Meaning |
| --- | --- |
| `approvalDeclined` | Native confirmation was declined; nothing persisted. |
| `buildBusy` | Another coordinated build is queued or running. |
| `validationFailed` | A saved executable, root, profile, path, or identity changed. Register again if intentional. |
| `failed` | The native process returned nonzero. No success stamp or cleanup was committed. |
| `cancelled` | Cancellation won. No success stamp or cleanup was committed. |
| `analysisFailed` | The build succeeded, but safe analysis or enforcement did not fully finish. Completed quarantine moves remain undoable; failed items remain tracked. |
