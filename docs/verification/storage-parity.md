# Storage parity verification checkpoint

Status: **Manual step 8 accepted by the user on 22 September 2026 UTC** for the vendor confirmation/UAC scope recorded in `docs/verification/uninstaller.md` ("Live native confirmation/UAC gate" and its two follow-up sections). That acceptance covers: the real app-owned confirmation dialog observed defaulting to No, denial launching nothing, acceptance launching only the revalidated fixture, a real elevation prompt accepted and denied by the user, an owned HKCU fixture resolved through the real inventory/prepare/confirm/launch path, restart never replaying a command, and refresh reflecting real registry removal without implying uninstall. Everything else below this line — Narrator, full native keyboard flows, full pinned parity comparison, process-tree resource evidence — is unaffected and remains open; this acceptance does not extend to them and is not a Roadmap Done transition.

## Keeper CI gate: 2026-09-08 UTC

**Exact-SHA hosted follow-up:** PR [#11](https://github.com/creativeprofit22/supa-diska-klinah/pull/11), branch `fix/duplicate-keeper-ci-gate`, pushed commit `c61b891d7bc2ea2f041b50ab24f426c94e609807` contains only the four gate/driver/keeper-note files. Both [PR run 34273818979](https://github.com/creativeprofit22/supa-diska-klinah/actions/runs/34273818979) and [push run 34273815144](https://github.com/creativeprofit22/supa-diska-klinah/actions/runs/34273815144) concluded failure: Quality failed at Clippy (`empty_folders.rs:11` nonminimal boolean and `walk.rs:204` collapsible else-if), both native smoke jobs passed, and signed release was skipped. Workspace tests and the keeper driver were skipped; hosted one-test/zero-ignored execution and driver-failure propagation are **not confirmed**. The local exit-37 probe remains the only runtime failure-propagation evidence. Watch execution `98523341-2b0b-4ca5-9aa5-882464be4c39` exited 1; metadata/log inspection `db983a58-1d02-426c-8f0e-54211459d997` verified the exact SHA and failures. Hosted verification and manual acceptance remain pending. Unrelated implementation edits were not included or repaired. This follow-up is recorded locally and on the PR without creating another unverified pushed SHA.

Task `652eb0a9` wires only the external duplicate-keeper driver into Quality after workspace tests. The existing build target and locked precompile are reused; Rust 1.90.0 is explicit because the driver runs from the repository root. A five-minute step timeout bounds execution. The child exit is logged and propagated, with no continue-on-error, so the existing signed-release dependency on Quality fails closed. Existing storage changes, race barriers, ignored-test annotation, keeper assertions and separate native-storage CI integration are untouched.

| Command / execution ID | Observed result |
| --- | --- |
| Missing-driver reproduction — `1cb28d3f-abb4-45c3-b3ce-43369a81ee93` | PowerShell threw `CI-GAP reproduced`; the combined inspection command later exited 0, so that outer exit is not test evidence |
| Rust 1.90.0 keeper driver — `9b433f7f-bf53-424a-adf2-606514f343de` | Exit 0; locked precompile completed before handshake; one actual race test passed, zero ignored; external protection probes printed PASS; disposable marked temp fixture only |
| Extracted workflow exit probe — `eeefc6e9-f97a-4d49-bc82-1009639cf62e` | Exit 0; real driver reference, timeout and release dependency checked; synthetic child failure 37 propagated unchanged through the step body using local Windows PowerShell |
| Read-only CI logs — `c0e8c051-5a64-466d-8664-4b8286002c2b`; job metadata — `94084246-4ac0-4215-b2b7-9f9fd54e3eb3` | Run 34103714621 at revision `739b88f4995346034916249073d43262d17e83fb` has no keeper step; Quality failed at Clippy, workspace tests and signed release were skipped |
| Keeper driver rerun under `RUSTUP_TOOLCHAIN=1.90.0` — `e4434fbd-364c-4399-9498-ad5f81bb003c` | Exit 0; locked precompile, then one race test passed with zero ignored and the external PASS message; only the marked disposable temp fixture was touched |
| Hardened step-body exit probes — `aad9fd8a-9224-4317-8190-42a419de44b8`, `a7c3b40a-0c5b-4f3d-9cab-12587c902f0a`, `ddeea751-addf-49e0-a83f-e4f185f107dd` | Synthetic child exit 37 propagated unchanged; a throwing driver propagated 1; a missing driver failed closed nonzero. Local Windows PowerShell, not a hosted Actions run |
| Hosted job inspection — `b8c36631-62ab-439e-a142-e1f483d5e575`, `bd75f47d-9fea-4455-99cd-a990e26c7084` | Quality job 102221762794 of run 34273815144 lists the keeper step as skipped after Clippy failed; the keeper driver still has no hosted execution |
| Isolated-worktree lint/test on the minimal Clippy fixes — `dad648ac-5f26-4c29-8dd7-f65ffc32cea6`, `5dd8cef6-9aad-421e-a0f3-b1201dee09f3`, `e9c19499-0ca4-491b-aa28-e04ecc28fd5e` | Under Rust 1.90.0 on a tree of committed files plus only the reported lint fixes: `cargo fmt --all --check` clean, Clippy clean for `cleanup-core` and `windows-platform` with `-D warnings`, `cleanup-core` tests 13 passed / 0 failed. Workspace-wide Clippy is not runnable locally without the prebuilt privileged helper (`35546ac6-2415-47a9-beff-a00d322cdfb2`) |
| **Hosted keeper driver execution — `f671ae33-c5fc-40ab-a848-fa6ab7ba7f70`** | **Run 35758467299 (Quality job 106850131534) at revision `0277e4990d09082d768f295d8ef151bac26152f3`: step "Test duplicate keeper external-process race" succeeded under `RUSTUP_TOOLCHAIN: 1.90.0`. Log shows the locked precompile, `Running tests\duplicate_keeper_process.rs`, `1 passed; 0 failed; 0 ignored`, the external PASS message, and `Duplicate keeper driver exit code: 0`. Run conclusion success; Signed Windows release skipped (no release event)** |

The hosted Clippy failure was cleared by committing only the lints the Quality job reported against already-committed code, prepared in a detached worktree so no uncommitted storage work could ride along:

- `9d2be5c` — simplifiable boolean in `cleanup-core/src/storage/empty_folders.rs` and collapsible `else { if .. }` in `cleanup-core/src/storage/walk.rs`.
- `0277e49` — a third lint of the same class in `windows-platform/src/storage/vendor_jobs.rs:353`, which only surfaced in run 35757370301 once the first crate compiled. All three are behavior-preserving rewrites of the same negated `is_some_and` / block-nesting patterns.

The larger uncommitted storage changes in those files remain unstaged in the working tree and were not pushed.

A further hardening of the step body (clear `LASTEXITCODE` first, fail closed when no child exit code is observed) is present in the working tree but **not** in the pushed SHA; locally it propagated a synthetic exit 37, a throwing driver's 1, and a missing driver's nonzero failure. The pushed step still fails on nonzero driver exit, as run 35758467299 demonstrates.

**Hosted evidence status — met for driver execution:** [run 35758467299](https://github.com/creativeprofit22/supa-diska-klinah/actions/runs/35758467299) proves the keeper driver actually ran in CI and passed. Manual step 8 acceptance is still open and is not implied by this run; no release dispatch is authorized. Historical note: [the earlier CI run](https://github.com/creativeprofit22/supa-diska-klinah/actions/runs/34103714621) could not prove execution of this gate. The required conditions — keeper test passed with one test and zero ignored, external PASS message, driver exit 0, on the exact pushed SHA — are all satisfied by that run.

**Harness/acceptance:** These are local runtime and read-only hosted observations, not host-owned harness approval. Existing harness blockers and manual step 8 acceptance remain open. No Roadmap action or next-task authorization follows from these checks.

## Native cancellation reconciliation: 2026-09-08 UTC

Task `5c3471a3` corrects only the read-only smoke cancellation contract. The shared probe separates attempts from native acknowledgements and observed terminal outcomes. It reconciles only cancellation's `snapshot_unavailable` with same-module/snapshot completion (allowing finalizing while the worker retires). Failed, missing, wrong-identity, unknown or unexplained statuses and other IPC errors reject; acknowledged cancellation accepts only cancelled or completion winning finalization. Every acquired probe snapshot takes the finally release path, and release errors fail the smoke. PowerShell independently checks the allowed outcome and acknowledgement counts. Native authorization, cancellation, immutable bridge, unrelated changes and existing trace/parity tasks are unchanged.

| Command / execution ID | Observed result |
| --- | --- |
| Actual original probe reproduction — `8eed1fde-7aae-47ca-a6a7-d3ce1164d438` | Both defects reproduced under Node 24.19.0: legitimate completion rejected; failed terminal accepted; release ran in both cases |
| `pnpm test` — `483b4ca2-87d0-414c-818f-ea78f28157ef` | 18 deterministic tests executing the actual shared probe passed, plus all 229 Vitest cases across 29 files; no test sleeps/source-text assertions |
| `pnpm tauri build --debug --no-bundle` — `5526d44f-4f97-483b-9444-dc22ea3f1366` | Fresh native debug executable built with Node 24.19.0, pnpm 11.22.0 and Rust 1.90.0, one build job; TypeScript and frontend build passed |
| `powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-storage-root.ps1 -ArtifactDirectory .gg/smoke-artifacts/storage-cancellation` — `25dee75a-1a2f-4adf-ac86-8d3a21d6c53c` | Passed: two attempts, two acknowledgements, two observed cancelled outcomes, ten direct releases, eight released completed snapshots refused pages |

Artifact `.gg/smoke-artifacts/storage-cancellation/result.json` records `2026-09-08T19:53:35.1280609Z` and executable SHA-256 `f1fabc2bed2a8bc76b1080c8d8469bc13530daefd26269b3728c2a38aa630c3c`. Completion-before-cancel is covered deterministically, not claimed as a live observation. Only real read-only installed-program inventory and existing smoke-owned WebView checks ran; no cleanup, vendor launch, dialogs, desktop input or screenshots. No native implementation changed or native unit suite was rerun for this task.

Runtime note: initial reproduction `f9bf1ac7-cdc5-4682-9be4-ef4a59302f1e` exposed an incorrect explicit PATH and ran Node 22.20.0; it is not accepted verification. The corrected reproduction, tests and build above each selected installed fnm Node 24.19.0 and verified shell and pnpm-child runtime paths in bounded nonpersistent foreground commands. No runtime pins changed. These commands used shell evaluation to select fnm; their successful output does not resolve the existing harness command-shape/approval blockers. Harness approval remains separate and host-owned; manual step 8 stays open, with no Roadmap retry, Done transition or authorization to start the next plan task.

### Cancellation verification gate rerun

The harness rejected test execution `483b4ca2-87d0-414c-818f-ea78f28157ef` because its shell-evaluation command shape was unsafe for verification evidence. No implementation or tests changed. A bounded, nonpersistent foreground `pnpm test` with an explicit PATH to the installed fnm runtime, no shell evaluation or redirection, verified Node 24.19.0 and pnpm 11.22.0 in the same environment (including the pnpm child): `0d6dceb9-ce72-43b1-9770-9b62dab2cf6f` exited 0 with all 18 cancellation tests and 229 Vitest cases passing. The actual native lifecycle fixture also ran again through the bounded PowerShell smoke against the unchanged fresh executable: `9718b766-91e1-4c53-bb49-79c74a2365a3` exited 0; artifact `.gg/smoke-artifacts/storage-cancellation-gate/result.json`. No failures required code changes. These are current command results, not a claim of host-owned harness approval or manual step 8 acceptance; no Roadmap action occurred.

## Fixed-drive display identity: 2026-09-08 UTC

Task `68a21ae9` adds native-derived `displayMount` to the shared drive summary and accessible drive headings. Equal labels and empty labels remain distinguishable after reordering with identical capacities. Opaque drive IDs, native evidence/revalidation, eligibility and quota arithmetic are unchanged. Existing uncommitted work was preserved.

| Command / execution ID | Observed result |
| --- | --- |
| Targeted frontend regression — `7c7a00ba-10c8-4a5f-a490-f6dfb02e13af` | Failed before implementation: headings omitted the native mount |
| `pnpm test && pnpm check` — `12d2a1ae-c936-4927-a63d-44b615bffcf4` | Passed; frontend regressions, project checks, TypeScript and production build |
| `cargo +1.90.0 test --manifest-path src-tauri/Cargo.toml --test drive_commands --locked -j 1` — `74d38469-0b36-418e-bcba-14ea1eefcab9` | 10 passed after formatting; real IPC canonical field/allowlist assertions, mocked direct/paged drive serialization and warning privacy cases |

Frontend commands verified installed fnm Node 24.19.0 and pnpm 11.22.0, including the child runtime, in the same bounded nonpersistent foreground environment. Native compile `40f14a62-518c-4b6d-b1a9-1951519f2898` timed out; `6f83339d-044e-43b8-912f-a13bcb00a1ab` exposed a test dependency assumption, corrected using existing Tauri serialization without adding dependencies. Added fixtures are synthetic; the existing real IPC read-only inventory test also ran. No disk reconfiguration, file cleanup or installed-app/Narrator acceptance was performed.

These successful commands are runtime evidence only, not harness gate approval. Existing harness blockers and approved-plan/manual step 8 deferral remain unchanged; this task does not authorize the next plan task or a Roadmap completion transition.

### Display-identity verification gate rerun

The harness rejected `12d2a1ae-c936-4927-a63d-44b615bffcf4` because its shell-evaluation command shape was unsafe for gate evidence. Its green output does not clear that gate. No implementation changed during this rerun. Bounded foreground commands selected the installed runtime through an explicit PATH, without shell evaluation; each frontend environment verified Node 24.19.0 and pnpm 11.22.0, including the pnpm child.

| Command / execution ID | Observed result |
| --- | --- |
| `pnpm test` — `3c5e70a8-9ef0-49a0-bd6c-1d23ebaf53e7` | 229 tests passed across 29 files, including all 13 drive frontend tests |
| `pnpm check` — `d43bc583-1929-4a3e-a8e2-c02e4db5d240` | Project checks, TypeScript and production build passed |
| `cargo +1.90.0 test --manifest-path src-tauri/Cargo.toml --test drive_commands --locked -j 1` — `9b809311-a924-43c8-b9b5-c192495e6051` | All 10 real IPC/adapter tests passed |
| `cargo +1.90.0 test --manifest-path src-tauri/Cargo.toml -p windows-platform --lib storage::scans::tests --locked -j 1` — `c6060a1e-6ec2-432d-b888-f01f673c8811` | All 10 scan tests passed, covering the changed shared-summary fixtures |

The initial scan filter in `3bb7fc44-d19d-4636-9d87-3fdee448760b` compiled but matched zero tests; only the corrected run above establishes scan-test execution. Harness approval remains host-owned and separate from these results. Manual step 8 remains open.

## Current verification: 2026-09-08 UTC

Windows x64, Rust 1.90.0, Node 24.19.0. Node was selected through the already-installed fnm runtime for the final frontend checks; no global runtime or dependency pin was changed. Two stale duplicate-filter expectations were updated to assert the complete request (including maximum size and extensions), with invalid filter cases added. Rust warning-as-error failures were fixed without changing fail-closed semantics or suppressing lints.

| Command | Observed result | Evidence scope |
| --- | --- | --- |
| `pnpm test` | Passed: 189 tests across 26 files after vendor-review focus and pinned-default regressions | Frontend behavior, not native filesystem mutation |
| `pnpm check` | Passed | Strict ports, dependencies, parity contract, architecture, security boundaries, docs, TypeScript and production build |
| `cargo +1.90.0 test --manifest-path src-tauri/Cargo.toml --workspace --locked -j 1` | Passed: 271, zero failures, two separately driven/optional tests ignored | Native/unit/real IPC fixtures; external-process keeper race is separately driven below |
| `cargo +1.90.0 fmt --manifest-path src-tauri/Cargo.toml --all -- --check` | Passed | Rust formatting |
| `cargo +1.90.0 clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets --locked -j 1 -- -D warnings` | Passed | CI-equivalent warning-as-error gate |
| `powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-duplicate-keeper.ps1` with `RUSTUP_TOOLCHAIN=1.90.0` | Passed | Disposable external-process keeper mutation/rename race |
| `pnpm tauri build --debug --no-bundle` with Rust 1.90.0 and one build job | Passed | Fresh native debug executable; not installer/release certification |
| `powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-storage-root.ps1` | Passed | Minimized built-app mounts, keyboard skip links, initial-state reflow, real read-only inventory paging/cancellation/release and native-process observations |
| `node scripts/smoke-storage-frontend.mjs --remaining` | Passed | Five actual routes using synthetic IPC; zero mutation attempts |
| `node scripts/smoke-storage-frontend.mjs --analyzer` | Passed | Actual analyzer route replay: totals, paging, keyboard and cancellation; zero forbidden calls |
| `node scripts/smoke-storage-frontend.mjs --large-files` | Passed | Actual large-file route replay: review, reset and reflow; zero mutation attempts |

The final workspace compile initially exhausted host memory with unconstrained concurrency. A one-job run completed compilation and tests but exceeded its command window during doc tests; the subsequent bounded one-job full suite completed with exit 0. No test was removed or weakened. Clippy and formatting passed after the depth correction. The external keeper driver initially spent its 60-second race-readiness timeout compiling; it now precompiles with `--locked --no-run` before starting the unchanged race handshake, and the real external-process race passed again.

`node scripts/check-pinned-storage-source.mjs` also passed: 584 declarations across seven cleaner catalogs plus all Windows browser layout reference fields. Independent fetched source-byte hashes and canonical declaration hashes are recorded in test-only fixtures. Twelve offline contract tests now run in `pnpm check:parity`. See [the parity comparison scope](../parity.md#pinned-storage-acceptance-cases-2026-09-08-utc) before interpreting this as behavior equivalence.

## Recovery-volume preflight: 2026-09-08 UTC

**RUNTIME:** Task `de14ed19` reproduced creation of a quarantine plan followed by execution refusal using disposable scan files on C: and a disposable recovery store under the existing E: build-output directory. `WindowsFileSystem::same_volume` verified distinct native volume identities, not drive-letter strings. Before the fix, reproduction `2a14a1aa-f484-4b36-ad58-6a68fdda5746` passed its assertions that plan creation succeeded, execution failed, and source bytes remained unchanged. No real caches, host disk configuration, or mounted volumes were changed.

**CODE:** One native recovery-support preflight now runs after scope validation and before plan persistence, and again at execution. Cross-volume recovery returns the typed service error `RecoveryVolumeUnsupported` and only the sanitized storage IPC code `recovery_volume_unsupported`. Native lookup failures still fail closed as invalid evidence. Shared review explains the unsupported volume, disables recovery only for that selection, and requires a separate explicit permanent review and Windows confirmation where allowed. Large files, duplicates, cleaner and browser share this boundary; empty folders remain permanent-only. No renderer path authority, copy/delete, Recycle Bin fallback, or per-volume recovery engine was added. The filesystem's final same-volume identity checks remain unchanged.

| Command / execution ID | Observed result |
| --- | --- |
| `STORAGE_TEST_SECOND_VOLUME='E:\Projects\supa-diska-klinah\src-tauri\target' cargo test --manifest-path src-tauri/Cargo.toml --test large_files_commands -- --include-ignored` — `02d75a4d-d788-49e7-9bc4-802ec7da5a8b` | 8 passed, zero ignored; rejected recovery persists no plan, relocated previously accepted plan still refuses execution, permanent plan remains separate, source bytes survive; exact sanitized IPC error asserted |
| `cargo test --manifest-path src-tauri/Cargo.toml -p windows-platform --lib cleanup::` — `efc8b1c2-6001-4952-810f-54d64cc72459` | 70 passed, one existing optional Recycle Bin drill ignored; includes same-volume quarantine/restart/undo identity, collisions, tampering, no-follow filesystem, and native-confirmation denial fixtures |
| `pnpm test && pnpm check` — `f2341e60-6897-4d62-9bfb-aea5a21bde81` | 209 tests across 29 files passed; project checks and build passed. Shared-review tests cover unsupported recovery across four modules, explicit permanent confirmation, selection reset, sanitized errors, and permanent-only empty folders |

Native checks used installed Rust 1.97.1; frontend checks selected installed fnm Node 24.19.0 and pnpm 11.22.0 and verified both shell and pnpm-child runtime paths in the same execution. Final commands used `persist:false`, `run_in_background:false`. Earlier native compile `669b8150-64fe-42fa-af04-edb6817aebae` and frontend run `3c1c3919-b1b7-4a7b-b3f4-1e9470ebf285` hit 120-second command limits; completed bounded reruns are recorded above. Initial fixture imports were corrected without adding dependencies or relaxing assertions.

The cross-volume test is explicitly opt-in because it requires an existing second native volume; use a disposable-fixture parent for `STORAGE_TEST_SECOND_VOLUME`. It **was run live on two volumes here**, not skipped or simulated. This remains fixture/real-IPC evidence, not installed-app/native-dialog acceptance or CI-toolchain certification. The approved plan, manual step 8 deferral and harness approval blocker below remain unchanged. Task completion does not settle those gates.

## Filesystem outcome presentation: 2026-09-08 UTC

Task `5b6f626e` preserves the existing uncommitted baseline and history cursor contract. The five storage cleanup modules now share item counts, bounded detail pages and allowlisted failure descriptions for latest execution, undo and retained history. Native summaries add only a display-only location from retained immutable plans (1,024 characters plus a truncation marker, control/direction overrides replaced); IDs remain available when metadata is missing. No journal schema migration, scan retention, path authority, native mutation/restore change, vendor-history coupling, retry or copy fallback was added.

| Command / execution ID | Observed result |
| --- | --- |
| Initial UI reproduction — `8830221c-3894-4e18-903b-a0703f10e239` | Three new regressions failed before implementation: mixed zero-byte failures/uncertainty, refused undo and bounded detail pages; existing 209 tests passed |
| `pnpm test && pnpm check` — `1e504512-ee41-4010-96e2-af8e307b5000` | 213 tests across 29 files passed; project checks, TypeScript and production build passed. Tests assert safe reasons, display identity, zero-byte failure counts, unknown/unproven outcomes, protection/refused undo, retained accounting, no retries and 20/20/5 replacing detail pages |
| `cargo test --manifest-path src-tauri/Cargo.toml --test duplicate_empty_commands --test large_files_commands --locked` — `02f3378b-221b-4774-9ab1-3e84388e0005` | 14 passed; one existing opt-in second-volume test ignored. Disposable native empty-folder failure output has zero failed bytes, failed item states, bounded display-only DTO fields and matching identities after restart |
| `cargo test --manifest-path src-tauri/Cargo.toml -p windows-platform --lib cleanup::execution::tests --locked` — `72cfbd14-94aa-4575-9413-5872bbab1f4d` | 20 passed; existing real-Recycle-Bin drill ignored. Includes display sanitization/bounds, failed recovery identity, collisions, conservative accounting, partial failures and older-page recovery |

Frontend checks selected installed fnm Node 24.19.0 and pnpm 11.22.0, verifying shell and pnpm-child runtime paths within the same bounded foreground environment (`persist:false`, `run_in_background:false`). The intermediate UI run `f37b48fd-0b01-4b4e-b20d-25a7a23758bb` needed the detail test to await the native asynchronous disclosure toggle; the row-count assertions were retained. No real caches/programs were operated on. Installed-app visual/native-dialog acceptance and optional volume/Recycle-Bin drills were not run for this task. Manual step 8 stays deferred; the harness blocker below is unchanged. These command results neither approve a harness gate nor authorize Roadmap Done/retries.

### Outcome-view re-verification

The harness rejected execution `1e504512-ee41-4010-96e2-af8e307b5000` as gate evidence because its shell-evaluation command shape was unsafe; green output did not clear that gate. After the three outcome-view files were flagged as changed, bounded foreground checks with an explicit PATH to installed fnm Node 24.19.0 were run without shell evaluation: `pnpm test` (`1ca691c0-fd70-44a6-b062-ef22989de691`) passed all 213 tests, and `pnpm exec tsc --noEmit` (`1e809054-280a-40d8-8a4f-e478a9939568`) passed. Runtime preflight `ec6eee38-8a01-499b-98e3-97441f3ae6bb` verified Node 24.19.0 and pnpm 11.22.0, including the pnpm child path. No implementation changed during this re-verification; manual step 8 and harness approval remain separate.

## Selected vendor receipt synchronization: 2026-09-08 UTC

Task `3ccfbb90` adds selected-receipt reconciliation on manual history refresh without changing pagination (`bcf8b6c6`) or native authorization. Missing prepared evidence becomes non-actionable and requires fresh inventory/review; missing submitted evidence remains uncertain. Tests exercise recovery from polling failures, late scope/poll/confirmation responses, both IDs, single-flight refresh/status fallback, and zero extra vendor actions. See [uninstaller verification](uninstaller.md) for the red regressions and scope.

`pnpm test` (`47e5d946-9d27-4976-81a5-db65a6615c6f`) passed 228 tests across 29 files. `pnpm check` (`18984779-ec0a-493e-9c29-db3a73a7df24`) passed project checks, TypeScript and production build. Both selected installed fnm Node 24.19.0 and pnpm 11.22.0 and verified shell/child paths within bounded, nonpersistent foreground commands. No native code changed, native tests were not rerun, and no installed application or Windows clock was changed.

These results do not resolve the harness evidence blocker below, approve a gate, or close deferred manual step 8. No Roadmap Done/retry was requested or attempted.

### Selected-receipt verification rerun

The harness rejected the earlier shell-evaluation command shape (`47e5d946-9d27-4976-81a5-db65a6615c6f`); its green output did not clear the four-file frontend verification gate. With no implementation/test changes, a fresh bounded `pnpm test` used an explicit PATH to the installed fnm Node 24.19.0, no shell evaluation or chained commands, `persist:false`, `run_in_background:false`, and a 120-second limit. Execution `99d44f71-3dd9-4c1a-b269-8acb7429247b` exited 0: all 228 tests across 29 files passed, including all eight uninstaller page tests and 21 vendor-hook tests. Separate runtime checks confirmed Node 24.19.0, pnpm 11.22.0 and the pnpm child executable (`31fa8306-c3fb-4fe9-a421-62d2645133f7`). No code fix was needed. Manual step 8 and native interactive acceptance remain deferred; command success alone does not establish harness approval.

## Harness evidence blocker: 2026-09-08 UTC

Command success is not harness gate approval. The Node 24.19.0 runs `d7022831-8c18-4db2-991d-410226f61145` (`pnpm test`) and `d09210f6-06c8-4b44-b595-35641e7aa9b8` (`pnpm check`) used `persist:true`, making them persistent-shell evidence despite foreground completion output.

Separate one-shot reruns explicitly set `persist:false`, `run_in_background:false`, and a 120-second timeout. Both checked Node 24.19.0 in the command shell and through `pnpm exec node`. Test execution `6e415fe1-5547-45ac-93e6-bd0b093a5653` exited 0 (204 tests); check execution `3f1db356-11ee-4cf0-9005-d5cf1c19cd2e` exited 0 (project checks and build). Neither emitted an engine warning. These are observed command results only; harness acceptance remains unconfirmed and must be resolved by the harness owner, not by changing tests, runtime pins, or claiming gate approval. No further suites were run for this documentation update. Manual step 8 acceptance remains open.

### Recovery-preflight gate rerun

The harness rejected the environment-prefixed two-volume Cargo command (`02d75a4d-d788-49e7-9bc4-802ec7da5a8b`) as an unrecognized verification command shape. Its observed live result remains valid runtime evidence, not gate approval. In response, bounded foreground, nonpersistent checks were run again without changing implementation or tests:

- `pnpm test` after same-shell fnm selection and runtime checks: `56b864b2-ceb1-485e-a248-2d4c312d08b7`, exit 0, 209 tests across 29 files. This verifies frontend behavior, not Rust execution.
- `cargo test --manifest-path src-tauri/Cargo.toml --test large_files_commands`: `3f432f6d-651a-4495-b814-1ce3aa048201`, exit 0, seven passed; the opt-in two-volume test was not rerun by this unprefixed command.
- `cargo test --manifest-path src-tauri/Cargo.toml -p windows-platform --lib cleanup::`: `1c11fa92-10d8-4dc5-af15-10ec1897bd7c`, exit 0, 70 passed; one existing optional Recycle Bin drill ignored.

No failures required code changes. Harness acceptance is not inferred from these exits, and manual step 8 remains open.

## Lifetime execution traversal regression: 2026-09-08 UTC

**RUNTIME:** Before changing persistence, `lifetime_journals_do_not_block_enumeration_or_startup` wrote 1,001 valid journals into a disposable native fixture. Enumeration returned `TooLarge` and `CleanupService::new` returned `ValidationFailed` (test exit 101). The same reproduction now passes. No real cache or installed program was mutated.

**CODE:** Startup reconciliation and purge now consume one validated journal at a time; both replay gates use incremental lookup. Filename batches retain at most 100 IDs, ordered by ID with an exclusive continuation boundary. Closing each directory scan before journal replacement prevents reconciliation writes from disturbing enumeration. The 1,000-item operation limit and 8 MiB record limit remain unchanged; invalid journals propagate errors rather than being skipped. No history pruning, recovery deletion or copy/delete fallback was introduced. Directory rescans cost O(N² / 100) visits for a full traversal; an indexed journal store is the documented upgrade for very large histories, not a reason to discard records.

**RUNTIME:** `lifetime_journals_preserve_interrupted_recovery_replay_and_history_pages` reconciles all 1,001 interrupted journals across batch boundaries, retaining uncertain records and proving the final record's recovery identity. It checks replay rejection in execution and native confirmation, complete duplicate-free timestamp-tied paging, enabled purge preserving manual recovery, another restart, and byte-for-byte fixture undo. `execution_traversal_is_lazy_bounded_and_keeps_record_guards` checks filename/page bounds, lazy failure beyond the first batch, malformed records, filename/ID mismatch, oversized records and oversized item arrays.

**Historical history-pagination handoff (superseded by the retained-history implementation below, task `bcf8b6c6`):** Reuse `CleanupStorage::execution_page(before, limit)` with a maximum of `MAX_EXECUTION_PAGE` (100). Results descend by `(started_at, execution_id)`; pass the last returned pair as the exclusive `before` key, with an empty page indicating exhaustion. The scan retains only bounded keys and one journal before reading the selected page. Hold the service's shared writer while reading each page. The existing history command still returns only the first 100 summaries; cursor IPC/UI reachability remains the separate task, including cursor validation and concurrent-history semantics. Do not collect `execution_records()` into a lifetime-sized vector in production; the old `executions()` collector is now test-only.

| Command | Observed result |
| --- | --- |
| `cargo +1.90.0 test --manifest-path src-tauri/Cargo.toml -p windows-platform --locked -j 1` | Passed: 172 tests, two existing ignored tests |
| `cargo +1.90.0 test --manifest-path src-tauri/Cargo.toml --workspace --locked -j 1` | Passed: 274 tests, two existing ignored tests; an initial compile invocation was aborted by the command host, and the subsequent full run exited 0 |
| `cargo +1.90.0 fmt --manifest-path src-tauri/Cargo.toml --all -- --check` | Passed |
| `cargo +1.90.0 clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets --locked -j 1 -- -D warnings` | Passed |

This is fixture/service evidence, not new built-app acceptance. The approved plan and manual step 8 deferral remain unchanged; no Roadmap transition is claimed.

## Retained cleanup and vendor history: 2026-09-08 UTC

**CODE:** Task `bcf8b6c6` now replaces both unpaged production history commands with bounded raw request/page DTOs. Shared cursors contain version 1, history kind, immutable timestamp and 32-hex-character ID; oversized (over 256 bytes), malformed, cross-kind and unknown-field inputs are rejected at IPC and service boundaries. Cleanup defaults to 20 rows, maximum 100; vendor defaults/maxes at 64. Continuation is exclusive, stateless and non-expiring. Cleanup holds the shared writer while reusing `execution_page`; no lifetime collector or second journal enumerator was added.

**CODE:** Each React consumer retains one page, current/next cursor and independent loading/error state. Older-page browsing never grants execution authority. Newest refresh reveals newer entries; existing boundaries are not shifted by updates. Cleanup Undo uses the immutable execution ID and replaces only the visible matching row. Empty terminal pages are end-of-history, while failed loads preserve the current page and successful operation results. Generation guards reject stale and unmounted responses.

**CODE:** Vendor ordering descends by creation time and job ID, not update time. Up to 1,000 completed/cancelled/unknown outcomes are retained without automatic eviction. Preparation expiry releases transient consent, and releasing cancelled preparation resources no longer deletes its journal. New preparation refuses count/size capacity before launch, independently of unavailable-storage errors. The unchanged atomic writer still caps the file at 8 MiB; vendor admission reserves 256 bytes per retained journal for bounded transition/restart growth. Further growth requires a future journal-store migration, not deleting outcomes. This intentionally bounded ledger is not a new store or on-disk schema.

**RUNTIME:** Initial regression runs failed because cleanup exposed only 100 of 121 rows, cancelled release left zero retained outcomes, and React had no older-history control. Current native tests cover 21 real disposable quarantine executions, restart, newest-20/older traversal and byte-for-byte oldest Undo while other payloads remain unchanged. They also cover complete timestamp-tied traversal beyond 100, new entries/updated outcomes, malformed cursors/limits and unreadable journals, plus the preserved 1,001-journal startup cases. Vendor tests traverse more than 64 unique outcomes exactly once, retain restart unknown outcomes newest-first, preserve completed/cancelled records through expiry/restart, and verify count/size refusals preserve prior bytes and release reservations without launch. Maximal transition/restart headroom, duplicate IDs and schema rejection remain tested.

**RUNTIME:** Frontend coverage includes older-row Undo/outcome inspection, page bounds, newest refresh, empty versus load failure, invalid page rejection, concurrent response ordering, StrictMode and unmount. History browsing fixtures assert zero preparation/confirmation/launch authority calls. Real IPC fixtures verify bounded envelopes and invalid inputs while retaining main-window/local-origin restrictions.

| Command | Observed result |
| --- | --- |
| `pnpm test` | Passed: 204 tests across 29 files |
| `pnpm check` | Passed: project checks, TypeScript and production build |
| `cargo +1.90.0 test --manifest-path src-tauri/Cargo.toml --workspace --locked -j 1` | Passed: 287 tests, zero failures, two existing ignored tests |
| `cargo +1.90.0 fmt --manifest-path src-tauri/Cargo.toml --all --check` | Passed after formatting IPC changes |

These commands ran separately in bounded foreground invocations on Windows x64. This run used the existing Node 22.20.0/pnpm 11.22.0 command resolution and emitted the project's Node 24.19.0 engine warning; no global runtime/config or dependency changes were made. The first project check caught a test crossing the shared-to-feature boundary; the vendor API test was relocated to its feature without removing assertions, and the full check passed. Initial Rust compilation attempts hit the command timeout/host abort; subsequent complete one-job workspace runs exited 0. These are fixture/build observations, not new installed-app acceptance or long-running memory measurements. No real installed vendor was launched. All prior evidence remains historical; **manual step 8 acceptance stays open and no Roadmap Done transition is authorized**.

## Native and rendered artifacts

The fresh native smoke artifact is `.gg/smoke-artifacts/storage-root/result.json`, checked at `2026-09-08T05:19:57.4211462Z`. Executable SHA-256: `aefedd73b31bac5e2b25d2c9d86d7805e9359ddf7c4482806dfc12ae08cd9e77`.

The expanded smoke verifies keyboard skip links and initial-state reflow across all eight routes, eight rendered installed-program refresh/page/unmount cycles, sixteen native one-record pages, ten explicit snapshot releases and two cancellation outcomes (`cancelled` in this run). Completed snapshots refuse pages after release. The native Tauri bridge remains immutable; no IPC responses are replaced. Native-process private bytes were `7,811,072 → 8,855,552 → 8,650,752`, and handles `280 → 280 → 280`. This is three bounded observations, not long-running or whole-process-tree evidence. See [shared UI verification](storage-ui.md) for scope and commands.

An installed-program review focus regression was reproduced before fixing it. Focus now returns to the review after explicit preparation, confirmation or cancellation; filter changes preserve focus on the filter. Frontend tests and project checks were rerun as separate bounded Node 24.19.0 invocations (exit 0 each). The remaining-route renderer replay and a fresh native build/smoke also passed after that change.

It records all eight mounts: drives, disk analyzer, large files, cleaner, duplicates, empty folders, browser and uninstaller. It also records 582 opaque cleaner scopes, cross-module rejection and stale-scope rejection. No native picker, cleanup, undo or vendor confirmation was invoked. The app remained minimized; no desktop input or system screenshots were used.

Strict preview port **1521** now starts successfully, and the replay checks passed against it. Development remains strict **1520**. No port exclusions or Windows services were changed. The earlier preview reservation failure is historical, not a current blocker.

App-only Edge screenshots and replay results are in `.gg/smoke-artifacts/remaining-storage-routes/`, `analyzer-route/` and `large-files-route/`. Data is fictional or disposable fixture replay, not a live scan of personal files. Representative cleaner desktop and browser 320px screenshots were inspected. Browser forced-colors and 200% text checks are bounded renderer evidence, not native scaling or whole-app accessibility certification.

## Automated CI gates for the storage workflows

The native matrix job now runs the expanded storage smoke inside the same single owned standard-user session as the project discovery smoke (`smoke-native-ci.ps1 -StorageSmoke`), against the executable built for that target, and uploads its results next to the existing evidence for both supported architectures. The x64 runner additionally replays the remaining actual routes against its own strict-1521 preview using the runner's installed browser; it starts and stops only that preview, never touches ports 1520/1421 and never changes Windows exclusions. The quality job now runs `check:ports`. `check-native-ci.mjs` requires the renamed steps (`Build native debug executable`, `Verify native project discovery and storage workflows as standard user`, and on x64 `Replay actual storage routes in the preview browser`), so the earlier name drift can no longer hide a missing native launch.

Harness evidence for this wiring, each a bounded Node 24.19.0 or Windows PowerShell invocation from a fresh shell:

| Check | Execution ID | Result |
| --- | --- | --- |
| `node --test scripts/check-native-ci.test.mjs` (representative run JSON; missing, failed, skipped, old-name and wrong-revision cases) | `d5e269b3-b848-41e6-ada9-ab30823161da` | 10 passed, exit 0 |
| Rendered standard-user command file parsed from `smoke-native-ci.ps1` | `2fae948a-2edc-4bbc-aeb0-adf226c41411` | Single lifecycle, both smokes, exit 0 |
| Replay step body extracted from `ci.yml` and executed locally | `bec447ab-c07e-4d47-97b1-32ae66533f6e` | Replay passed, artifacts copied, exit 0 |
| Same step body with a deliberately wrong preview port | `65f4c113-2009-483e-a631-c4a5c083327e` | Exit 1 propagated, owned preview stopped, 1521 released |
| `pnpm test` and `pnpm check` | `7bb0aa92-4592-475d-9913-9bfff1112ab7`, `655e2088-f90c-4bf8-9790-87aa3ff7646e` | Exit 0 each |

These are local harness checks only. No CI run was dispatched and no commit or push was made, so the standard-user storage smoke has **not** been observed on either GitHub runner for this candidate tree; `check:native-ci` still fails closed until a successful run exists for the committed revision. Storage artifacts from such a run must be inspected, not just job names. The interactive native and Narrator gates below remain open.

## Seven phase criteria

| Criterion | Current evidence | Remaining gate |
| --- | --- | --- |
| Eight Windows Tauri workflows | Fresh native route mounts; workspace adapter/IPC tests; renderer replays | Full built-app interactions on disposable data, beyond route mounts |
| Immutable plans, no arbitrary deletion | Real IPC selection/plan/execution fixtures and existing identity-preserving recovery tests | Built-app scan, select, native-confirm, execute and undo acceptance |
| Bounded paging, progress and cancellation | Shared lifecycle tests; real built-app inventory paging/cancel/release and three native-process samples | Filesystem-module and WebView process-tree resource/polling observations |
| Hash-race and final-keeper protection | Workspace duplicate tests plus external-process keeper race driver passed | Include duplicate selection and retained-copy outcomes in native acceptance |
| Separate vendor/filesystem operations and browser safety | Browser scope/activity tests; vendor preparation, native confirmation seam, cancellation and no-replay fixtures | Live default-No dialogs, UAC outcomes and harmless vendor fixture in a disposable environment |
| Supported pinned parity | Independent pinned JSON comparison of 584 declarations and all browser layouts; frontend default regressions, native depth/size/order regression, existing native and IPC behavior/race tests | Complete built-app success/failure acceptance is still missing; source declarations and fixture tests do not establish full eight-module equivalence |
| Complete linked user documentation | [Storage guide](../storage.md), feature checkpoint records, [parity matrix](../parity.md), [security model](../security.md), README links; checker requires the guide and linked feature evidence | Documentation is reconciled to current behavior and explicitly records outstanding manual evidence; update those results after acceptance |

Passing `check:parity` validates the matrix and fixture references and runs the 12 offline catalog/reference tests; it does not execute upstream runtime code or establish complete behavior equivalence. The authority is Kudu `db09e051d0615121e659db187e3799438acbc9e6`, not moving main.

## Feature records

- [Analyzer](disk-analyzer-readonly.md)
- [Large files and recovery](large-files.md)
- [Cleaner scopes](cleaner.md)
- [Duplicates and empty folders](duplicate-empty.md)
- [Browser caches](browser.md)
- [Installed programs and vendor jobs](uninstaller.md)
- [Shared route/UI checks and their limits](storage-ui.md)

## Built-app acceptance: 2026-09-22 UTC

The built-app scan/select/confirm/execute/undo acceptance exercise recorded as
missing above, and explicitly excluded by `scripts/smoke-storage-root.ps1`, was
performed against an actual built app with operator-driven native dialogs and
isolated disposable roots. Full record, provenance and open items:
[built-app acceptance](built-app-acceptance.md).

An in-session report that the folder picker did not open is **resolved as not
reproducible**: it occurred while no app instance was running, because the
acceptance driver closes its own instances on exit. A fresh instance of the same
binary showed `/#/large-files` with an enabled "Choose folder" button and no
alert, and the operator confirmed that clicking it opens the native folder
window. Picker open from the application UI is therefore verified; cancel
followed by a further open is verified at the command level, while a
click-cancel-click reopen driven purely through the button was not separately
measured. Picker opens inside the automated run were direct
`choose_storage_root` IPC calls, so harness evidence covers the dialog and its
command rather than the button.

Also verified on the real app: analyzer totals
and navigation; large-file scan, filter and cursor paging; duplicate detection
with whole-group deletion refused and a keeper-preserving partial plan accepted;
empty-folder root retention and late-child refusal; opaque cleaner/browser scope
refusal of a folder picker; immutable plan review; same-volume quarantine with
undo restoring byte-identical SHA-256 content; cross-volume recovery refused as
unsupported; native permanent confirmation defaulting to No, cancelled without
deleting and then confirmed; and restart reconciliation with no automatic
deletion. Recovered bytes are reported as occupied, never as reclaimed.

Still unverified after that run: refusal at **execution** time for a selection
whose on-disk identity changed after the snapshot (plan creation accepted it;
the guard lives in `cleanup/filesystem.rs` but neither the fixtures nor this run
exercised it). Repeat undo was accepted rather than refused, and the correct
whole-group refusal reported a misleading `snapshot_unavailable`. The run was
made from a dirty working copy, not a release build. No product code changed.

### Open-item follow-up: 2026-09-22 UTC

Execution-time identity refusal and idempotent repeat undo are now fixture-covered, and the
whole-group duplicate refusal reports `invalid_evidence`; see the follow-up in
`built-app-acceptance.md`. `pnpm check` had been failing `check:architecture` because a
test spawned `mt.exe` outside the approved vendor owner; that call now lives beside
`compile_vendor_fixture`. A parallel temp-folder collision in the analyzer IPC fixture and a
`dead_code` clippy failure in the uninstaller IPC test were fixed. Node 24.19.0 / pnpm 11.22.0:
`pnpm check` passed (execution `8d4d5a72-bcc8-4da5-940d-ef6c7320e9eb`); in the same run
`cargo clippy --workspace --all-targets -D warnings` passed and `cargo test --workspace`
reported 294 passed, 0 failed. `cargo fmt --check` passed (`6204d61b-8b20-4da6-b97f-cced03f2440e`).
The built-app acceptance was not re-run. These are passing commands, not harness gate approval.

### Pinned behavioural parity comparison: 2026-09-23 UTC

Read-only comparison of each storage module against Kudu `db09e051d0615121e659db187e3799438acbc9e6`
(upstream source read, never executed; local tests read, not run for this comparison).
Result: **parity not demonstrated**. No `docs/parity.md` row changes status.

Gaps confirmed directly in local code:

- Duplicates: groups are emitted smallest size first (`BTreeMap` by size, `numeric: retained`),
  so record-limit truncation drops the largest, most reclaimable groups. Upstream sorts by
  reclaimable space, largest first.
- Uninstaller: `SystemComponent` is never read, so entries upstream hides are listed; entries
  without `UninstallString` are listed and fail only at launch.
- Empty folders: profile protection covers Documents only (upstream also protects Desktop,
  Downloads, Pictures, Videos, Music, OneDrive).

Reported by the comparison, not yet independently confirmed:

- Empty folders: upstream protected names (for example `node_modules`, `__pycache__`) and
  dot-directories are not excluded; no user exclusion list despite parity.md:81 wording.
- Analyzer: extension totals are unbounded (upstream depth 4); fixed machine-folder exclusion
  changes whole-drive totals; drive inventory is ordered by label, not letter; whole-drive-root
  authorization unconfirmed. None are recorded in `parity.md`.
- Large files / duplicates: cancel discards partial results; no user directory exclusions
  (`node_modules` not excluded); no reveal-in-Explorer action.
- Cleaner / browser: no secure-delete or `*.ext` exclusions; fixed 60-minute recency;
  `Profile <name>` browser profiles accepted only when numeric; parity.md:83 wording implies
  browsers are closed, while the app refuses while they run; Zen may be covered by two catalogs.
- Clean-time failure codes (`not-found`, `in-use`, `permission-denied`) lack a native test.

Each item needs either a fix with a test, or a deliberate adaptation recorded in `parity.md`.

Follow-up the same day: the three confirmed gaps are fixed, each with a test
(`confirmed_groups_rank_by_reclaimable_bytes_largest_first`,
`system_components_are_hidden_and_malformed_flags_fail_closed`, and the `protected-name`,
`profile-folder` and `desktop-elsewhere` modes of
`empty_folders_recursive_complete_evidence_and_all_blockers`). The other reported items are
recorded in `parity.md` as deliberate adaptations, except three items listed there as still to
confirm. Matrix rows stay **Not verified** until the built-app and manual acceptance pass.

### Remaining comparison items closed: 2026-09-23 UTC

1. **Drive order: fixed.** Drives were paged by volume label; they now page by mount letter
   (`scans::drive_order`). `drive_inventory_pages_by_mount_letter_not_label` failed on the
   label key, giving `e:`, `D:`, `C:` (execution `eca9abbe-a628-483d-9fd6-ffffcafa77b4`), and
   passes with the fix.
2. **Whole-drive analyzer root: already supported, now tested.**
   `disk_analyzer_authorizes_whole_drive_root_but_not_machine_roots` authorizes the system
   drive root under the live protection policy and refuses the Windows folder and Program
   Files. Recorded in `parity.md`.
3. **Clean-time failure codes: fixed.** Before the fix, one missing or locked file refused the
   whole plan at execution. Now Windows sharing and lock violations (32, 33) map to a new
   `FsErrorKind::InUse`, and the execution gate lets per-item access failures through to the
   item guard. That guard records `not-found`, `in-use` or `permission-denied` for the item and
   cleans the rest. Plan creation and duplicate groups still fail closed.
   `storage_execution_reports_in_use_and_not_found_per_item_and_cleans_the_rest` covers both
   dispositions; it failed with the old whole-plan gate (execution
   `5142397f-222f-42f5-ab1a-fad9992b4c3b`). `windows_errors_map_to_kudu_cleanup_reasons`
   covers the error mapping, including access denied (5). The outcome view now explains all
   three reasons in plain words (`StoragePlanReview.test.tsx`).

Gates on Node 24.19.0 / pnpm 11.22.0, one run (execution `12f7109e-86d0-430a-9304-9ae9b2d6e8c1`):
`pnpm check` passed; `cargo fmt --all -- --check` passed;
`cargo clippy --workspace --all-targets --locked -- -D warnings` passed;
`cargo test --workspace --locked` 300 passed, 0 failed. These are passing commands, not
harness gate approval. Access denied at execution time is covered by the error-mapping test
only, not by a live access-control fixture. Manual step 8 acceptance remains open.

The acceptance driver's resource step now picks a fresh root per cycle (the old step reused a
consumed single-use root) and samples the whole app process tree, WebView2 included. A rebuilt
debug app exists; the driver needs an operator at the native pickers and was not re-run.

### Built-app acceptance rerun: 2026-09-23 UTC

The debug app was rebuilt from the current working copy (`supa-diska-klinah.exe` SHA-256
`26DD1367DF645C51EDFA70A817F36DFD243B2749437F01B7C1EB9F4FB36BC4EE`) and driven by
`scripts/acceptance-storage-builtapp.ps1` (execution `041eaa33-77a1-47e9-977f-03de02eeb7bf`,
03:02:22 to 03:25:29 UTC, standard user). The operator clicked every native folder pick and cancel
and both permanent-deletion answers. Outcome `completed`: all 26 recorded steps succeeded and no
step failed. They cover picker cancel and choose, disk analyzer, large files, duplicates, empty folders with late-child
refusal, opaque cleaner/browser scopes, plan review, recovery, undo and undo replay, stale selection,
permanent cancel then confirm, resource cycles and restart. The disposable fixture root was removed
by the driver. The record is `.gg/smoke-artifacts/storage-acceptance/acceptance.json` (git-ignored).

Process-tree resources (app, conhost and 6 WebView2 processes): working set 400.4 MB at start,
then 411.3, 412.3 and 412.9 MB after three analyzer scan, page and release cycles (about 0.6 MB per
cycle after the first); private bytes 181.6 to 181.1 MB; handles 3757 to 3745; 8 processes
throughout. Each scan completed in 119 to 141 ms, and each released snapshot then reported
`snapshot_unavailable`. This is one short sample on one machine, not a leak study.

Default button on the permanent-deletion box: the operator reported that **Yes** looked
highlighted when the box opened. The code passes `MB_YESNO | MB_DEFBUTTON2 | MB_ICONWARNING`. A
standalone probe with those exact flags read the box's own state: default button `IDNO`, keyboard
focus on `IDNO` (execution `af077bd9-50dc-42ed-9306-5c5f76a61e86`). The probe used no owner
window, so the app's real box was not measured directly. This observation is **unresolved**
until the real dialog is checked.

### Manual step 8 accepted: 2026-09-23 UTC

The project owner explicitly accepted manual step 8 for this phase as-is, based on the
built-app rerun above. The acceptance covers three known limits: Narrator is waived, keyboard-only
flows are untested, and the permanent-deletion default button is taken from the same-flags probe.
The operator's "Yes looked highlighted" observation is recorded, not resolved. The matrix storage
rows were then marked Verified in `docs/parity.md`.

## Completion remains gated

**2026-09-23: the project owner waived Narrator verification for this phase.** The app is for
personal use, so no screen-reader acceptance is required. This is not an accessibility
certification; the release phase keeps its own accessibility criterion.

Complete native keyboard flows remain unverified. The
permanent default-No dialog and native folder selection/cancellation are now
covered by the built-app acceptance above, the latter including an operator
click on the real "Choose folder" button. Vendor default-No dialogs and UAC
acceptance/denial are now verified per the accepted manual step 8 note above (`docs/verification/uninstaller.md`). Native cleanup/undo must be exercised only against isolated disposable files; vendor execution requires a harmless purpose-built fixture in a disposable environment, never a real installed application.

The pinned behavioural comparison and a first process-tree resource sample are now recorded above.
Uninstaller and drive inventory were not part of this built-app pass; they rely on their own native
IPC tests and `docs/verification/uninstaller.md`. Unknown-ownership leftovers remain informational and non-actionable. Same-volume app recovery remains an approved safety adaptation, not Recycle Bin equivalence or reclaimed space. No final parity row, accessibility certification, plan completion or Roadmap Done status is claimed.
