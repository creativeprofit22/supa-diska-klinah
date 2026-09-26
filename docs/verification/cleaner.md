# Rule cleaner integration checkpoint

Step 4, 7 September 2026. The route uses native catalog scope IDs, not typed paths. One scope is authorized/scanned/reviewed at a time; switching scopes releases old authority and clears results. Multi-root work is sequential, not atomic. Results require explicit selection and reuse existing storage plans, same-volume app recovery/undo and separately native-confirmed permanent execution. No automatic cleanup was expanded.

The old scope helper exposed only four bare known folders, which did not cover the catalog's exact bound roots. It now derives and deduplicates roots through the same native compiled-target binder used by discovery. Unsupported/missing targets remain visible and non-authorizable. The inventory is bounded by the fixed compiled catalog with a hard ceiling of 1024, not silently truncated to 64. Individual rule provenance, revisions, age, exclusions, matcher and unsupported reasons are returned through `list_cleaner_catalog`; the UI does not invent per-file rule attribution absent from native FileRecord output. Native scopes and catalog metadata render in 20-entry pages.

Native scope refresh still invalidates previous IDs. The shared frontend serializes these requests, joins StrictMode duplicates, and retains only one latest queued request, preventing out-of-order native refreshes from invalidating the newest route's inventory. A refresh control reloads expired/unavailable scope state. The shared Raw IPC decoder additionally rejects positional JSON arrays: serde otherwise accepts an empty array for an empty named input struct.

Actual checks:

- `cargo test --manifest-path src-tauri/Cargo.toml --locked --test cleaner_commands`: two real-IPC tests passed. Typed response fields round-trip against the compiled catalog; scope IDs are opaque/module-bound; unavailable scopes, malformed/unknown input and foreign callers are rejected. This lists and authorizes scopes but does not scan developer caches.
- `cargo test --manifest-path src-tauri/Cargo.toml --locked -p windows-platform storage::cleaner::tests --lib`: 11 tests passed. Disposable known-folder resolver fixtures prove complete exact-root coverage, distinct sequential scopes, protection, recency, unsupported roots, reparse rejection, bounded anchors and partial retention.
- `pnpm exec vitest run src/features/cleaner src/shared/storage/api.test.ts --pool=threads --maxWorkers=1`: eight tests passed, covering catalog metadata/unavailable scopes, explicit review without execution, scope/category state invalidation and bounded latest-intent scope refreshes.

No tests were suppressed. Draft tests were corrected to use typed serde DTOs without adding dependencies and to reject an unrelated existing directory rather than the bare local root (which is legitimate for compiled wildcard targets). The unavailable-label assertion follows the required no-em-dash UI copy; authority assertions remain unchanged.

Rendered route/keyboard/Narrator and actual native cleaner scan-to-recovery smoke remain later shared verification gates. No real caches were scanned or cleaned during this checkpoint. Recovery retains occupied space; permanent deletion remains separately confirmed and non-undoable. Pinned source behavioral parity is not newly claimed by these app integration tests.
