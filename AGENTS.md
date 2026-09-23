# Agent instructions

## Node runtime

- Use the already-installed fnm Node **24.19.0** and pnpm **11.22.0**. Never fall back to `E:\nodejs\node.exe` (Node 22.20.0), or change project pins to match the shell.
- Before Node-based work, check `node --version`, `node -p 'process.execPath'`, `where.exe node`, `pnpm --version`, and `pnpm exec node -p 'JSON.stringify({version:process.version,execPath:process.execPath})'` in the actual execution environment. A different or earlier shell is not evidence.
- If necessary, activate the installed version using fnm: in Bash, `eval "$(fnm env --shell bash)"` then `fnm use 24.19.0`. Recheck the runtime before proceeding. Do not install anything automatically; stop and report the blocker if the required Node or pnpm cannot be selected.
- For bounded one-shot checks, select the installed runtime within each command environment; use `persist:false` and `run_in_background:false`. Do not rely on a persistent shell's PATH carrying into a fresh shell.

## Verification evidence

Keep harness evidence blockers separate from runtime guidance: record them in `docs/verification/storage-parity.md`, with execution IDs. Successful commands do not establish harness gate approval. Manual step 8 acceptance remains open until explicitly accepted; do not close it or start the next task based on these checks alone.
