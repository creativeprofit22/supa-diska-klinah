# Release verification

Evidence for the unsigned release candidate. Manual items stay open until they are explicitly accepted; automated results do not stand in for them.

## Status

**Unsigned release candidate: automated gates and non-elevated acceptance passed; elevated install/uninstall and the installer UAC/SmartScreen observations were accepted by the user on 2026-09-26; the per-operation UAC counts, parity drivers, drill items, Spanish review, published update and the signed run remain open.** No release has been published from this candidate.

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
- [ ] Exactly one UAC prompt per helper operation during the system acceptance drivers.
- [ ] Parity drivers (`acceptance-storage-builtapp.ps1`, `acceptance-system-builtapp.ps1`, `acceptance-protection-builtapp.ps1`) on the installed build.
- [ ] The four open build-artifact drill items in the [release checklist](../release-checklist.md): budget enforcement/quarantine, protected identities, build-generation undo, screenshot set.
- [ ] Native Latin American Spanish review of every `es419` catalog, `windows-platform/src/i18n.rs`, and the installer strings.
- [ ] End-to-end published update (tag → release → in-app update) once `UPDATE_SIGNING_KEY` and `WINDOWS_SIGNING_MODE` are configured.

## Signed release

**Pending certificate.** No Windows code-signing certificate exists. The Authenticode path (`prepare-windows-signing.ps1`, `verify-windows-release.ps1 -SigningMode authenticode`, the signed-manifest policy) is built and unit-tested but has not produced a signed installer. Repeat this acceptance in `authenticode` mode after the [code-signing checklist](../release.md#turning-on-code-signing) is complete.
