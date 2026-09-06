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

export type ArtifactEcosystem =
  | "rust"
  | "nodeJs"
  | "nextJs"
  | "angular"
  | "nuxt"
  | "vite"
  | "svelteKit"
  | "astro"
  | "python"
  | "dotNet"
  | "gradle"
  | "maven"
  | "cmake"
  | "unity"
  | "unreal"
  | "godot";
export type ArtifactType =
  | "installedDependencies"
  | "buildOutput"
  | "compilerCache"
  | "frameworkCache"
  | "virtualEnvironment"
  | "testCache"
  | "generatedIntermediate"
  | "importedAssetCache";

export interface ArtifactIntelligence {
  ecosystem: ArtifactEcosystem;
  artifactType: ArtifactType;
  confidence: "high" | "medium";
  recoverability: "rebuildable";
  rebuildConsequence:
    | "localRebuild"
    | "networkDownloadRequired"
    | "toolchainRequired"
    | "expensiveReimport";
}

export interface ProjectArtifactRecord extends PreviewRecord {
  ageSeconds?: number | null;
  activity: "idle" | "inUse";
  projectName: string;
  projectPath: string;
  artifact: ArtifactIntelligence;
  risk: "safe" | "recoverable" | "highImpact";
  defaultSelected: false;
}

export interface ProjectRoot {
  id: string;
  displayPath: string;
  paused: boolean;
  addedAtUnixSeconds: number;
  lastScannedAtUnixSeconds?: number | null;
}

export interface ProjectArtifactDiscovery {
  roots: ProjectRoot[];
  records: ProjectArtifactRecord[];
  diagnostics: ScanDiagnostic[];
  scannedAtUnixSeconds: number;
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

export function listProjectRoots(): Promise<ProjectRoot[]> {
  return invoke<ProjectRoot[]>("list_project_roots");
}

export function addProjectRoot(path: string): Promise<ProjectRoot[]> {
  return invoke<ProjectRoot[]>("add_project_root", { path });
}

export function setProjectRootPaused(rootId: string, paused: boolean): Promise<ProjectRoot[]> {
  return invoke<ProjectRoot[]>("set_project_root_paused", { rootId, paused });
}

export function removeProjectRoot(rootId: string): Promise<ProjectRoot[]> {
  return invoke<ProjectRoot[]>("remove_project_root", { rootId });
}

export function discoverProjectArtifacts(
  rootId?: string,
): Promise<ProjectArtifactDiscovery> {
  return invoke<ProjectArtifactDiscovery>("discover_project_artifacts", { rootId });
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
