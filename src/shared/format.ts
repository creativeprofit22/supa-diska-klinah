import { DEFAULT_LOCALE, type Locale } from "./i18n/locale";

export function formatNumber(value: number, locale: Locale = DEFAULT_LOCALE): string {
  return new Intl.NumberFormat(locale, { maximumFractionDigits: 0 }).format(value);
}

/** Binary byte size (1 KB = 1024 B). Unit symbols are the same in English and Spanish. */
export function formatBytes(bytes: number, locale: Locale = DEFAULT_LOCALE): string {
  if (bytes < 1024) return `${formatNumber(bytes, locale)} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = -1;
  do {
    value /= 1024;
    unit += 1;
  } while (value >= 1024 && unit < units.length - 1);
  return `${new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(value)} ${units[unit]}`;
}

/** Local date and time for a Unix timestamp in seconds; null when out of range. */
export function formatDateTime(unixSeconds: number, locale: Locale = DEFAULT_LOCALE): string | null {
  const date = new Date(unixSeconds * 1000);
  if (!Number.isFinite(date.getTime())) return null;
  return new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeStyle: "short" }).format(date);
}

export type PluralForms = Readonly<{ one: string; other: string; many?: string }>;

/** Picks a plural form with CLDR rules for the locale (Spanish has a "many" category). */
export function selectPlural(count: number, forms: PluralForms, locale: Locale = DEFAULT_LOCALE): string {
  const category = new Intl.PluralRules(locale).select(count);
  if (category === "one") return forms.one;
  if (category === "many" && forms.many !== undefined) return forms.many;
  return forms.other;
}
