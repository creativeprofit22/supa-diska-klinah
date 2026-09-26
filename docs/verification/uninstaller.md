# Installed-program/vendor-job integration

Step 7 checkpoint, 7 September 2026. Inventory and vendor actions are separate from filesystem cleanup. All app commands reuse `CleanupService::vendor_jobs()` and the existing registry/process/journal/writer coordination. Inventory is read-only and paged through the shared storage controller. No raw executable, argument list, registry location or caller ownership claim is accepted by IPC.

A selected program ID from a live snapshot can prepare a job, never launch one. The separately invoked native confirmation resolves the prepared registry/executable evidence, shows the backend program name, family, executable and arguments, uses an app-owned window and defaults to No. Oversized consent text is rejected rather than silently truncated. Denial cancels before launch. State, registry and executable evidence are checked again after consent and before launch. No actual confirmation window was automated at this checkpoint.

The UI requires explicit preparation and confirmation. It cancels/releases late or abandoned unsubmitted consent, ignores stale poll responses, polls one request at a time, and stops on terminal/unmount. Navigation after submission does not kill or claim to stop the vendor. History requests are single-flight, including the selected receipt's bounded status fallback; root StrictMode replay waits for the obsolete request before loading again. Retained history has 64-job pages; vendor jobs have no undo or replay controls. Unknown-ownership leftovers are information only.

Exit states remain truthful: only standardized MSI codes receive standardized success/reboot meanings. A Win32 zero exit may remain OutcomeUnknown; neither that nor a launcher exit proves removal. Cancellation/timeout can stop waiting without stopping the installer. Inventory estimates are not reclaimed bytes; refresh observes registry changes, not definitive removal.

Actual checks:

- `cargo test --manifest-path src-tauri/Cargo.toml --locked --test uninstaller_commands`: one real IPC test passed, covering read-only host inventory, typed bounded pages, raw object/filter restrictions, denied foreign origins/windows, no filesystem plan authority, release, and empty isolated vendor history. No real program was prepared or confirmed.
- `cargo test --manifest-path src-tauri/Cargo.toml --locked -p windows-platform storage::vendor_jobs::tests --lib`: 18 tests passed, including confirmation denial, revalidation after consent, no replay, queued/launch cancellation, writer coordination, MSI outcome distinctions, persistence failure, unknown retention and the existing harmless disposable native process probe. Fixture-confirmation callbacks never open real dialogs.
- `pnpm exec vitest run src/features/uninstaller --pool=threads --maxWorkers=1`: seven tests passed for no auto-prepare/confirm, stale prepared-job release, post-submission navigation, serial polling, stale-error rejection, bounded StrictMode history, mismatched confirmation rejection and truthful page outcomes.

Test corrections retained safeguards: the StrictMode fixture now enables root StrictMode using Testing Library's supported option, matching the app; the affirmative Win32 fixture expects its actual conservative OutcomeUnknown plus exit code zero and exactly one launch, not an invented standardized success. Foreign-caller assertions were strengthened to require ACL denial, not merely any error.

## Selected receipt synchronization — 8 September 2026

Task `3ccfbb90` preserves the uncommitted baseline and the separate retained-history pagination work (`bcf8b6c6`). Manual newest-history refresh reconciles the selected receipt using both job and program IDs, with one status read if it is absent from that page. Page absence alone is not expiry. Scope generation, selected identity and poll version guard late responses; accepting terminal truth retires old polls without recursively refreshing. Recovered pending truth resumes polling, and recovered evidence clears obsolete request errors.

Missing/unreadable prepared evidence stays selected but non-actionable, with an explicit fresh-inventory/review requirement. Native expired/cancelled evidence also removes confirmation. Missing submitted outcomes remain visibly uncertain, without inventing a successful or retryable native job. Retired/submitted receipts cannot regain confirmation authority. Refresh invokes no prepare, confirm, cancel or release command. Native authorization, expiry, Windows consent, persistence and no-replay rules are unchanged; the existing native fixture ages `created_at` by `EXPIRY`, without changing the Windows clock.

Red regressions: hook execution `2648a031-412a-42b1-ac5b-ddb43112e3e4` failed both missing/expired receipt cases before implementation; page execution `f1820137-83bc-42bd-867c-a615dfa3a3bf` observed the old confirmation button still present before the page gate fix. Fake-timer hook tests cover polling-error recovery through history/status, missing submitted outcomes, late poll success/errors, late scope responses, confirmation races, ID mismatches, single-flight status fallback and pending recovery. Page tests cover both missing and native-retired evidence, disabled preparation and zero extra vendor actions.

Verification for this synchronization change:

- `pnpm test` — execution `47e5d946-9d27-4976-81a5-db65a6615c6f`, exit 0: 228 tests across 29 files, including 21 vendor-hook tests and eight vendor-page tests.
- `pnpm check` — execution `18984779-ec0a-493e-9c29-db3a73a7df24`, exit 0: project checks, TypeScript and production build passed.
- Both were bounded, nonpersistent foreground commands using installed fnm Node 24.19.0 and pnpm 11.22.0; shell and pnpm-child runtime paths were checked in each execution. No runtime/dependency pins changed. Native source/tests were not changed or rerun; no real vendor operation or clock change occurred.
- These are command results, not harness acceptance. Manual step 8 and the separate harness evidence blocker in `storage-parity.md` remain open; no Roadmap Done/retry is authorized.

Live native confirmation/UAC and installed-app interactive smoke remain later gates. No installed application was uninstalled and no leftover path was deleted. Full rendered routes/accessibility and pinned upstream parity have not been claimed here.

## Live native confirmation/UAC gate — 22 September 2026 UTC

Task `8347ca53` (VERIFY-BEFORE-SHIP, High, UNTESTED-BLAST-RADIUS) assessed the installed-program native confirmation and vendor launch/cancel/UAC/restart path for shippability. **The gate is not cleared. Release stays gated.** No implementation, test or fixture changed for this assessment, and no real installed program was prepared, confirmed, repaired or uninstalled.

Current automated coverage rerun (bounded, nonpersistent foreground, `rustc` 1.90.0 toolchain): `cargo test --manifest-path src-tauri/Cargo.toml --locked -j 1 -p windows-platform --lib storage::vendor_jobs` — execution `e9c2dbbf-9726-45f1-9d41-32a935141e17`, exit 0, 23 passed / 0 failed. This includes `native_disposable_launch_identity_cancellation_and_timeout_never_kill`, which compiles the purpose-built harmless fixture (`src-tauri/crates/windows-platform/tests/fixtures/vendor-disposable.rs`) and really launches it through `NativeProcessBoundary`/`ShellExecuteExW`. No dialog, desktop input or registry mutation occurred; every confirmation used the `confirm_with` seam.

Two further targeted reruns, same session, both exit 0 with no source change:

- `cargo test --manifest-path src-tauri/Cargo.toml --locked -j 1 --test uninstaller_commands` — execution `665ebcd6-5baf-495b-bc61-9be0a2bf4835`, 4 passed / 0 failed: read-only host inventory, closed vendor IPC capabilities, and the stable non-sensitive error mappings. No real program was prepared or confirmed.
- `pnpm exec vitest run src/features/uninstaller --pool=threads --maxWorkers=1` — execution `4eee92cf-5db5-4340-9756-d06cf2957ebb`, 30 passed / 0 failed across the page, `useVendorJobs` hook and history suites. Bounded nonpersistent foreground command on installed fnm Node 24.19.0 and pnpm 11.22.0, with shell and pnpm-child runtime paths checked in the same environment (`0a92b1f5-f060-47fb-9c40-139a6af61dcb`). An initial explicit-PATH attempt resolved Node 22.20.0 and is not accepted as verification. No runtime or dependency pins changed. These are renderer-level checks and are explicitly not evidence for native confirmation or UAC.

Already covered by real, current evidence (not the gap):

- Real disposable child process launch, observed via the fixture's own `started`/`finished` markers.
- Post-consent executable identity change refused before launch (fixture overwritten with non-PE bytes; `started` never appears).
- Registry command change refused both before the prompt and after an affirmative prompt (`RegistryChanged`, zero launches).
- Cancelling the wait after launch differs from cancelling before launch: the launched fixture keeps running and completes its own write after the handle/wait is released; exit code stays `None`; state stays `OutcomeUnknown`. Timeout behaves the same and never terminates the child.
- Generic Win32 exit zero remains `OutcomeUnknown`; only standardized MSI codes receive standardized meanings (`outcome_table_is_msi_only`).
- Restart never replays a command; unknown outcomes cannot expire or be released.
- Consent text content — backend-resolved program name, job ID, family, executable, arguments — asserted through the seam.

### Real dialog now verified without human input

The live `MessageBoxW` consent window no longer needs a human. `real_confirmation_dialog_is_app_owned_defaults_to_no_and_drives_both_answers` (`vendor_jobs_tests.rs`) creates a genuine process-owned top-level window, calls the production `confirm_native` so the actual dialog opens, and a watcher thread reads what Windows reports about it before answering:

- The dialog is located by its real title and accepted only when `GetWindow(GW_OWNER)` equals our own window — native proof it is app-owned, not a renderer element.
- `DM_GETDEFID` is queried directly: Windows reports default control `7` (`IDNO`). Both `IDYES` and `IDNO` controls exist.
- The watcher then posts only the answer a human would click. Denial yields `CancelledBeforeLaunch` and, after a settling delay, the disposable fixture's `started` marker never appears. Acceptance launches only the revalidated fixture, reaching `OutcomeUnknown` with the fixture's own `started`/`finished` markers.

This supplies the click; it does not bypass or relax any production consent check — every observation above is native Windows state. Non-vacuity was proven by mutation: asserting the default is `IDYES` failed with `left: 7, right: 6`, i.e. Windows really reported No as default. Command: `cargo test ... -p windows-platform --lib storage::vendor_jobs` — execution `6fb8274f-9b7d-4482-b050-7b5188ab00c4`, exit 0, 24 passed / 0 failed (up from 23). `cargo fmt --check -p windows-platform` — execution `98e0f018-46bd-45f3-885e-33795466f96b`, clean. Production source was not modified; this is a new test only.

### Owned HKCU fixture: end-to-end real command path and post-restart refresh

`owned_hkcu_entry_flows_through_real_command_path_and_refresh_after_removal` (`vendor_jobs_tests.rs`) closes the remaining two non-UAC gaps by automation, with production boundaries only — the real `NativeRegistry` reader and the real `NativeProcessBoundary` launcher, no registry or process double:

- **Owned disposable registration.** A uniquely named entry (`ZZ Disposable Vendor Fixture <opaque id>`) is created under **HKCU only**, pointing at the purpose-built harmless fixture. HKLM is never written, and no real installed program is named, modified, repaired or uninstalled. The entry is removed by the test and again on `Drop`.
- **Live host resolution through the real inventory.** `start_inventory` — the same backend call the UI's refresh uses — enumerates the real registry and resolves the fixture exactly once; `prepare` then reports that backend-resolved program name.
- **Real dialog, real launch.** The genuine `MessageBoxW` opens, is confirmed app-owned and default-No, is answered Yes, and only the revalidated fixture runs, ending at `OutcomeUnknown`.
- **Restart never replays.** Markers are deleted, the manager is dropped and reopened over the same journal: state stays `OutcomeUnknown`, no marker reappears after a settling delay, and history length is unchanged.
- **Refresh after real removal does not imply uninstall.** With the registration actually deleted, a second real inventory returns nothing for that name, while the recorded job keeps its `OutcomeUnknown` state, its exit code, and its history entry — refresh never upgrades an unknown outcome into a success.

Non-vacuity proven by two temporary mutations, each reverted: suppressing the registry value writes failed with `left: 0, right: 1` ("fixture must resolve exactly once"), and skipping the fixture removal failed "refresh must reflect the removed fixture registration". Both prove the test observes live registry state rather than passing trivially.

Hygiene after the full run: `reg query` over the HKCU uninstall key reports **0** leftover fixture entries, and `tasklist` reports **0** stray `vendor-disposable.exe` processes — owned fixtures removed only after their processes stopped.

Verification commands, this session:

- `cargo +1.90.0 fmt --check -p windows-platform` — execution `a523a263-4c43-45dc-9891-714a8254d5fe`, exit 0, clean.
- `cargo +1.90.0 test --manifest-path src-tauri/Cargo.toml --locked -j 1 -p windows-platform` (full crate) — execution `41d5b299-c015-4c47-8c49-20282c50ecf5`, exit 0: 180 passed / 0 failed / 1 ignored, plus 5 passed in the integration target. No production source changed; both additions are tests.

### Real UAC elevation prompt — human-interactive, accepted and denied

UAC renders on the secure desktop, which no program can drive; the gap was closed with a genuinely human-interactive test rather than by weakening UAC policy. `human_uac_prompt_accept_then_deny` (`vendor_jobs_tests.rs`, `#[ignore]`, run explicitly with `-- --ignored --nocapture`) builds the same harmless disposable fixture used elsewhere, embeds a `requireAdministrator` manifest with the real Microsoft Manifest Tool (`mt.exe`), registers it as a uniquely named, owned HKCU-only entry, and drives it through the real confirmation dialog into a real elevation prompt, printing on-screen instructions for exactly which button to click.

Run interactively this session with a human present:

- **ACCEPT** scenario: the human clicked Yes on the real elevation prompt. Result: elevation granted, the fixture actually launched (`started` marker present), and the job state stayed truthfully `OutcomeUnknown` — launcher success is never conflated with vendor-confirmed removal.
- **DENY** scenario: the human clicked No. Result: the real `ShellExecuteExW` returned `ERROR_CANCELLED` (1223), mapped to `VendorJobState::CancelledByVendorOrUAC`, and the fixture never launched (no `started` marker).

Command: `cargo +1.90.0 test --manifest-path src-tauri/Cargo.toml -p windows-platform --lib storage::vendor_jobs::tests::human_uac_prompt_accept_then_deny -- --ignored --nocapture` — execution `92e27677-9add-40f5-9490-a8ef8e958cd2`, exit 0, 1 passed / 0 failed, both scenarios verified in the same run. Afterward: `reg query` over the HKCU uninstall key reported 0 leftover fixture entries, `tasklist` reported 0 stray fixture processes, and the disposable per-run temp directories were removed. `cargo +1.90.0 fmt --check -p windows-platform` — clean. No production source changed; this is a new, explicitly opt-in test that never runs in CI or a default `cargo test`.

### Still UNVERIFIED

1. Human visual confirmation of these flows in the shipped UI shell (the rendered app window, not just the backend path it calls). The backend path behind every flow above — inventory, prepare, real dialog, real elevation accept/deny, restart, refresh — is now covered end to end by tests, including one just run interactively by a human. What remains is watching the rendered page itself during a live vendor job; that observation belongs to manual step 8 and was not performed in this session.

The `storage-parity.md` markers for the live default-No dialog and UAC acceptance are both superseded by the automated and human-interactive tests above; they are not rewritten here so as to confine this update to the uninstaller record, per scope.

**Manual step 8 accepted by the user on 22 September 2026 UTC**, for this vendor confirmation/UAC scope specifically, based on the evidence above (automated dialog test, automated HKCU end-to-end test, and the human-interactive elevation-accept/deny test just run and observed). This acceptance is scoped to this section; it does not extend to Narrator, full native keyboard flows, full pinned parity comparison, or process-tree resource evidence, which remain open per `docs/verification/storage-parity.md`.

No code defect was demonstrated during this assessment, so no narrow fix was made. Helper operations and signing work were not touched.
