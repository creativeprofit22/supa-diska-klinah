# Kudu parity contract

This inventory maps Kudu v2.4.0 at commit `db09e051d0615121e659db187e3799438acbc9e6`. The source of record is [`src/main/ipc/index.ts`](https://github.com/AdventDevInc/kudu/blob/db09e051d0615121e659db187e3799438acbc9e6/src/main/ipc/index.ts). It is a planning contract, not evidence of behavioral parity.

`Contract mapped` means ownership has a destination but no complete compatible module contract is implemented. `Not verified` means no Kudu-equivalence test has passed. The foundation-only `foundation_status` command is deliberately excluded from parity claims. A narrow restore-point creation command now exists, but the broader Restore module remains contract-mapped and unverified until parity behavior is exercised.

## Registered module parity

| Kudu module | Kudu source module | Target Tauri command module | Target Rust crate/module | Target frontend feature | Implementation status | Verification status |
| --- | --- | --- | --- | --- | --- | --- |
| Cleaner | `system-cleaner.ipc.ts` | `commands::cleaner` | `windows-platform::filesystem` | `features/cleaner` | Contract mapped | Not verified |
| Browser | `browser-cleaner.ipc.ts` | `commands::browser` | `windows-platform::browser` | `features/browser` | Contract mapped | Not verified |
| LargeFiles | `large-file-finder.ipc.ts` | `commands::large_files` | `windows-platform::filesystem` | `features/large-files` | Contract mapped | Not verified |
| Duplicates | `duplicate-finder.ipc.ts` | `commands::duplicates` | `windows-platform::filesystem` | `features/duplicates` | Contract mapped | Not verified |
| Memory | `perf-monitor.ipc.ts` | `commands::memory` | `windows-platform::performance` | `features/memory` | Contract mapped | Not verified |
| Startup | `startup-manager.ipc.ts` | `commands::startup` | `windows-platform::startup` | `features/startup` | Contract mapped | Not verified |
| Registry | `registry-cleaner.ipc.ts` | `commands::registry` | `windows-platform::registry` | `features/registry` | Contract mapped | Not verified |
| Uninstaller | `program-uninstaller.ipc.ts` | `commands::uninstaller` | `windows-platform::packages` | `features/uninstaller` | Contract mapped | Not verified |
| Drivers | `driver-manager.ipc.ts` | `commands::drivers` | `windows-platform::drivers` | `features/drivers` | Contract mapped | Not verified |
| Network | `network-cleanup.ipc.ts` | `commands::network` | `windows-platform::network` | `features/network` | Contract mapped | Not verified |
| DiskHealth | `perf-monitor.ipc.ts` | `commands::disk_health` | `windows-platform::storage` | `features/disk-health` | Contract mapped | Not verified |
| StorageSense | `disk-analyzer.ipc.ts` | `commands::storage_sense` | `windows-platform::storage` | `features/storage-sense` | Contract mapped | Not verified |
| Battery | `perf-monitor.ipc.ts` | `commands::battery` | `windows-platform::power` | `features/battery` | Contract mapped | Not verified |
| Debloater | `debloater.ipc.ts` | `commands::debloater` | `windows-platform::packages` | `features/debloater` | Contract mapped | Not verified |
| Privacy | `privacy-shield.ipc.ts` | `commands::privacy` | `windows-platform::privacy` | `features/privacy` | Contract mapped | Not verified |
| Optimizer | `database-optimizer.ipc.ts` | `commands::optimizer` | `windows-platform::optimizer` | `features/optimizer` | Contract mapped | Not verified |
| System | `service-manager.ipc.ts` | `commands::system` | `windows-platform::system` | `features/system` | Contract mapped | Not verified |
| Telemetry | `privacy-shield.ipc.ts` | `commands::telemetry` | `windows-platform::privacy` | `features/telemetry` | Contract mapped | Not verified |
| Notifications | `breach-monitor.ipc.ts` | `commands::notifications` | `windows-platform::notifications` | `features/notifications` | Contract mapped | Not verified |
| PowerPlan | `game-mode.ipc.ts` | `commands::power_plan` | `windows-platform::power` | `features/power-plan` | Contract mapped | Not verified |
| Hosts | `malware-scanner.ipc.ts` | `commands::hosts` | `windows-platform::network` | `features/hosts` | Contract mapped | Not verified |
| Restore | `index.ts` restore-point handlers | `commands::restore` | `windows-platform::restore` | `features/restore` | Contract mapped | Not verified |
| Environment | `environment-cleaner.ipc.ts` | `commands::environment` | `windows-platform::environment` | `features/environment` | Contract mapped | Not verified |
| Repair | `disk-analyzer.ipc.ts` | `commands::repair` | `windows-platform::repair` | `features/repair` | Contract mapped | Not verified |
| Scheduler | `privacy-shield.ipc.ts` | `commands::scheduler` | `windows-platform::scheduler` | `features/scheduler` | Contract mapped | Not verified |
| Updates | `software-updater.ipc.ts` | `commands::updates` | `windows-platform::updates` | `features/updates` | Contract mapped | Not verified |
| Firewall | `firewall-audit.ipc.ts` | `commands::firewall` | `windows-platform::firewall` | `features/firewall` | Contract mapped | Not verified |
| ContextMenu | `context-menu-cleaner.ipc.ts` | `commands::context_menu` | `windows-platform::shell` | `features/context-menu` | Contract mapped | Not verified |
| Gpu | `gaming-cleaner.ipc.ts` | `commands::gpu` | `windows-platform::graphics` | `features/gpu` | Contract mapped | Not verified |
| BootTrace | `startup-manager.ipc.ts` | `commands::boot_trace` | `windows-platform::startup` | `features/boot-trace` | Contract mapped | Not verified |
| RegistryBackup | `registry-cleaner.ipc.ts` | `commands::registry_backup` | `windows-platform::registry` | `features/registry-backup` | Contract mapped | Not verified |
| CloudCleanup | `cloud-agent.ipc.ts` | `commands::cloud_cleanup` | `windows-platform::cloud` | `features/cloud-cleanup` | Contract mapped | Not verified |
| FileShredder | `file-shredder.ipc.ts` | `commands::file_shredder` | `windows-platform::filesystem` | `features/file-shredder` | Contract mapped | Not verified |
| GameMode | `game-mode.ipc.ts` | `commands::game_mode` | `windows-platform::gaming` | `features/game-mode` | Contract mapped | Not verified |

## Direct handler parity

These groups cover handlers implemented directly in Kudu's `index.ts` rather than delegated registrars.

| Direct handler group | Kudu source | Target Tauri command module | Target Rust crate/module | Target frontend feature | Implementation status | Verification status |
| --- | --- | --- | --- | --- | --- | --- |
| Cleaner location and blockers | `index.ts:93-104` | `commands::cleaner` | `windows-platform::filesystem` | `features/cleaner` | Contract mapped | Not verified |
| Platform information | `index.ts:106-120` | `commands::platform` | `windows-platform::status` | `features/dashboard` | Contract mapped | Not verified |
| Settings and backup directory | `index.ts:122-167` | `commands::settings` | `windows-platform::settings` | `features/settings` | Contract mapped | Not verified |
| Onboarding | `index.ts:169-176` | `commands::onboarding` | `windows-platform::settings` | `features/onboarding` | Contract mapped | Not verified |
| Elevation | `index.ts:178-222` | `commands::elevation` | `windows-platform::elevation` | `features/elevation` | Contract mapped | Not verified |
| Restore points | `index.ts:224-232` | `commands::restore` | `windows-platform::restore` | `features/restore` | Contract mapped | Not verified |
| Scan, deletion, and cloud history | `index.ts:234-302` | `commands::history` | `windows-platform::history` | `features/history` | Contract mapped | Not verified |
| Updater operations | `index.ts:304-308` | `commands::updater` | `windows-platform::updates` | `features/updates` | Contract mapped | Not verified |

## Privilege classification

The complete inventory is classified as standard-user, mixed, or helper-only in [`security.md`](security.md). Classification is not permission. Only restore-point creation is currently approved for the elevated helper. Kudu's whole-application elevation route is rejected; scanning and ordinary cleanup remain standard integrity.

Manual temporary-cache cleanup is implemented at standard integrity with opaque Rust-owned plans, final containment revalidation, Windows Recycle Bin undo, app quarantine, delayed opt-in purge, and separate permanent confirmation. No privileged or arbitrary-path delete, command, registry, service, or shell operation is exposed.

## Updating this contract

Add or rename a row only after reviewing the pinned upstream revision. A behavior becomes `Implemented` only when its command contract exists. It becomes `Verified` only after a parity test exercises equivalent success, failure, validation, and safety behavior.
