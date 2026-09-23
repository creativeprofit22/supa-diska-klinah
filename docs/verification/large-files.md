# Large files and approved recovery extension

Step 3 implementation checkpoint, 7 September 2026. The large-file route joins the read-only disk analyzer. Later features and full native interaction/parity gates remain outstanding.

## Scope and safety adaptation

The native start adapter accepts only a module-bound opaque root ID, bounded traversal depth and validated size/extension/category/sort filters. The renderer retains one page of at most 100 rows and at most 1,000 explicitly selected candidate IDs. Scope/filter/new-scan changes clear selection, ignore late replies and release native authority. Files are never selected automatically.

Native verification uncovered two gaps in the original assumptions: schema-2 storage Recycle Bin execution deliberately refuses pathname fallback, and the permanent command was only a separate native entry point, not a native confirmation dialog. The user approved extending the existing engine with identity-preserving recovery and a genuine permanent-confirmation boundary. No file-identity or protection checks were relaxed.

Storage recovery now uses the existing Quarantine journal fields and history/undo APIs, labelled **app recovery** in the new UI. Only regular files on the recovery store's volume are supported. Atomic rename keeps the original DELETE handle and pinned no-follow destination ancestors, uses a directory-relative native rename, never overwrites a destination and never falls back to copy/delete or shell recycle. Unknown/moved roots, reparse ancestors, changed recovery identity/size/mtime and occupied restore destinations are refused. Directory recovery stays unsupported; empty-folder mutation must remain atomic empty-only removal. Recovery does not free space and is not automatically purged.

The Win32 rename wrapper returned error 87 for its documented RootDirectory form in real tests. The implementation instead uses the native handle-relative call used by Rust std (`library/std/src/sys/fs/windows/dir.rs`, corpus revision `f248f4038796913873f11ca65b1b901e311c8dae`): `NtSetInformationFile(FileRenameInformation)`, with ReplaceIfExists false. Only features of the existing pinned windows-sys dependency were enabled; no new package or journal schema was introduced.

Permanent execution is now wired through an app-owned HWND and a native default-No warning showing native-resolved plan count, bytes, ID and bounded escaped path previews. Denial/error never calls execution; execution still revalidates after confirmation. The callback seam is tested without opening desktop dialogs. Actual dialog interaction remains for native smoke.

## Executed checks

- `cargo test --manifest-path src-tauri/Cargo.toml --locked -p windows-platform cleanup::filesystem::tests --lib -- --test-threads=1`: six tests passed, including identity-preserving move/restore and refusal of collisions, tampering, root replacement, reparse parents and directories.
- Focused engine recovery/confirmation checks reported 64 passed, zero failures, one existing ignored real-Recycle-Bin drill. They cover interrupted intent/accounting, recovery-path validation, collision refusal, no mutation replay and confirmation denial.
- `cargo test --manifest-path src-tauri/Cargo.toml --locked --test large_files_commands`: seven tests passed. Real Tauri IPC on disposable files exercises bounded pages, immutable recovery-plan creation, guarded move, history-compatible execution, undo restoring original contents, stale-plan rejection, denied foreign callers, malformed filters, unchanged-source refusal of shell recycle, and unknown permanent plans rejected before confirmation.
- `pnpm exec vitest run src/features/large-files src/shared/storage src/features/disk-analyzer --pool=threads --maxWorkers=1`: 67 tests passed. Tests include filter limits, explicit selections, stale scope/page/plan responses, cancellation, root lifetime, scan reset, unsupported-Recycle-Bin omission, native-confirmation copy and truthful completed-but-failed outcomes.

New tests were added, not suppressed. The initial positive Recycle Bin expectation was replaced by the explicitly approved recovery contract; a separate regression still requires native shell-recycle refusal and unchanged source data.

## Rendered actual-route evidence

The native integration test exports disposable Raw IPC status, three pages (9/6/4-byte files) and an immutable recovery-plan summary to `.gg/smoke-artifacts/large-files-native.json` before executing/undoing its fixture. After `pnpm build` and `pnpm preview` (strict port 1521), `node scripts/smoke-storage-frontend.mjs --large-files` replays that data against the real `/#/large-files` route in isolated headless Edge.

The final generated result at `.gg/smoke-artifacts/large-files-route/result.json` reports passing desktop/mobile, selection/page replacement, scope/filter/new-scan reset, immutable recovery review, Tab/Shift+Tab/Space/Enter/Escape, focus return, forced colors and 200% text reflow. Mutation attempts and forbidden native calls are both zero. Captures are browser-target screenshots, never desktop captures; the narrow recovery dialog was visually inspected. The fixture labels native pageSize-1 responses replayed below the UI's requested maximum of 100. Permanent summary copy is adapted from the recovery summary and is not native permanent-execution evidence.

## Remaining verification

No real native permanent dialog, Narrator session or second physical volume was exercised here. The existing real-Recycle-Bin drill stays ignored because storage workflows do not use that unsafe fallback. Full workspace/CI gates, all feature routes, pinned upstream parity comparison and final interactive native smoke remain the later plan gates. Same-volume limitations are explicit; cross-volume operations must fail before mutation, never silently copy/delete. No developer caches or installed programs were cleaned.

## Built-app acceptance follow-up: 2026-09-22 UTC

The real native permanent dialog and a second physical volume, listed above as
not exercised, were covered in [built-app acceptance](built-app-acceptance.md).
Against an actual built app and disposable roots: the permanent confirmation
defaulted to No, was cancelled without deleting the target, and then confirmed
to a `purged` outcome with no undo available; same-volume quarantine and undo
restored byte-identical SHA-256 content; and a cross-volume selection was
refused with `recovery_volume_unsupported` before any mutation. Recovered bytes
remain reported as occupied, not reclaimed. Execution-time refusal for a
selection whose identity changed after the snapshot is still unverified.
