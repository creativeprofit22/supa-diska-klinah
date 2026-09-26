export type StorageModule = "cleaner" | "diskAnalyzer" | "largeFiles" | "duplicates" | "emptyFolders" | "browser" | "uninstaller" | "drives";
export type PageCollection = "files" | "tree" | "extensions" | "duplicateGroups" | "duplicateMembers" | "emptyFolders" | "programs" | "drives";
export type StoragePhase = "queued" | "walking" | "grouping" | "partialHash" | "fullHash" | "finalizing" | "complete" | "cancelled" | "failed";
export interface Completeness { reasons: readonly string[] }
export type CandidateEligibility = { kind: "eligible"; candidate_id: string } | { kind: "readOnly" } | { kind: "ineligible"; reason: string };
export interface StorageFileRecord {
  recordId: string;
  displayPath: string;
  logicalBytes: number;
  allocatedBytes: number | null;
  modifiedUnixSeconds: number | null;
  eligibility: CandidateEligibility;
}
export interface StorageStatus {
  snapshotId: string;
  module: StorageModule;
  phase: StoragePhase;
  visitedEntries: number;
  retainedRecords: number;
  hashedBytes: number;
  completedHashes: number;
  completeness: Completeness;
}
export interface SnapshotInput { module: StorageModule; snapshotId: string }
export interface PageRequest extends SnapshotInput {
  collection: PageCollection;
  parentId?: string;
  cursor?: string;
  pageSize: number;
}
// Feature-owned row types mirror their native tagged records. Shared state never
// retains proof records, paths as authority, or a second copy of previous pages.
export interface StoragePage<Row> {
  snapshotId: string;
  records: readonly Row[];
  nextCursor: string | null;
  retainedTotal: number;
  completeness: Completeness;
}
export interface StorageSelection extends SnapshotInput { readonly candidateIds: readonly string[] }
export interface RootChoice { rootId: string; module: StorageModule; displayPath: string }
export interface NativeScope { scopeId: string; module: StorageModule; label: string; displayPath: string | null; available: boolean }
export interface ScanApi<Row> {
  status(input: SnapshotInput): Promise<StorageStatus>;
  page(input: PageRequest): Promise<StoragePage<Row>>;
  cancel(input: SnapshotInput): Promise<void>;
  release(input: SnapshotInput): Promise<void>;
}
export const PAGE_SIZE = 100;
export const MAX_SELECTION = 1_000;
export const validId = (id: string): boolean => /^[a-f0-9]{32}$/.test(id);
export type ScanPhase = "idle" | "starting" | "scanning" | "ready" | "cancelling" | "cancelled" | "failed";
