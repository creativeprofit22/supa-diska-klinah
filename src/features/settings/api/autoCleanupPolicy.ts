import { invoke } from "@tauri-apps/api/core";

export interface AutoCleanupPolicy {
  schemaVersion: number;
  enabled: boolean;
  graceDays: number;
}

export function getAutoCleanupPolicy(): Promise<AutoCleanupPolicy> {
  return invoke<AutoCleanupPolicy>("get_auto_cleanup_policy");
}

export function setAutoCleanupPolicy(
  enabled: boolean,
  graceDays: number,
): Promise<AutoCleanupPolicy> {
  return invoke<AutoCleanupPolicy>("set_auto_cleanup_policy", { enabled, graceDays });
}
