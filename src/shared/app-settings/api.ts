import { invoke } from "@tauri-apps/api/core";
import { isLanguagePreference, type LanguagePreference } from "../i18n/locale";

export type AppSettings = Readonly<{
  schemaVersion: 1;
  language: LanguagePreference;
  /** Opt-in: no update server is contacted while this is false. */
  updateCheck: boolean;
}>;

export const DEFAULT_APP_SETTINGS: AppSettings = { schemaVersion: 1, language: "system", updateCheck: false };

/** Validates the IPC response instead of trusting its declared type. */
export function parseAppSettings(value: unknown): AppSettings | null {
  if (typeof value !== "object" || value === null) return null;
  const record = value as Record<string, unknown>;
  const keys = Object.keys(record).sort().join(",");
  if (keys !== "language,schemaVersion,updateCheck") return null;
  if (record.schemaVersion !== 1 || !isLanguagePreference(record.language) || typeof record.updateCheck !== "boolean") {
    return null;
  }
  return { schemaVersion: 1, language: record.language, updateCheck: record.updateCheck };
}

export type AppSettingsResult = { ok: true; value: AppSettings } | { ok: false; error: "invalidResponse" | "failed" };

export async function getAppSettings(): Promise<AppSettingsResult> {
  try {
    const parsed = parseAppSettings(await invoke<unknown>("get_app_settings"));
    return parsed ? { ok: true, value: parsed } : { ok: false, error: "invalidResponse" };
  } catch {
    return { ok: false, error: "failed" };
  }
}

export async function setAppSettings(settings: AppSettings): Promise<AppSettingsResult> {
  if (!parseAppSettings(settings)) return { ok: false, error: "invalidResponse" };
  try {
    const parsed = parseAppSettings(await invoke<unknown>("set_app_settings", { settings }));
    return parsed ? { ok: true, value: parsed } : { ok: false, error: "invalidResponse" };
  } catch {
    return { ok: false, error: "failed" };
  }
}
