import type { Completeness, StorageFileRecord } from "../../shared/storage/types";
export interface DuplicateGroup { groupId: string; memberCount: number; independentCopies: number; bytesPerCopy: number; completeness: Completeness }
export type DuplicateRow = { kind: "duplicateGroup"; record: DuplicateGroup } | { kind: "duplicateMember"; record: { groupId: string; file: StorageFileRecord } };
export function selectableMember(row: DuplicateRow, groupId: string | undefined, keeper: string | null): string | null {
  if (!keeper || row.kind !== "duplicateMember" || row.record.groupId !== groupId || row.record.file.recordId === keeper) return null;
  const eligibility = row.record.file.eligibility;
  return eligibility.kind === "eligible" ? eligibility.candidate_id : null;
}
