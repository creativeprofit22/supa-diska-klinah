# Duplicate and empty-folder integration

## Keeper CI gate: 8 September 2026

Task `652eb0a9` adds the existing external-process driver to Quality immediately after workspace tests, reusing their target directory. The step pins Rust 1.90.0, has a five-minute timeout, logs the driver exit code and exits with that code. Quality remains required by the signed release job; no continue-on-error or privileged helper was added. The driver's existing `--locked --no-run` precompile still precedes its 60-second handshake, and the ignored Rust test and keeper assertions are unchanged. The separate native-storage smoke integration remains outside this change.

Fresh local execution `9b433f7f-bf53-424a-adf2-606514f343de` passed under Cargo/Rust 1.90.0: one test passed, zero ignored, followed by the external driver's PASS message and exit 0. Only its marked disposable temporary fixture was mutated. Workflow exit-propagation probe `eeefc6e9-f97a-4d49-bc82-1009639cf62e` confirmed the real driver reference and preserved a synthetic child exit 37 through the extracted step body (Windows PowerShell locally, not a hosted Actions run).

Re-checked on 22 September 2026: the driver passed again locally under Rust 1.90.0 (`e4434fbd-364c-4399-9498-ad5f81bb003c`: one test, zero ignored, external PASS, exit 0), and the step body was hardened to fail closed when the driver never produces an exit code. Hosted job inspection of run 34273815144 shows Quality failing at Clippy on the pushed SHA with the keeper step skipped, so the driver still has no hosted execution.

Hosted execution of the new gate remains **pending**, not failed or proved by compilation. Read-only inspection of [CI run 34103714621](https://github.com/creativeprofit22/supa-diska-klinah/actions/runs/34103714621) found the older workflow without this step. See [storage verification](storage-parity.md#keeper-ci-gate-2026-09-08-utc) for execution IDs and the remaining release-evidence blocker. PR-branch commit and push are now authorized for this gate and its notes only; hosted verification remains pending the exact pushed SHA. No release dispatch or manual acceptance is authorized.

Step 5 checkpoint, 7 September 2026. Both native start adapters use closed Raw JSON objects, opaque module-bound roots, bounded depth and fixed native resource limits. Duplicate scans expose a validated minimum-size filter; neither command accepts paths, hashes, keepers, proofs or caller-selected worker limits.

The duplicate route separately pages groups and members through the shared scan controller. It processes one group at a time, clearly clears selections on group changes, and reserves the first member of the first page as an unselectable keeper across later pages. This is not a claim that the keeper can be chosen arbitrarily in this UI. The backend independently rejects all-copy selections, collapses hard-link identities and revalidates keeper/content evidence. Hash-byte and completed-hash counters are non-live progress text, avoiding poll announcement spam. App recovery and separately native-confirmed permanent review reuse the existing cleanup engine; no default selection occurs.

Empty-folder results explain root retention and omission of non-empty/hidden/system/protected/incomplete trees. Only explicit permanent review is offered. Atomic empty-only removal cannot safely move a directory that might gain a child into recovery; native plan validation rejects directory recycle/quarantine before mutation. Late children survive and removal order remains deepest first.

Actual checks:

- `cargo test --manifest-path src-tauri/Cargo.toml --locked --test duplicate_empty_commands`: six tests passed, including real Tauri IPC on disposable files/directories, opaque filters/capabilities, hard-link collapse, all-copy rejection, subset recovery planning, changed-keeper refusal, root retention and new-child survival. The native permanent engine primitive was used only on disposable fixtures; no Windows prompt was opened.
- `cargo test --manifest-path src-tauri/Cargo.toml --locked -p windows-platform storage::duplicates_tests --lib`: three existing native tests passed (hash cancellation/bytes, all-copy/hardlinks/keeper changes, vanished keeper/same-size rewrite/writer conflict).
- `cargo test --manifest-path src-tauri/Cargo.toml --locked -p windows-platform storage::empty_folders_tests --lib`: three tests passed (hidden/system/junction parents, late-child/no-recycle-fallback, deepest-first execution/no replay).
- `pnpm exec vitest run src/features/duplicates src/features/empty-folders --pool=threads --maxWorkers=1`: four tests passed for keeper reservation across pages, group/filter reset, bounds, root/no-undo warnings and explicit review without execution.

Fixture fixes were not safety suppressions: concurrent native tests needed an atomic sequence in temporary directory names; the draft duplicate status fixture was completed with the native DTO counters. The existing empty-folder recycle regression now requires earlier plan refusal plus unchanged empty directory and empty history instead of accepting an unusable plan and refusing only at execution. Its no-fallback invariant remains, with separate file-Recycle-Bin execution refusal still tested in `large_files_commands`.

A too-narrow initial native test filter matched zero tests; it is not counted as evidence. The corrected module filters above ran the actual suites. Full rendered route, keyboard/Narrator and live permanent-confirmation checks remain later shared verification work. No real user directories were cleaned.

## Built-app acceptance follow-up: 2026-09-22 UTC

The live permanent-confirmation and rendered-route checks deferred above were
exercised against an actual built app in
[built-app acceptance](built-app-acceptance.md). All members of a duplicate
group are listed as eligible with nothing auto-selected; a plan selecting every
copy was refused, and a plan leaving one independent copy was accepted. Empty
folders retained the scan root and refused a directory that gained a child after
the snapshot. The whole-group refusal is correct but reports a misleading
`snapshot_unavailable` while the same snapshot still serves partial plans.
