# ADR 0002: System-change helper batch

- Status: Accepted
- Date: 2026-09-23
- Supersedes: the "restore point is the only approved privileged operation" consequence of [ADR 0001](0001-modular-boundaries.md). Every other part of ADR 0001 still applies.

## Context

System-management parity with Kudu v2.4.0 covers startup, services, drivers, firewall, hosts, privacy, hibernation, Windows Update policy, restore points, scheduling, and optimization. Many of these changes write machine-wide state such as HKLM, the Service Control Manager, the firewall policy, the driver store, the System32 hosts file, or `powercfg`. Those writes require administrator rights. The main application must stay `asInvoker`.

Kudu performs much of this work by interpolating strings into `powershell`, `cmd`, `sc`, `netsh`, and `schtasks`. Porting that design would put caller-controlled text in front of an elevated interpreter.

We want one confirmed plan to need only one UAC prompt, with results that stay honest when some changes in the batch fail.

## Decision

### One new operation, closed inner enum

`PrivilegedOperation` gains exactly one variant:

```text
ApplySystemChanges { changes: Vec<HelperChange> }   // 1..=32 entries
```

`HelperChange` is a closed, `deny_unknown_fields` enum. The helper resolves every identifier against a catalog compiled into its binary:

| Variant | Caller supplies | Helper resolves/validates |
|---|---|---|
| `SetServiceStartType` | `catalogId`, `startType` (`automatic`/`manual`/`disabled`) | Service name comes from the compiled service catalog. Boot and system start types can never be set. |
| `SetMachinePolicyValue` | `settingId`, `value` (bounded enum/u32) | HKLM key, value name, and type are fixed per catalog ID. The value must be in the catalog's allowed set. |
| `SetFirewallRuleEnabled` | `ruleName` (≤256 UTF-16, no control chars), `enabled` | The rule must exist in the current `INetFwPolicy2` rule set, and its current enabled flag must equal the item's `expectedPrior` (`{ enabled }`). |
| `SetFirewallProfileEnabled` | `profile` (`domain`/`private`/`public`), `enabled` | Fixed `NET_FW_PROFILE_TYPE2` mapping |
| `SetHibernation` | `enabled` | `%SystemRoot%\System32\powercfg.exe` is resolved from the Windows directory API and launched with argv `["/hibernate","on"|"off"]` only. |
| `DeleteDriverPackage` | `publishedName` | Must match `^oem\d{1,5}\.inf$`, currently be enumerated in the driver store, and not be bound to a present device. |
| `EditHosts` | `lineOps[]` (≤64, `line` index + `disable`/`restore`) | The file's current SHA-256 must equal the item's `expectedPrior` (`{ sha256 }`). A backup goes to `%ProgramData%\SupaDiskaKlinah\hosts-backups` before an atomic replace. Lines are only commented or uncommented, never authored. |
| `SetMachineStartupEntry` | `location` (`run`/`run32`/`startupFolder`), `name` (bounded), `enabled` | The entry must exist in the machine `Run`/`Run32` key or the `%ProgramData%` Startup folder. Only the matching HKLM `StartupApproved` bytes are written. |
| `SetSystemTaskEnabled` | `catalogId`, `enabled` | The task path comes from the compiled privacy task catalog (fixed `\Microsoft\Windows\...` paths). It is toggled through Task Scheduler COM `IRegisteredTask::put_Enabled`, never `schtasks`. |
| `SetWindowsUpdatePolicy` | `settingId`, `value` | Separate compiled catalog under `SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate\AU` |
| `CreateRestorePoint` | `description` (≤128 UTF-16, no control chars) | Same validation and `SRSetRestorePointW` backend as the top-level operation; lets a batch create a restore point before an irreversible change under one prompt. |

The expected prior state is not a field of any variant. Each batch entry is a `HelperChangeItem { change, expectedPrior }`, where `expectedPrior` is a `PriorState`: for example `{ "kind": "enabled", "enabled": true }` for a firewall rule or `{ "kind": "hosts", "sha256": "…" }` for the hosts file. The helper compares it against the state it re-reads before writing.

The top-level `CreateSystemRestorePoint` operation is unchanged. There is no variant that accepts a path, a command line, an arbitrary registry key, an executable, or an environment. The helper never invokes `cmd`, `powershell`, `sc`, `netsh`, `reg`, or `schtasks`.

### Prior-state re-read and per-change results

For each change, the helper:

1. re-reads the current state inside the elevated process;
2. applies the change only when the state differs from the target;
3. returns `{ prior, outcome }`.

`outcome` is one of `applied`, `alreadyApplied`, `stateChanged`, `unsupported`, or `failed` (with a bounded, non-sensitive code). The helper runs every change in the batch and never stops silently after the first failure. If the restore point in the same plan fails, irreversible changes after it are not attempted; the helper reports them as `failed` with code `notAttempted` and still runs reversible changes. Every entry carries the preview's `expectedPrior` (for example the hosts SHA-256 or the firewall rule's current enabled flag). On mismatch the helper returns `stateChanged` and does not write. The unprivileged side journals the returned prior state so rollback does not depend on data the webview supplied.

### Envelope limits

`MAX_FRAME_BYTES` rises from 4 KiB to 16 KiB so that 32 bounded changes fit. The protocol version rises to 2, and version 1 frames are rejected. The existing loopback-only transport, 32-byte token, request ID, 60-second expiry, one-shot exchange, and elevation self-check are unchanged.

### Standard-integrity changes stay out of the helper

HKCU Run/StartupApproved, HKCU privacy values, the app's own scheduled tasks under `\SupaDiskaKlinah\`, and all reads run in the main unelevated process. The broker is only invoked when a confirmed plan contains at least one change whose `Privilege` is `Helper`.

### Plan, confirmation, and journal

The webview never sends `HelperChange` values. It asks for a plan built from preview identifiers and receives an opaque, single-use plan ID that expires after 60 seconds. A native Windows confirmation dialog lists every change and its reversibility. Execution journals an intent record before each change and an outcome record after it. Rollback builds an inverse plan from the journaled prior state and goes through the same preview and confirmation flow. Irreversible changes (driver package deletion) say so before confirmation and cannot be rolled back.

## Threat model

| Threat | Mitigation |
|---|---|
| A compromised webview requests arbitrary elevated work | The webview holds only opaque plan IDs. The helper accepts only the closed `HelperChange` enum, whose identifiers resolve to compiled catalogs. |
| A crafted identifier reaches a sensitive target | Catalog lookup fails closed. Driver names are regex-bounded and must be enumerated and unbound. Firewall rule names must already exist. Startup entry names must already exist in a machine startup location. Task catalog IDs map only to fixed Microsoft task paths. |
| A time-of-check/time-of-use race between preview and apply | The helper re-reads the prior state. Every item carries an `expectedPrior`: hosts edits compare its SHA-256 and firewall rule toggles compare its `enabled` flag. On mismatch, both the helper and the unprivileged executor report `stateChanged` without writing. |
| Oversized or malformed frames | 16 KiB cap, a 32-change cap, `deny_unknown_fields`, and bounded strings |
| Replay of a captured request | The same per-launch token, request ID, 60-second expiry, and one-shot socket as protocol v1 |
| Shell injection | No shell interpreter is reachable. The only process launch is `powercfg.exe`, from a resolved system path, with fixed argv. |
| Partial failure hides damage | Per-change outcomes, an intent-before-write journal, and a crash-recovery scan of intent records that have no outcome |
| Hosts file loss | A backup is written before an atomic replace, and restore uses the journal. |
| Irreversible driver removal | Declared `irreversible` in the preview. A restore point is offered first. Only superseded, unbound `oem*.inf` packages qualify. |

## Rejected alternatives

- **One helper operation per change type, each with its own UAC prompt:** rejected because a quick-optimization plan would trigger many consent prompts, and users would learn to click through them.
- **Passing registry paths or service names from the caller:** rejected because it turns the helper into a generic elevated registry or SCM writer.
- **Running `schtasks`, `sc`, `netsh`, or PowerShell with argument arrays:** rejected where a typed COM or Win32 API exists, because output parsing is locale-dependent and those tools accept rich sub-languages. `powercfg /hibernate` is the single exception, because Windows exposes no supported API for toggling the hibernation file.
- **A persistent elevated service:** still rejected, for the reasons in ADR 0001.

## Consequences

The allowlist check pins the exact `HelperChange` variant set. Adding a variant requires amending this ADR with its threat-model row. Home editions and MDM- or domain-managed devices report Windows Update policy as `unsupported` or `managed` rather than writing values that would be ignored. Live privileged verification needs a disposable administrator machine and cannot run in CI.
