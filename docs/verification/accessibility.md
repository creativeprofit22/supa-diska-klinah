# Accessibility verification

## Automated

Run with `pnpm test` and `pnpm check:a11y` (2026-09-26, uncommitted release-phase worktree on HEAD `0a062f8`):

- `src/app/a11y.test.tsx`: axe-core 4.13.0 audits every route in English and Spanish with fixture data. Zero serious or critical violations. `color-contrast` is disabled in jsdom and covered by the contrast script instead. A sensitivity test confirms the same audit reports a planted `button-name` and `image-alt` violation, so a pass is not vacuous.
- `src/app/keyboard.test.tsx`: skip link is the first Tab stop and moves focus to `main`; sidebar links are reachable in order; no positive `tabindex`; review dialogs trap focus, close on Escape and return focus to the trigger.
- Large results: a 100,000-record storage result renders at most one page of rows and announces the total once through a single `role=status`.
- `scripts/check-contrast.mjs`: every text token meets 4.5:1 on every surface, rule-level colour pairs pass, transitions and animations are neutralised under `prefers-reduced-motion`, and no stylesheet uses `px` font sizes.

## Manual Narrator pass

**Open.** A person needs to run Narrator (Windows + Ctrl + Enter) through the dashboard, one storage flow (scan → review → cleanup outcome), one system change (review → Windows confirmation → result), Settings (language, updates) and one protection page, in English and Spanish, and record here whether headings, landmarks, statuses and errors are announced. Not yet performed; not accepted.

## 200 % text scale

Checked 2026-09-26 in a 360 × 240 CSS-pixel viewport, which is a 720 × 480 window at 200 % scaling, against a production build in Spanish (the longer language): dashboard, Large files and Settings. The sidebar collapses to two columns, all form controls stack, text wraps, and there is no horizontal scrolling and no action off-screen. Screenshots are in `.gg/screenshots/a11y-360-*.png` (ignored, local). A check at Windows' own 200 % text-size setting on a real display is part of the manual pass above.
