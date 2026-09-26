# Browser cache integration checkpoint

Step 6, 7 September 2026. The route lists native browser bases and policy metadata, authorizes an opaque scope ID, scans one scope, and reviews only explicitly selected file candidate IDs. No typed path or renderer activity claim grants access. Service-worker opt-in is false initially; changing it remounts the session, releases the old snapshot/root and clears selection. The native disclosure, recency threshold, exclusions, risk, provenance and unsupported operations are shown.

Profile/shared labels are display-only interpretations of returned paths relative to the authorized base and native cache templates. Unknown paths stay unclassified. These strings never enter authorization or mutation requests. Discovery and revalidation refuse active or unknown browser activity; private profile databases, cookies, passwords, history and bookmarks remain excluded. No automatic browser closing was added.

App recovery and native-confirmed permanent review reuse the existing engine. Recovery is same-volume, manual and not automatic purge; it does not reclaim disk space. Browser recovery undo also rechecks current browser scope/activity before restoration. Scope replacement and opt-in changes cannot reuse the old UI selection.

Executed checks:

- `cargo test --manifest-path src-tauri/Cargo.toml --locked --test browser_commands`: two real IPC tests passed for policy round-trip, opaque scopes, unavailable/cross-module rejection, malformed input and foreign callers. Only scope metadata/authorization were exercised against the host, not real profile scans or cleanup.
- `cargo test --manifest-path src-tauri/Cargo.toml --locked -p windows-platform storage::browser::tests --lib`: one native layout/exclusion test passed.
- `cargo test --manifest-path src-tauri/Cargo.toml --locked -p windows-platform storage::browser::native_tests --lib`: five disposable native fixture tests passed, covering canonical roots, activity refusal, Chromium/Opera/shared/Firefox fork layouts, recency, private descendants, protected paths and partial retention.
- `pnpm exec vitest run src/features/browser --pool=threads --maxWorkers=1`: seven tests passed for unavailable metadata, opt-in defaults/change invalidation, profile/shared labels, explicit app-recovery review without execution, sequential root isolation and activity-error recovery copy.

No assertions were suppressed. Native policy enum types are re-exported through the platform facade rather than adding an app-to-core dependency. Corrupted draft UI punctuation was corrected. Live browser-profile mutation, actual route/Narrator/keyboard smoke and installed-app native confirmation remain later verification gates; no developer browser data was cleaned.
