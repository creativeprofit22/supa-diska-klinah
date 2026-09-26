/** Mirrors `windows_platform::power` status structs (serde camelCase). */
export interface HibernationStatus {
  supported: boolean;
  enabled: boolean;
  hiberfileBytes: number | null;
}

export interface PowerSchemeInfo {
  /** Power scheme GUID. */
  id: string;
  name: string;
  active: boolean;
}

export interface PowerStatus {
  hibernation: HibernationStatus;
  schemes: PowerSchemeInfo[];
}
