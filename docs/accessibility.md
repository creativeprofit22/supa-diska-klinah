# Accessibility

The goal is WCAG 2.2 AA for the whole app in English and Latin American Spanish. This page lists what is in place, how it is checked, and what is still open.

## What is supported

- Full keyboard operation, with a visible focus indicator.
- Landmarks, headings and labels that screen readers can use.
- Status and error announcements.
- Text sizes in `rem`, so the app follows the Windows text-size setting.
- The reduced-motion preference and Windows contrast themes (forced colors).
- Large result lists split into pages.

## Keyboard

- The first Tab stop is a **Skip to content** link. It moves focus to the main region.
- Every interactive element shows a `:focus-visible` outline.
- Sidebar links are reached by Tab in visual order. The current page is marked with `aria-current="page"`.
- Modal dialogs, such as the cleanup confirmation and the storage plan review, trap Tab inside the dialog. Escape closes them, and focus returns to the control that opened them.

`src/app/keyboard.test.tsx` tests all of the above.

## Screen readers

- Progress, completion and result totals are announced through `role="status"` regions. Errors use `role="alert"`.
- Large results announce their total once, not once per row. `src/app/large-results.test.tsx` checks this for 100 000 records.
- `<html lang>` follows the active language, so screen readers pronounce Spanish text correctly.
- Native confirmation dialogs are standard Windows message boxes.

The full Narrator pass has not been done yet. See [known gaps](#known-gaps).

## Text size and zoom

- Font sizes use `rem`. Layout adapts to larger text.
- The window's minimum size is 720 × 480 (`src-tauri/tauri.conf.json`).
- At 200 % text scale in that window (360 × 240 CSS px), content reflows, there is no horizontal scrolling, and controls stay reachable. This was checked on 2026-09-26. See [accessibility verification](verification/accessibility.md#200--text-scale).
- Large lists, such as storage scan results and the cleanup preview, show 100 rows per page. This keeps the page responsive at any zoom level.

## Motion and contrast

- `prefers-reduced-motion: reduce` turns off transitions, animations and smooth scrolling.
- With `forced-colors: active` (Windows contrast themes), borders and the current-page marker use system colors, so they stay visible.
- Text color tokens meet a contrast ratio of at least 4.5:1 on every surface they are drawn on.

## Automated checks

- `pnpm check:a11y` runs `scripts/check-contrast.mjs`. It checks the 4.5:1 contrast of color tokens and color pairs in `src/styles.css`, and checks that the reduced-motion block neutralizes every transition and animation.
- `pnpm test` runs `src/app/a11y.test.tsx`. It audits every route with axe-core in English (`en-US`) and Spanish (`es-MX`), and fails on serious or critical violations. A sensitivity test proves that the audit catches the violations it is meant to catch.
- `pnpm test` also runs `src/app/keyboard.test.tsx` and `src/app/large-results.test.tsx`.
- `pnpm check:i18n` makes sure accessible names (`aria-label`, `title`, `alt`, …) come from translated catalogs.

## Known gaps

- **The manual Narrator pass is pending.** Automated checks cannot confirm how things are actually announced. See [accessibility verification](verification/accessibility.md#manual-narrator-pass).
- Right-to-left layout is not supported, because no RTL language ships.
- The 200 % check covered the dashboard, large files and settings pages in `es-419`. Other pages rely on the same layout rules and have not been checked by hand.
