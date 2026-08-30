# Cleanup execution and recovery

## Preview and plan

Preview scans fixed Windows temporary-cache locations and displays paths for review. Selecting items creates an immutable Rust-owned plan from random candidate identifiers. Renderer requests never contain mutation paths, roots, rules, quarantine locations, or deletion primitives.

A preview is not a backup. Immediately before each item changes, Rust reloads the plan as untrusted data, recompiles current protections, rejects reparse points, proves strict scan and marker-context containment, confirms type and Windows identity, repeats rule, marker, exclusion, age, and activity checks, and remeasures occupancy. A failed item is recorded without weakening checks for siblings.

## Manual cleanup and undo

Manual cleanup defaults to the Windows Recycle Bin. The app compares Recycle Bin listings before and after each deletion and records the exact new Windows trash identifier, original path, and deletion time. Undo restores only that exact identifier and path. A missing item or destination collision fails visibly; same-name neighbors are never substituted.

“Delete permanently” uses a separate command and second irreversible warning. It is unavailable through normal execution and has no app undo. The app remains at standard integrity and does not invoke a shell or elevated helper.

## Automatic quarantine

Automatic cleanup is disabled by default. Enabling it runs maintenance at startup and immediately after opt-in. Eligible items first enter app-managed quarantine for the configured 1–30 day grace period. Disabling automatic cleanup stops future quarantine and purge. Due purge runs only while the policy remains enabled.

Same-volume quarantine uses atomic rename. Cross-volume quarantine stages a bounded no-follow copy, verifies its shape and size, publishes it, revalidates the source, then removes the source. Ambiguous crash states preserve data rather than assuming success.

## Interrupted work

Plans and per-item journals are stored as bounded versioned JSON beneath the application-data cleanup directory. Writes use create-new temporary files, flush, and atomic replacement. A journal is persisted before and after every side effect.

At startup, pending journals are reconciled. If a source remains, validation can retry. If the source is absent and its quarantine payload exists, the item is recorded as quarantined. If neither exists, the item becomes failed or unknown; it is never silently marked successful. Completed items are skipped.

These records improve operation recovery but are not backups. Keep separate backups for important files.

## Byte accounting

History reports distinct values:

- `selectedBytes`: logical preview bytes selected.
- `processedBytes`: logical bytes whose mutation was attempted after final validation.
- `failedBytes`: logical bytes rejected or failed.
- `quarantinedBytes`: allocated bytes currently retained by the app.
- `purgedBytes`: allocated bytes permanently removed.
- `occupiedBytes`: allocated occupancy measured immediately before mutation.
- `reclaimedBytes`: non-negative observed free-space increase, capped at occupied bytes.

Recycle Bin and recoverable quarantine report zero reclaimed bytes because content still occupies storage. `reclaimedBytes` is a conservative filesystem observation, not proof that cleanup alone caused the free-space change while other processes use the disk.
