export interface BrowserPolicy { source: string; revision: string; lifecycle: string; risk: string; consequence: string; minimumAgeSeconds: number; serviceWorkerDisclosure: string; unsupported: string[]; exclusions: string[]; profileCacheRoots: string[]; sharedCacheRoots: string[] }
export interface FileRow {
  kind: "file";
  record: {
    recordId: string; displayPath: string; logicalBytes: number; allocatedBytes: number | null;
    modifiedUnixSeconds: number | null;
    eligibility: { kind: "eligible"; candidate_id: string } | { kind: "readOnly" } | { kind: "ineligible"; reason: string };
  };
}
export const candidateId = (row: FileRow) => row.record.eligibility.kind === "eligible" ? row.record.eligibility.candidate_id : null;
