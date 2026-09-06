// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ArtifactBudgetPolicy } from "./api";
import { useBuildArtifacts } from "./useBuildArtifacts";

const api = vi.hoisted(() => ({
  cancelBuildRun: vi.fn(),
  getArtifactBudgetPolicy: vi.fn(),
  getBuildRun: vi.fn(),
  getActiveBuildRun: vi.fn(),
  isBuildArtifactCommandError: (error: unknown) =>
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    typeof error.code === "string" &&
    "message" in error &&
    typeof error.message === "string",
  listBuildProfiles: vi.fn(),
  previewArtifactBudgets: vi.fn(),
  registerBuildProfile: vi.fn(),
  removeBuildProfile: vi.fn(),
  setArtifactBudgetPolicy: vi.fn(),
  startBuildRun: vi.fn(),
}));
vi.mock("./api", () => api);

const enabledPolicy: ArtifactBudgetPolicy = {
  schemaVersion: 1,
  enabled: true,
  globalLimits: { maximumAllocatedBytes: null, maximumAgeSeconds: null },
  scheduledAnalysisIntervalSeconds: 86_400,
  staleChangeGraceSeconds: 86_400,
  quarantineGraceSeconds: 604_800,
  projectOverrides: [],
};

function Harness() {
  const state = useBuildArtifacts();
  return <div>
    <span>{state.run?.state ?? "idle"}</span>
    <span>{state.policy?.enabled ? "enabled" : "disabled"}</span>
    {state.error && <span>{state.error}</span>}
    <button disabled={state.buildActionsDisabled} onClick={() => void state.start("profile")}>Start</button>
    <button onClick={() => void state.cancel()}>Cancel</button>
    <button onClick={() => void state.savePolicy(enabledPolicy)}>Save policy</button>
  </div>;
}

function defaults() {
  api.getActiveBuildRun.mockResolvedValue(null);
  api.listBuildProfiles.mockResolvedValue([]);
  api.getArtifactBudgetPolicy.mockResolvedValue({ enabled: false });
  api.previewArtifactBudgets.mockResolvedValue({ enabled: false, decision: {}, generations: [] });
}

describe("useBuildArtifacts", () => {
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("rediscovers a run after remount, resumes polling, and cancels its opaque ID", async () => {
    defaults();
    const running = { runId: "opaque-native-id", profileId: "profile", state: "running" };
    api.startBuildRun.mockResolvedValue(running);
    api.getBuildRun.mockResolvedValue(running);
    api.cancelBuildRun.mockResolvedValue({ ...running, state: "cancelled" });
    const first = render(<Harness />);
    await waitFor(() => expect((screen.getByRole("button", { name: "Start" }) as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(screen.getByRole("button", { name: "Start" }));
    await screen.findByText("running");
    first.unmount();
    api.getActiveBuildRun.mockResolvedValue(running);

    render(<Harness />);

    expect(await screen.findByText("running")).toBeTruthy();
    expect((screen.getByRole("button", { name: "Start" }) as HTMLButtonElement).disabled).toBe(true);
    await waitFor(() => expect(api.getBuildRun).toHaveBeenCalledWith(running.runId));
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await screen.findByText("cancelled");
    expect(api.cancelBuildRun).toHaveBeenCalledWith(running.runId);
    expect(api.getActiveBuildRun).toHaveBeenCalledTimes(2);
  });

  it("keeps build actions disabled when active-run discovery fails", async () => {
    defaults();
    api.getActiveBuildRun.mockRejectedValue(new Error("Run status unavailable"));
    render(<Harness />);
    expect((screen.getByRole("button", { name: "Start" }) as HTMLButtonElement).disabled).toBe(true);
    await screen.findByText("Run status unavailable");
    expect((screen.getByRole("button", { name: "Start" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("polls an active run through its terminal state", async () => {
    defaults();
    api.startBuildRun.mockResolvedValue({ runId: "run", profileId: "profile", state: "running" });
    api.getBuildRun.mockResolvedValue({ runId: "run", profileId: "profile", state: "succeeded" });
    render(<Harness />);
    await screen.findByText("idle");
    fireEvent.click(screen.getByRole("button", { name: "Start" }));
    expect(await screen.findByText("running")).toBeTruthy();
    await waitFor(() => expect(api.getBuildRun).toHaveBeenCalledWith("run"), { timeout: 1_500 });
    expect(await screen.findByText("succeeded")).toBeTruthy();
  });

  it("sends cancellation for the active opaque run ID", async () => {
    defaults();
    api.startBuildRun.mockResolvedValue({ runId: "run", profileId: "profile", state: "running" });
    api.cancelBuildRun.mockResolvedValue({ runId: "run", profileId: "profile", state: "cancelled" });
    render(<Harness />);
    await screen.findByText("idle");
    fireEvent.click(screen.getByRole("button", { name: "Start" }));
    await screen.findByText("running");
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(api.cancelBuildRun).toHaveBeenCalledWith("run"));
    expect(await screen.findByText("cancelled")).toBeTruthy();
  });

  it.each([
    { code: "approvalDeclined", message: "The native approval was declined." },
    { code: "buildBusy", message: "Another build is already running." },
    {
      code: "validationFailed",
      message: "The operation stopped because registered build state changed.",
    },
  ])("shows the backend message for a $code rejection", async (failure) => {
    defaults();
    api.startBuildRun.mockRejectedValue(failure);
    render(<Harness />);
    await screen.findByText("idle");

    fireEvent.click(screen.getByRole("button", { name: "Start" }));

    expect(await screen.findByText(failure.message)).toBeTruthy();
  });

  it("shows capability-neutral text for a policy-invalid rejection", async () => {
    defaults();
    const failure = {
      code: "invalidInput",
      message: "The build artifact request was invalid.",
    };
    api.setArtifactBudgetPolicy.mockRejectedValue(failure);
    render(<Harness />);
    await screen.findByText("disabled");

    fireEvent.click(screen.getByRole("button", { name: "Save policy" }));

    expect(await screen.findByText(failure.message)).toBeTruthy();
  });

  it("reloads persisted policy after the mutation response fails", async () => {
    defaults();
    api.setArtifactBudgetPolicy.mockRejectedValue({
      code: "operationFailed",
      message: "The build operation could not be completed.",
    });
    api.getArtifactBudgetPolicy
      .mockResolvedValueOnce({ enabled: false })
      .mockResolvedValue({ enabled: true });
    render(<Harness />);
    await screen.findByText("disabled");

    fireEvent.click(screen.getByRole("button", { name: "Save policy" }));

    expect(await screen.findByText("enabled")).toBeTruthy();
    expect(api.previewArtifactBudgets).toHaveBeenCalledTimes(2);
  });

  it("keeps the saved policy visible when immediate analysis fails", async () => {
    defaults();
    api.setArtifactBudgetPolicy.mockResolvedValue({
      policySaved: true,
      policy: { enabled: true },
      analysisStatus: "failed",
    });
    api.getArtifactBudgetPolicy
      .mockResolvedValueOnce({ enabled: false })
      .mockResolvedValue({ enabled: true });
    api.previewArtifactBudgets
      .mockResolvedValueOnce({ enabled: false })
      .mockRejectedValue({
        code: "operationFailed",
        message: "The build operation could not be completed.",
      });
    render(<Harness />);
    await screen.findByText("disabled");

    fireEvent.click(screen.getByRole("button", { name: "Save policy" }));

    expect(await screen.findByText("enabled")).toBeTruthy();
    expect(await screen.findByText(/saved policy remains active/i)).toBeTruthy();
  });
});
