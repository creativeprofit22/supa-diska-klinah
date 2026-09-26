import { useStrings } from "../i18n/I18nProvider";
import { systemChangeStrings, type SystemChangeStrings } from "./strings";
import type { ChangeOutcome, PlannedChange, Reversibility } from "./types";

export type SystemChangeLabels = ReturnType<typeof systemChangeLabels>;

/** Localized labels for system-change values, bound to one catalog. */
export function systemChangeLabels(t: SystemChangeStrings) {
  const reversibility = (value: Reversibility): string => {
    switch (value.kind) {
      case "reversible": return t.reversible;
      case "reversibleWithBackup": return t.reversibleWithBackup;
      case "irreversible": return t.irreversible(value.reason);
    }
  };
  const outcome = (value: ChangeOutcome | null): string => {
    if (value === null) return t.outcomeInterrupted;
    switch (value.status) {
      case "applied": return t.outcomeApplied;
      case "alreadyApplied": return t.outcomeAlreadyApplied;
      case "stateChanged": return t.outcomeStateChanged;
      case "denied": return t.outcomeDenied;
      case "unsupported": return t.outcomeUnsupported(t.unsupported[value.reason]);
      case "failed": return value.code === "notAttempted" ? t.outcomeNotAttempted : t.outcomeFailed;
    }
  };
  const plannedMeta = (change: PlannedChange): string[] => [
    t.risk[change.impact.risk],
    reversibility(change.reversibility),
    t.restart[change.impact.restart],
    change.privilege === "helper" ? t.needsAdministrator : "",
  ].filter((value) => value.length > 0);
  return { t, risk: t.risk, unsupported: t.unsupported, rollback: t.rollback, reversibility, outcome, plannedMeta };
}

export function useSystemChangeLabels(): SystemChangeLabels {
  return systemChangeLabels(useStrings(systemChangeStrings));
}

export const succeeded = (outcome: ChangeOutcome | null): boolean =>
  outcome?.status === "applied" || outcome?.status === "alreadyApplied";
