# Manual Narrator / keyboard / scaling / forced-colors acceptance session

Purpose: close the human-only evidence gap called out in `docs/verification/storage-ui.md`
("Evidence still outstanding") and `DESIGN.md` (step 8 note: "Manual Narrator, full native
keyboard workflows and actual Windows-dialog evidence remain unverified"). This session covers
what no automated script can: hearing Narrator, real OS-level 200% scaling, real forced-colors
mode, and real native dialog focus return.

Ship decision context: VERIFY-BEFORE-SHIP, Medium severity, Regression lane,
UNTESTED-BLAST-RADIUS. Use **disposable/fictitious data only**. Never confirm a real permanent
delete or a real vendor uninstall unless you deliberately created the disposable target
yourself. When a native confirmation dialog appears, its default button must be **No/Cancel** —
confirm that, then either back out or proceed only against your own disposable fixture.

## 0. Build the release candidate once

```
pnpm tauri build --debug --no-bundle
```

Record the resulting binary's hash for the evidence log:

```
Get-FileHash src-tauri/target/debug/supa-diska-klinah.exe -Algorithm SHA256
```

Launch it normally (not minimized, not through the smoke script) for this session.

## 1. Turn on the tools before you start

- **Keyboard-only**: put the mouse aside. Tab / Shift+Tab / Enter / Space / Arrow keys / Esc only.
- **Narrator**: `Ctrl+Win+Enter` to start, same to stop. Increase or decrease speech rate with
  `Ctrl+Win+Plus/Minus` if it talks too fast to transcribe.
- **200% scaling**: Settings → System → Display → Scale → 200%. Sign out/in only if the app
  doesn't pick it up live.
- **Forced colors**: Settings → Accessibility → Contrast themes → pick a theme → Apply. (This is
  a real Windows high-contrast/forced-colors mode, not an emulated media query.)
- Do all four together where practical; note if you test them in separate passes instead — that's
  fine, just say so per row in the results table.

Each is a real global Windows setting change — you're doing this on your own machine with your
own approval, not me changing it for you. Revert all four when the session ends.

## 2. Routes and cases to cover

The eight storage workflows, in nav order: **Disk analyzer, Large files, Duplicates,
Empty folders, Rule cleaner, Browser caches, Uninstaller, Cleanup (history/outcome)**.

For every route, keyboard-only:

1. From the shell nav, Tab to the route's link, Enter to go there. Confirm focus lands somewhere
   sensible (heading or skip-link target), not lost to `<body>`.
2. Narrator announces a heading/landmark when the route loads (listen for it, write down roughly
   what it said).
3. Tab through every control in document order; confirm the visible focus ring never disappears
   and never jumps somewhere unexpected.
4. Trigger the route's chooser/scan action (see per-route notes) with Enter/Space only.
5. Trigger at least one validation error (e.g. an out-of-range depth) and confirm Narrator speaks
   the error without you having to hunt for it, and that repeated polling doesn't make Narrator
   repeat the same status over and over ("poll spam").
6. If the route pages results, page forward/back with keyboard only.
7. At 320px-equivalent width (resize the window narrow, or use 200% scale on a small display) and
   again at real 200% scaling: confirm no horizontal scrollbar/clipped controls, and long paths
   wrap instead of overflowing.
8. Under forced colors: confirm text, borders and the focus ring are still visible (not "invisible
   because color-only").

Per-route specifics:

- **Disk analyzer**: "Choose folder" opens the native folder picker — cancel it once (Esc) and
  confirm the previous view is preserved, then pick a disposable folder. Tab into a folder
  breadcrumb button and confirm focus moves to the folder heading.
- **Large files**: set Scan filters/Scan depth, "Scan for large files", page results, sort by
  Category, select then "Clear selection" with keyboard.
- **Duplicates**: scan a disposable folder with duplicate files, "Review group", confirm the
  first-page keeper's checkbox is disabled/cannot be unchecked with keyboard, "Clear selection".
- **Empty folders**: scan a folder with a nested empty folder and confirm the empty-only/no-undo
  warning text is reachable and announced, review then clear selection.
- **Rule cleaner**: "Refresh native scopes", page scopes, pick a scope, "Authorize selected
  scope", page catalog rules, "Scan cleaner scope", review, reach "Continue"/confirmation step —
  stop before actually deleting anything unless it's your disposable fixture.
- **Browser caches**: same authorize/scan/review flow; toggle "Include service-worker caches"
  with Space.
- **Uninstaller**: "Refresh inventory", page installed programs, select a disposable/test entry
  (never a real installed program you care about), "Continue to Windows confirmation" — confirm
  the native dialog's default is No, press Esc/No, confirm focus returns to a sensible control in
  the app (not lost). Check "Refresh retained history" and paging of retained vendor jobs.
- **Cleanup (history/outcome)**: trigger a disposable delete/move-to-Recycle-Bin flow, confirm the
  native confirmation dialog defaults to No, cancel it once and confirm focus return, then
  (optional, disposable data only) confirm once and check outcome/history/undo controls are
  reachable and Narrator announces the outcome without spamming during polling.

## 3. Record results

Fill one row per route/case in `docs/verification/storage-ui.md` under a new "Manual session"
table: route, case, keyboard-only pass/fail, Narrator announcement heard (paraphrase, not a
transcript), 320px/200%-scale pass/fail, forced-colors pass/fail, native dialog default+focus
return pass/fail, notes. Mark anything you couldn't get to as `unverified`, not pass.

Report back here with: app hash, Windows/Narrator version, which of the four conditions you
tested together vs separately, and the filled-in table (or paste the raw notes and I'll format
and file them, and fix any concrete defect you hit with a targeted regression test).
