import { invoke } from "@tauri-apps/api/core";

export type BuildEcosystem = "rust" | "node" | "generic";
export type ArtifactRole = "generation" | "dependency" | "incremental";
export type RebuildCost = "low" | "medium" | "high";
export type BuildRunState =
  | "queued"
  | "running"
  | "succeeded"
  | "failed"
  | "cancelled"
  | "analysisFailed";

export interface RegisteredArtifactPath {
  relativePath: string;
  role: ArtifactRole;
}

export interface RegisterBuildProfileInput {
  rootId: string;
  displayName: string;
  ecosystem: BuildEcosystem;
  executable: string;
  argv: string[];
  workingDirectory: string;
  profileLabel: string;
  toolchainLabel: string;
  targetLabel: string;
  rebuildCost: RebuildCost;
  artifactPaths: RegisteredArtifactPath[];
}

export interface BuildProfile extends RegisterBuildProfileInput {
  profileId: string;
  executableIdentity: { volume: number; file: number };
}

export interface BuildRun {
  runId: string;
  profileId: string;
  state: BuildRunState;
  startedAt?: number | null;
  completedAt?: number | null;
  exitCode?: number | null;
}

export interface BudgetLimits {
  maximumAllocatedBytes?: number | null;
  maximumAgeSeconds?: number | null;
}

export type ProjectBudgetOverride =
  | { mode: "inherit" }
  | { mode: "disabled" }
  | { mode: "explicit"; limits: BudgetLimits };

export interface ArtifactBudgetPolicy {
  schemaVersion: 1;
  enabled: boolean;
  globalLimits: BudgetLimits;
  scheduledAnalysisIntervalSeconds: number;
  staleChangeGraceSeconds: number;
  quarantineGraceSeconds: number;
  projectOverrides: { rootId: string; policy: ProjectBudgetOverride }[];
}

export interface SetArtifactBudgetPolicyResult {
  policySaved: true;
  policy: ArtifactBudgetPolicy;
  analysisStatus: "notRequested" | "completed" | "skipped" | "failed";
}

export interface BuildArtifactCommandError {
  code: string;
  message: string;
}

export function isBuildArtifactCommandError(
  error: unknown,
): error is BuildArtifactCommandError {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    typeof error.code === "string" &&
    "message" in error &&
    typeof error.message === "string"
  );
}

export type ProtectionReason =
  | "unowned"
  | "nonGeneration"
  | "active"
  | "currentProfile"
  | "currentTarget"
  | "dependency"
  | "incremental"
  | "latestSuccessfulBuild"
  | "recentExternalChange"
  | "unreadable"
  | "ambiguous"
  | "missingIdentity";

export interface ProtectedGeneration {
  generationId: string;
  rootId: string;
  allocatedBytes: number;
  reasons: ProtectionReason[];
}

export interface BudgetDecision {
  selectedGenerationIds: string[];
  protected: ProtectedGeneration[];
  currentAllocatedBytes: number;
  projectedAllocatedBytes: number;
  quarantineBytes: number;
  unsatisfiedProtectedByteFloor?: number | null;
  projectCurrentBytes: Record<string, number>;
  projectProjectedBytes: Record<string, number>;
  projectProtectedByteFloors: Record<string, number>;
}

export interface GenerationState {
  generationId: string;
  profileId: string;
  rootId: string;
  normalizedPath: string;
  allocatedBytes: number;
  lastSuccessfulTouch?: number | null;
  lastExternalChange?: number | null;
  profileLabel: string;
  toolchainLabel: string;
  targetLabel: string;
  rebuildCost: RebuildCost;
  owned: boolean;
  role: ArtifactRole;
  active: boolean;
  readable: boolean;
  ambiguous: boolean;
  touchedByLatestSuccess: boolean;
}

export interface ArtifactBudgetPreview {
  enabled: boolean;
  decision: BudgetDecision;
  generations: GenerationState[];
}

interface NativeArtifactSmokeAdapter {
  listBuildProfiles(): BuildProfile[];
  getArtifactBudgetPolicy(): ArtifactBudgetPolicy;
  previewArtifactBudgets(): ArtifactBudgetPreview;
  startBuildRun(profileId: string): BuildRun;
  getBuildRun(runId: string): BuildRun;
  getActiveBuildRun(): BuildRun | null;
  cancelBuildRun(runId: string): BuildRun;
}

declare global {
  interface Window {
    __SUPA_ARTIFACT_SMOKE__?: NativeArtifactSmokeAdapter;
  }
}

const smokeAdapter = (): NativeArtifactSmokeAdapter | undefined =>
  typeof window === "undefined" ? undefined : window.__SUPA_ARTIFACT_SMOKE__;

export const listBuildProfiles = (): Promise<BuildProfile[]> => {
  const smoke = smokeAdapter();
  return smoke ? Promise.resolve(smoke.listBuildProfiles()) : invoke("list_build_profiles");
}

export const registerBuildProfile = (
  input: RegisterBuildProfileInput,
): Promise<BuildProfile> => invoke("register_build_profile", { input });

export const removeBuildProfile = (profileId: string): Promise<void> =>
  invoke("remove_build_profile", { profileId });

export const startBuildRun = (profileId: string): Promise<BuildRun> => {
  const smoke = smokeAdapter();
  return smoke
    ? Promise.resolve(smoke.startBuildRun(profileId))
    : invoke("start_build_run", { profileId });
}

export const getActiveBuildRun = (): Promise<BuildRun | null> => {
  const smoke = smokeAdapter();
  return smoke
    ? Promise.resolve(smoke.getActiveBuildRun())
    : invoke("get_active_build_run");
}

export const getBuildRun = (runId: string): Promise<BuildRun> => {
  const smoke = smokeAdapter();
  return smoke
    ? Promise.resolve(smoke.getBuildRun(runId))
    : invoke("get_build_run", { runId });
}

export const cancelBuildRun = (runId: string): Promise<BuildRun> => {
  const smoke = smokeAdapter();
  return smoke
    ? Promise.resolve(smoke.cancelBuildRun(runId))
    : invoke("cancel_build_run", { runId });
}

export const getArtifactBudgetPolicy = (): Promise<ArtifactBudgetPolicy> => {
  const smoke = smokeAdapter();
  return smoke
    ? Promise.resolve(smoke.getArtifactBudgetPolicy())
    : invoke("get_artifact_budget_policy");
}

export const setArtifactBudgetPolicy = (
  policy: ArtifactBudgetPolicy,
): Promise<SetArtifactBudgetPolicyResult> => invoke("set_artifact_budget_policy", { policy });

export const previewArtifactBudgets = (): Promise<ArtifactBudgetPreview> => {
  const smoke = smokeAdapter();
  return smoke
    ? Promise.resolve(smoke.previewArtifactBudgets())
    : invoke("preview_artifact_budgets");
}
