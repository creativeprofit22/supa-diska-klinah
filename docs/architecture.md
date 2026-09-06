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

Project artifact discovery is a deliberately separate read-only projection. `windows-platform` persists at most 32 canonical roots in an app-owned atomic registry, while `cleanup-core` applies marker-aware ecosystem rules, no-follow traversal, identity checks, containment within the selected root and project context, bounded measurement, exact deduplication, and parent-child overlap suppression. All-root scans collapse covered child roots and divide aggregate budgets before scanning sequentially. Every private discovery snapshot is dropped, so discovery returns no identifier usable by plan creation or deletion. See the [project artifact guide](project-artifacts.md).

The opt-in build artifact coordinator is a separate trust grant layered on saved root IDs. `cleanup-core` owns exact-path snapshots, protection reasons, deterministic age and size selection, and protected-byte floors. `windows-platform` owns native approval, executable identity, immutable argv launch, in-memory runs, atomic success ledgers, conservative external observations, and artifact-plan reconstruction. Artifact plans reuse the same final revalidator, serialized writer, pre-mutation journal, quarantine, undo, and purge path. The privileged helper has no build or artifact operation. See [build artifact budgets](build-artifact-budgets.md).

`scripts/check-architecture.mjs` reads locked Cargo metadata and rejects any other workspace edge. It allows runtime `std::process::Command` only in the build artifact coordinator and rejects Tauri dependencies outside the application.

## Frontend ownership

```text
app/router
  -> feature route exports
  -> shared AppShell

features/dashboard       -> its API adapter and status state
features/cleanup         -> preview, plan, execution, undo, history, and artifact coordinator composition
features/build-artifacts -> typed build APIs, polling state, and reusable budget surfaces
features/settings        -> persisted cleanup and artifact-budget policy composition
shared                   -> no app or feature imports
```

A feature normally imports only its own files and shared code. The explicit build-artifact bridge is limited to the Cleanup and Settings composition files plus existing project-root display adapters; the architecture check pins those exact imports. Shared code cannot import app or feature code.

The hash router keeps packaged navigation independent of an HTTP fallback. Route composition belongs to `src/app`; API adapters, state, pages, and route objects belong to their feature.

## Tauri capability boundary

The application exposes foundation and restore-point commands plus cleanup preview, project-root list/add/pause/remove/discovery, plan creation, safe execution, separate permanent execution, undo, history, automatic-policy commands, and build profile/run/artifact-budget commands. Build profile registration accepts typed data and invokes native confirmation. Later start, get, cancel, remove, and preview operations accept opaque IDs or bounded policy values; runtime launch commands accept no executable, argv, environment, working directory, artifact path, or deletion primitive. `build.rs`, `generate_handler!`, and `capabilities/main.json` contain the same command set for the local Windows `main` webview only.

Production navigation allows only the packaged Tauri origin. Development additionally allows exactly `http://127.0.0.1:1420`. Content security policies are explicit, asset protocol is disabled, and no generic shell, filesystem, process, dialog, or updater plugin is granted. One reviewed standard-integrity coordinator launches only natively approved canonical `.exe` profiles through `Command::new(executable).args(argv)` with disconnected streams. The main window is created hidden and unfocused; startup policy shows and focuses it only for foreground launches. The main executable is `asInvoker` and rejects an elevated token before constructing Tauri. Only the separately packaged helper requests UAC.

Adding a command requires all of the following:

1. Put platform work behind `windows-platform`.
2. Register the command in the app manifest and invoke handler.
3. Add the narrowest main-window permission only when required.
4. Validate all command input at the boundary and fail closed.
5. Run `pnpm check:security` to reject command-list or capability drift.
6. Add a helper enum variant only when standard integrity cannot perform the operation.
7. Update architecture, threat-model, and parity documents where ownership changes.
