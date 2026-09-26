# Supa Diska Klinah

Supa Diska Klinah is a Windows-first Tauri desktop storage application. It keeps discovery and mutation authority in Rust, runs the webview at standard integrity, and isolates System Restore creation in a one-shot elevated helper. New storage cleanup uses identity-preserving app recovery where supported; permanent and vendor actions require separate native confirmations. See the [storage user guide](docs/storage.md) before cleaning files.

## Current status

- Tauri 2.11.5 with React 19, Vite 8, and TypeScript 6.
- Native Windows x64 and ARM64 build targets.
- Thin application, one-shot helper, `windows-platform`, and `cleanup-core` crate boundaries.
- Eight storage routes: drives, analyzer, large files, rule cleaner, duplicates, empty folders, browser caches and installed programs.
- Opaque-ID cleanup plans with final no-follow revalidation, resumable journals, and bounded history.
- Same-volume, file-only app recovery with guarded undo; atomic empty-only removal; native-confirmed permanent/vendor actions. Legacy temporary cleanup retains its separate Recycle Bin/quarantine behavior.
- Platform-neutral validated cleanup rules and a bounded, cancellable preview scan engine.
- Kudu v2.4.0 compatibility scope mapped but not behaviorally verified.
- Per-machine NSIS installer published on GitHub Releases, with SHA-256 checksums and build-provenance attestations.
- Opt-in, user-approved self-updates verified against an Ed25519-signed manifest (off by default).
- English and Latin American Spanish (`es-419`) interface; Spanish strings are a draft pending native review.
- **Releases are currently unsigned** (no Windows code-signing certificate yet). Windows shows "Unknown publisher"; [verify your download](docs/installation.md#verify-your-download) before running it.

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
cargo +1.90.0 fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo +1.90.0 clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets --locked -- -D warnings
cargo +1.90.0 test --manifest-path src-tauri/Cargo.toml --workspace --locked
pnpm tauri build --debug --no-bundle --target x86_64-pc-windows-msvc
```

The x64 executable is launched locally as a smoke check. GitHub Actions builds and launches the ARM64 executable on a native `windows-11-arm` runner. Release installers are built only by the tag-triggered release jobs; see the [release process](docs/release.md).

## Project documents

- [Installation and download verification](docs/installation.md)
- [User guide](docs/user-guide.md)
- [Privacy and network connections](docs/privacy.md)
- [App updates](docs/updates.md)
- [Localization](docs/localization.md)
- [Accessibility](docs/accessibility.md)
- [Release process and signing modes](docs/release.md)
- [Release verification](docs/verification/release.md)
- [Accessibility verification](docs/verification/accessibility.md)
- [Storage user guide](docs/storage.md)
- [Storage verification and remaining completion gates](docs/verification/storage-parity.md)
- [Architecture and ownership rules](docs/architecture.md)
- [Cleanup rule schema and authoring guide](docs/cleanup-rules.md)
- [Cleanup execution and recovery](docs/cleanup-recovery.md)
- [Project artifact discovery](docs/project-artifacts.md)
- [Safe post-build artifact budgets](docs/build-artifact-budgets.md)
- [Performance methodology, results and budgets](docs/performance.md)
- [Performance verification runs](docs/verification/performance.md)
- [Windows release checklist](docs/release-checklist.md)
- [Windows development and troubleshooting](docs/development.md)
- [System management user guide](docs/system-management.md)
- [System management administrator guide](docs/system-management-admin.md)
- [System management verification](docs/verification/system-management.md)
- [Protection guide](docs/protection.md)
- [Protection verification](docs/verification/protection.md)
- [Kudu parity contract](docs/parity.md)
- [Threat model and privileged-operation inventory](docs/security.md)
- [Licensing and source boundaries](docs/licensing.md)
- [ADR 0001: modular boundaries](docs/adr/0001-modular-boundaries.md)
- [ADR 0002: system-change helper batch](docs/adr/0002-system-change-helper.md)
- [ADR 0003: local-first protection](docs/adr/0003-local-first-protection.md)
- [Contributing](CONTRIBUTING.md)
- [Project MIT license](LICENSE)
- [Third-party notices](THIRD_PARTY_NOTICES.md)
