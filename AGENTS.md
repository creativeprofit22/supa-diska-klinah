# Agent instructions

## Node runtime

- Use the already-installed fnm Node **24.19.0** and pnpm **11.22.0**. Never fall back to `E:\nodejs\node.exe` (Node 22.20.0), or change project pins to match the shell.
- Before Node-based work, check `node --version`, `node -p 'process.execPath'`, `where.exe node`, `pnpm --version`, and `pnpm exec node -p 'JSON.stringify({version:process.version,execPath:process.execPath})'` in the actual execution environment. A different or earlier shell is not evidence.
- If necessary, activate the installed version using fnm: in Bash, `eval "$(fnm env --shell bash)"` then `fnm use 24.19.0`. Recheck the runtime before proceeding. Do not install anything automatically; stop and report the blocker if the required Node or pnpm cannot be selected.
- For bounded one-shot checks, select the installed runtime within each command environment; use `persist:false` and `run_in_background:false`. Do not rely on a persistent shell's PATH carrying into a fresh shell.

## Verification evidence

Keep harness evidence blockers separate from runtime guidance: record them in `docs/verification/storage-parity.md`, with execution IDs. Successful commands do not establish harness gate approval. Manual step 8 acceptance remains open until explicitly accepted; do not close it or start the next task based on these checks alone.

<!-- gg:init:start -->
## Project

Supa Diska Klinah: Windows-only disk cleanup / system optimization desktop app. It is a Tauri v2 port of Kudu v2.4.0, with a React+TS frontend (`src/features/*`) and a Rust workspace (`src-tauri/`).

- `src-tauri/src/commands/*`: thin IPC wrappers over services; `lib.rs` registers them and runs as a **standard user** (`require_standard_user`).
- `crates/windows-platform`: all Windows API access and services.
- `crates/cleanup-core`, `crates/protection-core`: portable domain logic with **no** platform/app deps (ADR 0003).
- `crates/privileged-helper`: the single elevated sidecar, bundled as `binaries/supa-diska-klinah-privileged-helper`. It is the only path to admin operations.

## Gotchas / invariants (enforced by `pnpm check` scripts, which CI runs)

- **Crate edges are fixed** (`scripts/check-architecture.mjs`): app→windows-platform, helper→windows-platform, windows-platform→{cleanup-core, protection-core}, cores→nothing. Put Windows code in windows-platform, never in the cores.
- **Adding/removing a Tauri command touches three places**, which must match exactly: the `.commands(&[...])` manifest in `src-tauri/build.rs`, `generate_handler![...]` in `src-tauri/src/lib.rs`, and the `allow-<cmd>` permissions in `src-tauri/capabilities/main.json`. `check-security-boundaries.mjs` fails on drift.
- **Security config is locked:** capability limited to the local `main` webview on Windows; no `core:default`/shell/fs/process/sidecar permissions; asset protocol and `withGlobalTauri` off; the app manifest stays `asInvoker`; exactly one externalBin; per-machine NSIS, sha256 + timestamp, no `certificateThumbprint` (signing happens externally in CI). Don't loosen these to make a feature work. Route elevated work through the helper.
- **Parity matrix:** `docs/parity.md` must cite Kudu v2.4.0 at revision `db09e051…` and map every Kudu module to its command/crate/feature. Fixture references must resolve to real test functions (e.g. `cleanup-core/tests/storage_parity.rs`). Adding or renaming a feature means updating the matrix.
- **Storage operations fail closed:** blocked, partial or inaccessible items must surface as incomplete, never silently skipped or counted as freed. Tests named `*fail_closed*` enforce this.
- **Signed rule packs** under `windows-platform/src/protection/baseline/**` are `binary` in `.gitattributes` because the signature covers exact bytes. Never reformat or re-save them. `.ps1` files must stay CRLF.
- **The helper binary must exist before Rust builds, clippy or the app run.** Tauri's before-dev/before-build hooks run `pnpm build:helper` first. Plain `cargo clippy`/`cargo test` do not, so build it manually first (see below).
- **Ports are fixed with strictPort:** dev 127.0.0.1:1520 (Tauri `devUrl`), preview 1521 (the storage smoke replay uses it). A busy port fails instead of shifting.
- Deps are exact-pinned (`save-exact`, `engine-strict`); cargo runs `--locked`.

## Workflows

- `pnpm check` does not cover Rust. The Rust gate (run in `src-tauri/`), in order:
  `cargo fmt --all -- --check` → `TAURI_ENV_TARGET_TRIPLE=x86_64-pc-windows-msvc TAURI_ENV_DEBUG=true pnpm build:helper -- --target x86_64-pc-windows-msvc` → `cargo clippy --workspace --all-targets --locked -- -D warnings` → `cargo test --workspace --locked`.
- Full-workspace Rust builds can exhaust host memory; retry with `-j 1`.
- Native smoke test: `pnpm tauri build --debug --no-bundle --target <triple>`, then `scripts/smoke-native-ci.ps1 -Target <triple> -StorageSmoke ...`. The app must launch **hidden** (`SUPA_DISKA_KLINAH_SMOKE_MINIMIZED=1`, `-WindowStyle Hidden`), never become visible, and run non-elevated. The helper exe must sit beside the app exe.
- Releases are signed only in CI (tag or dispatch with `release: true`, `windows-release` environment, PFX secrets). There is no local signed-release path.
<!-- gg:init:end -->
