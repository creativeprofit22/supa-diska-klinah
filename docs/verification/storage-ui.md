# Storage UI integration verification: step 8 automated portion

## Current scope

Automated checks passed for the actual cleaner, duplicate, empty-folder, browser and uninstaller routes using an isolated synthetic IPC replay. Manual Narrator and live native-dialog evidence remain outstanding; this is not whole-app accessibility or installed-workflow certification.

Earlier development-server attempts stalled before application mount. Running the built application through a production preview resolved the serving problem. The browser checks then reproduced a real skip-link defect (hash navigation instead of focus) and narrow grid/fieldset overflow. Both were fixed without relaxing the assertions. A draft fixture cursor was corrected to the native 32-hex ID shape; strict cursor validation remains unchanged.

## Assigned-port verification (resolved on 2026-09-08 UTC)

The project is configured for strict development port 1520 and strict preview port 1521. Resolved-config tests and native navigation tests passed. The owned preview on the previous port was stopped without targeting other processes.

The earlier replacement launch failed with `EACCES` while Windows reported an exclusion covering both requested ports. On the verification rerun, `pnpm preview` successfully bound to `http://127.0.0.1:1521/` with strict ports unchanged. No Windows service, exclusion, unrelated process or GG Coder port was changed. The `--remaining`, `--analyzer` and `--large-files` actual-route headless checks all passed on that preview, and were repeated with Node 24.19.0 after rebuilding. They reported zero mutation attempts/forbidden calls. This resolves preview replacement verification, not native cleanup acceptance.

## Assigned ports and reproduction

Development uses strict port **1520**. Production previews use strict port **1521**. Both bind only `127.0.0.1` and fail rather than falling back. GG Coder's ports are not used. The fixture harness accepts only these two assigned project ports and defaults to the preview.

```sh
pnpm build
pnpm preview
# In a separate terminal:
node scripts/smoke-storage-frontend.mjs --remaining
```

For an intentional development-server run, use `pnpm dev` and `STORAGE_UI_PORT=1520 node scripts/smoke-storage-frontend.mjs --remaining` from bash. Never stop an unrelated listener to make room. The prior owned preview was stopped by its exact managed task; no other process was targeted during the port change.

The wrapper owns only an isolated headless Edge profile. Captures use CDP `Page.captureScreenshot`, never system screenshots. Artifacts are under `.gg/smoke-artifacts/remaining-storage-routes/`.

## What the replay verifies

The existing default component, analyzer, and large-file modes remain. The additional mode visits real hash routes `/cleaner`, `/duplicates`, `/empty-folders`, `/browser`, `/uninstaller`, not demo components. A visible banner labels synthetic data. Every native invoke is intercepted; execute, undo, purge and vendor confirmation are forbidden. Successful completion requires zero mutation attempts and zero forbidden calls. No actual picker, vendor or cleanup process is connected.

The fixture uses complete native-shaped status counters, kind-tagged records, snake-case `candidate_id`, nullable program metadata, opaque IDs and fictional paths. One/two-record replay pages stay below the requested maximum of 100.

Passed assertions:

- Desktop 1280px and narrow 320px routes, with overflow measured against client width including the scrollbar.
- Skip-link keyboard activation preserves the current route and focuses main content.
- Explicit selection, page replacement, immutable review, Tab/Shift+Tab containment, Enter cancellation, Escape dismissal and focus return.
- Duplicate group paging and an unselectable first-member keeper across member pages.
- Browser service-worker opt-in starts false; changing it clears selection/results and changes the next scan request.
- Browser forced-colors and 200% text review states.
- Uninstaller unknown metadata/history remains truthful, with no undo/leftover actions and preparation only, never confirmation/launch.

Generated screenshots were visually inspected, including narrow browser recovery review. The newest run's `result.json` records its actual route/commands; no old port is presented as a current endpoint.

## Native mount and project gate checkpoint (04:27 UTC)

The app was rebuilt with Node 24.19.0 and Rust 1.90.0 using a bounded standalone `pnpm tauri build --debug --no-bundle` invocation. The expanded `scripts/smoke-storage-root.ps1` passed at `2026-09-08T04:27:25.4550935Z` against executable SHA-256 `82bd57515894b809f77f20c2a4a10b021a356934d79deb23bbb535b28932d8b1`.

It now verifies all eight native route mounts and Enter activation of their skip links, initial-state reflow at emulated 320/640/1280px, forced-colors reflow, 582 opaque cleaner scopes and cross-module/stale-scope rejection. Keyboard events are sent only through the smoke-owned WebView debug connection; no desktop input or screenshots are used.

Two rounds each exercise four real rendered installed-program refresh/page/unmount cycles. The largest rendered page remained 100 rows. Separately, each round performs four real native inventory scan/page/release cycles, eight one-record pages with four cursor advances, and one cancel/release cycle. Both cancellation outcomes were `cancelled`. This historical run did not exercise completion-before-cancel; the reconciliation correction below supersedes the earlier fast-completion claim. All eight completed snapshots refused pages after release. Ten directly invoked snapshots were released overall. The Tauri bridge is immutable and was neither replaced nor weakened. There are no synthetic responses in this native check, no dialogs, and no cleanup/vendor execution. Installed-program names are not written to the evidence artifact.

Native-process-only observations before/after the rounds: private bytes `8,376,320 → 8,896,512 → 8,888,320`; handles `280 → 282 → 282`; cumulative CPU milliseconds `1,296.875 → 2,093.75 → 2,640.625`. These three observations are not a leak-free claim and do not measure the WebView process tree or all filesystem modules. The smoke explicitly waits for its owned native process to exit.

App-only desktop and narrow screenshots were regenerated. A representative cleaner desktop and browser 320px capture were inspected: shared utility styling and wrapped fictional paths were retained. This does not establish full keyboard completion, native scaling or assistive-technology output.

The installed-program review previously left focus on a disabled/removed button. A failing regression test reproduced this; explicit review, confirmation and cancellation now return focus to the review region without refocusing on polling or filter changes. `pnpm test` passed 186 tests and `pnpm check` passed under Node 24.19.0. The remaining-route headless replay passed again on strict preview 1521. Rust 1.90.0 workspace tests, formatting and CI-equivalent Clippy passed. The separate external-process duplicate-keeper race driver also passed. See the [consolidated verification record](storage-parity.md) for commands and unresolved gates.

## Final automated rerun after pinned-default and depth fixes

The expanded native smoke passed again at `2026-09-08T05:19:57.4211462Z` against rebuilt executable SHA-256 `aefedd73b31bac5e2b25d2c9d86d7805e9359ddf7c4482806dfc12ae08cd9e77`. The same route/keyboard/reflow and real inventory lifecycle assertions passed. Both cancellation outcomes remained `cancelled`; all rendered next-page cycles passed with a maximum of 100 rows.

The three native-process private-byte observations were `7,811,072 → 8,855,552 → 8,650,752`; handles were `280 → 280 → 280`. Cumulative CPU milliseconds were `1,375 → 2,093.75 → 2,718.75`. The same process-only and short-run limitations apply.

Frontend tests now pass 189 cases, including three pinned-default tests. All three renderer replay modes passed again on strict preview 1521; project checks passed, including the new 12-test offline source-contract gate. See the consolidated record for Rust results and source-comparison limits. Manual evidence below is still deferred, not passed.

## Native cancellation reconciliation: 2026-09-08 UTC

The native lifecycle now executes the same small cancellation helper as 18 deterministic Node tests. Evidence separates `cancellationAttempts` (before IPC), `cancellationAcknowledgements` (successful native response), and `cancelOutcome` (validated same-module/snapshot terminal status). Only `snapshot_unavailable` from cancellation permits reconciliation: finalizing may retire to complete; cancelled without acknowledgement, failed status, wrong identities, disappearance and other IPC errors reject the smoke. Acknowledged cancellation must retire to cancelled or honestly observed complete. Release remains in `finally`, and release failures cannot produce PASS. The PowerShell gate explicitly permits only `cancelled`/`complete` with consistent acknowledgement counts.

The fresh debug build and bounded native smoke passed at `2026-09-08T19:53:35.1280609Z`, executable SHA-256 `f1fabc2bed2a8bc76b1080c8d8469bc13530daefd26269b3728c2a38aa630c3c`. Artifact: `.gg/smoke-artifacts/storage-cancellation/result.json`. Each of two rounds recorded one attempt, one acknowledgement, `cancelled`, and five released snapshots; the eight completed snapshots rejected subsequent pages. Fast-completion and failure branches are deterministic test evidence, not observed live races in this run. The bridge and native cancellation gate remain unchanged. Only read-only installed-program inventory and the existing smoke-owned WebView checks ran; no dialog, cleanup, vendor launch, desktop input or screenshot occurred. Execution IDs and harness limitations are in [the consolidated record](storage-parity.md#native-cancellation-reconciliation-2026-09-08-utc).

## Evidence still outstanding

Manual Narrator, actual Windows confirmations/UAC, and full installed-app fixture workflows remain unverified here. Native IPC, filesystem recovery, browser-policy and vendor-state tests provide separate evidence in the feature checkpoint documents. This replay cannot replace those native tests or the final parity/completion gate.

### 2026-09-22 VERIFY-BEFORE-SHIP check (Medium/Regression/UNTESTED-BLAST-RADIUS)

Re-confirmed the release-candidate baseline only: `pnpm test` on Node 24.19.0/pnpm 11.22.0 passes
29 files / 229 tests. This is the same headless/IPC-mocked evidence class already described
above and in `DESIGN.md`; it adds no new Narrator, native-keyboard, real-scaling or
forced-colors coverage. Those remain unverified and require a human operator (Narrator output is
audible only; real 200% scaling and forced-colors are OS-level settings this agent will not
change unilaterally). A step-by-step manual acceptance script for a human tester is filed at
`docs/verification/storage-ui-manual-session.md`; results from that session should be appended
here as a dated table once run. Step 8 / ship acceptance stays open until that session runs.
