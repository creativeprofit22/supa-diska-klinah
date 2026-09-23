# Product interface direction

## Read-only drive inventory checkpoint

The Drives route serves Windows users checking local fixed-drive capacity, not selecting cleanup targets. Reuse the shell navigation, 52rem content rail, bordered status panels, definition lists, Segoe typography, existing colors, button feedback, and keyboard focus rules. Labels and system-drive identity precede total, used, and account-available capacity; Refresh is the only action. No icons, charts, or decorative surfaces are added.

Loading, empty, sanitized retryable errors, populated results, and incomplete inventory are distinct. Missing drive information never becomes zero capacity. Refresh clears old results; duplicate outstanding reads share one request, and unmounted views ignore late results. Native labels are text, never markup. Long labels wrap and narrow layouts use the existing stacked definition lists and two-column navigation.

Changed accessibility scope is the Drives route and its navigation entry: native links/buttons, meaningful headings and list semantics, status announcements, visible keyboard focus, and responsive reflow. UI-state tests cover the real hook/API path with controlled IPC responses; they are not native evidence. Native rendering, keyboard/reflow evidence, and remaining assistive-technology checks are recorded separately in the checkpoint report; no WCAG conformance, installed-workflow, parity, or documentation-completion claim is made.

## Read-only disk analyzer checkpoint

The Disk analyzer route lets users inspect a native-selected folder without granting mutation authority. Reuse the 52rem rail, shell navigation, Segoe type, native controls, bordered status surfaces and shared storage paging. Show selected scope and display-depth control first, root totals second, then a folder breadcrumb and paged child summaries or global extension aggregates. No cleanup actions, selectable file candidates, charts or new dependencies are included.

Display depth is distinct from traversal: shallower rows still report full measured subtree totals within native safety limits. Logical size is not reclaimable space; unknown allocation stays unknown. Hard-link counts and incomplete totals remain explicit. A cancelled picker preserves the current view; scan cancellation, scope/filter changes and expired pages hide stale results. Single-use folder authorization is explained before another scan.

Breadcrumb navigation moves keyboard focus to the current-folder heading. Folder/extension buttons expose pressed state without overriding the shared keyboard focus ring. Long paths and labels wrap; definition lists and controls reflow at 320px. Headless desktop/mobile, keyboard, forced-colors and enlarged-text evidence uses the actual route with replayed disposable native fixture output, not a live cleanup connection. See `docs/verification/disk-analyzer-readonly.md` for evidence and unverified scope; no whole-app accessibility or pinned parity claim is made.

## Rule cleaner checkpoint

Rule cleaner uses native-resolved, opaque catalog scopes, explicit authorization and a separate scan action. No default scope or file selection; switching scopes or the catalog display filter releases the previous session. Multi-root work is sequential, never one atomic plan. Catalog categories, age rules, provenance, exclusions and unsupported targets remain separate from file rows because native rows do not identify their rule.

Reuse the 52rem rail, wrapping paths, native radios, selects, buttons, shared bounded paging and immutable review. App recovery defaults to same-volume manual undo without reclaiming space or automatic purge; permanent deletion requires Windows confirmation, with no Recycle Bin fallback. IPC-mocked interaction tests are not native dialog, Narrator or visual evidence; see `docs/verification/cleaner.md`.

## Step 8 automated integration evidence

The remaining-route harness reuses the isolated headless Edge/CDP wrapper and preserves the earlier component, analyzer and large-files scenarios. Actual cleaner, duplicates, empty-folders, browser and uninstaller routes receive explicitly labelled synthetic native DTO replay, not live filesystem or vendor authority. Keyboard review/focus, bounded paging, nullable metadata, opt-in reset and scrollbar-aware 320px reflow are assertions, not new visual patterns. Representative forced-colors and 200% text checks remain browser-only evidence.

The earlier startup and skip-link failures have been reproduced and resolved. Strict preview 1521 runs the actual-route replay checks. The minimized built-app smoke now exercises Enter activation of the skip link on all eight routes, 320/640/1280px initial-state reflow, and emulated forced-colors reflow without desktop input or screen capture. Read-only installed-program scans use the real immutable Tauri bridge, with repeated rendered refresh/page/unmount flows and separate native scan/page/cancel/release authority checks. Resource observations cover only the native process, not the WebView process tree.

Installed-program review receives focus after explicit preparation, confirmation or cancellation completes; polling does not repeatedly move focus. A scope change discards pending focus intent. The review region is programmatically focusable without adding a tab stop, matching the shared cleanup review pattern. This behavior has a reproduced frontend regression test, not a live vendor execution test.

See `docs/verification/storage-ui.md` for measured scope and remaining gaps. Manual Narrator, full native keyboard workflows and actual Windows-dialog evidence remain unverified, so step 8 is not complete.

## Surface and audience

Supa Diska Klinah is a Windows desktop utility for developers managing local disk use. The Cleanup flow must make the safety boundary obvious before users grant repeatable process-launch authority or allow artifact quarantine.

## Design thesis

Use the existing flat, bordered utility surfaces and shared 52rem Cleanup rail. Present profile registration as a precise native form, saved profiles as an operational list, and budget evidence as totals followed by selected and protected generations. The primary action is native review and registration; destructive-looking actions always explain their real effect.

## Reuse map

- Existing page headers, buttons, danger buttons, status/error treatments, accounting lists, fields, spacing, colors, focus styles, forced-colors rules, and responsive breakpoint remain authoritative.
- Native inputs, selects, fieldsets, lists, headings, and status regions preserve platform semantics.
- No new icon, font, animation, elevation, gradient, or decorative card system is introduced.

## States and behavior

- Loading, empty, error, disabled, queued, running, cancelled, failed, analysis-failed, selected, and protected-floor states remain in normal reading order.
- Arguments are separate repeatable fields; artifact rows pair a relative path with a role.
- Running profiles expose Cancel. Forget explains that it removes authority without deleting artifacts.
- Settings name every unit and keep automatic enforcement off by default.
- At narrow widths, grids collapse to one column and actions become full width.

## Accessibility scope

Changed scope covers Cleanup profile registration, saved runs, budget preview, and Settings budget controls. Native controls provide names, roles, values, keyboard behavior, and validation. Async outcomes use status or alert regions. Focus remains visible through existing `:focus-visible` rules. The layout supports 320 CSS-pixel reflow, text expansion, reduced motion, and forced colors. Screen-reader, 200% zoom, and Windows high-contrast manual release evidence remain required before any conformance claim.
