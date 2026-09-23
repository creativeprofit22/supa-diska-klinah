# Storage parity: step 2 frontend verification

Scope: shared components only, 7 September 2026. No feature start adapters, routes, navigation, or step 3 work. No native app, cleanup, permanent confirmation, or undo operation was run during this step.

## Integration contract

- `src/shared/storage/useStorageScan.ts` accepts a feature-owned start callback returning an opaque snapshot ID. No native callback name or renderer-chosen runner is sent through IPC.
- Pass a stable `scopeKey` covering the selected native scope and every scan filter. Changing it invalidates responses and releases the previous job. Keep an injected API object stable (the production default already is).
- Native status polling is serial, every 500 ms after the previous request resolves. Terminal states and unmount stop polling. Late start results are released rather than adopted.
- The hook keeps one page, at most 100 rows, and at most 1,000 selected IDs. Next/First replace rows, with no cursor history or accumulated page cache. Feature-specific rows and eligibility extraction stay feature-owned. Selection callbacks from previous scans cannot change the new selection.
- A page request in flight temporarily removes plan authority. Scope/filter/new-scan/cancel resets discard selections. Cancelled results never produce a review selection.
- `StoragePlanReview` captures IDs and the backend's immutable plan summary. Changing the selection invalidates pending and visible reviews. Dispositions default to Recycle Bin; a feature must explicitly supply other supported dispositions.
- Cleanup execution, permanent native confirmation, history, and undo are the existing APIs, moved without changing command signatures to `src/shared/cleanup/api.ts`. The old feature API re-exports them so existing callers and architecture boundaries remain intact. No second execution engine was added.
- History retains at most 20 summary entries, rather than all per-item outcome arrays. Selected, processed, occupied, and reclaimed bytes remain distinct. Undo is offered only when returned outcomes contain recoverable items, never for permanent operations.

## Automated evidence

`pnpm test`: 115 tests passed, including 20 new shared-storage tests and the existing cleanup/drive tests.

The new tests cover:

- Raw UTF-8 JSON for all eight step-1 commands; no extra proof/path fields in a storage selection; malformed, duplicate, and oversized selections rejected before IPC; sanitized errors.
- Cancellation and cancellation failure, late status/start/page responses, unmount release, serial polling, terminal timer teardown, page replacement, cursor ownership in requests, stale selection callbacks, new-scan and scope/filter reset, selection/page bounds, busy/expired/partial states.
- Explicit review without mutation, late-plan rejection, cancellation invalidating visible plans, immutable plan ID execution through injected test doubles, duplicate-click prevention, separate permanent-confirmation command, truthful reclaimed bytes, history/undo reuse, failure without replay.

`pnpm check`: passed the existing project checks, architecture scanner tests, security boundaries, documentation checks, TypeScript and production frontend build. The checks were not weakened.

Environment caveat: these runs used Node 22.20.0 and pnpm 11.22.0. The repository pins Node 24.19.0; pnpm reported the engine mismatch. This is not evidence of running on the pinned Node version.

## Rendered fixture evidence

Run `pnpm dev` on strict development port 1520, then `STORAGE_UI_PORT=1520 node scripts/smoke-storage-frontend.mjs` from bash. This source-only component fixture is not shipped in the production preview; actual-route scenarios default to strict preview port 1521. The latter is a bounded, headless Edge run with its own temporary browser profile. It targets only `scripts/fixtures/storage.html`. It neither launches the native application nor captures the desktop. The browser's native IPC stub throws if reached; the injected execution and undo functions also reject. Successful runs assert zero native IPC calls and zero mutation attempts.

Current result: passed on the final shared components. Captures and machine-readable evidence are generated under `.gg/smoke-artifacts/storage-frontend/` and intentionally ignored by Git.

| Changed-scope check | Actual evidence |
| --- | --- |
| Desktop 1280 × 1000 | `desktop-results.png`, `desktop-review-keyboard.png`; reviewed renders use the existing 52rem rail, Segoe type, tokens, status surfaces and native controls. |
| Narrow 320 × 850 | `mobile-results.png`, `mobile-review.png`; long paths wrap, actions stack, no horizontal overflow against actual client width including the scrollbar. |
| Keyboard | Browser-dispatched Tab, Shift+Tab, Space, Enter and Escape complete selection, paging, review and dismissal. Modal focus wraps between actions; dismissal returns focus to the review trigger. No confirm/execute control was activated. |
| 200% text at 640 × 900 | `text-200-percent-review.png`, `text-200-percent-confirm-focus.png`; dialog scrolls, focused confirm is brought into view, no horizontal overflow. This is text-size testing, not a claim of testing every OS DPI setting. |
| Forced colors | `forced-colors-review.png`; visible dialog border, button borders, text and keyboard focus. |
| Recovery states | `state-partial.png`, `state-empty.png`, `state-expired.png`, `state-busy.png`, `state-slow.png` (cancelled slow scan). |
| Mutation isolation | The browser smoke's final assertions require both native-call and mutation-attempt counters to remain zero. |

Fixes discovered by these checks: React's commit-time focus restoration could return focus into a closed dialog; focus now returns after the DOM commit. A previous-scan selection callback could repopulate a new selection; current page and job identity now guard it. The root's fixed 20rem minimum caused overflow when a scrollbar reduced the available 320px viewport; its minimum now respects available width. The overflow assertion measures client width, not outer viewport width.

## Limits

No Narrator/screen-reader session, real WebView2 native confirmation, actual deletion, or actual undo was exercised. No whole-app accessibility/conformance claim is made. Screen-reader announcements are code-reviewed (phase-only live messages, non-live progress counters); screen-reader output remains unverified. No memory benchmark or native resource measurement was run; serial polls, stopped timers, one-page retention and selection caps have executable tests. The fixture's injected backend is not proof of full native scan-to-execution integration; later feature steps retain that responsibility.
