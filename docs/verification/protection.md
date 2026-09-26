# Protection verification

Scope: local-first protection ([ADR 0003](../adr/0003-local-first-protection.md)). Automated evidence below was recorded on 2026-09-23 on the development machine with Node 24.19.0 (fnm) and pnpm 11.22.0.

**Status: Accepted by the owner on 2026-09-24.** All 14 checklist rows passed in the built app (two adapted, see below). Harness blockers 1 and 2 remain open; they are not in protection code.

## Automated evidence

| Check | Command | Execution ID | Result |
| --- | --- | --- | --- |
| Repository checks | `pnpm check` | `2f81df6b-e58e-4057-8e0c-723d56429c70` | Passed: dependency pins, parity, architecture (45 regression cases), security boundaries, docs, types, build |
| Rust lint | `cargo clippy --workspace --all-targets --locked -- -D warnings` | `313c97bf-a6f1-40ff-86af-11d1ac21e1b8` | Passed |
| Protection Rust tests | `cargo test --locked -p protection-core -p windows-platform -p supa-diska-klinah protection` and `cargo test --locked -p protection-core` | `469139a2-63c8-407f-84ec-02505a6b6326` | Passed: 65 platform, 3 command-layer, 22 core |
| Frontend and script tests | `pnpm test` | `11ae07a3-7ac2-48c4-931c-b9b818b21dc3` | All executed tests passed, including 8 protection UI tests. Exit 1 came from two worker **start-up** timeouts (see blockers) |
| Worker start-up re-run | `vitest run --maxWorkers=1` on the two affected files | `02729899-cb88-47f3-9e04-6b410e26a3ed` | Passed: 21 tests |
| Full Rust workspace | `cargo test --workspace --locked` | `56ace6fc-b617-4a19-8239-a29c1aa800d9` | 431 passed, 4 failed (see blockers) |

## Harness evidence blockers

These are recorded separately from runtime guidance, as `AGENTS.md` requires. None is in protection code.

1. **Build-artifact real-cargo tests time out on this machine.** `cleanup::build_artifacts::tests::real_*` (4 tests) run real `cargo` builds with a roughly 2-second completion window and time out under the machine's current load. This work did not change `src/cleanup/` (confirmed with `git diff --stat HEAD`). Re-run in execution `210588eb-1fc7-4cc6-aa05-021d13054bf4`: same timeouts. Needs a re-run on an idle machine or in CI.
2. **Vitest worker start-up timeouts.** Under load, `pnpm test` reported two workers that failed to start (`BuildArtifactCoordinator.test.tsx`, `FirewallPage.test.tsx`). Both files pass alone (execution `02729899-…`).
3. **Corrupted incremental build cache.** rustc crashed while reading the incremental cache (`on_disk_cache.rs:519`), and a debug-symbol link failed with LNK1207. Checks above were run with `CARGO_INCREMENTAL=0`. **Resolved 2026-09-24:** with the owner's approval, the `target/debug/incremental` folders for `windows_platform` and `supa_diska_klinah` were deleted (execution `ce998b8f-0efe-485e-906c-3dbdf37bfa61`). A normal incremental rebuild then compiled without the crash, and the protection tests passed: 65 windows-platform, 3 command-layer, 1 other (execution `0c471b78-c70f-4362-98d7-249eb6623841`). Blockers 1 and 2 remain open.

## Manual Windows acceptance (Accepted 2026-09-24)

Run on Windows 10 22H2 with Microsoft Defender active. Windows 11 was not tested.

- [x] Fresh install: Protection shows the built-in pack, sequence 1, and all three network toggles off.
- [x] With all toggles off, a network monitor (Resource Monitor or Wireshark) shows no connections from the app during scan, process list, quarantine and Defender history.
- [x] Quick scan completes; results group into signed-rule, heuristic and not-checked sections; the wording never certifies the device.
- [x] Folder scan: the Windows folder picker opens; cancelling it shows no error.
- [x] An EICAR test file created in a test folder (with Defender real-time protection paused for that folder) appears as a signed-rule match. *(Adapted: the intended Defender folder exclusion was never applied, so Defender kept blocking the file and a signed marker rule was used instead; see the table.)*
- [x] Quarantine asks in a Windows dialog; declining changes nothing; accepting removes the file.
- [x] Restore with a file already at the original path is refused with the collision message, and the existing file is unchanged.
- [x] Restore without a collision returns the exact original bytes.
- [x] The process list shows signers for signed apps and offers no way to end a process.
- [x] Release build: import and download are disabled while the test key is compiled in.
- [x] Password check with the toggle on: only a request to `api.pwnedpasswords.com/range/<5 hex>` is observed.
- [x] AMSI toggle on: a scan of a flagged file shows an external response or "not checked", never a clean verdict.
- [x] Defender history lists recorded detections, or shows "not checked" when Defender is not the active antivirus.
- [x] Keyboard only: every Protection page, toggle and action is reachable and has a visible focus indicator.

Acceptance: **Accepted by the owner on 2026-09-24**, on the evidence in the built-app run below. Accepted with these noted limits: the EICAR row was adapted to a signed marker rule, the keyboard row passed after a fix, and Windows 11 was not tested.

## Built-app acceptance run: 2026-09-24 UTC

Windows 10 Pro 22H2 (10.0.19045), Microsoft Defender active. Drivers: `scripts/acceptance-protection-builtapp.ps1` (drives the app through its own command bridge; no synthetic desktop input) and `scripts/acceptance-protection-netwatch.ps1` (records every non-loopback TCP endpoint of the app and its WebView2 child processes). Raw step results are in `.gg/smoke-artifacts/protection-acceptance/`. The checkboxes above stay unticked until the owner accepts.

| Checklist row | Result | Evidence |
| --- | --- | --- |
| Fresh install | **Pass** (debug build, no prior protection data) | Built-in pack, sequence 1, 2 rules; all three rendered toggles unchecked; quarantine 0 (execution `34833be6-2e64-42aa-857d-49cffd7635ea`) |
| No connections with toggles off | **Pass after fix** | Before the fix: app code made none (download and password check returned `networkDisabled`), but the WebView2 runtime's own network process opened one HTTPS connection to Microsoft (`150.171.28.11`, ARIN owner MSFT) shortly after launch, in both builds. The owner chose to block it: the main window now sets `--disable-background-networking --disable-component-update` (plus the default wry flags) in `additionalBrowserArgs`, enforced by `scripts/check-architecture.mjs`. After the fix, using the release build with no extra launch flags, the flags were confirmed on the engine process, and a 4-minute window from launch covering a quick scan, the process list, Defender history and every page produced **0** outside connections (execution `648812a9-944f-4b39-b774-40345b08d008`, monitor `e4c077b6-df49-4805-ac95-4bb7a1b3cbd6`) |
| No connections with toggles off: installed NSIS build | **Pass** (unsigned local build, idle launch) | The CI release config comes from `scripts/prepare-windows-signing.ps1` and holds only `bundle.windows.certificateThumbprint`. Tauri merges `--config` as a JSON merge patch, so `app.windows` and its `additionalBrowserArgs` are kept. A merge-patch file with the same shape but no thumbprint (`{ "bundle": { "windows": {} } }`, because there is no local signing certificate) built the per-machine installer with `pnpm tauri build --ci --target x86_64-pc-windows-msvc --bundles nsis --config <file>` (execution `f6ab3c5d-3415-4752-a44b-4a6734cb92e2`; installer SHA-256 `9E9A7EEE…BECFE`). It was installed silently to `C:\Program Files\Supa Diska Klinah` (execution `37935325-1dcd-4761-854e-2e0732b73ab4`). With all three toggles off in `settings.json`, the installed exe (SHA-256 `D9AEA5F9…6A1F`) was launched with no extra WebView2 environment arguments. The engine process command line had all three flags, and `acceptance-protection-netwatch.ps1 -RootProcessId` recorded **0** non-loopback endpoints over 251 s from launch (execution `2fbbebde-f329-4104-bd1f-4ba2a0913c0f`; output `installed-netwatch.jsonl`). The app was left idle in this run; scan, process-list and Defender pages were covered by the release-exe run above |
| Quick scan grouping and wording | **Pass** | Release: 29,311 files in 83 s; 141 heuristic, 59 not checked, 86 links skipped, not truncated (execution `53240b52-c175-43b3-8767-7bf54086e4ae`). All six rendered pages contain no certification wording and no unnamed controls (execution `6412c70a-0647-4cd0-974f-5bcc6e684e4d`). Screenshots at 1280 px and 420 px wide show no overlap. The debug build took 11.5 min for the same scan (unoptimized hashing) |
| Folder picker | **Pass** | The picker opened as a child of the app window, titled "Choose a folder to scan"; Cancel (control 2) closed it with no error and cleared "Scanning…" (execution `651b7a7a-dafe-44ec-8df2-3d223ca07de1`). Picking `E:\sdk-protection-acceptance` scanned it and reported `invoice.pdf.exe` as heuristic H002 (execution `593fced6-8b24-4616-927c-d8783bddf493`) |
| EICAR signed-rule match | **Pass, adapted** | With Defender on, a 68-byte EICAR file could be written but Defender blocked reading it and logged the detection. The app reported it as "Not checked" (`readFailed`), with no clean verdict and no quarantine button (execution `0391492c-2a41-4d1f-a7bd-28970f7bc0ae`). A Defender folder exclusion was requested, but Defender still blocked and logged the file (execution `53b0cc5f-bb65-438c-9807-719c38cd9520`). A later elevated check found that the exclusion had never been applied: no path, process or extension exclusion referenced the test folder or EICAR (executions `ba591866-b2e9-4410-9085-e70c83e8d249`, `b2c8b66c-1429-49c0-85bb-4baea390b37e`). The row was adapted: a pack with sequence 2 (the built-in EICAR rules plus a byte rule for the harmless text `SDK-ACCEPTANCE-MARKER`), signed with the test key, was imported through the real folder picker in the debug build ("Rule pack installed.", source installed, 3 rules; execution `60e26140-dbd4-4c57-8c44-cada956b0f54`). A folder scan then reported `marked.txt` as a **Signed-rule match** (rule `acceptance.marker`, pack 2, byte pattern), shown under that heading with a quarantine action (execution `c5fcdbbf-c8bc-427d-bfe3-e2d7f294a157`). The EICAR hash and byte rules are covered by the protection-core and rules-store tests. All EICAR test files were deleted. The acceptance pack stays installed in this machine's app data; it keeps all built-in rules |
| Quarantine confirmation | **Pass** | Windows dialog "Quarantine file" named the exact path, with No as the default button. No: file unchanged (SHA-256 `5de767fc…`), quarantine empty, no error (execution `7ea801d4-c3b9-478d-8cbb-36f20354e334`). Yes: the file was removed; the entry recorded path, hash and finding; the stored payload starts with the `SDKQRNT1` header and does not contain the original text (execution `0b405f44-8a46-49e4-89a7-5d413d90da52`) |
| Restore collision | **Pass** | A different file was placed at the original path. The Windows dialog "Restore file" defaulted to No; after Yes, the app showed "…nothing was restored or overwritten…", the other file's SHA-256 was unchanged (`6a36c5a4…`), and the entry was kept (execution `b3bb14fb-431b-4a15-b0cc-94dc83deeacd`) |
| Restore exact bytes | **Pass** | After moving the other file aside, Restore then Yes returned the file byte-for-byte (SHA-256 `5de767fc03658da235346add41202a4ed062f46504e8a2fe0b563a74bc115fcb` before and after) and emptied the quarantine (execution `1c924575-2ff1-455a-8bbf-96cd79edbccd`) |
| Process list | **Pass** | 291 processes; signers read offline (for example Microsoft Windows, NVIDIA Corporation); 128 other-user or protected processes reported as not inspectable; command lines reported unavailable; the page's only controls are Refresh and Filter (execution `04f34678-64d1-487f-9147-8c4e3811716f`) |
| Release build disables import and download | **Pass** | `externalPacksAllowed: false`; Import, Download and Go back buttons disabled; test-key notice shown (executions `78d1c527-99b1-4271-84cb-6ca45cf5d025`, `143814c2-41ba-4675-8ddb-e063d31af29f`) |
| Password check endpoint | **Pass** | With only that toggle on: the result was external evidence (52,372,427 occurrences of a known-weak test password); the only new connection was to `104.17.96.141`, which is `api.pwnedpasswords.com`; the toggle was switched back off (execution `f43bfb1d-9062-445c-9103-0f3ffee0d266`). The request path is encrypted and was not observed; the 5-character path is covered by a unit test |
| AMSI | **Pass** | Toggle on: 136 flagged files answered "did not report this content. This is not a guarantee."; no clean verdict; toggle switched back off (execution `e1823fcb-9fd0-4f2a-b07b-da9c677c5f8c`) |
| Defender history | **Pass** | 1 recorded detection read, labelled external (execution `04f34678-64d1-487f-9147-8c4e3811716f`) |
| Keyboard only | **Pass after fix** | Trusted key events (DevTools input) on all six pages: every enabled control is reachable with Tab, in page order and with no trap, and each shows the 3 px white `:focus-visible` outline. Enter on a section tab navigates. **Bug found and fixed:** each settings toggle disabled the whole group while saving, which dropped keyboard focus, so a second Space went nowhere. The group now stays enabled (`aria-busy`) and ignores changes during a save, with a regression test that fails on the old code. Rebuilt release: Space turned the toggle on and back off (execution `d55c0503-c6f9-4307-86dd-6cb13242881f`). The test left rule downloads on; it was switched back off (execution `7bf1b9e0-3cdc-4db6-935c-019bb71265a7`) |

Native dialogs were answered by `scripts/acceptance-protection-dialogs.ps1`. It sends `BM_CLICK` to standard dialog control IDs (1 and 6 = confirm, 2 = cancel, 7 = No), so it works in any Windows language; this machine runs in Spanish. It touches only dialogs owned by the app process.

Not covered: a Windows 11 machine.
