import { formatDateTime } from "../../shared/format";
import { DEFAULT_LOCALE, type Locale } from "../../shared/i18n/locale";

export { formatBytes } from "../../shared/format";

export function formatModified(seconds: number, locale: Locale = DEFAULT_LOCALE): string {
  return formatDateTime(seconds, locale) ?? "";
}
