/** Mirrors `windows_platform::restore` serde shapes (camelCase). */
export type RestorePointKind = "applicationInstall" | "applicationUninstall" | "deviceDriverInstall" | "modifySettings" | "cancelledOperation" | "other";
export interface RestorePoint {
  sequenceNumber: number;
  description: string;
  /** Unix seconds (UTC). */
  createdAt: number | null;
  restorePointType: number | null;
  kind: RestorePointKind;
}
export type RestorePointListStatus = "available" | "requiresAdministrator" | "unavailable";
export interface RestorePointList { status: RestorePointListStatus; points: RestorePoint[] }
export interface RestoreProtection {
  policyDisabled: boolean;
  protectionEnabled: boolean | null;
  creationFrequencyMinutes: number;
}
