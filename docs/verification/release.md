# Release verification

Evidence for the unsigned release candidate. Manual items stay open until they are explicitly accepted; automated results do not stand in for them.

## Status

**Unsigned release candidate: automated gates and non-elevated acceptance passed; elevated install/uninstall and the installer UAC/SmartScreen observations were accepted by the user on 2026-09-26, as were the per-operation UAC counts and the parity drivers on the installed build; the drill items, Spanish review, published update and the signed run remain open.** No release has been published from this candidate.

- Candidate: HEAD `0a062f8` plus the uncommitted release-phase worktree (2026-09-26).
- Installer: `src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/Supa Diska Klinah_0.1.0_x64-setup.exe`, built with `pnpm tauri build --ci --target x86_64-pc-windows-msvc --bundles nsis` and the plain `tauri.conf.json` (no signing config). 3,572,954 bytes, SHA-256 `F2BDE7097E5E69547FFA10E75FDCBA0FCE10DEF43852BEE7012F4EEAA81928C7`.
- Signing mode: `unsigned`. `Get-AuthenticodeSignature` reports `NotSigned` for the installer, the app and the helper, as unsigned mode requires.
- Local evidence: `.gg/smoke-artifacts/release-acceptance/results.jsonl` (ignored; not a portable release attachment).

## Automated gates

Full gate on 2026-09-26 (Node 24.19.0, pnpm 11.22.0), HEAD `0a062f8` plus the release-phase worktree:

| Gate | Execution ID | Result |
| --- | --- | --- |
| `pnpm check` (ports, dependencies, parity, architecture, security, docs, i18n, a11y, perf, build) | `0b3c73f5-cea5-41cd-8d90-e6657f5a436a` | Pass |
| `pnpm test` | `7f8e6fa5-f844-45d0-a448-e0359efc3858` | Pass: 60 script tests, 464 vitest tests in 51 files |
| Rust fmt → helper → clippy → workspace tests | `7abf736e-60fa-44d6-ad1d-8423defe83a0` | 1 failure: a missing file was classified `Invalid` instead of `Unavailable` (both fail closed). Fixed by opening the file before `WinVerifyTrust` |
| Rust fmt check + workspace tests after the fix; update rehearsal rerun | `631faf22-4e5c-4163-970b-0727a1f05cd5` | Pass: 668 tests in 30 suites, 8 ignored (manual/rehearsal); rehearsal 10/10 |
| Clippy after the fix (`windows-platform`, all targets) | `b38d42f2-3faa-4506-a7bc-97377a051fd2` | Pass |
| Native smoke (`smoke-native-ci.ps1`) | `06008b0c-3c86-4544-afb0-10995820b9d9` | **Not run**: the script needs PowerShell 7 (`pwsh`), which is not installed on this machine (Windows PowerShell 5.1 lacks `RandomNumberGenerator.GetBytes`). CI runs it under `pwsh`. The release-build offline tour below covers a hidden, non-elevated launch of every route. |

Release-specific checks added in this phase:

- `scripts/workflow-rules.test.mjs`: the two-job release pipeline (least privilege, mode validated before build, signing steps behind the `authenticode` guard, verify → stage → attest → upload, publish re-verifies and pushes the manifest last), with one mutation test per rule.
- `scripts/release-assets.test.mjs`: publish-side re-verification rejects a wrong tag, a mode/docs mismatch, manifest signing vs. mode, a tampered installer or manifest, extra files, an unconfigured key, and expiry.
- `scripts/sign-update-manifest.test.mjs` and `protection-core/tests/update_manifest.rs`, including a manifest signed by the Node release script and accepted by the Rust verifier.
- `scripts/release-doc-rules.test.mjs`: the documentation gate (`pnpm check:docs`).

## Release candidate acceptance

`scripts/acceptance-release.ps1`, signing mode `unsigned`.

| Phase | Execution ID | Result |
| --- | --- | --- |
| Update rehearsal (real `WinVerifyTrust`, real PE files) | `8a6b02b6-ef7c-434b-886d-4313d0b8de32` | Pass, 10/10 cases |
| Offline tour of the release build | `ffa2c68f-9c80-4715-9ff8-1522a159a3b7` (command `112c622f-0fff-4660-9769-c738b8e86f29`) | Pass, 4/4 checks |
| Scheduled-task uninstall verb (standard + elevated semantics) | commands recorded below | Pass |
| Elevated silent per-machine install of the rebuilt candidate (HEAD `4db358a`) | `54bb2244-51e3-4fae-865e-547e55df2434` | Pass: exit code 0, installed under Program Files. The same run's offline tour of the installed app recorded no results, because the app refuses to start elevated (`require_standard_user`) and the tour launched it from the elevated shell |
| Offline tour of the installed app, run from a standard (non-elevated) shell (`-Phase Offline -Executable "C:\Program Files\Supa Diska Klinah\supa-diska-klinah.exe"`) | `d43228de-2dcc-438b-85bd-0aabdfb3c5ee` (command `e0fc4591-e2e3-4f97-b3df-487b38e0b48d`) | Pass, 4/4 checks: update check off, medium integrity (`0x2000`), 28/28 routes render a heading, zero non-loopback connections from a 7-process tree |
| Silent uninstall cleanup, elevated (`-Phase Uninstall`) | `d267e3c0-41ae-4b8d-bcd0-e3cfd3930cae` | Pass, 6/6 checks: uninstaller exit 0, install directory removed, HKLM uninstall key removed, `\SupaDiskaKlinah` task folder removed (disposable task registered first), no helper process, user data in `%APPDATA%\com.supadiskaklinah.app` kept |

Rebuilt candidate installer (2026-09-26, after the update public key was committed): `pnpm tauri build --ci --target x86_64-pc-windows-msvc --bundles nsis` at HEAD `4db358a`, command `dcedffe3-f38e-48fb-a365-cb2516fa34ca`. 3,575,207 bytes, SHA-256 `6B3E47D4C00A7F1F9B4B09E7722BC63729ED8F6E805574F16ABAE62D7AE373B5`, `NotSigned`. A previous 0.1.0 install (2026-09-24) was silently uninstalled first, so the install check could not pass on stale files.

Interactive install observed by the user (2026-09-26, same rebuilt candidate, double-clicked from File Explorer): exactly one UAC prompt, showing "Publisher: Unknown". The user chose `E:\Supa Diska Klinah` instead of the default Program Files location. The HKLM uninstall key was written and the running app's path is under that folder. SmartScreen observed by the user on a downloaded copy: a byte-identical copy (SHA-256 `6B3E47D4…73B5`) was placed in Downloads with a `Zone.Identifier` stream (`ZoneId=3`, internet zone), and running it showed the Microsoft Defender SmartScreen block ("Windows protegió su PC", unknown app; Spanish-language Windows). Ticked delete-app-data uninstall, done by the user from Settings → Apps: afterwards `%APPDATA%\com.supadiskaklinah.app` and `%LOCALAPPDATA%\com.supadiskaklinah.app` both report `False` (user's shell and re-checked, command `e51810e7-6bf3-4d09-a443-7453085d3139`). `E:\Supa Diska Klinah` and the HKLM uninstall key are gone, and no app process is running.

Step 3 reinstall (2026-09-26): the same candidate installer was run silently per-machine from the standard shell with `Start-Process -Verb RunAs -Wait` (command `aa46c1a4-e499-439b-90e3-b1e73f0f3d0b`): exit 0, installed under `C:\Program Files\Supa Diska Klinah` (app, helper and uninstaller beside each other), HKLM uninstall key written, one UAC prompt. Installed app SHA-256 `DFBE36193FEBCE536CA067D8890C2175275B90304AB22E1978607A3D958D8A6E`.

Storage parity driver on the installed build (`acceptance-storage-builtapp.ps1 -Executable "C:\Program Files\Supa Diska Klinah\supa-diska-klinah.exe"`, standard shell with an elevation guard, run `b1e2fe55-84b4-400f-9e29-c26ee88f9df5`, 2026-09-26 08:53–09:26 UTC, artifacts `.gg/smoke-artifacts/release-step3-storage/acceptance.json`): outcome `completed`, zero step failures, fixture removed afterwards. The user drove every native picker (cancel returned `null`, then the fixture root for each module) and the permanent-deletion confirmation. The user observed **No** as the default focused button and cancelled; the fixture was unchanged. Then Yes purged exactly the 1 MiB fixture file, and undo was refused. Also recorded: analyzer, large-file paging, duplicates, empty-folder late-child refusal (`invalid_evidence`), opaque cleaner/browser scopes refusing the picker, quarantine recovery and undo (both items restored), 3 analyzer resource cycles (7-process tree, private bytes 177.7 → 179.2 MB, released snapshots `snapshot_unavailable`), and restart without automatic deletion. `undoReplayRefusal` and `staleSelection` report `refused: false`; the replay returns the same completed execution idempotently. This is the same shape as the 2026-09-23 accepted storage run. UAC prompts expected from reading the driver: 0, because it makes no helper calls. None was reported by the user during the run.

System driver on the installed build, short scope chosen by the user (`acceptance-system-builtapp.ps1 -Launch -Executable "C:\Program Files\Supa Diska Klinah\supa-diska-klinah.exe"`, standard shell, launch command `d1aa5b88-e15f-4714-8e6d-1afd7925c6ad`, app pid 18080; artifacts `.gg/smoke-artifacts/release-step3-system/steps.jsonl`). The Fax service was independently checked with `Get-Service` after each step. The full 12-row matrix was accepted earlier against a debug build (see `storage-parity.md`) and was not rerun.

| Step | Run | Result | User-counted UAC prompts (expected) |
| --- | --- | --- | --- |
| Apply Fax Disabled → Manual, app confirmation Yes, UAC Yes | `bc0c8b53-07ed-4959-b9b2-abb8c32d9792` | Pass: plan `0e483205…` → entry `fba4ad43…` `applied`; Windows reports Manual | 1 (1) |
| Undo through the app's rollback, both Yes | `3ad9ab7f-ba9f-4a28-b290-66266216563d` | Pass: plan `528800fb…` → entry `f04c3269…` `applied`; Windows reports Disabled | 1 (1) |
| Apply again, app confirmation No | `46fd708d-4bab-4e0c-9c8c-fd18676141de` | Pass: `confirmationDeclined` before any helper launch; Fax still Disabled | 0 (0) |
| Apply again, app confirmation Yes, UAC No | `571d2bd2-b09a-4f3a-9f39-7d557ea55bbc` | Pass: plan `dc7ad1b3…` → entry `7549fad5…` `denied`; journal 2 → 3; Fax still Disabled | 1 (1) |

Final Fax state is Disabled, matching the pre-test state (command `debe8231-e73b-430c-aa19-71021ccb9a54`).

Protection driver on the installed build (`acceptance-protection-builtapp.ps1 -Launch -Executable "C:\Program Files\Supa Diska Klinah\supa-diska-klinah.exe"`, standard shell). Launch command `61cc34b2-5c5c-4bbb-a2b3-fd7413a628f9`, pid 24164, artifacts `.gg/smoke-artifacts/release-step3-protection/steps.jsonl`. After the user restarted the PC, the app was relaunched in command `6e09b274-740a-4609-bb4f-78164314c73a`, pid 9252, artifacts `.gg/smoke-artifacts/release-step3-protection-after-restart/steps.jsonl`. UAC prompts expected from reading the driver and the protection code: 0, because protection makes no helper calls. None was reported by the user.

| Check | Result |
| --- | --- |
| Fresh state | Pass: embedded baseline pack, sequence 1, 2 rules, `externalPacksAllowed: false`; all three network/AMSI toggles off; quarantine 0 |
| Offline quick scan, process list, Defender history | Pass: 42,977 files, 158 heuristic, 48 not checked, 102 links skipped, not truncated. 323 processes with offline signers (131 not inspectable). 4 Defender detections read. Download and password check refused with `networkDisabled`. The scan took 375 s, versus 83 s on 2026-09-24, with the machine under load. A first attempt timed out in the harness while the app kept scanning; the retry got `busy` (the scan was still running), then passed |
| Release lockouts | Pass: the rules page's Import, Download and Go back buttons are disabled, and the Spanish test-key notice is shown. The original driver's English text match reported `testKeyNotice: false` because the UI is in Spanish; it was re-checked by control state (`05b-release-ui`) |
| Folder scan | Pass: the user picked `E:\sdk-protection-acceptance`; 5 files; `invoice.pdf.exe` flagged heuristic H002, quarantine offered |
| Quarantine confirmation | Pass: the user observed **No** as the default button. After No, the file was unchanged (SHA-256 `5DE767FC…`) and the quarantine stayed empty. After Yes, the file was removed |
| Quarantine across reboot | Pass: after the user's PC restart, the entry `0a4d3fce…` was still listed with the same SHA-256, 62 bytes, `damaged: false` |
| Restore exact bytes | Pass: after the user clicked Yes, the app showed "Restaurado en E:\sdk-protection-acceptance\invoice.pdf.exe", the SHA-256 matched the original (`5DE767FC03658DA235346ADD41202A4ED062F46504E8A2FE0B563A74BC115FCB`), and the quarantine was empty (command `7b35ce3b-bb6f-4470-837c-0b34e121e6fb`) |

Not rerun on the installed build (already accepted 2026-09-24 in `protection.md`): network capture, password-check endpoint, AMSI, restore collision and keyboard pass.

Harness gap: `-Phase Install` launches its offline tour from the elevated shell, which the app rejects. Until the script drops elevation for the tour, run `-Phase Offline` against the installed executable from a standard shell.

Update rehearsal (`cargo test -p windows-platform --lib real_signature_rehearsal -- --ignored`, candidate installer as the unsigned file, Microsoft Edge as a real third-party signed file):

- unsigned → unsigned accepted (`Verified`)
- real broken signature rejected (`SignatureRejected`)
- `authenticode` manifest, wrong signer rejected (`SignatureRejected`)
- `authenticode` manifest, matching real signer accepted (`Verified`)
- signed app → unsigned update downgrade rejected (`SignatureRejected`)
- tampered installer rejected and deleted (`IntegrityFailed`)
- rollback / same version not offered (`UpToDate`)
- tampered manifest rejected (`BadManifest`)
- interrupted download leaves nothing staged; recovery `Clean`
- declined confirmation launches nothing; a started-but-unfinished install is reported `Interrupted` at the next start, and the re-verified installer is kept

The wrong-signer case uses a real third-party signature instead of a throwaway self-signed certificate. That exercises the same thumbprint mismatch without adding a trusted root to the machine.

Offline tour (release `supa-diska-klinah.exe`, hidden window, fresh settings):

- update check off by default (`updateCheck: false`)
- main process at medium integrity (RID `0x2000`)
- all 28 routes render a heading
- zero non-loopback TCP connections from the app's 7-process tree during the tour

Scheduled-task uninstall verb (`privileged-helper --remove-scheduled-tasks`, debug build):

- with an extra argument: exit code 2, nothing touched
- a disposable `scan-…` task in `\SupaDiskaKlinah`: task and folder removed, exit 0
- a foreign task beside an app task: only the app task removed, the foreign task and folder kept
- test fixtures removed afterwards

## Manual items

Open until explicitly accepted:

- [x] Elevated install of the candidate (accepted by the user 2026-09-26; evidence above): per-machine install under Program Files, one UAC prompt, "Publisher: Unknown" shown, SmartScreen warning observed on a downloaded copy (`scripts/acceptance-release.ps1 -Phase Install -InstallerPath <installer>` from an elevated shell).
- [x] Uninstall cleanup (accepted by the user 2026-09-26; execution `d267e3c0-41ae-4b8d-bcd0-e3cfd3930cae`): no install directory, no HKLM uninstall key, no `\SupaDiskaKlinah` folder, no helper process, user data kept on the default (silent) uninstall (`-Phase Uninstall`).
- [x] (Accepted by the user 2026-09-26; command `e51810e7-6bf3-4d09-a443-7453085d3139`.) Uninstall with the delete-app-data checkbox ticked removes `%APPDATA%\com.supadiskaklinah.app` and `%LOCALAPPDATA%\com.supadiskaklinah.app`. The ticked case can't be driven silently.
- [x] (Accepted by the user 2026-09-26.) Exactly one UAC prompt per helper operation during the system acceptance drivers. Installed build: user counted 1, 1, 0 (declined before the helper) and 1 (UAC declined), matching expectations; storage and protection runs 0 as expected.
- [x] (Accepted by the user 2026-09-26; system run was the user-chosen short scope.) Parity drivers (`acceptance-storage-builtapp.ps1`, `acceptance-system-builtapp.ps1`, `acceptance-protection-builtapp.ps1`) on the installed build. Storage run `b1e2fe55-84b4-400f-9e29-c26ee88f9df5` completed and system short scope passed 4/4; protection passed 7/7 including a reboot between quarantine and restore.
- [ ] The four open build-artifact drill items in the [release checklist](../release-checklist.md): budget enforcement/quarantine, protected identities, build-generation undo, screenshot set.
- [ ] Native Latin American Spanish review of every `es419` catalog, `windows-platform/src/i18n.rs`, and the installer strings.
- [ ] End-to-end published update (tag → release → in-app update) once `UPDATE_SIGNING_KEY` and `WINDOWS_SIGNING_MODE` are configured.

## Signed release

**Pending certificate.** No Windows code-signing certificate exists. The Authenticode path (`prepare-windows-signing.ps1`, `verify-windows-release.ps1 -SigningMode authenticode`, the signed-manifest policy) is built and unit-tested but has not produced a signed installer. Repeat this acceptance in `authenticode` mode after the [code-signing checklist](../release.md#turning-on-code-signing) is complete.
