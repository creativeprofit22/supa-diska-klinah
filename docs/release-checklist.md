# Windows x64 alpha release checklist

This checklist gates the alpha on Windows 10 x64. Never invoke `SRSetRestorePointW` in CI; automated checks cannot replace the two local observations below.

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

## Alpha decision

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

## Current execution record — 2026-08-27

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
