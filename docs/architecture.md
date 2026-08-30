# Architecture

## Runtime flow

```text
React dashboard
  -> typed @tauri-apps/api invoke
  -> capability-scoped Tauri command
  -> windows-platform
     -> ordinary adapter -> cleanup-core contracts
     -> restore broker -> elevated one-shot helper -> System Restore
  -> serialized, non-sensitive response to React
```

The command layer delegates immediately. It does not contain platform operations or domain policy.

## Rust dependency direction

```text
supa-diska-klinah (thin Tauri app) -> windows-platform
privileged-helper (one-shot elevated binary) -> windows-platform
windows-platform (Windows adapter and security policy) -> cleanup-core
cleanup-core (portable contracts)
```

`cleanup-core` contains serializable domain types, validated cleanup rules, scan policy, and platform-neutral filesystem traits. It cannot depend on Tauri, Windows bindings, registries, services, or processes. `windows-platform` implements no-follow metadata, canonical paths, Windows file identities, and rejection of every reparse-point attribute. It also owns Windows path policy, protocol validation, broker behavior, and helper dispatch. The application owns only Tauri registration and typed command input. The helper owns only process entry and fixed exit codes. Neither helper nor domain crate depends on Tauri.

The scan engine accepts caller-resolved absolute root bindings and a complete protection policy. Independent `direct` and `projectArtifacts` scanners share bounded traversal contracts. Rust-owned snapshots resolve opaque candidate IDs into immutable persisted plans. `windows-platform` serializes final validation, Recycle Bin, quarantine, permanent deletion, undo, purge, journals, and accounting; Tauri never receives a mutation path.

`scripts/check-architecture.mjs` reads locked Cargo metadata and rejects any other workspace edge. It also rejects runtime `std::process::Command` and Tauri dependencies outside the application.

## Frontend ownership

```text
app/router
  -> feature route exports
  -> shared AppShell

features/dashboard -> its API adapter and status state
features/cleanup   -> preview, plan, execution, undo, and history state
features/settings  -> persisted automatic-cleanup policy state
shared             -> no app or feature imports
```

A feature may import its own files and shared code. It cannot import another feature or the app composition layer. Shared code cannot import app or feature code. The architecture check resolves local TypeScript imports and enforces these rules.

The hash router keeps packaged navigation independent of an HTTP fallback. Route composition belongs to `src/app`; API adapters, state, pages, and route objects belong to their feature.

## Tauri capability boundary

The application exposes foundation and restore-point commands plus cleanup preview, plan creation, safe execution, separate permanent execution, undo, history, and automatic-policy commands. The cleanup webview can supply only bounded opaque identifiers, a disposition, and policy values; it cannot supply paths, roots, rules, limits, protection policy, or deletion primitives. `build.rs`, `generate_handler!`, and `capabilities/main.json` contain the same command set for the local Windows `main` webview only.

Production navigation allows only the packaged Tauri origin. Development additionally allows exactly `http://127.0.0.1:1420`. Content security policies are explicit, asset protocol is disabled, and no shell, filesystem, process, dialog, or updater plugin is granted. The main window is created hidden and unfocused; startup policy shows and focuses it only for foreground launches, preventing minimized launches from flashing or taking focus. The main executable is `asInvoker` and rejects an elevated token before constructing Tauri. Only the separately packaged helper requests UAC.

Adding a command requires all of the following:

1. Put platform work behind `windows-platform`.
2. Register the command in the app manifest and invoke handler.
3. Add the narrowest main-window permission only when required.
4. Validate all command input at the boundary and fail closed.
5. Run `pnpm check:security` to reject command-list or capability drift.
6. Add a helper enum variant only when standard integrity cannot perform the operation.
7. Update architecture, threat-model, and parity documents where ownership changes.
