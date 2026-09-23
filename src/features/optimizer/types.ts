import type { PlannedChange } from "../../shared/system-change/types";

/** Mirrors `windows_platform::optimizer::ProposalGroup` (serde camelCase). */
export type ProposalGroup = "services" | "privacy" | "performance" | "power";

/** Mirrors `windows_platform::optimizer::Proposal`. */
export interface Proposal {
  group: ProposalGroup;
  label: string;
  /** Pre-selected only for low-risk, reversible proposals. */
  suggested: boolean;
  planned: PlannedChange;
}

/** Mirrors `windows_platform::optimizer::OptimizerReport`. */
export interface OptimizerReport {
  proposals: Proposal[];
  alreadyApplied: number;
  unavailable: number;
}
