import type { StartupLocation, StartupScope } from "../../shared/system-change/types";

/** Mirrors `windows_platform::startup_items::StartupSource`. */
export type StartupSource = "run" | "run32" | "startupFolder" | "runOnce" | "logonTask";

/** Mirrors `windows_platform::startup_items::StartupItem` (serde camelCase). */
export interface StartupItem {
  name: string;
  scope: StartupScope;
  /** Set for toggleable kinds; null for RunOnce and logon tasks. */
  location: StartupLocation | null;
  source: StartupSource;
  /** Display only. */
  command: string;
  enabled: boolean;
  toggleable: boolean;
}
