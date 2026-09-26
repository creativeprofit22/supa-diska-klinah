import type { FirewallProfile, RiskLevel } from "../../shared/system-change/types";

export type { FirewallProfile };
export type FirewallAction = "allow" | "block" | "unknown";
export type RuleDirection = "inbound" | "outbound" | "unknown";

export interface FirewallProfileStatus {
  profile: FirewallProfile;
  enabled: boolean;
  defaultInboundAction: FirewallAction;
  defaultOutboundAction: FirewallAction;
  blockAllInboundTraffic: boolean;
  active: boolean;
}

export interface FirewallRule {
  name: string;
  enabled: boolean;
  direction: RuleDirection;
  action: FirewallAction;
  /** NET_FW_PROFILE_TYPE2 bitmask (1 domain, 2 private, 4 public). */
  profiles: number;
  applicationName: string | null;
  localPorts: string;
  remoteAddresses: string;
  grouping: string | null;
}

export interface FirewallFinding {
  id: string;
  severity: RiskLevel;
  title: string;
  detail: string;
  relatedRule: string | null;
  relatedProfile: FirewallProfile | null;
}

export interface FirewallStatus {
  profiles: FirewallProfileStatus[];
  currentProfiles: FirewallProfile[];
  ruleCount: number;
  rulesTruncated: boolean;
  rules: FirewallRule[];
  findings: FirewallFinding[];
}
