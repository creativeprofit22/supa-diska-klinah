import type { ChangeOutcome, PlannedChange, Reversibility, RestartRequirement, RiskLevel, UnsupportedReason } from "./types";

export const riskLabel: Record<RiskLevel, string> = { low: "Low risk", medium: "Medium risk", high: "High risk" };
export const restartLabel: Record<RestartRequirement, string | null> = {
  none: null, signOut: "Sign-out required", reboot: "Restart required",
};
export const unsupportedLabel: Record<UnsupportedReason, string> = {
  apiUnavailable: "Windows does not provide this feature here",
  osVersion: "Not supported on this Windows version",
  editionUnsupported: "Not honored by this Windows edition",
  managedDevice: "Managed by your organization",
  notPresent: "Not present on this device",
};

export function reversibilityLabel(reversibility: Reversibility): string {
  switch (reversibility.kind) {
    case "reversible": return "Can be undone";
    case "reversibleWithBackup": return "Can be undone from a backup";
    case "irreversible": return `Cannot be undone: ${reversibility.reason}`;
  }
}

export function outcomeLabel(outcome: ChangeOutcome | null): string {
  if (outcome === null) return "Interrupted before finishing; state unknown";
  switch (outcome.status) {
    case "applied": return "Applied";
    case "alreadyApplied": return "Already in place; nothing changed";
    case "stateChanged": return "Skipped: the setting changed since review";
    case "denied": return "Denied: administrator permission was not granted";
    case "unsupported": return `Skipped: ${unsupportedLabel[outcome.reason]}`;
    case "failed": return outcome.code === "notAttempted" ? "Not attempted" : "Failed";
  }
}

export const succeeded = (outcome: ChangeOutcome | null) =>
  outcome?.status === "applied" || outcome?.status === "alreadyApplied";

export function plannedMeta(change: PlannedChange): string[] {
  return [
    riskLabel[change.impact.risk],
    reversibilityLabel(change.reversibility),
    restartLabel[change.impact.restart],
    change.privilege === "helper" ? "Needs administrator" : null,
  ].filter((value): value is string => value !== null);
}
