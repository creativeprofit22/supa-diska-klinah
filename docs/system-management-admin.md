# System management: administrator guide

This guide covers the privileged parts of the Startup, Services, Drivers, Firewall, Hosts, Privacy, Power, Restore, Windows Update, Scheduler, and Quick optimization pages. The design is recorded in [ADR 0002](adr/0002-system-change-helper.md), and the security posture in [security](security.md).

## Prerequisites

- Windows 10 22H2 (build 19045) or Windows 11 23H2/24H2 (22631/26100), x64. Earlier builds are unsupported.
- A standard user can review everything. Helper-privileged changes need an account that can approve UAC.
- The signed `privileged-helper.exe` must sit next to the app executable. If it is missing or replaced, every helper change is reported as *helper unavailable*, and nothing runs.
- The Windows Defender Firewall service (for the firewall page), System Protection on the system drive (for restore points), and the Windows Update Agent (for update status) must be present. If any is missing, its page reports it as unavailable rather than failing.

## Privilege boundary

| Runs unelevated | Runs in the helper (one UAC prompt per confirmed plan) |
| --- | --- |
| All inventory and preview reads | Service start types (compiled service catalog) |
| HKCU startup `StartupApproved` values | HKLM startup `StartupApproved` values |
| HKCU privacy and performance values | HKLM privacy policy values (compiled catalog) |
| Active power plan | Microsoft telemetry task enable state (compiled task catalog) |
| This app's scheduled scans (`\SupaDiskaKlinah\scan-<uuid>`) | Firewall rules and profiles |
| Windows Update detect-now | Windows Update policy values (compiled catalog) |
| | Hosts line edits, driver package removal, hibernation, restore points |

The helper accepts only the closed `HelperChange` enum. It never receives a path, command line, registry key, or executable from the app. The only process it starts is `%SystemRoot%\System32\powercfg.exe /hibernate on|off`. No module uses `cmd`, PowerShell, `sc`, `netsh`, `reg`, `schtasks`, `wmic`, or `pnputil`.

## Unsupported editions and managed devices

- **Windows Home:** Group Policy values under `SOFTWARE\Policies` are ignored. Windows Update policies and policy-backed privacy entries (such as consumer features) are reported as *not honored by this edition* and are never written. Telemetry level 0 is only honored on Enterprise and Education; elsewhere Windows treats it as 1.
- **Domain-joined or MDM-managed devices:** Windows Update policy is reported as *managed by your organization* and is not written, because central policy would override it. Other settings may also be overwritten at the next policy refresh. Prefer central management on those devices.
- **System Restore disabled by policy:** creating a restore point is reported as managed. Listing restore points usually needs administrator rights and otherwise shows *requires administrator*.
- **No S4 support:** the hibernation toggle is reported as unavailable.

## Journal and recovery

- **Journal:** `%APPDATA%\com.supadiskaklinah.app\system-changes\journal.json` (the Tauri app-data directory). It is versioned JSON with at most 500 entries, replaced atomically with write-through.
- **Damaged journal:** if the journal cannot be read (for example, after a disk error or antivirus truncation) or was written by an older format, the app renames it to `journal.corrupt-<unix-seconds>.json` in the same folder and starts a new, empty journal. Changes recorded in the renamed file can no longer be undone from Change history; keep the file for manual inspection.
- **Newer-version journal:** if the journal was written by a newer version of the app (for example, after a downgrade), every system change and undo is blocked with *The change history could not be read* so that this version never overwrites undo data it cannot read. Update the app to fix this.
- **Manual reset:** only if updating is not possible, close the app and move `journal.json` out of the `system-changes` folder. Entries in the moved file can no longer be undone automatically.
- **Entries:** each change gets an *intent* record before it runs and an *outcome* after it finishes. An intent without an outcome means the process stopped mid-change. Opening Change history re-reads the current state of each interrupted entry and resolves it:
  - If the setting still matches the recorded prior state, the entry is recorded as failed (*interrupted*), and nothing needs undoing.
  - If the setting matches the change's target, the entry is recorded as *applied* and can be undone like any other applied change.
  - Otherwise (the setting cannot be read, or it holds some third value), the entry stays open. Inspect that setting manually.
- **Undo:** builds an inverse plan from the recorded prior state, then goes through the same preview and native confirmation. If the setting changed since, the inverse reports *state changed* and writes nothing.
- **Hosts backups:** `%ProgramData%\SupaDiskaKlinah\hosts-backups\` keeps the newest 20 copies. To restore manually, copy a backup over `%SystemRoot%\System32\drivers\etc\hosts` from an elevated session.
- **Driver removal:** it is irreversible. Recover by using the restore point offered before removal, or by reinstalling the vendor driver.
- **Scheduled scans:** these tasks live under `\SupaDiskaKlinah\` in Task Scheduler and run `<app>.exe --scheduled-scan <uuid>` as the current user with least privilege. The Scheduled scans page lists orphaned tasks (a moved app or malformed arguments) so they can be removed. Scan summaries are stored in `%APPDATA%\com.supadiskaklinah.app\scheduled-scans\summaries.json`.

## Troubleshooting

| Symptom | Cause | Action |
| --- | --- | --- |
| Every helper change says *Denied* | UAC declined, or a non-admin account | Approve UAC with an admin account |
| *Helper unavailable* | `privileged-helper.exe` missing, blocked, or replaced | Reinstall the app; check that AV has not quarantined the helper |
| *The helper stopped responding* | The helper may have applied some changes before the connection was lost | Open Change history and refresh the page; entries are recorded per change |
| *State changed* | Another tool or policy changed the value after preview | Refresh and review again; investigate the competing tool or policy |
| *The change history could not be read* | The journal was written by a newer app version | Update the app, or reset the journal manually (see Journal and recovery) |
| Firewall page unavailable | Firewall service stopped or third-party firewall | Use the vendor's console |
| Update policy has no effect | Home edition or managed device | See above; use Settings > Windows Update or central policy |

Manual privileged acceptance and its status are recorded in [system-management verification](verification/system-management.md).
