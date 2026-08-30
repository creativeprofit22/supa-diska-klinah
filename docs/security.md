# Security model

## Scope and assets

The application protects user files, Windows system configuration, restore-point integrity, the packaged helper binary, the one-shot authentication token, and the integrity of each requested operation. A restore point is **not a backup** and does not replace file backups.

The normal Tauri process runs at standard integrity. Its manifest requests `asInvoker`, and startup checks `TokenElevation` before creating a webview. An elevated launch exits. Preview, Recycle Bin, quarantine, undo, purge, and permanent cleanup remain standard-integrity operations.

## Trust boundaries

| Boundary | Enforcement |
| --- | --- |
| Webview to Rust IPC | Only local content in the `main` webview receives explicit application-command permissions. Rust validates typed input. |
| Top-level navigation | Production accepts only `http://tauri.localhost`; development additionally accepts exactly `http://127.0.0.1:1420`. Credentials, remote hosts, alternate ports, schemes, and lookalikes are rejected. |
| Standard app to elevated helper | The helper exposes one operation enum, authenticates one loopback connection, enforces request freshness, then exits. |
| Loopback transport | The app binds `127.0.0.1` first, uses a random 256-bit token and independent request ID, caps frames at 4 KiB, applies 120-second socket timeouts, and permits 60-second authorizations within a 90-second handshake deadline. Tokens are compared without early exit and are not logged. |
| Filesystem containment | Rust rejects relative paths, lexical `..`, root equality, sibling-prefix confusion, missing paths, and every reparse-point component before and after canonicalization. |
| Cleanup execution | Rust chooses the fixed temporary root and rule. Display paths cross only in preview; plan, execution, and undo requests contain random opaque IDs. Persisted plans are treated as untrusted, bounded, and revalidated against current protections, strict containment, reparse state, type, identity, markers, age, activity, and occupancy immediately before each mutation. |
| Installed helper | The broker resolves one exact filename beside the current executable, requires a regular contained non-reparse file, and never searches `PATH`. Protected installation and code signing remain deployment responsibilities. |
| Windows elevation | Only Windows UAC and the separately manifested helper cross into high integrity. The helper checks its own process token before dispatch. |
| System Restore | `SrClient.dll` loads only from System32. COM security is initialized for required local service identities, descriptions are bounded, and begin/end calls are paired. |

No generic filesystem, shell, process, arbitrary-path deletion, registry, service, or remote-content capability is granted. Destructive cleanup is reachable only through Rust-owned plans; permanent deletion has a distinct command and confirmation. There is no privileged deletion operation.

## Attacker model and assumptions

Untrusted inputs include webview content, command payloads, helper arguments, loopback peers, framed JSON, local filesystem entries, repository content, and build environment variables. The design assumes same-user processes may race or guess ports, local web content may be compromised, Windows UAC behaves correctly, the installed directory is protected from standard users, and no administrator compromise already exists.

A process already executing inside the standard-integrity app can request the same bounded restore-point operation the app exposes. The operation enum, description validation, helper token, freshness limit, and one-request lifetime cap that residual blast radius. They do not protect against an attacker who already controls an administrator process or can replace a legitimately trusted installed helper.

## Approved privileged operation

`CreateSystemRestorePoint` is the only approved helper operation. Its sole argument is a nonempty description containing no control or NUL characters and no more than 128 UTF-16 code units. It accepts no path, executable, registry key, service, command, or shell string. It creates and closes a `MODIFY_SETTINGS` restore point and returns only its sequence number.

Cleanup commands accept opaque scan, candidate, plan, and execution identifiers. Rust resolves paths, journals before and after each item, and fails individual items closed. Manual cleanup defaults to the Windows Recycle Bin; automatic cleanup uses recoverable quarantine before opt-in delayed purge. Privileged deletion remains prohibited.

## Privileged-operation inventory

This classification covers every Kudu v2.4.0 module recorded in `docs/parity.md`. “Standard” means the expected parity path should remain unelevated. “Mixed” means most inspection stays standard and a future narrow mutation may need a newly reviewed helper enum variant. “Helper-only” means the parity operation is genuinely administrative. Classification is not authorization; only restore-point creation is currently implemented in the helper.

| Classification | Kudu modules |
| --- | --- |
| Standard | Browser, LargeFiles, Duplicates, Memory, DiskHealth, Battery, Notifications, CloudCleanup, FileShredder |
| Mixed | Cleaner, Startup, Registry, Uninstaller, Network, StorageSense, Debloater, Privacy, Optimizer, System, Telemetry, PowerPlan, Hosts, Environment, Scheduler, Updates, Firewall, ContextMenu, Gpu, RegistryBackup, GameMode |
| Helper-only | Drivers, Restore, Repair, BootTrace |

Direct Kudu handlers are classified as follows: platform information and onboarding are standard; cleaner location/blockers, settings/backup directory, scan/deletion/history, and updater operations are mixed; elevation and restore-point handlers are helper-only. Kudu's whole-application relaunch-as-administrator behavior is explicitly rejected.

## Failure modes and recovery

UAC cancellation or denial, disabled System Restore, safe mode, COM initialization failure, low disk space, Windows timeout, missing or replaced helper, malformed or stale messages, wrong tokens, helper loss, and non-loopback peers all fail closed. The standard app remains running and is never relaunched elevated.

The helper and command error boundaries expose only fixed, non-sensitive codes. Cleanup uses `invalidInput`, `notFound`, `cleanupBusy`, `validationFailed`, `persistenceFailed`, `operationFailed`, and `taskUnavailable`. Raw Windows, trash, persistence, COM, protocol, token, path, and system error details never cross into the webview or production logs.

Callers should retry cancelled authorization only after approving UAC; repair or reinstall an unavailable helper; retry expired requests; and check Windows System Protection plus available disk space after timeout or System Restore failure. A privilege failure means the helper launched without required elevation. If Windows accepts the begin call but the end call fails, inspect System Protection before retrying; do not assume the partial restore point is usable.

The Windows 10 x64 alpha follows the [release checklist](release-checklist.md). It requires the hardened automated gates, one successful local restore-point run, and one local UAC-cancellation run. Both manual runs confirm the original app remains unique and unelevated. CI never invokes `SRSetRestorePointW`, and automated results must not be represented as manual Windows verification.

## Future release checks

Windows 11, ARM64, code signing, disposable-machine installation, and disabled-System-Restore behavior are non-blocking for the alpha. They must be reconsidered before broad Windows distribution.

## Residual risks

Loopback authentication does not isolate a compromised standard app from its approved operations. Final filesystem validation narrows link and containment races but cannot eliminate a race after handles close. Recycle Bin enumeration can be affected by concurrent trash activity, so undo requires the exact captured identifier and original path. Concurrent disk activity also means reclaimed bytes are a capped observed free-space delta, not guaranteed causation. Installation ACLs, certificate custody, Windows restore-point policy, and backup quality remain operational responsibilities.

MangoDisk informed containment behavior only. No GPL implementation or test code was copied; licensing details remain in `docs/licensing.md`.
