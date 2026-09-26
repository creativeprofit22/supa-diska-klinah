import type { UnsupportedReason } from "../../shared/system-change/types";

/** Mirrors `windows_platform::privacy` report structs (serde camelCase). */
export type SettingHive = "user" | "machine";
export type SettingCategory = "privacy" | "performance";

export interface PrivacySettingReport {
  id: string;
  hive: SettingHive;
  label: string;
  description: string;
  category: SettingCategory;
  current: number | null;
  recommended: number | null;
  applied: boolean;
  supported: boolean;
  unsupportedReason: UnsupportedReason | null;
}

export interface PrivacyTaskReport {
  id: string;
  path: string;
  label: string;
  enabled: boolean | null;
  recommended: boolean;
  present: boolean;
}

export interface PrivacyReport {
  settings: PrivacySettingReport[];
  tasks: PrivacyTaskReport[];
  relatedServices: string[];
}
