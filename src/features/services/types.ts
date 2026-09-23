import type { RiskLevel, ServiceStartState, ServiceStartType } from "../../shared/system-change/types";

/** Mirrors `windows_platform::services::ServiceCategory`. */
export type ServiceCategory = "telemetry" | "gaming" | "legacy" | "performance" | "other";

/** Mirrors `windows_platform::services::ServiceItem` (serde camelCase). */
export interface ServiceItem {
  id: string;
  serviceName: string;
  label: string;
  description: string;
  risk: RiskLevel;
  category: ServiceCategory;
  recommended: ServiceStartType;
  installed: boolean;
  /** null when the service is absent or its configuration cannot be read. */
  start: ServiceStartState | null;
  delayedAutoStart: boolean;
  running: boolean;
}
