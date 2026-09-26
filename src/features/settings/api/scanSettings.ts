import { invoke } from "@tauri-apps/api/core";

export const SCAN_PROFILES = ["auto", "ssd", "hdd"] as const;
export type ScanProfile = (typeof SCAN_PROFILES)[number];

export interface ScanSettings {
  readonly schemaVersion: 1;
  readonly profile: ScanProfile;
}

export function isScanProfile(value: unknown): value is ScanProfile {
  return typeof value === "string" && (SCAN_PROFILES as readonly string[]).includes(value);
}

/** Validates the IPC response instead of trusting its declared type. */
export function parseScanSettings(value: unknown): ScanSettings {
  if (typeof value !== "object" || value === null) throw new Error("Invalid scan settings response.");
  const record = value as Record<string, unknown>;
  if (record.schemaVersion !== 1 || !isScanProfile(record.profile)) {
    throw new Error("Invalid scan settings response.");
  }
  return { schemaVersion: 1, profile: record.profile };
}

export async function getScanSettings(): Promise<ScanSettings> {
  return parseScanSettings(await invoke<unknown>("get_scan_settings"));
}

export async function setScanProfile(profile: ScanProfile): Promise<ScanSettings> {
  if (!isScanProfile(profile)) throw new Error("Unknown scan profile.");
  return parseScanSettings(await invoke<unknown>("set_scan_settings", { profile }));
}
