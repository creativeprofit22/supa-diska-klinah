# Security model

## Scope and assets

The application protects user files, Windows system configuration, restore-point integrity, the packaged helper binary, the one-shot authentication token, and the integrity of each requested operation. A restore point is **not a backup** and does not replace file backups.

The normal Tauri process runs at standard integrity. Its manifest requests `asInvoker`, and startup checks `TokenElevation` before creating a webview. An elevated launch exits. Preview, Recycle Bin, quarantine, undo, purge, and permanent cleanup remain standard-integrity operations.

## Trust boundaries

| Boundary | Enforcement |
| --- | --- |
| Webview to Rust IPC | Only local content in the `main` webview receives explicit application-command permissions. Rust validates typed input. |
| Project discovery input | Only root add accepts an untrusted absolute path, bounded to 4,096 UTF-8 bytes. Rust rejects empty, relative, lexical-parent, control-containing, missing, file, identity-less, drive, protected, and reparse roots, then stores the canonical path under an opaque ID. List, pause, remove, and scan accept IDs only. |
| Top-level navigation | Production accepts only `http://tauri.localhost`; development additionally accepts exactly `http://127.0.0.1:1520`. Credentials, remote hosts, alternate ports (including browser preview port 1521), schemes, and lookalikes are rejected. |
| Standard app to elevated helper | The helper exposes one operation enum, authenticates one loopback connection, enforces request freshness, then exits. |
| Loopback transport | The app binds `127.0.0.1` first, uses a random 256-bit token and independent request ID, caps frames at 4 KiB, applies 120-second socket timeouts, and permits 60-second authorizations within a 90-second handshake deadline. Tokens are compared without early exit and are not logged. |
| Filesystem containment | Rust rejects relative paths, lexical `..`, root equality, sibling-prefix confusion, missing paths, and every reparse-point component before and after canonicalization. |
| Cleanup execution | Rust chooses the fixed temporary root and rule. Display paths cross only in preview; plan, execution, and undo requests contain random opaque IDs. Persisted plans are treated as untrusted, bounded, and revalidated against current protections, strict containment, reparse state, type, identity, markers, age, activity, and occupancy immediately before each mutation. |
| Build profile registration | Rust accepts one canonical local `.exe`, bounded argv, a contained working directory, and bounded relative artifact paths. A native Windows prompt displays every value before persistence. Shells, scripts, script hosts, PATH lookup, environment overrides, and runtime argv are rejected. |
| Coordinated artifact execution | Executable identity is revalidated before direct argv launch. Streams are disconnected. Only successful complete snapshots commit ownership. Exact artifact plans bind live root, profile, generation, path, and identity before using the existing journaled quarantine path. |
| Installed helper | The broker resolves one exact filename beside the current executable, requires a regular contained non-reparse file, and never searches `PATH`. Protected installation and code signing remain deployment responsibilities. |
| Windows elevation | Only Windows UAC and the separately manifested helper cross into high integrity. The helper checks its own process token before dispatch. |
| System Restore | `SrClient.dll` loads only from System32. COM security is initialized for required local service identities, descriptions are bounded, and begin/end calls are paired. |

No generic filesystem, shell, process, arbitrary-path deletion, registry, service, or remote-content capability is granted. Standard-integrity process-launch sinks are confined to the approved build coordinator and the separate native-confirmed vendor-uninstall boundary; the elevated helper cannot reach either. Vendor operations can cause their own Windows elevation prompt, but the main app does not elevate itself. Destructive cleanup is reachable only through Rust-owned plans; permanent deletion has a distinct command and confirmation. There is no privileged deletion operation.

## Storage workflow boundaries

Storage commands accept bounded Raw JSON objects, not positional arrays, arbitrary paths or serialized proof records. Native folder selection or exact known-folder/browser scope resolution creates opaque, module-bound, single-use root authorizations. Status, page, cancel, release and plan selection remain snapshot-bound. A cancelled scan cannot produce mutation authority through retained rows. Catalog scope requests are serialized in the frontend so late refreshes cannot invalidate the latest UI inventory.

Fixed-drive `displayMount` is derived from the native validated logical mount: exactly three ASCII bytes, uppercase `A`–`Z` followed by `:\`. It is display-only in direct inventory and shared drive records, never a renderer-selected filesystem root or identity authority. Opaque `driveId`, retained volume evidence and native revalidation remain authoritative; volume GUIDs and serials stay native. Warning identifiers keep the same bounded canonical format and never include arbitrary native error paths.

Disk analysis and drive inventory are read-only. File cleanup requires native candidate evidence and an immutable plan. The user-approved recovery adaptation uses the existing journal with same-volume file-only app recovery: a held source handle, pinned no-follow destination ancestors, native directory-relative rename, and no overwrite/copy-delete fallback. Restoring checks the original root, original file identity/metadata, exact native-derived recovery location, current protections and occupancy. Browser restore also rechecks browser activity/scope. Storage recovery is not automatically purged. Empty folders remain permanent-only and use atomic empty-only removal; a new child prevents deletion.

Permanent execution now requires an app-owned native window and a default-No native confirmation displaying the native-resolved plan. Validation occurs before and after confirmation. Vendor jobs independently bind registry and executable evidence, require their own native confirmation and revalidation, never accept caller argv, and never replay stored commands. A vendor exit does not prove removal; cancellation/timeout need not terminate the vendor. Unknown-ownership leftovers never become filesystem targets.

Traversal limits count directory recursion from the native-authorized root. Files in the last permitted directory remain visible; deeper directories remain blocked and totals marked incomplete. The 64-directory depth cap, visited-entry/record caps, no-follow identity checks and immutable-plan eligibility are unchanged.

The optional pinned-source verification tool reads only fixed-revision JSON from the upstream raw-content host, refuses redirects, and bounds request time and response bytes. It does not execute downloaded code, update fixtures automatically, or grant catalog authority. Offline checks preserve unsupported Steam/database maintenance and private-data exclusions. The native smoke uses an immutable bridge, application-only WebView input and a read-only command allowlist; its inventory results are recorded as counts rather than program names.

No new elevated-helper operation or journal schema was added. Native scopes, plan summaries and displayed paths are not permission to bypass identity, browser-activity, duplicate-keeper or protected-root checks. Same-volume limits and unverified native/manual evidence are recorded in [storage verification](verification/storage-parity.md); this document is not a security certification.

## Protection boundaries

[ADR 0003](adr/0003-local-first-protection.md) and [protection](protection.md) define local-first protection. Its security properties:

- **Rule authenticity.** Packs are verified with Ed25519 over their exact bytes before parsing, against a compiled-in public key, with a bounded schema that rejects unknown fields. A monotonic sequence refuses downgrades; only the retained previous pack can be restored, behind native confirmation. Release builds refuse external packs while only the test key exists.
- **Rule-store recovery.** Install stages, flushes, renames and then atomically replaces a pointer. Startup removes staging leftovers and falls back to the previous pack, then to the embedded signed baseline. Reparse points inside the store are never followed.
- **Trust by signer.** Trust comes from offline Authenticode (no revocation or network retrieval), never from folder names. A valid signature can lower a heuristic's severity but never hides a signed-rule match.
- **Read-only processes.** The process inventory requests only limited query rights and never terminates, suspends or reads the memory of another process.
- **Quarantine containment.** Storage paths derive from 32-hex IDs; records are validated and never supply paths. Sources must sit under the scanned root, have no reparse ancestors, avoid protected paths and still match their scanned hash. The source is deleted through the same handle that was hashed. Payloads are neutered. Restore is hash-checked and create-new, so it never overwrites.
- **Network opt-in.** One WinHTTP sink with fixed hosts, HTTPS only, no redirects, cookies or automatic authentication, and bounded time and size. Each request needs a capability minted from its own opt-in flag. The breach check sends only a 5-character hash prefix. The webview CSP stays IPC-only. The full endpoint inventory is in [privacy](privacy.md).
- **Command surface.** Protection commands take bounded JSON objects with fixed enums, booleans or opaque IDs. Folders come from native pickers.

Residual risks: detection is only as good as the imported rules; AMSI providers may use their own cloud; a compromised process at the same integrity level can use the same confirmed commands; a leaked rule-signing key requires shipping a new app build.

## App update boundaries

[Updates](updates.md) describes the user flow. Its security properties:

- **Opt-in network use.** Update requests need an `UpdateCheck` capability minted only from the saved "Allow checking for updates" setting, which defaults to off. The setting is read in Rust, not taken from the webview. Protection's opt-ins never enable update checks, or the reverse.
- **Sink inventory.** Updates reuse the single WinHTTP sink. The manifest and signature come from a fixed path on `raw.githubusercontent.com` with redirects refused. The installer comes from a fixed `github.com` release path and is the only request that may follow a redirect: exactly one hop, HTTPS only, to `release-assets.githubusercontent.com` or `objects.githubusercontent.com`, with a size cap equal to the manifest's size (at most 512 MiB).
- **Manifest authenticity.** `update.json` is verified with a dedicated Ed25519 key (`src-tauri/keys/update.pub`, separate from the rule-pack key) over its exact bytes before parsing. It is rejected when it is not newer than the running version, expired, not yet valid, valid for more than 90 days, oversized, or names a different installer.
- **Installer integrity.** The download is hashed while streaming; size and SHA-256 must match the signed manifest. The file is then opened with a share mode that denies writes and deletes and stays open from re-verification through launch.
- **No signature downgrade.** A manifest that says `authenticode` requires a valid signature by its named thumbprint. A signed installed app only accepts updates signed by its own certificate, whatever the manifest says. A broken signature is always rejected.
- **No elevation by the app.** After a native confirmation (default No) the app opens the per-machine NSIS installer, which raises its own UAC prompt; the app stays `asInvoker`.
- **Uninstall cleanup.** The NSIS pre-uninstall hook runs the helper's argument-only `--remove-scheduled-tasks` verb, which deletes only tasks matching the app's naming scheme in `\SupaDiskaKlinah` and removes the folder only when empty. It is skipped during updates.

## Attacker model and assumptions

Untrusted inputs include webview content, the explicit project-root add path, persisted app registries and plans, command payloads, helper arguments, loopback peers, framed JSON, local filesystem entries, repository content, and build environment variables. The design assumes same-user processes may race or guess ports, local web content may be compromised, Windows UAC behaves correctly, the installed directory is protected from standard users, and no administrator compromise already exists.

Project discovery accepts no runtime rule JSON, shell string, command, or executable argument. It requires ecosystem-compatible marker sets, skips link-like children, verifies canonical containment in both the saved root and marker context, retains identities through revalidation, and caps aggregate roots, workers, traversal, candidates, diagnostics, measurement, and output. Covered roots and overlapping artifacts are suppressed deterministically. Discovery snapshots and resolved candidates are dropped immediately; responses have no scan or plan identifier. See the [project artifact guide](project-artifacts.md).

Build profile registration is the only route that accepts an executable, argv, working directory, or artifact paths. A native prompt, not webview state, grants repeatable authority. Start and cancel accept opaque IDs only. One build runs globally; cancellation is checked before launch, during polling, after exit, before stamp persistence, and before budget execution. Nonzero, cancelled, interrupted, incomplete, ambiguous, or changed builds authorize no cleanup. Scheduled scans never infer success or ownership. See [build artifact budgets](build-artifact-budgets.md).

A process already executing inside the standard-integrity app can request the same bounded restore-point operation the app exposes or invoke an already approved build profile. Native approval, fixed argv, executable identity, one active build, explicit paths, protected generations, and recoverable quarantine cap the build-profile blast radius. They do not sandbox an intentionally approved executable or protect against an attacker who already controls an administrator process.

## Approved privileged operations

The helper accepts exactly two operations (protocol version 2, 16 KiB frames). `ApplySystemChanges` is reviewed in [ADR 0002](adr/0002-system-change-helper.md). It carries 1 to 32 entries of the closed `HelperChange` enum:

- service start type
- machine policy value
- Microsoft task enable state
- Windows Update policy
- firewall rule and profile
- hibernation
- driver package removal
- hosts line disable/restore
- machine startup entry
- restore point

Every identifier resolves against a catalog compiled into the helper or against a live enumeration. The helper re-reads the prior state, and it writes nothing when that state has changed since the preview. It returns one `{ prior, outcome }` per entry. The only process it launches is `<System32>\powercfg.exe` with the fixed argv `/hibernate on|off`. `scripts/check-security-boundaries.mjs` pins the exact variant set, and `scripts/check-architecture.mjs` bans shell and management-CLI strings in system-management modules.

The webview never sends helper entries. It previews typed changes, receives an opaque single-use plan ID that expires after 60 seconds, and cannot confirm. A native Windows dialog lists every change and its reversibility. Execution journals intent before each change and the outcome after it. See the [administrator guide](system-management-admin.md).

`CreateSystemRestorePoint` remains available as a standalone operation. Its sole argument is a nonempty description containing no control or NUL characters and no more than 128 UTF-16 code units. It accepts no path, executable, registry key, service, command, or shell string. It creates and closes a `MODIFY_SETTINGS` restore point and returns only its sequence number.

Cleanup commands accept opaque scan, candidate, plan, and execution identifiers. Rust resolves paths, journals before and after each item, and fails individual items closed. Manual cleanup defaults to the Windows Recycle Bin; automatic cleanup and artifact budgets use recoverable quarantine before opt-in delayed purge. Artifact eviction never uses the privileged helper and never permanently deletes directly.

## Privileged-operation inventory

This classification covers every Kudu v2.4.0 module recorded in `docs/parity.md`. “Standard” means the expected parity path should remain unelevated. “Mixed” means most inspection stays standard and narrow mutations need a reviewed helper enum variant. “Helper-only” means the parity operation is genuinely administrative. Classification is not authorization. The implemented helper surface is exactly the ADR 0002 set above. HKCU startup and privacy values, active power plan, and this app's own scheduled scans stay unelevated.

| Classification | Kudu modules |
| --- | --- |
| Standard | Browser, LargeFiles, Duplicates, Memory, DiskHealth, Battery, Notifications, CloudCleanup, FileShredder |
| Mixed | Cleaner, Startup, Registry, Uninstaller, Network, StorageSense, Debloater, Privacy, Optimizer, System, Telemetry, PowerPlan, Hosts, Environment, Scheduler, Updates, Firewall, ContextMenu, Gpu, RegistryBackup, GameMode |
| Helper-only | Drivers, Restore, Repair, BootTrace |

Direct Kudu handlers are classified as follows: platform information and onboarding are standard; cleaner location/blockers, settings/backup directory, scan/deletion/history, and updater operations are mixed; elevation and restore-point handlers are helper-only. Kudu's whole-application relaunch-as-administrator behavior is explicitly rejected.

## Failure modes and recovery

UAC cancellation or denial, disabled System Restore, safe mode, COM initialization failure, low disk space, Windows timeout, missing or replaced helper, malformed or stale messages, wrong tokens, helper loss, and non-loopback peers all fail closed. The standard app remains running and is never relaunched elevated.

Build failure, cancellation, snapshot failure, restart, executable replacement, profile/root changes, external writes, identity changes, unreadable paths, activity, and protected-byte floors stop artifact mutation. Failed or cancelled builds may leave their own partial output, but the coordinator does not remove it. Journaled quarantine remains undoable until its configured purge deadline.

The helper and command error boundaries expose only fixed, non-sensitive codes. Build artifacts add `approvalDeclined` and `buildBusy` to existing fixed errors. Discovery and build errors never echo submitted paths, executables, argv, child output, or raw filesystem/process details. Raw Windows, trash, persistence, COM, protocol, token, path, and system errors never cross into the webview or production logs.

Callers should retry cancelled authorization only after approving UAC; repair or reinstall an unavailable helper; retry expired requests; and check Windows System Protection plus available disk space after timeout or System Restore failure. Register a build profile again only after intentionally replacing its executable or root. Inspect a protected floor rather than weakening its protections.

The Windows 10 x64 alpha follows the [release checklist](release-checklist.md). It requires the hardened automated gates, one successful local restore-point run, and one local UAC-cancellation run. Both manual runs confirm the original app remains unique and unelevated. CI never invokes `SRSetRestorePointW`, and automated results must not be represented as manual Windows verification.

## Future release checks

Windows 11, ARM64, disposable-machine installation, and disabled-System-Restore behavior are non-blocking for the alpha. They must be reconsidered before broad Windows distribution. Code signing is tracked in [release](release.md#turning-on-code-signing).

## Residual risks

Loopback authentication does not isolate a compromised standard app from its approved operations. Final filesystem validation narrows link and containment races but cannot eliminate every filesystem race after handles close. Approved native build executables are not sandboxed and may read or change anything available to the standard user; native confirmation is therefore a consequential trust grant. Wrappers that daemonize can outlive cancellation, so external-change grace must remain enabled. Recycle Bin enumeration can be affected by concurrent trash activity, so undo requires the exact captured identifier and original path. Concurrent disk activity also means reclaimed bytes are a capped observed free-space delta, not guaranteed causation. Installation ACLs, certificate custody, Windows restore-point policy, and backup quality remain operational responsibilities.

**Unsigned releases.** Releases currently ship without a Windows code-signing certificate. Windows shows "Unknown publisher" in SmartScreen and UAC, so there is no publisher identity and no SmartScreen reputation. What remains: HTTPS downloads from GitHub, published `SHA256SUMS`, and GitHub build-provenance attestations for manual checks ([installation](installation.md#verify-your-download)). For in-app updates, the Ed25519 update key is the **sole** integrity root while releases are unsigned. Anyone holding it can push an update to users who turned update checks on. Mitigations: the key is a `windows-release` environment secret used only by the tag-triggered build job, the updater is opt-in and asks before installing, and provenance attestations let anyone audit a release. Once releases are signed, both signatures are required and the no-downgrade rule prevents a return to unsigned updates.

MangoDisk informed containment behavior only. No GPL implementation or test code was copied; licensing details remain in `docs/licensing.md`. Cargo and cargo-sweep references informed behavior only; no implementation was copied.
