/** Locales the app ships. Every catalog must provide all of them. */
export const SUPPORTED_LOCALES = ["en", "es-419"] as const;
export type Locale = (typeof SUPPORTED_LOCALES)[number];

/** The user's language choice: follow Windows, or force one shipped locale. */
export const LANGUAGE_PREFERENCES = ["system", "en", "es-419"] as const;
export type LanguagePreference = (typeof LANGUAGE_PREFERENCES)[number];

export const DEFAULT_LOCALE: Locale = "en";

export function isLanguagePreference(value: unknown): value is LanguagePreference {
  return typeof value === "string" && (LANGUAGE_PREFERENCES as readonly string[]).includes(value);
}

/**
 * Maps one BCP 47 tag to a shipped locale, or null when unsupported.
 * Every Spanish variant (es, es-MX, es-AR, es-ES, es-419, …) resolves to
 * Latin American Spanish; every English variant resolves to English.
 */
export function matchLocale(tag: string): Locale | null {
  const primary = tag.trim().toLowerCase().split(/[-_]/, 1)[0];
  if (primary === "es") return "es-419";
  if (primary === "en") return "en";
  return null;
}

/**
 * Resolves the active locale: an explicit preference wins; otherwise the first
 * supported entry of the OS/WebView language list (WebView2 mirrors the Windows
 * display language); otherwise English.
 */
export function resolveLocale(preference: LanguagePreference, languages: readonly string[]): Locale {
  if (preference !== "system") return preference;
  for (const tag of languages) {
    const match = matchLocale(tag);
    if (match) return match;
  }
  return DEFAULT_LOCALE;
}

/** Reads the WebView's language list without trusting it to be well formed. */
export function systemLanguages(): readonly string[] {
  if (typeof navigator === "undefined") return [];
  const list = Array.isArray(navigator.languages) ? navigator.languages : [];
  const values = list.length > 0 ? list : [navigator.language];
  return values.filter((value): value is string => typeof value === "string" && value.length > 0);
}
