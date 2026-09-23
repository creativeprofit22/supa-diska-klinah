/** Mirrors `cleanup_core::system_change` serde shapes. */
export type StartupScope = "user" | "machine";
export type StartupLocation = "run" | "run32" | "startupFolder";
export type ServiceStartType = "automatic" | "manual" | "disabled";
export type ServiceStartState = "boot" | "system" | ServiceStartType;
export type FirewallProfile = "domain" | "private" | "public";
export type Weekday = "monday" | "tuesday" | "wednesday" | "thursday" | "friday" | "saturday" | "sunday";
export type ScheduleCadence =
  | { kind: "daily"; hour: number; minute: number }
  | { kind: "weekly"; day: Weekday; hour: number; minute: number };
export interface HostsLineOp { line: number; action: "disable" | "restore" }

export type SystemChange =
  | { kind: "setStartupEntry"; entry: { scope: StartupScope; location: StartupLocation; name: string }; enabled: boolean }
  | { kind: "setServiceStartType"; catalogId: string; startType: ServiceStartType }
  | { kind: "setUserSetting"; settingId: string; value: number | null }
  | { kind: "setMachineSetting"; settingId: string; value: number | null }
  | { kind: "setSystemTaskEnabled"; catalogId: string; enabled: boolean }
  | { kind: "setWindowsUpdatePolicy"; settingId: string; value: number | null }
  | { kind: "setFirewallRuleEnabled"; ruleName: string; enabled: boolean }
  | { kind: "setFirewallProfileEnabled"; profile: FirewallProfile; enabled: boolean }
  | { kind: "setHibernation"; enabled: boolean }
  | { kind: "setActivePowerScheme"; scheme: string }
  | { kind: "deleteDriverPackage"; publishedName: string }
  | { kind: "editHosts"; lineOps: HostsLineOp[] }
  | { kind: "createRestorePoint"; description: string }
  | { kind: "upsertScanSchedule"; scheduleId: string; cadence: ScheduleCadence }
  | { kind: "removeScanSchedule"; scheduleId: string };

export type Reversibility = { kind: "reversible" } | { kind: "reversibleWithBackup" } | { kind: "irreversible"; reason: string };
export type RiskLevel = "low" | "medium" | "high";
export type RestartRequirement = "none" | "signOut" | "reboot";
export type UnsupportedReason = "apiUnavailable" | "osVersion" | "editionUnsupported" | "managedDevice" | "notPresent";
export type FailureCode = "systemError" | "helperUnavailable" | "timeout" | "invalidRequest" | "interrupted" | "notAttempted";
export type ChangeOutcome =
  | { status: "applied" } | { status: "alreadyApplied" } | { status: "stateChanged" } | { status: "denied" }
  | { status: "unsupported"; reason: UnsupportedReason }
  | { status: "failed"; code: FailureCode };

export interface ImpactSummary { component: string; effect: string; restart: RestartRequirement; risk: RiskLevel }
export interface PlannedChange {
  change: SystemChange; module: string; privilege: "standard" | "helper"; reversibility: Reversibility;
  impact: ImpactSummary; expectedPrior: unknown; inverse: SystemChange | null;
}
export interface PlanTicket { planId: string; changes: PlannedChange[]; requiresHelper: boolean; expiresInSeconds: number }
export interface ChangeResult { change: SystemChange; outcome: ChangeOutcome; journalEntryId: string | null }
export interface ExecutionReport { planId: string; results: ChangeResult[] }
export type RollbackStatus = "available" | "alreadyRolledBack" | "irreversible" | "interrupted" | "nothingApplied";
export interface JournalEntry {
  id: string; planId: string; recordedAt: number; change: SystemChange; prior: unknown; reversibility: Reversibility;
  inverse: SystemChange | null; outcome: ChangeOutcome | null; rolledBackBy: string | null;
}
export interface JournalView { entry: JournalEntry; rollback: RollbackStatus }
export interface SystemChangeError { code: string; message: string }
