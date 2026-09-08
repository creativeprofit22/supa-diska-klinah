# Duplicate and empty-folder integration

## Keeper CI gate: 8 September 2026

Task `652eb0a9` adds the existing external-process driver to Quality immediately after workspace tests, reusing their target directory. The step pins Rust 1.90.0, has a five-minute timeout, logs the driver exit code and exits with that code. Quality remains required by the signed release job; no continue-on-error or privileged helper was added. The driver's existing `--locked --no-run` precompile still precedes its 60-second handshake, and the ignored Rust test and keeper assertions are unchanged. The separate native-storage smoke integration remains outside this change.

Fresh local execution `9b433f7f-bf53-424a-adf2-606514f343de` passed under Cargo/Rust 1.90.0: one test passed, zero ignored, followed by the external driver's PASS message and exit 0. Only its marked disposable temporary fixture was mutated. Workflow exit-propagation probe `eeefc6e9-f97a-4d49-bc82-1009639cf62e` confirmed the real driver reference and preserved a synthetic child exit 37 through the extracted step body (Windows PowerShell locally, not a hosted Actions run).

Hosted execution of the new gate remains **pending**, not failed or proved by compilation. Read-only inspection of [CI run 34103714621](https://github.com/creativeprofit22/supa-diska-klinah/actions/runs/34103714621) found the older workflow without this step. See [storage verification](storage-parity.md#keeper-ci-gate-2026-09-08-utc) for execution IDs and the remaining release-evidence blocker. PR-branch commit and push are now authorized for this gate and its notes only; hosted verification remains pending the exact pushed SHA. No release dispatch or manual acceptance is authorized.
