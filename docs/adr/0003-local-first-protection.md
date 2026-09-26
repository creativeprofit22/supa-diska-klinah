# ADR 0003: Local-first protection

- Status: Accepted
- Date: 2026-09-23
- Relates to: [ADR 0001](0001-modular-boundaries.md) (crate boundaries, no shell strings) and [ADR 0002](0002-system-change-helper.md) (the helper is not extended by this ADR).

## Context

Kudu v2.4.0 (pinned `db09e051`) ships a malware scanner, a process monitor, a quarantine, signature updates, and breach, CVE and "safety rating" features. Most of those depend on the Kudu cloud account (`cloud.usekudu.com`):

- YARA rules are downloaded automatically every 6 hours and integrity-checked only against a SHA-256 the same server supplies. That proves transport integrity, not authorship.
- Files are trusted when their folder name contains strings such as `git`, `python` or `microsoft`. Any process can create such a folder.
- Quarantine stores raw executable payloads beside a JSON manifest that is trusted on read, and restore can overwrite an existing file.
- The process monitor can terminate processes.
- Breach monitoring, the CVE feed and safety ratings send e-mail addresses and software inventories to the vendor.

The phase goal is to port the capabilities without overstating detection and without silently sending local data off the device.

## Decision

1. **No Kudu cloud dependency.** Nothing in this app contacts `usekudu.com` or any other vendor account service.
2. **Rules are Ed25519-signed packs.** A pack is `pack.json` plus a detached `pack.sig` over its exact bytes. The public key is compiled into the binary from `src-tauri/keys/rule-pack.pub`; the private key never enters the repository. The signature is verified **before** the JSON is parsed. Packs carry a monotonic `sequence`; installing a lower or equal sequence is refused except for the explicit "restore previous pack" action. A signed baseline pack is embedded in the binary so scanning always works offline.
3. **No YARA.** `yara-x` pulls in a WebAssembly runtime that breaks the binary budget, and `libyara` would add a C toolchain. The rule language is a small, bounded JSON format: SHA-256 blocklist entries, byte patterns with offset and size constraints (Aho-Corasick), and file-name rules. YARA syntax is documented as unsupported.
4. **Trust comes from signers, not folder names.** Authenticode (`WinVerifyTrust`) is evaluated offline (`WTD_REVOKE_NONE`, cache-only URL retrieval). A valid signature can downgrade a heuristic; it never suppresses a deterministic match.
5. **Processes are read-only.** The app lists processes with image path, parent and signer. It never terminates, suspends or injects into them. The UI points to Task Manager.
6. **Every network capability is separately opt-in and off by default.**
   - Rule-pack download of two fixed files (`pack.json`, `pack.sig`) from the `rule-packs` branch on `raw.githubusercontent.com`. Release assets are not used because they redirect, and the network sink refuses redirects.
   - Pwned Passwords k-anonymity range check: only the first five hex characters of the SHA-1 leave the device, with response padding requested.
   - AMSI scanning is a separate toggle, because the installed antivirus may apply its own cloud settings.
   The only network sink in the codebase is `windows-platform::protection::net` (WinHTTP). It requires a `NetworkCapability` token that can only be minted from an enabled policy flag. The webview CSP stays IPC-only.
7. **Evidence is typed.** Every finding is `deterministic`, `heuristic`, `unavailable` or `external`. UI and docs never claim a device is "clean", "safe", "certified" or "protected".
8. **Quarantine is contained.** Paths are derived natively from 32-hex IDs; journal records never supply storage paths. Payloads are XOR-neutered so they are not directly executable. Restore writes with `CREATE_NEW`, so it never overwrites.

### Unsupported Kudu features

| Kudu feature | Reason |
| --- | --- |
| E-mail breach monitor | Requires a paid HIBP API key and sends identity off the device. |
| CVE feed | Requires the Kudu cloud and a software inventory upload. |
| Startup and program "safety ratings" | Cloud reputation; replaced by local signer and location evidence. |
| Process termination | Destructive and easy to misuse; Task Manager already provides it with proper UI. |
| YARA rules | See decision 3. |
| Automatic background rule updates | Replaced by explicit, user-initiated import or opt-in download. |

## Threat model

| Threat | Mitigation |
| --- | --- |
| A malicious or tampered rule pack | Signature verified over exact bytes before parsing; bounded schema with `deny_unknown_fields` |
| Downgrade to an older, weaker pack | Monotonic sequence; only the retained `previous` may be restored, explicitly |
| Interrupted install corrupts rules | Stage → flush → rename → atomic pointer replace; startup recovery falls back to previous, then embedded baseline |
| Forged "trusted" folder names | Trust is derived from Authenticode signer only |
| Quarantine path escape through a crafted record | IDs are 32-hex; paths are derived natively; records are validated on read |
| Restore overwrites a newer file | `CREATE_NEW`; collisions fail visibly |
| Silent data exfiltration | One network sink, capability tokens from opt-in flags, architecture check, CSP IPC-only |
| Password disclosure in breach check | Backend copy and hash are zeroized; only a 5-hex SHA-1 prefix is sent; never logged or persisted |

## Consequences

Detection is intentionally narrow: the baseline pack contains the EICAR test file and a small set of documented rules. The app complements an antivirus; it does not replace one. The maintainer must generate and guard the release signing key; until then only the test key exists and downloads stay disabled in release builds. Adding a network capability requires amending this ADR.
