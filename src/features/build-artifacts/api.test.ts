import { afterEach, describe, expect, it, vi } from "vitest";
import {
  cancelBuildRun,
  getActiveBuildRun,
  isBuildArtifactCommandError,
  registerBuildProfile,
  setArtifactBudgetPolicy,
  startBuildRun,
  type ArtifactBudgetPolicy,
  type RegisterBuildProfileInput,
} from "./api";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const input: RegisterBuildProfileInput = {
  rootId: "root-id",
  displayName: "Debug",
  ecosystem: "rust",
  executable: "C:\\Tools\\cargo.exe",
  argv: ["build", "--profile", "debug local"],
  workingDirectory: "app",
  profileLabel: "debug",
  toolchainLabel: "stable",
  targetLabel: "x86_64-pc-windows-msvc",
  rebuildCost: "low",
  artifactPaths: [
    { relativePath: "target/debug", role: "generation" },
    { relativePath: "target/debug/incremental", role: "incremental" },
  ],
};

const policy: ArtifactBudgetPolicy = {
  schemaVersion: 1,
  enabled: false,
  globalLimits: { maximumAllocatedBytes: null, maximumAgeSeconds: null },
  scheduledAnalysisIntervalSeconds: 86_400,
  staleChangeGraceSeconds: 86_400,
  quarantineGraceSeconds: 604_800,
  projectOverrides: [{ rootId: "root-id", policy: { mode: "disabled" } }],
};

describe("build artifact commands", () => {
  afterEach(() => invoke.mockReset());

  it("preserves argv and artifact row boundaries during registration", async () => {
    invoke.mockResolvedValue({ profileId: "profile-id", ...input });
    await registerBuildProfile(input);
    expect(invoke).toHaveBeenCalledWith("register_build_profile", { input });
    expect(invoke.mock.calls[0][1].input.argv).toEqual(["build", "--profile", "debug local"]);
  });

  it.each([null, { runId: "opaque-run", profileId: "opaque-profile", state: "running" }])(
    "discovers the active run without requiring any IDs (%j)", async (active) => {
      invoke.mockResolvedValue(active);
      expect(await getActiveBuildRun()).toEqual(active);
      expect(invoke).toHaveBeenCalledWith("get_active_build_run");
    },
  );

  it("starts and cancels using opaque IDs without runtime arguments", async () => {
    invoke.mockResolvedValue({});
    await startBuildRun("profile-id");
    await cancelBuildRun("run-id");
    expect(invoke.mock.calls).toEqual([
      ["start_build_run", { profileId: "profile-id" }],
      ["cancel_build_run", { runId: "run-id" }],
    ]);
  });

  it("serializes disabled defaults and per-project policy modes", async () => {
    invoke.mockResolvedValue(policy);
    await setArtifactBudgetPolicy(policy);
    expect(invoke).toHaveBeenCalledWith("set_artifact_budget_policy", { policy });
  });

  it.each([
    ["approvalDeclined", "The native approval was declined."],
    ["buildBusy", "Another build is already running."],
    ["validationFailed", "The operation stopped because registered build state changed."],
    ["invalidInput", "The build artifact request was invalid."],
  ])("recognizes a structured %s rejection", (code, message) => {
    expect(isBuildArtifactCommandError({ code, message })).toBe(true);
  });

  it("rejects malformed command errors", () => {
    expect(isBuildArtifactCommandError({ code: "invalidInput", message: 42 })).toBe(false);
  });
});
