# Read-only disk analyzer checkpoint

7 September 2026. Step 3 is **partially complete**: this checkpoint implements only the disk analyzer. It adds no large-file workflow, cleanup action, storage plan review, delete, move, or undo control to that route.

## Implemented boundary and route

- `start_disk_analyzer` accepts Raw UTF-8 JSON containing only an opaque `rootId` and integer `displayedDepth` (0–64). Unknown fields, arbitrary paths, caller-supplied limits and invalid filters are rejected before starting work. Rust chooses the existing analyzer runner and fixed native limits; no renderer runner names cross IPC.
- It reuses the existing long-lived storage service, native root selection, module-bound single-use authorizations, protection policy, bounded worker, snapshot status, paging, cancellation and release. Shared parsing/error helpers are visible only to sibling adapters. The new command is registered in the real app manifest and local main-window capability.
- `/disk-analyzer` is registered in the actual hash router and shell navigation. It reuses shared storage status and paging, with at most one page of 100 rows. Breadcrumb metadata is bounded to the native maximum depth plus the root.
- Folder depth controls displayed rows, not full measured subtree totals. The route exposes root and current-folder logical/allocated totals, independent-file and hard-link counts, child-folder pages, breadcrumbs, and extension aggregates across the scanned root. Unknown allocation and partial totals are not converted into zero or complete totals.
- Folder selection cancellation preserves existing results. Scope/filter changes, scan cancellation, late replies and expired paging do not restore old totals. Unused root authorizations are released on replacement, unmount and failed/busy starts. Rescanning requires a new native folder selection because root authorizations are single-use.

## Disposable native fixture evidence

New `src-tauri/tests/disk_analyzer_commands.rs` tests include the real app command adapters and use Tauri's generated capability context, not replacement command handlers:

| Test | Observed result |
| --- | --- |
| `analyzer_real_ipc_full_subtree_totals_bounded_pages_extensions_and_no_mutation` | Disposable root with 205 child folders, 207 independent files and 222 logical bytes. Display depth 1 still includes the deeper seven-byte file; `d000` totals eight bytes. Child pages contain 100, 100 and 5 rows. Extension totals equal 222 bytes for this fixture without hard links. Analyzer plan creation is refused, and fixture contents remain unchanged. |
| `analyzer_rejects_untrusted_filters_paths_modules_and_foreign_callers` | Invalid/unknown fields, path input, out-of-range filters and wrong-module roots fail. Foreign windows and remote origins are denied by actual IPC permissions. |
| `analyzer_snapshot_ids_cursors_and_parent_nodes_cannot_cross_scans_or_survive_release` | Cross-snapshot parent/cursor IDs and released snapshots are rejected; the other live snapshot remains usable. |
| `analyzer_cancellation_discards_native_results_and_revokes_authority` | Real native discovery is paused at a deterministic publication seam, then cancelled through the actual IPC command. The resulting snapshot is Cancelled and contains no rows. This tests cooperative cancellation without a timing-dependent large fixture. |

The totals test exports actual native status and page response JSON to `.gg/smoke-artifacts/analyzer-native.json` for rendering checks. Only disposable fixture data is exported. The native fixture directory is removed afterward; the JSON is replay data, not live authority.

Existing analyzer/storage tests also passed, including deep totals, hard links, unknown allocation/protection, native traversal limits and cancellation. Existing storage regression tests may mutate their own isolated test data; no cleanup was run against developer folders or through the analyzer route.

## Frontend and rendered evidence

Eight new frontend tests cover native-shaped totals, unknown allocation, global extensions, breadcrumbs/focus, validated opaque start input, busy-start root release, picker cancellation, scan cancellation plus a late status, a late page after scope replacement, expired-page hiding, and late picker release. Test transport rejects unrelated/mutation commands. Existing assertions were not removed or silenced. A draft fixture was corrected to the native extension representation (bare `dat`); the visible `.dat` assertion was retained. A cross-realm byte assertion was corrected to still require Uint8Array without incorrectly requiring jsdom's constructor identity.

After `pnpm build` and `pnpm preview` (strict port 1521), `node scripts/smoke-storage-frontend.mjs --analyzer` drives the **actual app route** at `/#/disk-analyzer` in headless Edge using replay of the native fixture JSON. It does not substitute a demonstration page for the route. A visible banner identifies fixture replay; only the read-only storage commands are serviced. Unknown/mutation calls throw and the final forbidden-call count must be zero.

Actual rendered checks passed:

- Desktop 1280 × 1000: folder selection, keyboard entry of display depth 1, 222-byte/207-file root totals, 100/100/5 paging, deeper subtree total, empty depth-limited children, global extensions and no cleanup controls.
- Mobile 320 × 850: actual shell/route reflow, breadcrumb keyboard navigation and heading focus, preserved context after picker cancellation, scan cancellation clearing totals. Horizontal overflow is checked against client width, including scrollbar space.
- Forced-colors partial state and 200% text at 640 × 900: no horizontal overflow; warnings and controls remain visible. This is not a claim of testing every Windows DPI setting.
- The original shared-storage headless scenario still passes; its assertions were retained when the analyzer scenario was added.

Captures and `result.json` are under `.gg/smoke-artifacts/analyzer-route/` (ignored generated evidence). Representative reviewed images: `desktop-analyzer.png`, `desktop-folder-results.png`, `desktop-extensions.png`, `mobile-analyzer.png`, `mobile-child-keyboard.png`, `mobile-cancelled.png`, `forced-colors-partial.png`, `text-200-percent.png`. Screenshots are browser-target captures, never whole-desktop captures.

## Separate bounded checks

- `pnpm check`: passed the existing checks, including architecture, security boundaries, documentation, TypeScript and frontend build.
- `pnpm test`: 123 tests passed (115 pre-existing plus eight analyzer tests).
- `cargo test --manifest-path src-tauri/Cargo.toml --locked --test disk_analyzer_commands --test storage_commands --test drive_commands`: 18 tests passed.
- `cargo test --manifest-path src-tauri/Cargo.toml --locked -p windows-platform storage::`: 70 tests passed.

## Still unverified / intentionally excluded

No live native-picker interaction on this route, Narrator session, installed-app route smoke, or pinned-upstream parity comparison was performed in this checkpoint. Native IPC fixture tests and actual-route fixture replay are complementary evidence, not a claim of one uninterrupted installed-app end-to-end run. Browser partial/slow states are controlled simulations; normal totals/status/pages come from the native export. Node 22.20.0 was used rather than the pinned Node 24.19.0, producing the existing engine warning. The large-file and cleanup portions of step 3 remain unimplemented.
