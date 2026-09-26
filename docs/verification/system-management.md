# System-management verification

Phase `6f9c28c5-1cd7-4560-a351-28484321ad66` on branch `feat/windows-system-management`.

Successful commands are automated evidence only. They do not constitute harness gate approval, and they do not replace the manual privileged acceptance below. That acceptance stays **open** until the repository owner explicitly accepts it.

## Runtime

fnm Node `v24.19.0` (`C:\Users\SPARTAN PC\AppData\Local\fnm_multishells\...\node.exe`) and pnpm `11.22.0` were verified in each command environment. `pnpm exec node` reported the same version and path. Rust 1.88+ toolchain; `Cargo.lock` gained no new crates (only `windows`/`windows-sys` features were enabled).

## Automated evidence (2026-09-23)

| Check | Result | Execution ID |
| --- | --- | --- |
| `pnpm test` (Vitest + node script tests) | 41 files / 288 Vitest tests passed; 28 node tests passed | `acd062a6-4ca1-4499-98a5-f7c236a589c6` |
| `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` | Clean | `077f1c41-3c4c-4c7e-93a1-e3b750d877aa` |
| `cargo test --workspace --locked` | 515 passed, 0 failed, 4 ignored (all pre-existing, none in system-management code) | `077f1c41-3c4c-4c7e-93a1-e3b750d877aa` |
| Read-only real-IPC tests `src-tauri/tests/system_management_commands.rs` | 17 passed on this machine, unelevated | `7246fb70-ae2c-40e9-a2b8-fee1f4abb913` |
| `scripts/check-security-boundaries.mjs` variant pin, mutation-tested by injecting a `RunShell` variant | Rejected as intended; file restored | recorded in the session log, not re-run |
| `pnpm check` (pins, parity, architecture, security boundaries, docs, `tsc`, `vite build`) | All passed | `116c7310-11b4-4f83-a525-0731bcea4090` |
| Scheduled scan, old path (Tauri app built, then `exit(0)` from `setup`): debug exe `--scheduled-scan 0f8fad5b-d9cb-469f-a165-70867728950e`, polled for visible non-console windows and WebView2 children | Exited 0 in 9.9 s with one summary appended and the main window never shown, **but** a WebView2 child process was started (the frontend could load against unmanaged state) | `b28f78ce-ee31-40c6-bba1-d60875f134c2` |
| Scheduled scan, new path (scan runs in `run()` before any Tauri builder; app data = `%APPDATA%` + bundle identifier) | Exited 0 in 9.4 s, no visible app window, no WebView2 child, one summary appended to `%APPDATA%\com.supadiskaklinah.app\scheduled-scans\summaries.json`, no stray temp files | `91185fcb-9a37-4790-842d-19e6cc005ea6` |
| Concurrent `record_summary` (8 threads × 40 writes): unique `summaries-<random>.tmp` via `create_new`, removed on failure | Fails against the old fixed `summaries.json.tmp` name; passes with the fix | red `bff64bd6-266a-4700-9d17-c7e54a694ae5`, green `710ccae2-202a-41b3-9390-7867376c4e99` |
| `cargo test -p windows-platform --locked --lib scheduler`, `cargo test -p supa-diska-klinah --locked --lib`, `cargo fmt --all --check`, `cargo clippy -p windows-platform -p supa-diska-klinah --all-targets --locked -- -D warnings` | 17 passed; 13 passed; clean; clean | `c0531c51-c591-402e-978b-eb1d3de7a4c9` |

### What the automated tests cover

- **Supported Windows versions and editions:** Builds 19045, 22631, and 26100 are tested against Home, Pro, and Enterprise, both managed and unmanaged, in the updates, privacy, and `os_info` tests. Home and managed gating returns `Unsupported`.
- **Unavailable APIs:** COM class not registered or firewall service stopped (firewall, updates, restore), missing SCM services (services), no S4 support (power), and a missing WMI namespace (restore).
- **Partial failure:** a failure mid-batch still runs and reports later changes (`system_change::tests`, `security::system_changes::tests`, helper dispatch). Helper-response length mismatch and a lost response are recorded per item as interrupted.
- **Rollback:** only applied entries can be undone. Irreversible entries (driver removal), already-rolled-back entries, and interrupted entries are refused. The inverse is built from the journaled prior state, per module.
- **Idempotency:** re-applying a change reports `alreadyApplied`. Scheduler upsert is idempotent by UUID, and remove-twice is idempotent.
- **Privilege denial:** UAC cancel and `PrivilegeFailure` become `Denied` for every helper item, with nothing written. Access denied in each adapter becomes `Denied`.
- **Helper protocol:** v1 frames, unknown variants, extra fields, empty batches, more than 32 changes, and oversize frames are all rejected before system access. The frame-fit check runs before UAC.
- **Live disposable scheduler task:** registered, read back, re-upserted, and removed under `\SupaDiskaKlinah\`, with a drop guard.
- **Scheduled-scan summaries:** concurrent writers no longer collide on the temporary file. This is not a lock: when two writers overlap, the last rename wins and the other summary can be dropped. The test asserts only that every write succeeds and no temp files are left behind.

## Manual privileged acceptance (ACCEPTED on Windows 10)

Status: **accepted by the repository owner on 2026-09-23 for Windows 10 22H2.** Windows 11 23H2/24H2 was not run; the owner accepted that gap (GitHub CI covers build and automated tests only, not UAC or elevated apply/undo). This cannot run in CI. The full per-row record, execution IDs and findings are in [storage-parity verification](storage-parity.md#system-management-elevated-acceptance-2026-09-23-utc).

For each row, on each OS: review the change, confirm the native dialog, approve UAC, and check that the result is `Applied`. Then undo it from Change history and check that the prior state is restored. Record the machine, build, date, and outcome.

| # | Change | Apply | Undo / recovery |
| --- | --- | --- | --- |
| 1 | Service start type (e.g. `Fax` → Disabled) | ☑ Win10 | ☑ restores prior start type |
| 2 | HKLM privacy policy (e.g. `advertising-id-policy`) | ☑ Win10 | ☑ value restored/deleted |
| 3 | Microsoft telemetry task disable (e.g. `ceip-consolidator`) | ☑ Win10 | ☑ re-enabled |
| 4 | Windows Update policy on Pro (`no-auto-reboot-with-users`) | ☑ Win10 Pro | ☑ value restored; Home not tested |
| 5 | Firewall rule toggle and one profile off/on | ☑ Win10 | ☑ restored |
| 6 | Hosts line disable | ☑ Win10 | ☑ restore; backup present in `%ProgramData%\SupaDiskaKlinah\hosts-backups` |
| 7 | Machine startup entry disable | ☑ Win10 | ☑ re-enabled |
| 8 | Hibernation off/on (`powercfg` argv only) | ☑ Win10 (on → off) | ☑ restored |
| 9 | Restore point + superseded driver package removal in one plan (one UAC prompt) | ☑ Win10, after the restore-point verification fix | ☑ removal marked irreversible; restore point 157 listed; driver re-added from backup |
| 10 | UAC **cancel** on a helper plan | ☑ every item `Denied`, nothing changed | — |
| 11 | Stale state: change a service externally between review and apply | ☑ `stateChanged`, nothing written | — |
| 12 | Standard changes (HKCU startup, HKCU privacy, power plan, scheduled scan) with no UAC prompt | ☑ no `consent.exe` seen | ☑ |

Acceptance record: **accepted by the owner on 2026-09-23**, Windows 10 Pro 22H2 build 19045.6466 (owner's own PC, not disposable). Windows 11 was not run (gap accepted). Row 9 first failed because Windows reported a restore point that did not exist. The helper now verifies the point before reporting success, and the retest passed.
