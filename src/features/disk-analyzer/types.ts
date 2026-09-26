import type { Completeness } from "../../shared/storage/types";
export interface DirectorySummary {
  nodeId: string;
  parentId: string | null;
  displayPath: string;
  logicalBytes: number;
  allocatedBytes: number | null;
  independentFiles: number;
  hardLinkEntries: number;
  completeness: Completeness;
}
export interface ExtensionSummary {
  extension: string;
  fileCount: number;
  logicalBytes: number;
  allocatedBytes: number | null;
  completeness: Completeness;
}
export type AnalyzerRow = { kind: "directory"; record: DirectorySummary } | { kind: "extension"; record: ExtensionSummary };
