export interface CatalogTarget {
  catalogId: string; targetId: string; path: string; source: string; revision: string;
  ruleVersion: number; minimumAgeSeconds: number; consequence: string;
  exclusions: string[]; matcher: string; unsupportedReason: string | null;
}
export interface CleanerCatalog { targets: CatalogTarget[]; unsupportedOperations: [string, string][] }
export interface FileRow {
  kind: "file";
  record: {
    recordId: string; displayPath: string; logicalBytes: number; allocatedBytes: number | null;
    modifiedUnixSeconds: number | null;
    eligibility: { kind: "eligible"; candidate_id: string } | { kind: "readOnly" } | { kind: "ineligible"; reason: string };
  };
}
export const candidateId = (row: FileRow) => row.record.eligibility.kind === "eligible" ? row.record.eligibility.candidate_id : null;
