import type { UnsupportedReason } from "../../shared/system-change/types";

/** Mirrors `windows_platform::os_info::Edition`. */
export type Edition = "home" | "pro" | "education" | "enterprise" | "server" | "other";
export interface AllowedOption { value: number; label: string }
/** Mirrors `AllowedValues` (`#[serde(tag = "kind")]`). */
export type AllowedValues =
  | { kind: "options"; options: AllowedOption[] }
  | { kind: "range"; min: number; max: number };
export interface PolicyStatus {
  id: string;
  label: string;
  description: string;
  current: number | null;
  allowed: AllowedValues;
  applied: boolean;
}
/** Mirrors `windows_platform::updates::UpdateStatus`. Dates are Unix seconds. */
export interface UpdateStatus {
  apiAvailable: boolean;
  serviceEnabled: boolean | null;
  lastSearchSuccess: number | null;
  lastInstallSuccess: number | null;
  rebootRequired: boolean | null;
  edition: Edition;
  managed: boolean;
  policySupported: boolean;
  unsupportedReason: UnsupportedReason | null;
  policies: PolicyStatus[];
}
