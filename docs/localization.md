# Localization

The app, its native Windows dialogs and its installer are available in English and Latin American Spanish.

## Supported languages

| Locale | Language | Status |
| --- | --- | --- |
| `en` | English | Source language |
| `es-419` | Español (Latinoamérica) | **Reviewed:** accepted 2026-09-26 by the user, a Spanish speaker; no separate string-by-string pass. See [review rule](#review-rule). |

The NSIS installer is built with English and Spanish (`bundle.windows.nsis.languages` in `src-tauri/tauri.conf.json`). It has no language selector and follows Windows.

## How the language is chosen

In **Settings → Language** you can pick **System (Windows)**, **English** or **Español (Latinoamérica)**. The choice is saved in `app-settings.json` and applies to the web UI and to native dialogs.

With **System**:

- The web UI reads the Windows display languages in order (`systemLanguages` and `resolveLocale` in `src/shared/i18n/locale.ts`).
- Native dialogs read the same list through `GetUserPreferredUILanguages` (`src-tauri/crates/windows-platform/src/i18n.rs`).
- Every Spanish tag (`es`, `es-MX`, `es-ES`, `es-419`, …) resolves to `es-419`. English tags resolve to `en`. The first supported language wins. If none is supported, the app uses English.

The UI sets `<html lang>` to the active locale. Dates, numbers and sizes use locale-aware formatters (`useFormat` in `src/shared/i18n/I18nProvider.tsx`).

## Adding or changing text

- Web UI text lives in a per-feature catalog: `src/features/<feature>/strings.ts`, or `src/shared/<area>/strings.ts`. Each file exports an `en` object and an `es419` object.
- `es419` is declared `satisfies Catalog<typeof en>` (`src/shared/i18n/catalog.ts`). A missing key, an extra key or a different function signature is a compile error.
- Components read text with `useStrings(...)`. Never write text in JSX.
- `pnpm check:i18n` (`scripts/check-i18n.mjs`) rejects hard-coded user-visible text in `.tsx` files under `src/features` and `src/shared`. This covers JSX text, and literals in `aria-label`, `aria-description`, `title`, `placeholder`, `alt` and `label`.
- Native text (confirmation dialogs, update prompts) lives in the `NativeStrings` table in `src-tauri/crates/windows-platform/src/i18n.rs`. Add both languages there.
- Product names, such as Supa Diska Klinah and Windows Update, are not translated.
- When you change English text, update the Spanish text in the same change, and flag it for review.

## Adding a language

1. Add the locale to `SUPPORTED_LOCALES` and `LANGUAGE_PREFERENCES` in `src/shared/i18n/locale.ts`, and teach `matchLocale` its tags.
2. Add a catalog object to every `strings.ts`. The `Catalog` type makes the compiler list every missing string.
3. Add the locale to `Locale`, `LanguagePreference` and `match_locale` in `windows-platform/src/i18n.rs`, and add a `NativeStrings` table.
4. Add the option label, written in that language, to the Settings language options.
5. Add the NSIS language to `bundle.windows.nsis.languages`.
6. Run the axe audit (`src/app/a11y.test.tsx`) in the new locale.
7. Right-to-left languages are not supported yet. New CSS uses logical properties so that RTL remains possible.
8. Follow the review rule before release.

## Review rule

Every `es-419` string needs review by a native Latin American Spanish speaker before a release. Machine translation is not accepted unless it has been reviewed. The same rule applies to any language added later.

The current `es-419` catalog was **accepted on 2026-09-26** by the user, a Spanish speaker. The review was based on using the Spanish UI, native dialogs and installer during the release acceptance runs; no separate string-by-string pass over every catalog entry was recorded. It is tracked as a manual item in [release verification](verification/release.md#manual-items). Text changed after that date needs review again.
