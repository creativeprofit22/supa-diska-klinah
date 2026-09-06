# Windows x64 alpha release checklist

This checklist gates the alpha on Windows 10 x64. Never invoke `SRSetRestorePointW` in CI; automated checks cannot replace the two local observations below.

**Scope:** August alpha results are historical. The September build-artifact candidate remains **VERIFY-BEFORE-SHIP** with four open native requirements in the build artifact budget drill below. No historical pass certifies current HEAD or a later rebuild.

## Candidate

Use one identifiable x64 candidate for every check.

- Version or label:
- Commit:
- Build command ID:
- App SHA-256:
- Helper SHA-256:
- Windows edition, version, and build:
- Tester:
- Test timestamp (UTC):

## Automated gates

- [x] Hardened helper, frontend, Rust, security-boundary, and x64 smoke checks pass.
- [x] The main app starts at standard integrity with its adjacent helper present.

## Successful restore-point run

Run locally with System Protection enabled. Record observations, not expectations.

- [x] Creating a restore point displays exactly one UAC prompt.
- [x] Approving UAC returns sequence number `138`.
- [x] The app reports success and System Restore `LastIndex` matches `138`.
- [x] The original main process remains alive, unique, and unelevated.
- [x] No helper remains after completion.

```text
Command IDs: `d12f8a3f-ac0d-4f3a-9ce2-7a5ca93d02e2`, `a8298dde-bd34-4c9e-8f93-760ec58ed99d`, `5c8744b6-c0be-4b24-8934-7b83da9b7459`
Command: submit through the minimized WebView; inspect its result, process counts, helper count, and System Restore `LastIndex`.
Observed result: one UAC prompt was approved; the app reported sequence `138`; `LastIndex` became `138`; process `6272` remained the only app process; no helper remained.
```

## UAC cancellation

Cancel one restore request from the same standard-integrity app.

- [x] Exactly one UAC prompt appears.
- [x] The app displays its administrator-approval cancellation message.
- [x] The original main process remains alive, unique, and unelevated.
- [x] No helper remains and no restore point is created.

```text
Command IDs: `58d4003b-949f-462f-b110-63606228eedc`, `6c5243b0-4ee8-4a26-88e5-6f93b7906eef`, `2af01392-fd3b-4d84-afdb-cd09dd4f148b`, `4e195a3b-e7ac-4644-a608-5452ca510344`
Command: submit through the minimized WebView, cancel UAC, then inspect the app, processes, helper, and `LastIndex`.
Observed result: cancellation was reported; process `11600` stayed unique and unelevated; no helper remained; `LastIndex` stayed `137`.
```

## Disposable cleanup drills

Use only a newly created disposable directory containing synthetic files. Never use personal or shared data.

- [ ] Preview, move one item to the Windows Recycle Bin, and undo the exact item.
- [ ] Quarantine one item, restart the app, and undo it without duplicate removal.
- [ ] Enable automatic cleanup, verify the grace deadline, then verify due purge.
- [ ] Confirm permanent deletion requires the second warning and removes only the selected item.
- [ ] Record selected, processed, failed, quarantined, purged, occupied, and reclaimed totals.

### Cleanup drill record — 2026-08-30

Command: `cargo test --manifest-path src-tauri/Cargo.toml -p windows-platform cleanup::execution::tests::disposable_recycle_quarantine_purge_and_permanent_drill --locked -- --ignored --exact`

Observed: the test used uniquely named synthetic files under the Windows temporary directory; Recycle Bin deletion and exact undo, quarantine across service restart and undo, enabled-policy due purge, and separately invoked permanent deletion all passed. The owned fixture tree was removed afterward. This does not verify recovery of unrelated or valuable files.

## Build artifact budget drill

**Current release status: VERIFY-BEFORE-SHIP.** The checked results below describe only the 2026-09-05 executable identified by its hash, not current HEAD, a rebuilt app, or an installer. Four native requirements remain open: budget enforcement/quarantine, protected artifact identities during enforcement, build-generation undo, and the complete screenshot set. The older alpha pass later in this document applies to its separate August candidate.

Use the checked-in Rust fixture or a newly copied disposable project. Never register a shell, script host, personal repository wrapper, or executable you do not trust.

- [x] Register absolute `cargo.exe` with separate `build` arguments and approve the native detail prompt. On 2026-09-05 the user explicitly confirmed clicking the native **Sí (Yes)** button after PID-scoped exact-content capture. Read-only verification observed dialog closure and `Cargo manual Yes 20260905T195920Z` in both Saved profiles and storage with matching inputs. Evidence: `.gg/artifacts/cargo-manual-yes-20260905T195920Z/` (`native-dialog.png`, `dialog-verification.json`, `manual-action-attribution.json`, `after.png`, `outcome.json`). The click is user-attested, not independently instrumented; automated decision actions: zero.
- [x] Decline one native registration and confirm its unique new profile does not persist. User reported a manual decision on 2026-09-05; read-only verification independently observed “The native approval was declined.”, dialog closure, and absence of `Cargo native No 20260905T182228Z` from Saved profiles and the unchanged registry. Automated No actions: zero; the manual click itself was not independently observed. Evidence: `.gg/artifacts/cargo-native-decline-20260905T181750Z/manual-decision-check-20260905T1900/` (`outcome.json`, `rejection-status.json`, `current-main.png`). Fixture hashes match the prior owner-audit snapshot; automatic budgets remained disabled. This closes only the decline item; the remaining release drill stays open.
- [x] Cancel one running profile and confirm no success stamp or cleanup occurs. Verified through packaged-app Run/Cancel controls on 2026-09-05 using the existing approved disposable Cargo profile and a fixture-only 90-second build script. Real Cargo/build-script execution was observed; all captured build descendants, including conhost, were absent when cancelled was observed. Success-stamp ledger, complete storage hashes, journal/quarantine inventory, and profiles remained unchanged; automatic budgets stayed disabled. Evidence: `.gg/artifacts/cargo-cancel-20260905T191240Z/` (`report.md`, `outcome.json`, `running.png`, `cancelled.png`, before/after snapshots). Only this cancellation item is closed; the overall drill remains open.
- [x] Run one successful direct Cargo profile and record the fixed status only, not child output. Native evidence: `.gg/artifacts/cargo-visible-20260905T165001933Z/cargo-first-status.json`, `cargo-first.png`, and `outcome-final.json`; actual Run action and fixed `succeeded, exit code 0` status, not simulated smoke.
- [ ] Create an old owned release generation, enable a tight budget, and confirm only that generation enters quarantine.
- [ ] Confirm current debug output, dependencies, and incremental identities survive enforcement.
- [x] Run the same debug profile again and confirm the unchanged binary remains a warm build. Native evidence: `.gg/artifacts/cargo-visible-20260905T165001933Z/cargo-second-status.json`, `first-identities.json`, `second-identities.json`, and `warm-comparison.json`; all 18 recorded artifact identities/bytes/mtimes match. This proves warm state only with budgets disabled, not survival during enforcement.
- [ ] Undo the quarantined release generation before its purge deadline.
- [ ] Record disabled, selected-preview, protected-floor, running, and cancelled native smoke screenshots.

Automated fixture tests provide regression coverage. They do not replace native approval, decline, cancellation, direct Cargo, undo, or incremental-state observations on the release candidate.

### Historical candidate evidence reconciliation — 2026-09-05

**ORIGINAL BUDGETS-DISABLED VERIFICATION COMPLETE; BROADER RELEASE DRILL NOT COMPLETE — five requirements proven, four still missing.** The manually approved continuation at 20:09 UTC closes the original task's last gap. This reconciliation supersedes historical pending statements below without rewriting their observation history. The continuation used user-operated registration/Yes and read-only verification; no new build, production/test edit, or budget-policy change was performed.

Candidate: `src-tauri/target/release/supa-diska-klinah.exe`, SHA-256 `F7374DE82907FD6C4C42B049D5B5797228966E1A52CDBC740CC7B5138B5E3AD8`. `V/outcome-final.json` binds the first real runs to this hash; `D/process-identity.json` binds the later decline to the same hash. Cancellation reused the same process/path/start/hash guard (`C/drill.ps1`, `N/continuation-run.json`). These are packaged executable observations, not installer verification or evidence for a later rebuild.

Evidence aliases (all local ignored paths under `.gg/artifacts/`; not portable release attachments). Personal absolute paths below use descriptive placeholders, not literal Windows inputs; original canonical spellings remain in private evidence:

- **V** = `cargo-visible-20260905T165001933Z/`
- **N** = `cargo-native-decline-20260905T181750Z/`
- **D** = `cargo-native-decline-20260905T181750Z/manual-decision-check-20260905T1900/`
- **C** = `cargo-cancel-20260905T191240Z/`
- **Y** = `cargo-manual-yes-20260905T195920Z/`

| Drill requirement | Verdict | Evidence and limits |
| --- | --- | --- |
| Register canonical Cargo with separate arguments and explicitly approve the native prompt | **Proven — user-confirmed manual Yes, observed persistence** | `Y/candidate.json`, `Y/form-before.json`, `Y/native-dialog.json`, `Y/native-dialog.png`, and `Y/dialog-verification.json` bind PID 26576 and the recorded hash to the exact executable, separate `build` / `--offline`, authorized fixture working directory, and one generation path `target/debug/artifact-budget-fixture.exe`. The user explicitly confirmed clicking native Sí (Yes), ID6; `Y/manual-action-attribution.json` records this as user testimony, not an instrumented click. `Y/after.png`, `Y/saved-profiles-after.json`, `Y/profiles-after.json`, and `Y/outcome.json` prove closure and the exact unique profile with matching persisted inputs. The earlier V attribution gap remains historical; this new attempt fills the task requirement. |
| Decline registration; its unique new profile does not persist | **Proven — manual decision, observed native rejection** | `D/outcome.json`, `D/rejection-status.json`, `D/current-main.png`, `D/profiles-current.json`: user reported the manual decision; read-only verification observed rejection, closure, no unique profile in UI/storage, and unchanged registry. Zero automated No actions; the click itself was not independently captured. The originally intended `Canonical Cargo DECLINE drill` attempt is not the successful decline evidence. |
| Cancel a running build; no success stamp or cleanup | **Proven — actual native controls/processes** | `C/observed-build-processes.json`, `C/processes-running.json`, `C/processes-at-cancelled.json`, `C/running.png`, `C/cancelled.png`, `C/outcome.json`, and `C/report.md`: real Cargo/build script, Run/Cancel controls, no recorded descendant survivors at terminal observation, and identical before/after stamp/storage/journal/quarantine inventories. |
| Successful direct Cargo run; fixed status only | **Proven — actual native Run** | `V/cargo-first-status.json`, `V/cargo-first.png`, `V/outcome-final.json`: running then succeeded/exit 0; child output not captured. This does not retroactively establish the earlier Yes action. |
| Old owned release generation alone quarantined under tight budget | **Missing** | `V/outcome-final.json`, `C/state-paths-before.json`, `C/state-paths-after.json`: budgets remained disabled and no native enforcement/quarantine result is recorded. Unchanged empty quarantine is cancellation protection evidence, not positive quarantine evidence. |
| Debug/dependency/incremental identities survive enforcement | **Missing** | `V/first-identities.json`, `V/second-identities.json`, `V/warm-comparison.json` prove unchanged artifacts without enforcement only. No enabled-policy before/after identity evidence exists in these records. |
| Repeated debug run remains warm | **Proven — actual native second Run** | `V/cargo-second-status.json`, `V/cargo-second.png`, `V/warm-comparison.json`, and both identity snapshots: 18 entries unchanged. |
| Undo quarantined release generation before deadline | **Missing** | No build-artifact quarantine or corresponding native Undo record is present. `C/state-paths-before.json` / `C/state-paths-after.json` show unchanged quarantine inventory; the separate generic cleanup test earlier in this document is not this candidate's build-generation undo drill. |
| Disabled, selected-preview, protected-floor, running, cancelled screenshots | **Missing complete set** | `C/ui-before.json` proves disabled status, and `V/recorded-approval.png` is a native prompt screenshot; neither is a complete disabled-budget screen. `N/continuation-selected.png` visibly captures Automatic budgets disabled (its selected Project root is not a selected-budget preview). `C/running.png` and `C/cancelled.png` cover actual native run states. Native selected-budget-preview and protected-floor screenshots are missing. Simulated smoke does not fill them. |

Displayed-input verification is **proven separately from explicit consent**. The captured native dialog shows canonical Cargo `<canonical-user-home>/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin/cargo.exe`; ordered arguments `build`, `--offline`; working directory `<canonical-repo-root>/.gg/artifacts/cargo-visible-20260905T165001933Z/disposable-rust`; artifact paths `target/debug/artifact-budget-fixture.exe`, `target/debug/deps`, `target/debug/incremental`. `V/canonical-cargo.json` records equal ordinary/canonical executable hashes. The dialog does not display the unique profile name; association comes from the captured attempt/form/registry records, not from claiming that name was shown.

Synthetic harness smoke (`scripts/smoke-native.test.ps1`, historical logs below) remains **simulated smoke only**. The later read-only cancellation unittest (`.gg/artifacts/cargo-cancel-verification-20260905/test_cancellation.py`) asserts preserved native evidence; it does not perform a new native run or replace an absent observation. Ownerless-dialog UX remains a separate finding, not an excuse to alter the application during this drill.

**Original task complete; broader work remains open:** manual explicit Yes is now attributable through the user's confirmation plus captured native contents and correlated UI/storage persistence. Prior decline, successful real Cargo, warm-build, and cancellation evidence remains credited without repetition. Enforcement/quarantine, identity preservation during enforcement, build-generation undo, and selected-preview/protected-floor screenshots still require a separately authorized drill. Keep automatic budgets disabled; do not run or delete the newly registered profile as part of this verification.

`Y/outcome.json` verifies all 69 fixture file hashes/sizes/mtimes unchanged, both prior profiles preserved, non-profile storage unchanged, budgets disabled, and the candidate path/start/hash still matching. Only the intended new profile was added: ID `0bc307a4d0099abece15a060a76875e6`. Before-state evidence was saved before manual submission; the registry remained byte-identical while the dialog was open. The native dialog is ownerless (owner HWND 0); identity used candidate PID, exact native title/class/content, and localized Sí ID6, not ownership. An initial English-only button-label check failed; the preserved native capture established the actual Spanish label, and the read-only verifier then checked that exact label plus PID/class/native ID. No automated submission or decision was performed, and no build or direct IPC substitute was used. Native smoke was not repeated, per the user's later instruction; simulated smoke contributes no consent evidence.

### Documentation evidence review — scope and unresolved verification

The available primary Y/D/C/V outcomes and structured records support the historical results with the limits stated above. Read-only comparisons reconfirmed identical cancellation storage, directory inventories, generation ledger and profiles; identical warm-build identity snapshots; and identical 69-file manual-Yes fixture snapshots. The recorded three-process Cargo tree and previously observed rustc were absent from the terminal cancellation snapshot. These are comparisons of saved observations, not a new native drill or continuous process monitoring.

`C/counts.json` is an intermediate record, not a final failure: `C/drill.ps1:39` saves it **before** invoking Cancel, while `result` is still `BLOCKED` and `descendantsExitedAtCancelled` is false. Lines 47–56 record the terminal snapshot, check survivors, and set the successful outcome. `C/outcome.json`, the empty `C/surviving-build-descendants.json`, and the independently compared process snapshots support the final cancellation result; the earlier counts file was not rewritten.

Evidence not independently verified by this documentation review:

- Actual manual Yes/No clicks remain user testimony. `Y/dialog-verification.json` deliberately has `approvalProven: false`: it verifies dialog contents, not consent. Only the subsequent attribution and persistence records support the qualified manual-Yes result.
- Screenshot files are available, but their pixels were not re-inspected to avoid exposing private desktop/path metadata. Visual descriptions remain attributed to the historical reports and associated UI records.
- Four cited native-smoke stdout logs contain the harness verification message, but no exit-status metadata. Historical exit-0 values and exact execution timing remain recorded claims, not independently recovered exit codes. The logs are local under `<user-home>/.gg/foreground/`; they are not committed attachments.
- The current executable, current HEAD, any later rebuild, and installer behavior were not revalidated. The four open native release requirements have no completing evidence in the reviewed set. August restore-point/UAC results were not re-audited.

Keep raw UI/storage snapshots, screenshots, command logs, and original personal paths private. The documentation check validates required documents and links, not these native runtime claims.

### Historical original-task finalization — 2026-09-05, 20:14 UTC

At the user's explicit finalization request, `powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-native.test.ps1` was recorded as a standalone pass (reported exit 0; execution `d2153cd6-3830-4444-8d66-0fa4b630f5e3`). This is harness verification, not new native consent/build evidence. The evidence links above cover manual approval (Y), manual decline (D), successful and warm Cargo runs (V), and cancellation (C), all bound to candidate SHA-256 `F7374DE82907FD6C4C42B049D5B5797228966E1A52CDBC740CC7B5138B5E3AD8`. Finalization closes only the original budgets-disabled verification task, tracked as `Fix /ship: complete native release drill` (`e3869765`); its title is not a claim that the broader release drill is complete. The four unchecked broader requirements above remain open. No new drill, production change, fixture/profile deletion, or release-ready claim is authorized by this finalization.

### Historical attempts — superseded status, retained observations

The following attempts record the state at their own timestamps. Their pending instructions are historical, not current requests to repeat completed observations. The later reconciliation above credits manual approval, decline, and cancellation; it does not close the four current release gaps.

#### Explicit-Yes-only continuation — 2026-09-05, 19:36–19:37 UTC

**ORIGINAL BUDGETS-DISABLED TASK NOT COMPLETE — explicit approval remains the exact gap.** The previously recorded decline, real Cargo execution, warm-build preservation, and cancellation remain credited; none was repeated. Broader enforcement/quarantine, artifact identity preservation during enforcement, build-generation undo, and selected-preview/protected-floor screenshots remain separately open. No checkbox changed.

- Evidence: `.gg/artifacts/cargo-explicit-yes-20260905T1935/` (local ignored evidence). `candidate.json` and `preservation-verification.json` bind running PID `26576`, its start time, executable path, and SHA-256 `F7374DE82907FD6C4C42B049D5B5797228966E1A52CDBC740CC7B5138B5E3AD8`. Storage and the existing disposable fixture were snapshotted before UI input; prior evidence was preserved.
- UI execution lasted about 81 seconds, from `19:36:30.433Z` to `19:37:50.874Z`, within the five-minute limit. `outcome.json` records failed foreground acquisition; `focus-retry/outcome.json` records the bounded alternative UI Automation focus failure: the target cannot receive focus. Both stopped before form changes or submission. Intended unique profile: `Cargo explicit Yes 20260905T193630Z`.
- Attribution: **zero registration submissions, zero Yes actions**. `before.png`, `blocked.png`, `focus-retry/blocked.png`, corresponding UI dumps, and both failure records capture the actual state. No new native dialog was opened; no newly displayed native contents, Yes screenshot, or correlated approved-profile persistence is claimed. `expected-inputs.json` records canonical Cargo/hash, separate `build` / `--offline` arguments, the authorized disposable working directory, and the planned artifact path; it is input preparation, not displayed-dialog evidence. The prepared dialog guard uses candidate PID, native class, exact title/content and Yes ID6, without requiring HWND ownership; that guard was not reached.
- `preservation-verification.json` confirms all 69 fixture file hashes/sizes/mtimes and all three storage-file hashes/sizes unchanged, registry bytes unchanged, the unique name absent from storage, automatic budgets disabled, and candidate identity still matching. The initial comparison mixed deserialized objects with hashtables; the final verifier compares both saved snapshots as deserialized objects, corroborated by an independent Node comparison. No build, direct IPC, production-code change, real-project operation, or app-process termination occurred.
- Next required observation: establish usable candidate-window focus, populate the unique profile, verify the real PID-scoped dialog's exact executable/arguments/working/artifact contents, explicitly activate Yes, then confirm that exact profile in Saved profiles and persisted storage. Until this is observed, do not mark the original task done. Simulated smoke remains separate from native evidence.

#### Verification attempt — 2026-09-05

Ship decision remains **VERIFY-BEFORE-SHIP**. The current uncommitted worktree was approved as the verification candidate; none of the native drill checkboxes above is satisfied by this attempt.

- Command: `powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-native.test.ps1`
- Recorded result: exit code 0; the preserved stdout confirms native smoke launch guards, process cleanup, WebView interaction, and evidence capture verification. The stdout log does not itself record the exit code.
- Local command evidence: `<user-home>/.gg/foreground/eb463032-5bf8-4cc7-9ca3-0deec3f34903.log` (2026-09-05T16:18:43.494Z; local-only, not a portable release attachment).
- This command tests the smoke harness with synthetic fixtures; it does not establish packaged Cargo approval, direct executable/argv correctness, warm-build preservation, declined-registration persistence, or cancelled-build stamp/journal/quarantine behavior.
- Pending: standard-user packaged-app observations using only a disposable copied Cargo project, with automatic budgets disabled. Record the candidate executable hash, every displayed executable/argument/working/artifact path, native approval and decline screenshots, before/after profile persistence, two-run binary/dependency/incremental identity evidence, and cancellation before/after stamp/journal/quarantine evidence. No such native screenshots or report were captured in this attempt.

#### Minimized Windows UI Automation attempt — 2026-09-05

**Blocked; VERIFY-BEFORE-SHIP remains in effect.** Desktop enumeration worked in interactive session 1 without elevation. However, after launching the fresh release executable with PowerShell `WindowStyle Minimized`, process-scoped Windows UI Automation exposed no real application controls across 40 refreshed samples over 20,041 ms (16:34:28.955Z–16:34:48.910Z). The main window appeared in 39 samples, always hidden with zero descendants. No enumeration errors were recorded. This is a bounded observation, not proof that visible-window automation would fail.

- Candidate: `src-tauri/target/release/supa-diska-klinah.exe`, freshly built with `pnpm tauri build --no-bundle` from HEAD `6d6426a0c6f3e2d28f63e6388f4f2d3608844226` plus the approved uncommitted changes. SHA-256: `F7374DE82907FD6C4C42B049D5B5797228966E1A52CDBC740CC7B5138B5E3AD8`. Build completed successfully; it warned that Node `22.20.0` differs from the requested `24.19.0`. No installer was built or verified by this attempt.
- Local evidence directory: `.gg/artifacts/cargo-uia-20260905/` (ignored, not a portable release attachment). `build-retry-utf8.txt` records the build; `report.json` records the initial probe; `readiness-retry-20260905T163428806Z.json` records the bounded retry; `probe.ps1` and `probe-readiness-retry.ps1` record the executed automation.
- Only the copied `disposable-rust/` fixture was prepared; its hashes matched the checked-in fixture and remained unchanged. No artifact-budget policy file existed, automatic budgets were not enabled, and persisted cleanup-file snapshots were unchanged after both launches. These startup observations do not prove declined registration or cancellation safety.
- No native approval/decline dialog, Cargo run, warm-build comparison, or run cancellation was exercised. No screenshots were captured because the main window remained hidden; unrelated desktop windows were not captured. No window restore, approval bypass, mocks, CDP, or direct IPC substitutes were used. Both probe-owned processes were stopped.
- After reviewing the report, reran `powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-native.test.ps1`: recorded exit code 0. Local log: `<user-home>/.gg/foreground/9c6ca8e4-f015-4d2d-95e0-2316ff116fe1.log`. This remains smoke-harness evidence only, not proof of the native drill.
- To resume, an operator must permit a visible-window attempt or provide an environment exposing the actual controls while minimized. All native drill checkboxes and the task remain open.

#### Visible Windows UI Automation attempt — 2026-09-05

**Partial verification; VERIFY-BEFORE-SHIP remains in effect.** The user permitted showing, restoring and focusing the application and native dialogs. Real UI Automation navigation controls became accessible after three samples / 1,345 ms. The candidate executable remained SHA-256 `F7374DE82907FD6C4C42B049D5B5797228966E1A52CDBC740CC7B5138B5E3AD8` from the approved dirty worktree. No production code was changed for this drill.

Evidence below is relative to `.gg/artifacts/cargo-visible-20260905T165001933Z/`, except the initial visible-readiness report and screenshot in `.gg/artifacts/cargo-visible-20260905T164220588Z/`. These are local ignored files, not portable release attachments. Initial `state-pre-approval.json` and `state-blocked-final.json` captured cache metadata and must not be shared unrestricted; later snapshots record hashes and sizes instead.

**Inputs and real native prompt**

The ordinary DOS executable path was rejected before approval. Resolving the existing trusted Cargo executable through `CreateFile` / `GetFinalPathNameByHandle` supplied its required canonical Windows spelling; `canonical-cargo.json` records equal hashes for the ordinary and canonical paths. This corrected the submitted input without changing validation or bypassing approval.

The native prompt was captured in `recorded-approval-dialog.json` and `recorded-approval.png`, displaying:

- Executable: `<canonical-user-home>/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin/cargo.exe`
- Ordered arguments: `build`, `--offline` (separate rows).
- Working directory: `<canonical-repo-root>/.gg/artifacts/cargo-visible-20260905T165001933Z/disposable-rust`
- Artifact paths: `target/debug/artifact-budget-fixture.exe`, `target/debug/deps`, `target/debug/incremental`. The actual UI assigned Generation, Dependency and Incremental respectively.

**Verified runs and warm state**

- Actual Run controls were invoked twice. `cargo-first-status.json` records running followed by `succeeded, exit code 0` at 17:02:41.852Z; `cargo-second-status.json` records the same fixed success status at 17:03:14.454Z. Screenshots: `cargo-first.png`, `cargo-second.png`. Child build output was not captured.
- `ledger-after-second.json` records the second success stamp at 17:03:13Z, later than the first build's generation touch at 17:02:40Z. `first-identities.json` and `second-identities.json` contain 18 identical binary/dependency/incremental entries: paths, file identities, sizes, modification times and file hashes. `warm-comparison.json` records no differences; parent verification independently compared both snapshots.
- Automatic budgets stayed disabled; no budget policy was created. This verifies unchanged warm artifacts with budgets disabled, not survival during enforcement. Enforcement, quarantine, undo and the complete screenshot set remain unchecked.

**Native decision blocker and remaining gaps**

Native decision controls exposed no invocation patterns. UI Automation `SetFocus` failed with “El elemento de destino no puede recibir el foco.” (`recorded-yes-error.txt`). A guarded keyboard helper then failed foreground verification with “Native approval did not become foreground” (`recorded-decline-keyboard-error.txt`), before sending a decision key.

The captured approval dialog disappeared and a profile persisted without a successful automated decision action being recorded. The intended decline dialog was also captured (`decline-dialog.json`, `decline.png`), but the intended `Canonical Cargo DECLINE drill` profile nevertheless appears in `profiles-final.json`. These transitions cannot be attributed to a verified automated approval or decline; external interaction cannot be ruled out. They are not evidence that declining persists a profile. Both native decision checklist items remain unchecked.

Long-build cancellation was not attempted after this dialog-access blocker. No cancellation/no-new-stamp/journal/quarantine claim is made. No mocks, CDP, direct IPC substitutes or approval bypasses were used. The owned app process was stopped. Two disposable roots, two disposable profiles, successful ledger state and disposable build outputs remain; `outcome-final.json`, `profiles-final.json` and `ledger-final.json` record this residual state.

After reviewing native screenshots and reports, reran `powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-native.test.ps1`: recorded exit code 0 at 17:09Z. Local log: `<user-home>/.gg/foreground/caafe99e-33b0-45a5-8efa-0748826e4929.log`. The harness pass does not fill the native decision or cancellation gaps. Completion requires attributable approval/decline actions in a desktop session where the native decision controls can be operated, then a real long-build cancellation drill. The task remains open.

#### Single declined-registration retry — 2026-09-05, 17:23–17:26 UTC

**Blocked before submission; decline checklist item remains open.** Prior evidence and both existing profiles were preserved. Intended unique name: `Canonical Cargo UIA DECLINE retry 20260905T172334367Z`. Automatic budgets remained disabled.

- Evidence directory: `.gg/artifacts/cargo-decline-retry-20260905T172334367Z/` (local ignored evidence). `profiles-before-launch.json` captured the registry before any submission.
- Exact blocker: the intended disposable fixture's Project root option exposed `InvokePattern` but reported `IsEnabled=false`. The single invocation threw `ElementNotEnabledException` (`root-options.json`, `prepare-error.txt`, `prepare.ps1:5-7`). No fallback clicks or keyboard input were used.
- Zero registration submissions occurred. No native approval dialog opened, so No/ID7 could not be inspected or invoked. The main-window screenshot `pending-ui.png` and `pending-windows.json` record the pre-submission state; no native decision screenshot or successful decline claim exists.
- `profiles-before-launch.json`, `profiles-pending-before-stop.json`, `profiles-after-stop.json` and the live registry all matched SHA-256 `52D387E54D802E1BCA8D8BD9E6EBA735FE235E50E87BEC2712821F8F8A69B27F`. Both prior profiles remained byte-identical; the new name appeared only in the unsaved form and was absent from registered-profile controls and persisted records. Absence without submission does not verify rejection.
- Pending evidence was captured at 17:26:59.153Z, before stopping only the owned app process; final evidence completed at 17:26:59.408Z. Stopping the process was not a decline. `outcome.json` and `verification.json` record the result. No builds, smoke tests, policy changes or production edits were performed for this retry. The release drill remains open.

#### Popup-scoped selection attempt — 2026-09-05, 17:52–17:55 UTC

**Stopped on ambiguous popup targeting; decline remains unverified.** No option action, registration submission or native No invocation occurred. Existing profiles, policy and prior evidence were preserved.

- Evidence: `.gg/artifacts/cargo-popup-navigation-20260905T175233063Z/` (local ignored files). Earlier preparation snapshots remain in `cargo-popup-scoped-20260905T174740301Z/` and `cargo-popup-continuation-20260905T174843890Z/`; neither earlier preparation reached popup expansion or registration.
- Used the verified ComboBox's supported `ScrollItemPattern.ScrollIntoView`, rechecked its identity, then expanded once. `combo-expanded.json` records enabled, onscreen, Expanded state; `popup.png` shows the actual dropdown, with its right edge clipped by the main-window capture.
- `matching-options.json` records the exact authorized second disposable fixture as an enabled ListItem with Invoke/SelectionItem support beneath a List and the Project root ComboBox. However, its reported rectangle `(868, 588, 286, 11)` overlapped the ComboBox rectangle `(868, 588, 286, 45)`, rather than establishing association with the visible dropdown. No action was taken on this ambiguous representation.
- At fresh qualification, `combo-current.json` already reported Collapsed and still showed the first fixture. The popup was therefore unavailable for reliable selection. Capture and qualification occurred in separate PowerShell invocations, leaving an avoidable observation gap; the cause of collapse is unknown. This does not prove an application/provider defect. No second expansion or stale-option action was attempted.
- `outcome.json` records zero submissions/No invocations and unchanged registry snapshots (SHA-256 `52D387E54D802E1BCA8D8BD9E6EBA735FE235E50E87BEC2712821F8F8A69B27F`). Automatic budgets remained disabled. Pending evidence was captured before stopping only this attempt's launched app; stopping was not a decline. No native approval dialog opened, so no decline-dialog screenshot or rejection proof exists. No additional drill steps or production changes occurred. The checklist item and release drill remain open.

## Historical alpha decision — August candidate only

These checked gates belong to the August execution record below. They do not establish readiness of the September build-artifact candidate or current HEAD.

- [x] Automated gates pass for the recorded candidate.
- [x] One successful local restore-point run is recorded.
- [x] Local UAC cancellation evidence is recorded below.
- [x] Every observation refers to the recorded x64 candidate.
- [x] The Windows 10 x64 alpha gate passes.


## Future-release checks — non-blocking for alpha

- Windows 11 x64 compatibility.
- Native Windows ARM64 build and full restore-point exercise.
- Signed installer, app, and helper verification.
- Installation and testing on a disposable machine.
- Disabled-System-Restore failure behavior.

## Historical execution record — 2026-08-27

Status: **PASSED — successful local restore-point and UAC-cancellation runs are recorded.**

Candidate source commit: `58fed9f878add193af02292cbe13d18a9984b0ab` with documented uncommitted prerequisite changes.

### x64 observations available on this host

Command ID: `ba0cc4dd-3f51-44ad-83f1-e24853f1963a`

Command: `pnpm tauri build --debug --no-bundle --target x86_64-pc-windows-msvc`

Observed result: exited `0`; produced the identifiable loose x64 alpha application and helper. Node `v22.20.0` emitted an engine warning because the project requests `24.19.0`.

- App SHA-256: `65AD1692B444A21DF05BC132C0792B54E44EB2BB4C924BFD694EEA257F2FF569`
- Helper SHA-256: `7AED1B056C26AC6ED9417E77020EA37AB2C7F05C641D4DFE8D3E19C5887B7DE2`

Command ID: `c90d1991-5d97-4d7e-ac87-78bff924788f`

Command: `powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-native.ps1 -Target x86_64-pc-windows-msvc`

Observed result: exited `0`; the x64 main executable stayed alive during the smoke window at medium integrity (`S-1-16-8192`) with its helper present. The smoke did not launch the helper, display UAC, or invoke System Restore.

Command ID: `30620be7-aded-4f3d-b4ad-b69b43a309db`

Command: `Get-CimInstance Win32_OperatingSystem; Get-CimInstance Win32_ComputerSystem`

Observed result: this host is Microsoft Windows 10 Pro build `19045`, x64, matching the alpha target.

Command ID: `a3427437-67fa-461e-bc3f-61d9be99b446`

Command: `Get-ComputerRestorePoint`

Observed result: access was denied from the standard-integrity session. No restore-point metadata was observed.

Command ID: `031a379f-90ab-47ba-a97c-c7f80f836e67`

Command: inspect UAC policy, System Restore registry configuration, and VSS services.

Observed result: UAC and secure-desktop prompts were enabled; no policy disabled System Restore; `LastIndex` was `137`; VSS services were stopped with manual start.

### x64 UAC cancellation — observed

Command ID: `d52fb053-e013-4183-a7a9-9782cf616d50`

Command: launch the x64 application with `SUPA_DISKA_KLINAH_SMOKE_MINIMIZED=1` and wait for process `11600`.

Observed result: the minimized application started as process `11600`.

Command ID: `58d4003b-949f-462f-b110-63606228eedc`

Command: submit the restore-point confirmation through the minimized WebView and poll `Get-Process consent`.

Observed result: exactly one consent process appeared, process `14372`.

Command ID: `6c5243b0-4ee8-4a26-88e5-6f93b7906eef`

Command: send Escape to cancel the consent prompt, then poll `Get-Process consent`.

Observed result: the consent process exited after cancellation.

Command ID: `2af01392-fd3b-4d84-afdb-cd09dd4f148b`

Command: inspect the minimized WebView text, application process count, process `11600` token, helper count, and consent count.

Observed result: the app displayed “Administrator approval was cancelled”; process `11600` remained alive and unelevated; exactly one app process existed; no helper or consent process remained.

Command ID: `4e195a3b-e7ac-4644-a608-5452ca510344`

Command: read the System Restore `LastIndex` registry value and application process IDs after cancellation.

Observed result: `LastIndex` remained `137`, matching the pre-test value; process `11600` remained the only app process.

### Successful x64 restore-point run — observed

The minimized candidate received one approved UAC prompt and reported sequence `138`. System Restore `LastIndex` advanced from `137` to `138`; process `6272` remained unique and unelevated; the helper exited.

`Get-ComputerRestorePoint` remained unavailable to the standard-integrity inspection session. No additional elevation was requested.

Automated gate command ID `b988b32f-7978-4548-baa2-2eacc41daf9b` exited `0`: project checks, 11 frontend tests, Rust formatting, Clippy, 34 workspace tests, and the minimized x64 native smoke passed.

Corroboration command ID `a73776b5-8266-460a-9c4a-a8b590ded33f` read System Restore `LastIndex` and counted remaining app/helper processes from standard integrity. It exited `0`: `LastIndex=138` and `MatchingProcesses=0`.
