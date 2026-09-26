import { createContext, type ReactNode, useContext, useEffect, useMemo, useState } from "react";
import { formatBytes, formatDateTime, formatNumber, type PluralForms, selectPlural } from "../format";
import { type Translations, pickCatalog } from "./catalog";
import { DEFAULT_LOCALE, type LanguagePreference, type Locale, resolveLocale, systemLanguages } from "./locale";

type I18nContextValue = Readonly<{
  locale: Locale;
  preference: LanguagePreference;
  setPreference: (preference: LanguagePreference) => void;
}>;

// Without a provider (isolated component tests) the app renders English.
const I18nContext = createContext<I18nContextValue>({
  locale: DEFAULT_LOCALE,
  preference: "system",
  setPreference: () => undefined,
});

type I18nProviderProps = Readonly<{
  children: ReactNode;
  /** Starting preference; the settings loader replaces it once the saved value arrives. */
  initialPreference?: LanguagePreference;
  /** Injected for tests; defaults to the WebView's language list. */
  languages?: readonly string[];
}>;

export function I18nProvider({ children, initialPreference = "system", languages }: I18nProviderProps): ReactNode {
  const [preference, setPreference] = useState<LanguagePreference>(initialPreference);
  const locale = resolveLocale(preference, languages ?? systemLanguages());

  useEffect(() => {
    document.documentElement.lang = locale;
  }, [locale]);

  const value = useMemo<I18nContextValue>(() => ({ locale, preference, setPreference }), [locale, preference]);
  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n(): I18nContextValue {
  return useContext(I18nContext);
}

/** Returns the active locale's strings for one catalog. */
export function useStrings<T>(translations: Translations<T>): T {
  return pickCatalog(translations, useContext(I18nContext).locale);
}

export type Formatters = Readonly<{
  locale: Locale;
  bytes: (bytes: number) => string;
  number: (value: number) => string;
  /** Local date/time for Unix seconds, or null when the value is out of range. */
  dateTime: (unixSeconds: number) => string | null;
  plural: (count: number, forms: PluralForms) => string;
}>;

export function formattersFor(locale: Locale): Formatters {
  return {
    locale,
    bytes: (bytes) => formatBytes(bytes, locale),
    number: (value) => formatNumber(value, locale),
    dateTime: (unixSeconds) => formatDateTime(unixSeconds, locale),
    plural: (count, forms) => selectPlural(count, forms, locale),
  };
}

/** Locale-bound number, byte, date and plural helpers. */
export function useFormat(): Formatters {
  const { locale } = useContext(I18nContext);
  return useMemo(() => formattersFor(locale), [locale]);
}
