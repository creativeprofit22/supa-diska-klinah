# Development on Windows

## Prerequisites

Install these components before cloning dependencies:

- Windows 11 on x64 or ARM64.
- Microsoft WebView2 Runtime.
- Visual Studio 2022 Build Tools with **Desktop development with C++**, MSVC, and a current Windows SDK.
- Node.js `24.19.0`; `.node-version` is the source of truth.
- Corepack, included with supported Node distributions.
- Rustup. The repository automatically selects Rust `1.90.0`, rustfmt, Clippy, and both Windows MSVC targets.

Do not substitute GNU Rust targets. Tauri and CI use `x86_64-pc-windows-msvc` and `aarch64-pc-windows-msvc`.

## Setup

```powershell
corepack enable
pnpm install --frozen-lockfile
pnpm tauri dev
```

The first Rust command may download the pinned toolchain and targets. Tauri sets the target and profile environment variables used by `scripts/build-privileged-helper.mjs`; that script builds with argv and no shell, then prepares Tauri's target-suffixed sidecar. Development allows exactly `http://127.0.0.1:1420`; packaged navigation remains local.

## Verification

From the repository root:

```powershell
pnpm check:parity
pnpm check:architecture
pnpm check:security
pnpm check:docs
pnpm test
pnpm build
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets --locked -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --workspace --locked
pnpm tauri build --debug --no-bundle --target x86_64-pc-windows-msvc
```

The main executable and `supa-diska-klinah-privileged-helper.exe` are emitted together under `src-tauri/target/x86_64-pc-windows-msvc/debug/`. Run `pnpm test:native x86_64-pc-windows-msvc` to confirm both exist, the main process remains alive, and its token is not elevated. The smoke check never launches the helper or displays UAC.

## ARM64

The native ARM64 build command is:

```powershell
pnpm tauri build --debug --no-bundle --target aarch64-pc-windows-msvc
```

Cross-compilation does not prove launch behavior. CI builds the matching helper, launches only the main executable on GitHub's native `windows-11-arm` runner, and inspects its token. Local ARM64 completion requires an ARM64 Windows machine; x64 developers rely on that independent CI smoke job.

## Troubleshooting

- **WebView2 loader or blank window:** install or repair the Evergreen WebView2 Runtime.
- **`link.exe` or Windows SDK missing:** modify Visual Studio Build Tools and add Desktop development with C++.
- **Wrong Node or pnpm:** install Node `24.19.0`, run `corepack enable`, then verify `pnpm --version` prints `11.22.0`.
- **Frozen install fails after an intentional dependency update:** regenerate `pnpm-lock.yaml` with pinned Node and pnpm, then review both manifest and lockfile.
- **Cargo selects an unexpected compiler:** run commands from the repository or `src-tauri` so `rust-toolchain.toml` is discovered.
- **Architecture check reports a missing lock:** run `cargo generate-lockfile --manifest-path src-tauri/Cargo.toml` and review the complete lockfile change.
- **Helper build rejects its environment:** invoke it through Tauri, or set `TAURI_ENV_TARGET_TRIPLE` to a supported MSVC target and `TAURI_ENV_DEBUG` to exactly `true` or `false`.
- **Restore point fails:** verify System Protection and disk space; UAC cancellation is a normal closed failure. A manual restore-point check is optional and must run on a disposable machine.
