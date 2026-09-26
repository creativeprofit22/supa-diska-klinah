# Kudu parity contract

This inventory maps Kudu v2.4.0 at commit `db09e051d0615121e659db187e3799438acbc9e6`. The source of record is [`src/main/ipc/index.ts`](https://github.com/AdventDevInc/kudu/blob/db09e051d0615121e659db187e3799438acbc9e6/src/main/ipc/index.ts). It is a planning contract, not evidence of behavioral parity.

`Contract mapped` means ownership has a destination but no complete compatible module contract is implemented. `Not verified` means complete module equivalence and required acceptance evidence have not been established; bounded source-comparison cases can pass without completing the module. The foundation-only `foundation_status` command is deliberately excluded from parity claims. System-management rows marked `Implemented` became `Verified` when the owner accepted their manual privileged acceptance on Windows 10 22H2 (2026-09-23). Windows 11 was not run.

## System-management coverage

Implemented system-management rows are wired end to end: typed adapters, read-only inventory commands, the shared preview → plan → native confirmation → journal flow, and frontend pages. They are `Verified` by the manual privileged acceptance in [system-management verification](verification/system-management.md), which the owner accepted on 2026-09-23 on Windows 10 22H2. Windows 11 23H2/24H2 elevated acceptance was not run; the owner accepted that gap. See the [user guide](system-management.md) and the [administrator guide](system-management-admin.md).

Adaptations and gaps against Kudu:

- **Startup:** entries can be enabled or disabled through `StartupApproved`. Deleting entries is not offered, and boot trace is out of scope.
- **Drivers:** only superseded, unbound `oem*.inf` packages can be removed, and the removal is irreversible. Driver update scan and install are not offered.
- **Hosts:** a hash-guarded disable/restore of individual lines, with a backup. Kudu only checks for tampering.
- **Scheduler:** Microsoft task toggles live in Privacy. The Scheduler page manages this app's own read-only scan tasks, created through Task Scheduler COM rather than `schtasks` XML.
- **Windows Update policy** (`commands::updates`, `features/updates`) is an addition that uses documented policy values. Kudu's winget Software Updater (`Updates` row) is out of scope and remains contract-mapped under a separate target.
- **Quick optimization** (`commands::optimizer`, `features/optimizer`) composes individually selectable changes from the services, privacy, and power catalogs. It is not Kudu's database optimizer, and it does not port GameMode's Nagle or per-interface TCP tweaks. The `Optimizer` and `GameMode` rows therefore stay contract-mapped.
- Debloater and Software Updater are not part of this phase.

## Registered module parity

| Kudu module | Kudu source module | Target Tauri command module | Target Rust crate/module | Target frontend feature | Implementation status | Verification status |
| --- | --- | --- | --- | --- | --- | --- |
| Cleaner | `system-cleaner.ipc.ts` | `commands::cleaner` | `windows-platform::storage::cleaner` | `features/cleaner` | Implemented | Verified |
| Browser | `browser-cleaner.ipc.ts` | `commands::browser` | `windows-platform::storage::browser` | `features/browser` | Implemented | Verified |
| LargeFiles | `large-file-finder.ipc.ts` | `commands::large_files` | `windows-platform::storage::large_files` | `features/large-files` | Implemented | Verified |
| Duplicates | `duplicate-finder.ipc.ts` | `commands::duplicates` | `windows-platform::storage::duplicates` | `features/duplicates` | Implemented | Verified |
| Memory | `perf-monitor.ipc.ts` | `commands::memory` | `windows-platform::performance` | `features/memory` | Contract mapped | Not verified |
| Startup | `startup-manager.ipc.ts` | `commands::startup` | `windows-platform::startup_items` | `features/startup` | Implemented | Verified |
| Registry | `registry-cleaner.ipc.ts` | `commands::registry` | `windows-platform::registry` | `features/registry` | Contract mapped | Not verified |
| Uninstaller | `program-uninstaller.ipc.ts` | `commands::uninstaller` | `windows-platform::storage::uninstaller` | `features/uninstaller` | Implemented | Verified |
| Drivers | `driver-manager.ipc.ts` | `commands::drivers` | `windows-platform::drivers` | `features/drivers` | Implemented | Verified |
| Network | `network-cleanup.ipc.ts` | `commands::network` | `windows-platform::network` | `features/network` | Contract mapped | Not verified |
| DiskHealth | `perf-monitor.ipc.ts` | `commands::disk_health` | `windows-platform::storage` | `features/disk-health` | Contract mapped | Not verified |
| StorageSense | `disk-analyzer.ipc.ts` | `commands::storage_sense` | `windows-platform::storage` | `features/storage-sense` | Contract mapped | Not verified |
| Battery | `perf-monitor.ipc.ts` | `commands::battery` | `windows-platform::power` | `features/battery` | Contract mapped | Not verified |
| Debloater | `debloater.ipc.ts` | `commands::debloater` | `windows-platform::packages` | `features/debloater` | Contract mapped | Not verified |
| Privacy | `privacy-shield.ipc.ts` | `commands::privacy` | `windows-platform::privacy` | `features/privacy` | Implemented | Verified |
| Optimizer | `database-optimizer.ipc.ts` | `commands::optimizer` | `windows-platform::optimizer` | `features/optimizer` | Contract mapped | Not verified |
| System | `service-manager.ipc.ts` | `commands::services` | `windows-platform::services` | `features/services` | Implemented | Verified |
| Telemetry | `privacy-shield.ipc.ts` | `commands::privacy` | `windows-platform::privacy` | `features/privacy` | Implemented | Verified |
| Notifications | `breach-monitor.ipc.ts` | `commands::notifications` | `windows-platform::notifications` | `features/notifications` | Contract mapped | Not verified |
| PowerPlan | `game-mode.ipc.ts` | `commands::power` | `windows-platform::power` | `features/power` | Implemented | Verified |
| Hosts | `malware-scanner.ipc.ts` | `commands::hosts` | `windows-platform::hosts` | `features/hosts` | Implemented | Verified |
| Restore | `index.ts` restore-point handlers | `commands::restore` | `windows-platform::restore` | `features/restore` | Implemented | Verified |
| Environment | `environment-cleaner.ipc.ts` | `commands::environment` | `windows-platform::environment` | `features/environment` | Contract mapped | Not verified |
| Repair | `disk-analyzer.ipc.ts` | `commands::repair` | `windows-platform::repair` | `features/repair` | Contract mapped | Not verified |
| Scheduler | `privacy-shield.ipc.ts` | `commands::scheduler` | `windows-platform::scheduler` | `features/scheduler` | Implemented | Verified |
| Updates | `software-updater.ipc.ts` | `commands::software_updater` | `windows-platform::software_updater` | `features/software-updater` | Contract mapped | Not verified |
| Firewall | `firewall-audit.ipc.ts` | `commands::firewall` | `windows-platform::firewall` | `features/firewall` | Implemented | Verified |
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
| Restore points | `index.ts:224-232` | `commands::restore` | `windows-platform::restore` | `features/restore` | Implemented | Verified |
| Scan, deletion, and cloud history | `index.ts:234-302` | `commands::history` | `windows-platform::history` | `features/history` | Contract mapped | Not verified |
| Updater operations | `index.ts:304-308` | `commands::self_update` | `windows-platform::self_update` | `features/settings` | Implemented | Not verified |

**Self-update.** Kudu uses `electron-updater` against GitHub Releases: it checks automatically at start, downloads automatically, then calls `quitAndInstall`. **Adopted:** GitHub Releases as the host, and the NSIS installer replacing the whole install. **Changed:** the check is opt-in (off by default); check, download and install are separate user actions; install needs a native confirmation; the manifest is Ed25519-signed with a dedicated key; the installer's size and SHA-256 must match it; and a signed install refuses unsigned or differently signed updates. **Rejected:** automatic check and download, and `electron-updater`'s own HTTP stack. Updates reuse the single fixed-host WinHTTP sink with one allowlisted redirect for the installer asset. See [updates](updates.md). Unit, policy-matrix and real-`WinVerifyTrust` rehearsal evidence is in [release verification](verification/release.md); the row stays `Not verified` until a published end-to-end update is accepted.

## Localization

| Capability | Kudu v2.4.0 | Supa Diska Klinah | Status |
| --- | --- | --- | --- |
| Per-feature string namespaces | i18next namespaces | Typed per-feature `strings.ts` catalogs; a missing Spanish key is a compile error | Adopted |
| OS locale detection with English fallback | i18next language detector | `navigator.languages` (Windows display language) with a Settings override; `es-*` → `es-419`, else English | Adopted |
| Main-process strings | `src/main/i18n.ts` tray strings | Native Windows dialogs from `windows-platform/src/i18n.rs` (`GetUserPreferredUILanguages` or the saved setting) | Adopted |
| Installer language | Default | NSIS English and Spanish, following Windows | Adopted |
| Kudu's other 26 locales | Shipped | Not shipped; the catalog type accepts new languages | Deferred |
| Right-to-left languages | Partial | No RTL language ships; new CSS uses logical properties | Deferred |
| Machine translation (`scripts/translate.js`) | Unreviewed output shipped | Not accepted; every string needs native review ([localization](localization.md#review-rule)) | Rejected |

## Privilege classification

The complete inventory is classified as standard-user, mixed, or helper-only in [`security.md`](security.md). Classification is not permission. The elevated helper accepts only restore-point creation and `ApplySystemChanges`, a batch of the closed, typed `HelperChange` set reviewed in [ADR 0002](adr/0002-system-change-helper.md). It has no shell-string or arbitrary-path operation. Kudu's whole-application elevation route is rejected; scanning and ordinary cleanup remain standard integrity.

Manual temporary-cache cleanup is implemented at standard integrity with opaque Rust-owned plans, final containment revalidation, Windows Recycle Bin undo, app quarantine, delayed opt-in purge, and separate permanent confirmation. Cleanup exposes no privileged or arbitrary-path delete, command, or shell operation. Registry, service, and other system changes exist only as the typed system-management changes above.

Coding-project discovery is implemented as an original read-only extension, not a Kudu-equivalence claim. Explicit saved roots, ID-only management, marker-aware rules for Rust, Node and major frameworks, Python, .NET, Gradle/Maven, CMake, Unity, Unreal, and Godot, nested-project handling, typed rebuild intelligence, aggregate bounds, and conservative unselected results are covered in the [project artifact guide](project-artifacts.md). Project-artifact selection and cleanup remain deliberately unreachable.

Post-build artifact budgets are another original standard-integrity extension, not a Kudu-equivalence claim. Native-approved executable profiles, immutable argv, success-only ownership, stale generation budgets, conservative external analysis, protected incremental state, journaled quarantine, and undo are documented in [build artifact budgets](build-artifact-budgets.md). No build or artifact operation is added to the privileged helper.

## Storage capability parity

This independent inventory expands storage discovery without changing the registered-module inventory above. Sources below are relative to the [pinned Kudu tree](https://github.com/AdventDevInc/kudu/tree/db09e051d0615121e659db187e3799438acbc9e6). All eight scoped application workflows are implemented. Fixture references below name existing tests, not aspirational names. Passing safety/adapter tests is not complete Kudu equivalence: verification remains Not verified until the pinned comparison and installed-app/manual gates are complete.

| Storage feature | Pinned source | Supported behavior | Exclusions and adaptations | Required fixtures | Implementation status | Verification status |
| --- | --- | --- | --- | --- | --- | --- |
| Rule cleaner | `src/main/ipc/system-cleaner.ipc.ts`; `src/main/platform/win32/paths.ts`; `rules/win32/system.json` | Reviewed filesystem catalog, categories, age and exclusions | Native known folders; protected/admin targets excluded; no vendor maintenance commands | `storage_parity::all_six_storage_evidence_variants_round_trip_and_fail_closed`; `src-tauri/crates/windows-platform/src/storage/cleaner_tests.rs::literal_native_recency_protection_and_scope_revalidation` | Implemented | Verified |
| Disk analyzer | `src/main/ipc/disk-analyzer.ipc.ts` | Directory tree, full subtree bytes independent of displayed depth, extension totals | Read-only aggregates; explicit incomplete totals; no SFC, DISM, CHKDSK or TRIM | `storage_parity::step6_analyzer_full_depth_unique_subtrees_extensions_and_limits`; `src-tauri/tests/disk_analyzer_commands.rs::pinned_kudu_analyzer_orders_subtrees_and_extensions_by_descending_bytes` | Implemented | Verified |
| Large files | `src/main/ipc/large-file-finder.ipc.ts` | Size bounds, extension/category filters, sorting and bounded depth | Unselected personal files; snapshot IDs and fresh identity evidence, never raw-path deletion | `storage_parity::step6_large_files_filters_sorting_evidence_and_bounded_prefix`; `src-tauri/tests/large_files_commands.rs::large_file_real_ipc_pages_immutable_recovery_plan_execute_and_undo` | Implemented | Verified |
| Duplicates | `src/main/ipc/duplicate-finder.ipc.ts` | Size groups, first-4-KiB hash, full SHA-256 and cancellable progress | Collapse hard links; protect independently retained copy throughout mutation; equal hashes alone grant no deletion authority | `storage_parity::duplicate_rows_bind_group_keeper_counts_and_evidence`; `src-tauri/tests/duplicate_empty_commands.rs::pinned_kudu_duplicate_maximum_and_extension_filters_reach_native_discovery` | Implemented | Verified |
| Empty folders | `src/main/ipc/empty-folder-cleaner.ipc.ts` | Bottom-up recursive emptiness, exclusions and deepest-first execution | Root survives; hidden, protected, inaccessible and link-like content blocks parents; only race-safe empty-only mutation | `storage_parity::storage_walk_propagates_skipped_contents_to_every_ancestor`; `src-tauri/crates/windows-platform/src/storage/empty_folders_tests.rs::empty_folders_native_late_child_survives_and_recycle_has_no_fallback` | Implemented | Verified |
| Application uninstaller | `src/main/ipc/program-uninstaller.ipc.ts`; `src/main/services/program-uninstaller.ts` | Installed program inventory, separate vendor uninstall and post-uninstall leftovers | Native registry evidence; no renderer commands, ambiguous executables, fuzzy deletion authority or shared-root cleanup; vendor uninstall is not undoable | `storage_parity::storage_requests_reject_unknown_fields_and_limits`; `src-tauri/crates/windows-platform/src/storage/vendor_jobs_tests.rs::restart_never_replays_and_unknown_cannot_expire_or_release` | Implemented | Verified |
| Browser cache cleanup | `src/main/ipc/browser-cleaner.ipc.ts`; `src/main/services/chromium-cache.ts`; `rules/win32/browsers.json` | Profile/shared caches, Opera layouts, Firefox/forks and descendant recency | No cookies, passwords, bookmarks, history, sessions, extensions or entire profiles; close active browsers; service-worker caches require opt-in | `storage_parity::all_six_storage_evidence_variants_round_trip_and_fail_closed`; `src-tauri/tests/browser_commands.rs::browser_real_ipc_catalog_scopes_and_opaque_authorization_without_scanning` | Implemented | Verified |
| Drive inventory | `src/main/ipc/disk-analyzer.ipc.ts` | Fixed-drive label, total/free/used capacity and system-drive identity | Native Windows APIs instead of PowerShell; other drive types excluded; disconnected or inaccessible volumes reported explicitly | `storage_parity::storage_paging_is_stable_bounded_expiring_and_snapshot_bound`; `src-tauri/tests/drive_commands.rs::ipc_native_inventory_executes_and_releases_gate` | Implemented | Verified |

### Reviewed Windows rule inventory

The pinned `src/main/platform/win32/paths.ts` imports eight catalogs: `system.json`, `apps.json`, `gaming.json`, `gpu-cache.json`, `browsers.json`, `misc.json`, `steam.json` and `databases.json` under `rules/win32/`. These catalogs were inspected completely; their exact product/path entries remain the translation source rather than a generalized cache glob. Steam and database maintenance do not authorize new commands or database rewriting in this phase.

- System targets include temporary/service files, prefetch, logs/traces, Explorer/font/shader/Internet caches, updates/Delivery Optimization, WER/dumps, rollback, event logs, Defender, Windows.old, .NET/RDP/shell caches, reliability/search/diagnostics/power/WinSAT/AppCompat, certificates/qWAVE/peer-networking and USO logs. Protected or administrative targets remain excluded; MEMORY.DMP and energy-report.html are individual-file entries, not permission to remove their parents.
- Application targets span desktop/Electron, development/package managers, media, messaging, cloud clients, AI tools, Store packages, updater and WebView2 caches. Preserve literal roots, one-level wildcard plus child directory, bounded anchored recursion and direct-file allowlists separately. Cloud-client caches are not permission for cloud cleanup. Legacy Spotify storage, Telegram user_data and AWS credential caches are excluded pending evidence that personal/offline/credential data cannot be removed.
- Gaming catalogs cover Steam, Epic, EA/Origin, Ubisoft, GOG, Battle.net, Riot, Xbox, Rockstar, itch, Minecraft, Roblox, Valorant, Fortnite, Amazon and Overwolf. GPU entries cover NVIDIA, AMD, Intel and Unity caches. Only reviewed filesystem targets qualify; vendor maintenance commands do not.
- Browser rules describe thirteen Chromium brands, Default/Profile layouts, direct Opera/GX bases, seven profile and seven shared cache layouts. Firefox uses profile/cache2/entries; LibreWolf, Waterfox, Floorp and Zen have separate roaming profile and local cache roots in the browser catalog. Personal browser databases and whole profiles are excluded regardless of upstream catalog placement.
- `src/main/services/file-utils.ts` uses default 60-minute recency; optional descendant recency is bounded at depth eight and 250,000 entries. Application retention includes 1/7/30 days. Updater files require 14 days, exact installer.exe/current.blockmap names, an updater directory and no pending directory. This implementation must treat incomplete recency evidence as ineligible rather than infer inactivity.
- Large-file discovery defaults upstream to 10 MiB, depth 20 and top 500. Its pinned IPC scan applies minimum size but no backend category, maximum-size or extension filter. Backend category/maximum-size/extension filters and paging are explicit adaptations. Duplicate discovery defaults to 1 MiB, supports maximum size/extensions and hashes size groups in two stages. Both use case-insensitive directory-name exclusions.
- Uninstaller inventory uses three upstream registry roots and heuristic Prefetch usage. Native 32/64-bit HKLM/HKCU inventory is the adaptation; unavailable usage remains unknown. Shared-root/name/publisher leftover heuristics never become deletion authority. Registry force-removal is excluded.

#### Behavioural comparison outcomes (2026-09-23)

Now matched after the pinned comparison: duplicate groups rank by reclaimable bytes, largest first, so retention limits drop the least valuable groups; uninstaller inventory hides `SystemComponent = 1` entries (a malformed flag fails that entry closed); empty-folder discovery and pre-mutation revalidation apply Kudu's protected folder names (for example `node_modules`, `.git`, `__pycache__`, `appdata`) and protect Desktop, Documents, Downloads, Pictures, Videos, Music and OneDrive directly under the native profile.

Deliberate adaptations, not gaps:

- Browser cache cleanup refuses while any supported browser is running; it never closes browsers. Only numeric `Profile N` Chromium directories are accepted as profiles.
- Rule cleaner uses a fixed 60-minute recency window; secure delete and `*.ext` pattern exclusions are not offered. Removal is reversible quarantine or Recycle Bin, or separately confirmed permanent deletion.
- Large files, duplicates and empty folders offer no user directory exclusion list; the fixed protection policy, machine roots and hidden/system attributes decide. Cancel discards partial results rather than presenting an incomplete list.
- Uninstaller entries without a vendor uninstall command stay listed and are refused at launch.
- Disk analyzer extension totals cover the whole scanned subtree rather than Kudu's depth-4 sample, and fixed machine roots (Windows, Program Files, ProgramData) are excluded from walks.
- No reveal-in-Explorer action; the renderer receives snapshot IDs, never raw paths to open.

Closed on 2026-09-23 (previously listed as still to confirm):

- Drive inventory now pages by mount letter (`C:\` before `D:\`), matching Kudu; it was ordered by volume label.
- The disk analyzer accepts a whole drive root, as Kudu does. The Windows folder and machine roots stay refused as roots and are skipped inside the walk, so whole-drive totals exclude them (the adaptation above).
- Storage cleanup reports a missing, in-use (sharing or lock violation) or access-denied item per item as `not-found`, `in-use` or `permission-denied` and still cleans the other items, matching Kudu. Plan creation still refuses such items, and duplicate members keep their all-or-nothing group guard.

Upstream defects are not parity requirements: silent inaccessible totals, hard-link overcounting, ordinary shallow recency, raw-path deletion, trash-after-check races, protected empty children failing to block ancestors, and mixed-case event-log exclusions are not retained. The table remains **Not verified**: app/runtime fixtures exist, but the final installed-app, manual-accessibility and full pinned behavioral comparison gates are incomplete. See [storage verification](verification/storage-parity.md) for per-criterion evidence and limits.

Shared acceptance fixtures must cover cancellation during traversal and hashing, overlap rejection, stable bounded paging, invalid/expired cursors, scan caps, UNC/device/ADS/link/cloud rejection, stale rules and IDs, schema-1 compatibility, journal failures, interrupted recovery, replay and restore collisions. Native race fixtures use disposable roots only. UI smoke must exercise all eight routes, keyboard selection, confirmation, cancellation, paging and restart/undo without touching installed programs or actual browser profiles.

### Approved recovery adaptation and confirmed source checks

The user approved extending the existing cleanup engine with same-volume, file-only app recovery and guarded undo, because native storage deletion deliberately refuses a pathname Recycle Bin fallback. Storage directories remain permanent-only atomic empty-only removals. App recovery is not Windows Recycle Bin parity, does not free space, and is not automatically purged. Native permanent/vendor confirmations now use native-resolved operations and default No. This is an explicit safety adaptation, not a renamed exclusion for missing behavior.

The pinned analyzer source was re-read and its descending subtree/extension size order exposed a real integration gap. The new native regression failed before correcting the ascending snapshot sort keys. The pinned duplicate source applies maximum size and extension filtering before grouping; these controls are now exposed through the typed adapter and UI, with a native source-guided fixture. These bounded source-guided checks do not establish all-eight-workflow equivalence. UI defaults now match the pinned IPC thresholds and traversal defaults: large files start at 10 MiB, duplicates at 1 MiB, and both plus empty folders start at depth 20. Traversal/resource caps, extension normalization, one-group duplicate selection and first-copy keeper reservation are documented adaptations.

### Pinned storage acceptance cases (2026-09-08 UTC)

The independent reference check `node scripts/check-pinned-storage-source.mjs` passed against the pinned commit. It fetched JSON only, with a per-request timeout, response-size cap and redirects refused; no upstream JavaScript, commands or installers were executed. Source-byte hashes and canonical declaration hashes are retained in `scripts/fixtures/pinned-cleaner-declarations.json`. Default CI checks are offline and do not regenerate expected values from local implementation output.

`pnpm check:parity` now runs 12 Node contract tests in addition to matrix validation:

- **584 cleaner declarations:** apps 334, GPU 9, system 55, gaming 39, misc 16, Steam 23 and databases 108. Apps/GPU/gaming compare paths, explicit ages, one-child/updater matching, filenames, blockers and recursive exclusions/depth. System compares all paths and single-file declarations. Misc compares protected log names and preserves null-path Recycle Bin adaptation. Steam and databases compare declared discovery/maintenance targets while separately requiring their existing unsupported reasons. Descriptions, runtime discovery, permission behavior and deletion semantics are not certified by declaration hashes.
- **Browser layout fields:** all 13 Chromium base paths, Firefox plus four forks, all seven profile caches and seven shared caches, and absent Windows Safari are compared with a separately transcribed pinned fixture. Native profile/activity/private-data tests remain necessary; path comparison does not grant deletion authority.
- **Default controls:** frontend tests reproduce and fix drift from the pinned IPC defaults: large files 10 MiB and depth 20; duplicates 1 MiB and depth 20; empty folders depth 20. Explicit configured limits remain bounded at 64.
- **Depth boundary:** `storage_parity::step6_pinned_large_file_inclusive_depth_size_and_order` reproduces the missing deepest-allowed-directory file, then verifies inclusion at the size/depth boundary and descending order. Shared traversal now bounds directory recursion rather than excluding files in the last permitted directory. Deeper directories still produce `DepthLimit`, and identity/protection/record limits remain enforced. Existing core/native/IPC tests passed after the correction.

These are command-backed comparisons within named scopes. On 2026-09-23 the project owner accepted the storage phase on the pinned behavioural comparison, native tests, and the built-app acceptance rerun (see `docs/verification/storage-parity.md`). The eight storage rows and the five implemented storage modules are therefore marked Verified. That acceptance waived Narrator and left complete keyboard-only flows untested. The permanent-deletion default button was established by a same-flags probe, not by measuring the app's own dialog. The release phase keeps its own accessibility and end-to-end criteria.

## Protection capability parity

Kudu's protection features rely on its cloud account and on trust by folder name. [ADR 0003](adr/0003-local-first-protection.md) replaces them with local, signer-aware evidence and signed rule packs; see [protection](protection.md) for limits. Fixtures name existing Rust tests. The manual Windows acceptance in [protection verification](verification/protection.md) was accepted by the owner on 2026-09-24 on Windows 10 22H2; Windows 11 was not tested.

| Protection feature | Pinned Kudu behavior | Supported behavior | Exclusions and adaptations | Required fixtures | Implementation status | Verification status |
| --- | --- | --- | --- | --- | --- | --- |
| Process inspection | Process list with termination of non-protected processes | Read-only list with image path, parent, offline signer and heuristic flags | No termination, suspension or memory reads; command lines reported unavailable; Task Manager for control | `src-tauri/crates/windows-platform/src/protection/process.rs::inventory_includes_this_process_with_its_image`; `src-tauri/crates/windows-platform/src/protection/process.rs::deleted_image_is_flagged_and_denied_access_is_unavailable` | Implemented | Verified |
| Suspicious-file scan | Cloud YARA rules, file-name list, trust by folder name | Quick and picked-folder scans; SHA-256, byte-pattern and file-name rules; signer-aware heuristics; typed evidence | No YARA; no folder-name trust; links never followed; unreadable items reported as not checked | `src-tauri/crates/windows-platform/src/protection/scan/tests.rs::detects_marker_deterministically_and_heuristics_separately`; `src-tauri/crates/windows-platform/src/protection/scan/tests.rs::junctions_are_never_followed`; `src-tauri/crates/windows-platform/src/protection/scan/tests.rs::oversized_locked_and_cancelled_files_are_unavailable_not_clean` | Implemented | Verified |
| Quarantine | Raw payloads with a trusted JSON manifest; restore may overwrite | Native-derived paths, neutered payload, hash-checked create-new restore, confirmed delete, startup reconciliation | Native confirmation for every action; collisions refused, never overwritten | `src-tauri/crates/windows-platform/src/protection/quarantine/tests.rs::restore_collision_never_overwrites`; `src-tauri/crates/windows-platform/src/protection/quarantine/tests.rs::ids_cannot_traverse`; `src-tauri/crates/windows-platform/src/protection/quarantine/tests.rs::junction_inside_quarantine_root_is_refused_and_target_untouched` | Implemented | Verified |
| Signature updates | Automatic 6-hourly download, integrity-checked only by a server-supplied hash | Ed25519-signed packs, monotonic sequence, atomic install, previous-pack restore, embedded baseline; opt-in download | Manual import or opt-in download only; disabled in release builds while only the test key exists | `src-tauri/crates/windows-platform/src/protection/rules_store/tests.rs::interruption_at_every_step_keeps_a_working_pack`; `src-tauri/crates/windows-platform/src/protection/rules_store/tests.rs::rollback_is_refused_and_restore_previous_is_explicit`; `src-tauri/crates/windows-platform/src/protection/updates.rs::policy_off_makes_zero_network_calls` | Implemented | Verified |
| Breach check | E-mail breach monitoring through the Kudu cloud | Opt-in Pwned Passwords range check sending five SHA-1 hex characters with padding | E-mail monitoring unsupported; password never stored or logged | `src-tauri/crates/windows-platform/src/protection/breach.rs::only_the_five_character_prefix_leaves_the_device`; `src-tauri/crates/windows-platform/src/protection/breach.rs::policy_off_sends_nothing` | Implemented | Verified |
| Defender and AMSI | Not present | Opt-in AMSI query and read-only Defender detection history, labelled external | Absence of a provider or answer is reported as not checked, never as a clean result | `src-tauri/crates/windows-platform/src/protection/amsi.rs::clean_is_never_reported_as_a_verdict`; `src-tauri/crates/windows-platform/src/protection/defender_history.rs::history_is_read_only_or_honestly_unavailable` | Implemented | Verified |
| Kudu cloud features | CVE feed, startup and program safety ratings, e-mail breach monitor | Unsupported by design | Would send inventories or identity off the device; replaced by local signer and location evidence | `src-tauri/crates/windows-platform/src/protection/service/tests.rs::defaults_are_offline_and_network_features_make_zero_calls` | Implemented | Verified |

## Updating this contract

Add or rename a row only after reviewing the pinned upstream revision. A behavior becomes `Implemented` only when its command contract exists. It becomes `Verified` only after a parity test exercises equivalent success, failure, validation, and safety behavior.
