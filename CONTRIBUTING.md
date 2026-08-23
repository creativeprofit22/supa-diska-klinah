# Contributing

## Change workflow

1. Create a focused branch from the current default branch.
2. Keep changes inside the owning feature or crate.
3. Add or update focused tests for behavior changes.
4. Run the checks in [`docs/development.md`](docs/development.md).
5. Submit a reviewable change explaining scope, safety impact, and verification.

Do not commit secrets, local environment files, build output, generated Tauri targets, or logs.

## Required checks

Before review, run parity and architecture checks, the frontend production build, Rust formatting, Clippy with warnings denied, and workspace tests. Changes to native startup also require the x64 launch smoke. ARM64 launch evidence comes from the native GitHub-hosted runner.

## Dependency updates

Direct npm and Cargo requirements remain exact. Use Node `24.19.0`, pnpm `11.22.0`, and the pinned Rust toolchain when regenerating lockfiles. Review manifest and lockfile changes together. Do not hand-edit integrity data.

GitHub Actions must use a full immutable commit SHA with a nearby release-version comment. Dependabot proposals are inputs to review, not permission to merge unseen dependency code or install scripts. Review licenses and update [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md) when required.

## Architecture

Preserve `app -> windows-platform -> cleanup-core`. The core cannot acquire Tauri, Windows, filesystem, registry, process, or service dependencies. Keep command bodies thin and put operating-system behavior in `windows-platform`.

Frontend features own their routes, state, API adapters, and screens. Shared code cannot import features or app composition, and features cannot import each other. See [`docs/architecture.md`](docs/architecture.md).

## Tauri commands and parity

Every new command needs boundary validation, app-manifest registration, invoke-handler registration, and the narrowest local-window capability. Never broaden content security policy or permissions to silence an error.

If a change implements Kudu-compatible behavior, update [`docs/parity.md`](docs/parity.md). Mark behavior `Verified` only when an equivalence test covers success, failure, validation, and relevant safety behavior against the pinned contract.
