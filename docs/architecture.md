# Architecture

## Runtime flow

```text
React dashboard
  -> typed @tauri-apps/api invoke("foundation_status")
  -> Tauri commands::foundation
  -> windows-platform::foundation_status
  -> cleanup-core::FoundationStatus
  -> serialized response to React
```

The command layer delegates immediately. It does not contain platform operations or domain policy.

## Rust dependency direction

```text
supa-diska-klinah (thin Tauri app)
  -> windows-platform (Windows adapter)
    -> cleanup-core (portable contracts)
```

`cleanup-core` contains serializable domain types and contracts only. It cannot depend on Tauri, Windows bindings, filesystems, registries, services, or processes. `windows-platform` owns all future Windows filesystem, registry, service, process, elevation, and shell interaction. The application crate registers commands and capabilities but does not depend directly on `cleanup-core`.

`scripts/check-architecture.mjs` reads locked Cargo metadata and rejects any other workspace edge. It also rejects Tauri or Windows dependencies in `cleanup-core`.

## Frontend ownership

```text
app/router
  -> feature route exports
  -> shared AppShell

features/dashboard -> its API adapter and status state
features/settings  -> its local placeholder state
shared             -> no app or feature imports
```

A feature may import its own files and shared code. It cannot import another feature or the app composition layer. Shared code cannot import app or feature code. The architecture check resolves local TypeScript imports and enforces these rules.

The hash router keeps packaged navigation independent of an HTTP fallback. Route composition belongs to `src/app`; API adapters, state, pages, and route objects belong to their feature.

## Tauri capability boundary

The application exposes only `foundation_status`. `build.rs` declares that command to Tauri's app manifest. `capabilities/main.json` grants `allow-foundation-status` only to the local `main` window and only on Windows. No remote origins are accepted.

Production and development content security policies are explicit. Asset protocol is disabled. There are no shell, filesystem, dialog, or updater plugins or capabilities. Adding a command requires all of the following:

1. Put platform work behind `windows-platform`.
2. Register the command in the app manifest and invoke handler.
3. Add the narrowest main-window permission only when required.
4. Validate all command input at the boundary and fail closed.
5. Update architecture and parity checks where ownership changes.
