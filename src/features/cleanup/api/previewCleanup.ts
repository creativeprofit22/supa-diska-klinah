import { invoke } from "@tauri-apps/api/core";

export type PreviewKind = "file" | "directory";
export type CleanupDisposition = "recycleBin" | "quarantine" | "permanent";
export type CleanupItemState =
  | "pending"
  | "mutating"
  | "recycled"
  | "quarantined"
  | "purged"
  | "restored"
  | "failed"
  | "unknown";

export interface PreviewRecord {
  id: string;
  ruleId: string;
  displayPath: string;
  kind: PreviewKind;
  bytes: number;
  modifiedUnixSeconds?: number | null;
}

export interface ArtifactIntelligence {
  ecosystem: "nodeJs";
  artifactType: "installedDependencies";
  recoverability: "rebuildable";
  rebuildConsequence: "networkDownloadRequired";
}

export interface ProjectArtifactRecord extends PreviewRecord {
  projectName: string;
  projectPath: string;
  artifact: ArtifactIntelligence;
  risk: "recoverable";
  defaultSelected: false;
}

export interface ProjectArtifactDiscovery {
  records: ProjectArtifactRecord[];
  diagnostics: ScanDiagnostic[];
}

export interface ScanDiagnostic {
  ruleId: string;
  path: string;
  reason: string;
}

export interface CleanupPreview {
  scanId: string;
  records: PreviewRecord[];
  diagnostics: ScanDiagnostic[];
}

export interface CleanupPlanSummary {
  planId: string;
  disposition: CleanupDisposition;
  selectedCount: number;
  selectedBytes: number;
}

export interface CleanupItemOutcome {
  itemId: string;
  state: CleanupItemState;
  logicalBytes: number;
  failure?: string | null;
}

export interface ByteAccounting {
  selectedBytes: number;
  processedBytes: number;
  failedBytes: number;
  quarantinedBytes: number;
  purgedBytes: number;
  occupiedBytes: number;
  reclaimedBytes: number;
}

export interface CleanupExecutionSummary {
  executionId: string;
  planId: string;
  disposition: CleanupDisposition;
  completed: boolean;
  purgeAfter?: number | null;
  items: CleanupItemOutcome[];
  accounting: ByteAccounting;
}

export function previewCleanup(): Promise<CleanupPreview> {
  return invoke<CleanupPreview>("preview_cleanup");
}

export function discoverProjectArtifacts(root: string): Promise<ProjectArtifactDiscovery> {
  return invoke<ProjectArtifactDiscovery>("discover_project_artifacts", { root });
}

export function createCleanupPlan(
  scanId: string,
  candidateIds: string[],
  disposition: CleanupDisposition,
): Promise<CleanupPlanSummary> {
  return invoke<CleanupPlanSummary>("create_cleanup_plan", {
    scanId,
    candidateIds,
    disposition,
  });
}

export function executeCleanupPlan(planId: string): Promise<CleanupExecutionSummary> {
  return invoke<CleanupExecutionSummary>("execute_cleanup_plan", { planId });
}

export function executePermanentCleanupPlan(
  planId: string,
): Promise<CleanupExecutionSummary> {
  return invoke<CleanupExecutionSummary>("execute_permanent_cleanup_plan", { planId });
}

export function undoCleanup(executionId: string): Promise<CleanupExecutionSummary> {
  return invoke<CleanupExecutionSummary>("undo_cleanup", { executionId });
}

export function cleanupHistory(): Promise<CleanupExecutionSummary[]> {
  return invoke<CleanupExecutionSummary[]>("cleanup_history");
}
