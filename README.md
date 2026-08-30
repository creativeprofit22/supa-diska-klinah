# Supa Diska Klinah

Supa Diska Klinah is a Windows-first Tauri desktop foundation for future disk cleanup features. It runs the webview at standard integrity, exposes explicit capability-gated Rust commands, and isolates System Restore creation in a one-shot elevated helper. It does not delete user data.

## Current status

- Tauri 2.11.5 with React 19, Vite 8, and TypeScript 6.
- Native Windows x64 and ARM64 build targets.
- Thin application, one-shot helper, `windows-platform`, and `cleanup-core` crate boundaries.
- Dashboard and Settings hash routes with feature-owned state.
- Two commands: foundation status and validated System Restore creation; cleanup mutation is not implemented.
- Platform-neutral validated cleanup rules and a bounded, cancellable preview scan engine.
- Kudu v2.4.0 compatibility scope mapped but not behaviorally verified.
- Installer bundling, signing, and updater support intentionally deferred.

## Quick start

On Windows, install the prerequisites in the [development guide](docs/development.md), then run:

```powershell
corepack enable
pnpm install --frozen-lockfile
pnpm tauri dev
```

The toolchain is pinned to Node `24.19.0`, pnpm `11.22.0`, and Rust `1.90.0`. Use those versions for reproducible lockfile changes.

## Verification

```powershell
pnpm check:parity
pnpm check:architecture
pnpm check:security
pnpm check:docs
pnpm build
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets --locked -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --workspace --locked
pnpm tauri build --debug --no-bundle --target x86_64-pc-windows-msvc
```

The x64 executable is launched locally as a smoke check. GitHub Actions builds and launches the ARM64 executable on a native `windows-11-arm` runner. Bundling is disabled during the foundation phase.

## Project documents

- [Architecture and ownership rules](docs/architecture.md)
- [Cleanup rule schema and authoring guide](docs/cleanup-rules.md)
- [Windows development and troubleshooting](docs/development.md)
- [Kudu parity contract](docs/parity.md)
- [Threat model and privileged-operation inventory](docs/security.md)
- [Licensing and source boundaries](docs/licensing.md)
- [ADR 0001: modular boundaries](docs/adr/0001-modular-boundaries.md)
- [Contributing](CONTRIBUTING.md)
- [Project MIT license](LICENSE)
- [Third-party notices](THIRD_PARTY_NOTICES.md)
