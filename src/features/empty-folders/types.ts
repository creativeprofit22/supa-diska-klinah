import type { CandidateEligibility, Completeness } from "../../shared/storage/types";
export interface EmptyFolderRow { kind: "emptyFolder"; record: { recordId: string; displayPath: string; depth: number; descendantDirectories: number; completeness: Completeness; eligibility: CandidateEligibility } }
export function candidateId(row: EmptyFolderRow): string | null { return row.record.eligibility.kind === "eligible" ? row.record.eligibility.candidate_id : null; }
