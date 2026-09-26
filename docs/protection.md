# Protection

The Protection section checks files and running programs on this PC using signed rules and local heuristics. It works offline. It **complements** your antivirus and does not replace it. Design decisions are recorded in [ADR 0003](adr/0003-local-first-protection.md).

## What a result means

Every finding carries one evidence type:

| Evidence | Meaning | Confidence |
| --- | --- | --- |
| Signed-rule match | The file's SHA-256 or a byte pattern matched a rule in an Ed25519-signed pack. | Deterministic for that rule. The rule itself can still be wrong. |
| Heuristic | A local pattern that is often suspicious, such as a double extension. | Can be a false positive. Each heuristic lists its known false positives. |
| Not checked | The item could not be examined (in use, access denied, too large, a link). | Unknown. This is **not** a clean result. |
| External response | An answer from the installed antivirus (AMSI), Defender history, or Pwned Passwords. | As reliable as that provider. |

No result certifies a file or device. "No signed-rule matches" means only that the current rules did not match.

## Detection limits

- The built-in pack detects only the [EICAR test file](https://www.eicar.org/download-anti-malware-testfile/). Real detection depends on packs you import.
- YARA rules are not supported. The rule format supports SHA-256 hashes, byte patterns in the first MiB, and exact file names.
- Signatures are checked offline. Revocation is not checked, and catalog-signed Windows files appear as having no embedded signature.
- Scans skip links and junctions, stop at size and file-count limits, and report partial results honestly.
- Process command lines are not read.

## Heuristics and false positives

| ID | Flags | Known false positives |
| --- | --- | --- |
| `H001-system-name-outside-windows` | A Windows system binary name outside the Windows folder | Installers, backups and virtual machines. A valid Microsoft signature lowers the severity. |
| `H002-double-extension` | Names like `invoice.pdf.exe` | Some tools name generated files this way. |
| `H003-bidi-control-in-name` | Direction-control characters that reverse how a name displays | Right-to-left language names. |
| `H004-executable-content-wrong-extension` | Executable (MZ) content with a non-executable extension | Plugins or resources stored as PE files. |
| `H005-unsigned-executable-risky-location` | Unsigned executables in startup, Temp, Downloads or roaming AppData | Many open-source and portable tools are unsigned. |
| `H006-process-image-deleted-or-user-writable` | A running program whose file was deleted or lives in a user-writable folder | Updaters, and per-user apps such as browsers and chat clients. |

**Hide for this file** stores the file's SHA-256 so its heuristic findings stop appearing. Signed-rule matches are never hidden. **Show all again** on the Rules page clears the list.

## Data flows per setting

All network settings are off by default. The webview cannot reach the network; the only network code is one WinHTTP module that requires HTTPS, refuses redirects, and bounds time and size.

| Setting | Sends | To | When |
| --- | --- | --- | --- |
| Nothing enabled | Nothing | — | — |
| Allow downloading signed rule packs | A request for two fixed files | `raw.githubusercontent.com` | Only when you press **Download latest pack** |
| Allow the password breach check | The first 5 hex characters of the password's SHA-1, with response padding requested | `api.pwnedpasswords.com` | Only when you press **Check** |
| Ask the installed antivirus (AMSI) | Contents of files that already have findings (up to 16 MiB each) | Your installed antivirus | During scans. The antivirus may use its own cloud service, depending on its settings. |

The embedded Microsoft Edge engine (WebView2) is started with its background networking and component updates turned off, so it does not contact Microsoft on its own. Windows still updates the engine itself.

Reading Defender detection history uses local WMI only and does not start a Defender scan. E-mail breach monitoring, CVE feeds and program "safety ratings" are not offered because they would send identity or software inventories to a third party.

## Privacy

Scan results, file hashes and paths stay on this PC. Settings live in `protection/settings.json` under the app data folder. The password is never logged or stored. After the check, the backend's own copy and the hash are wiped from memory; copies held by the webview and the IPC layer are released but cannot be wiped. Only the first 5 characters of the hash leave this PC.

## Rule provenance and key handling

Packs are `pack.json` plus a detached `pack.sig` (128 hex characters) over the exact bytes. The app accepts a pack only if the signature matches the public key built into the app (`src-tauri/keys/rule-pack.pub`) and its sequence is higher than the active pack's. The built-in pack's sources are listed in `src-tauri/crates/windows-platform/src/protection/baseline/PROVENANCE.md`.

The current key is a **test key**; its private half is committed as a test fixture. Release builds therefore turn off import and download. To publish real packs, the maintainer:

1. Generates a key outside the repository: `node scripts/rule-pack.mjs keygen --private <offline path> --public src-tauri/keys/rule-pack.pub`. The script refuses to write a private key inside the repository. (The only exception is the committed test key under `src-tauri/crates/windows-platform/src/protection/fixtures/`.)
2. Re-signs the baseline pack with that key.
3. Publishes `pack.json` and `pack.sig` on the `rule-packs` branch, signing with `node scripts/rule-pack.mjs sign`.

A lost private key means no new packs can be published until a new key ships in an app update. A leaked key means a new app build with a new public key is required.

## Incident recovery

| Situation | What happens | What to do |
| --- | --- | --- |
| Newest pack corrupt or tampered | On start, the app falls back to the previous pack, then to the built-in pack, and shows a note. | Import or download the pack again. |
| Update interrupted | The pointer is replaced atomically, so the old pack stays active; leftover staging folders are removed on start. | Retry the update. |
| Bad new pack | — | **Go back to previous pack** on the Rules page (confirmed in a Windows dialog). |
| Restore collision | A file already exists at the original location, so nothing is restored or overwritten. The quarantine entry is kept. | Move or rename the existing file, then restore again. |
| Quarantine interrupted | On start, a confirmed copy completes removal of the original only if it is the same file; otherwise the original is kept and the partial entry removed. | Nothing. |
| Damaged quarantine entry | It is shown as damaged and can only be deleted. | Delete it. Its original file cannot be recovered from the app. |
| Lost quarantine folder | Entries disappear. The app never reads paths from outside its own folder. | Restore from your backups. Quarantine is not a backup. |

## Verification

Automated coverage and the open manual Windows acceptance items are listed in [protection verification](verification/protection.md).
