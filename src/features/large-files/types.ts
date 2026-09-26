import type { StorageFileRecord } from "../../shared/storage/types";
export const categories = ["any", "documents", "images", "audio", "video", "archives", "other"] as const;
export const sorts = ["size", "modified", "path"] as const;
export interface LargeFileFilter {
  minimumBytes: number;
  maximumBytes: number | null;
  extensions: string[];
  category: typeof categories[number];
  sort: typeof sorts[number];
  descending: boolean;
}
export interface LargeFileRow {
  kind: "file";
  record: StorageFileRecord;
}
export function candidateId(row: LargeFileRow): string | null {
  return row.record.eligibility.kind === "eligible" ? row.record.eligibility.candidate_id : null;
}
