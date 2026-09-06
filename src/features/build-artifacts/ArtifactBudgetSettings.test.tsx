// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ArtifactBudgetSettings } from "./ArtifactBudgetSettings";

const { listProjectRoots, useBuildArtifacts } = vi.hoisted(() => ({
  listProjectRoots: vi.fn(),
  useBuildArtifacts: vi.fn(),
}));
vi.mock("../cleanup/api/previewCleanup", () => ({ listProjectRoots }));
vi.mock("./useBuildArtifacts", () => ({ useBuildArtifacts }));

describe("ArtifactBudgetSettings", () => {
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("shows root loading failures and retries project overrides", async () => {
    listProjectRoots
      .mockRejectedValueOnce(new Error("storage unavailable"))
      .mockResolvedValueOnce([{ id: "root", displayPath: "C:\\Recovered", paused: false }]);
    useBuildArtifacts.mockReturnValue({
      policy: {
        schemaVersion: 1,
        enabled: false,
        globalLimits: { maximumAllocatedBytes: null, maximumAgeSeconds: null },
        scheduledAnalysisIntervalSeconds: 86_400,
        staleChangeGraceSeconds: 86_400,
        quarantineGraceSeconds: 604_800,
        projectOverrides: [],
      },
      pending: null,
      error: null,
      savePolicy: vi.fn(),
    });

    render(<ArtifactBudgetSettings />);

    expect((await screen.findByRole("alert")).textContent).toContain("Project roots could not be loaded.");
    expect(screen.queryByText("Add a project root on Cleanup to set a project override.")).toBeNull();
    expect((screen.getByRole("group", { name: "Per-project behavior" }) as HTMLFieldSetElement).disabled).toBe(true);

    fireEvent.click(screen.getByRole("button", { name: "Retry loading project roots" }));

    expect(await screen.findByText("C:\\Recovered")).toBeTruthy();
    expect((screen.getByRole("group", { name: "Per-project behavior" }) as HTMLFieldSetElement).disabled).toBe(false);
    expect(listProjectRoots).toHaveBeenCalledTimes(2);
  });

  it("keeps disabled defaults and converts explicit units before saving", async () => {
    const savePolicy = vi.fn().mockResolvedValue({});
    listProjectRoots.mockResolvedValue([{ id: "root", displayPath: "C:\\Long Project", paused: false }]);
    useBuildArtifacts.mockReturnValue({
      policy: {
        schemaVersion: 1,
        enabled: false,
        globalLimits: { maximumAllocatedBytes: null, maximumAgeSeconds: null },
        scheduledAnalysisIntervalSeconds: 86_400,
        staleChangeGraceSeconds: 86_400,
        quarantineGraceSeconds: 604_800,
        projectOverrides: [],
      },
      pending: null,
      error: null,
      savePolicy,
    });
    render(<ArtifactBudgetSettings />);
    expect((screen.getByRole("checkbox", { name: /^Enforce artifact budgets/ }) as HTMLInputElement).checked).toBe(false);
    fireEvent.click(screen.getByRole("checkbox", { name: /^Enforce artifact budgets/ }));
    fireEvent.change(screen.getByLabelText(/^Global maximum size \(GiB\)/), { target: { value: "2" } });
    fireEvent.change(screen.getByLabelText(/^Global maximum age \(days\)/), { target: { value: "14" } });
    fireEvent.click(screen.getByRole("button", { name: "Save artifact budgets" }));
    await waitFor(() => expect(savePolicy).toHaveBeenCalled());
    expect(savePolicy.mock.calls[0][0]).toMatchObject({
      enabled: true,
      globalLimits: {
        maximumAllocatedBytes: 2 * 1024 ** 3,
        maximumAgeSeconds: 14 * 86_400,
      },
    });
  });

  it("rejects an empty explicit project budget", async () => {
    const savePolicy = vi.fn();
    listProjectRoots.mockResolvedValue([{ id: "root", displayPath: "C:\\Project", paused: false }]);
    useBuildArtifacts.mockReturnValue({
      policy: {
        schemaVersion: 1,
        enabled: false,
        globalLimits: { maximumAllocatedBytes: null, maximumAgeSeconds: null },
        scheduledAnalysisIntervalSeconds: 86_400,
        staleChangeGraceSeconds: 86_400,
        quarantineGraceSeconds: 604_800,
        projectOverrides: [],
      },
      pending: null,
      error: null,
      savePolicy,
    });

    render(<ArtifactBudgetSettings />);
    fireEvent.change(await screen.findByRole("combobox", { name: /Budget behavior/ }), { target: { value: "explicit" } });

    const error = screen.getByRole("alert");
    expect(error.textContent).toContain("Enter a maximum size or maximum age for project limits.");
    expect(screen.getByLabelText("Maximum size (GiB)").getAttribute("aria-describedby")).toBe(error.id);
    expect(screen.getByLabelText("Maximum age (days)").getAttribute("aria-invalid")).toBe("true");
    const save = screen.getByRole("button", { name: "Save artifact budgets" }) as HTMLButtonElement;
    expect(save.disabled).toBe(true);
    fireEvent.submit(save.closest("form")!);
    expect(savePolicy).not.toHaveBeenCalled();
  });

  it("accepts size-only and age-only explicit project budgets", async () => {
    const savePolicy = vi.fn().mockResolvedValue({});
    listProjectRoots.mockResolvedValue([
      { id: "size-root", displayPath: "C:\\Size Project", paused: false },
      { id: "age-root", displayPath: "C:\\Age Project", paused: false },
    ]);
    useBuildArtifacts.mockReturnValue({
      policy: {
        schemaVersion: 1,
        enabled: false,
        globalLimits: { maximumAllocatedBytes: null, maximumAgeSeconds: null },
        scheduledAnalysisIntervalSeconds: 86_400,
        staleChangeGraceSeconds: 86_400,
        quarantineGraceSeconds: 604_800,
        projectOverrides: [],
      },
      pending: null,
      error: null,
      savePolicy,
    });

    render(<ArtifactBudgetSettings />);
    const selectors = await screen.findAllByRole("combobox", { name: /Budget behavior/ });
    selectors.forEach((selector) => fireEvent.change(selector, { target: { value: "explicit" } }));
    fireEvent.change(screen.getAllByLabelText("Maximum size (GiB)")[0], { target: { value: "2" } });
    fireEvent.change(screen.getAllByLabelText("Maximum age (days)")[1], { target: { value: "14" } });
    fireEvent.click(screen.getByRole("button", { name: "Save artifact budgets" }));

    await waitFor(() => expect(savePolicy).toHaveBeenCalled());
    expect(savePolicy.mock.calls[0][0].projectOverrides).toEqual([
      { rootId: "size-root", policy: { mode: "explicit", limits: { maximumAllocatedBytes: 2 * 1024 ** 3, maximumAgeSeconds: null } } },
      { rootId: "age-root", policy: { mode: "explicit", limits: { maximumAllocatedBytes: null, maximumAgeSeconds: 14 * 86_400 } } },
    ]);
  });
});