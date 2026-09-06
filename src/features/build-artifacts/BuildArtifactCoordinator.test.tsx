// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { BuildArtifactCoordinator } from "./BuildArtifactCoordinator";
import type { BuildProfile } from "./api";

const savedProfile: BuildProfile = {
  profileId: "profile",
  rootId: "root",
  displayName: "Release",
  ecosystem: "rust",
  executable: "C:\\Build Tools\\cargo.exe",
  executableIdentity: { volume: 987654, file: 123456 },
  argv: ["build", "two  words", '"quoted"', "", "<tag>&value"],
  workingDirectory: "packages/app",
  profileLabel: "release",
  toolchainLabel: "stable",
  targetLabel: "default",
  rebuildCost: "high",
  artifactPaths: [
    { relativePath: "target/app.exe", role: "generation" },
    { relativePath: "target/deps", role: "dependency" },
    { relativePath: "target/incremental", role: "incremental" },
  ],
};

const { listProjectRoots, useBuildArtifacts } = vi.hoisted(() => ({
  listProjectRoots: vi.fn(),
  useBuildArtifacts: vi.fn(),
}));
vi.mock("../cleanup/api/previewCleanup", () => ({ listProjectRoots }));
vi.mock("./useBuildArtifacts", () => ({ useBuildArtifacts }));

function state(overrides = {}) {
  return {
    profiles: [],
    policy: { enabled: false, globalLimits: {}, projectOverrides: [] },
    preview: {
      enabled: false,
      decision: {
        selectedGenerationIds: ["selected"],
        protected: [{
          generationId: "protected",
          rootId: "root",
          allocatedBytes: 20,
          reasons: ["incremental"],
        }],
        currentAllocatedBytes: 30,
        projectedAllocatedBytes: 20,
        quarantineBytes: 10,
        unsatisfiedProtectedByteFloor: 20,
        projectCurrentBytes: {},
        projectProjectedBytes: {},
        projectProtectedByteFloors: {},
      },
      generations: [
        { generationId: "selected", normalizedPath: "target/release", allocatedBytes: 10 },
        { generationId: "protected", normalizedPath: "target/debug/incremental", allocatedBytes: 20 },
      ],
    },
    run: null,
    loading: false,
    buildActionsDisabled: false,
    pending: null,
    error: null,
    register: vi.fn().mockResolvedValue({ profileId: "saved" }),
    remove: vi.fn(),
    start: vi.fn(),
    cancel: vi.fn(),
    reload: vi.fn(),
    ...overrides,
  };
}

describe("BuildArtifactCoordinator", () => {
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("caps argument and artifact rows at native limits", async () => {
    listProjectRoots.mockResolvedValue([{ id: "root", displayPath: "C:\\project" }]);
    useBuildArtifacts.mockReturnValue(state());
    render(<BuildArtifactCoordinator />);
    const addArgument = screen.getByRole("button", { name: "Add argument" });
    await waitFor(() => expect((addArgument.closest("fieldset")?.parentElement?.closest("fieldset") as HTMLFieldSetElement).disabled).toBe(false));
    for (let index = 1; index < 64; index++) fireEvent.click(addArgument);
    expect((addArgument as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(addArgument);
    expect(screen.getAllByLabelText(/^Argument \d+$/)).toHaveLength(64);
    const addPath = screen.getByRole("button", { name: "Add artifact path" });
    for (let index = 1; index < 16; index++) fireEvent.click(addPath);
    expect((addPath as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(addPath);
    expect(screen.getAllByLabelText(/^Relative path \d+$/)).toHaveLength(16);
    fireEvent.click(within(addPath.closest("fieldset")!).getAllByRole("button", { name: "Remove" })[0]);
    expect((addPath as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(within(addArgument.closest("fieldset")!).getAllByRole("button", { name: "Remove" })[0]);
    expect((addArgument as HTMLButtonElement).disabled).toBe(false);
  });

  it.each([
    ["Profile name", 1024],
    ["Profile label", 1024],
    ["Toolchain label", 1024],
    ["Target label", 1024],
    ["Argument 1", 4096],
  ])("enforces UTF-8 byte limits accessibly for %s", async (name, maximum) => {
    listProjectRoots.mockResolvedValue([{ id: "root", displayPath: "C:\\project" }]);
    const current = state();
    useBuildArtifacts.mockReturnValue(current);
    render(<BuildArtifactCoordinator />);
    await screen.findByRole("option", { name: "C:\\project" });
    fireEvent.change(screen.getByLabelText("Profile name"), { target: { value: "Build" } });
    fireEvent.change(screen.getByLabelText("Native executable (.exe)"), { target: { value: "C:\\cargo.exe" } });
    fireEvent.change(screen.getByLabelText("Relative path 1"), { target: { value: "target/app" } });
    const input = screen.getByLabelText(name);
    const submit = screen.getByRole("button", { name: "Review and register profile" });
    fireEvent.change(input, { target: { value: "😀".repeat(maximum / 4) + "é" } });
    expect(input.getAttribute("aria-invalid")).toBe("true");
    const error = document.getElementById(input.getAttribute("aria-describedby")!);
    expect(error?.textContent).toBe(`Use at most ${maximum} UTF-8 bytes.`);
    expect(error?.getAttribute("role")).toBe("alert");
    expect((submit as HTMLButtonElement).disabled).toBe(true);
    fireEvent.submit(submit.closest("form")!);
    expect(current.register).not.toHaveBeenCalled();
    fireEvent.change(input, { target: { value: "😀".repeat(maximum / 4) } });
    expect(input.getAttribute("aria-invalid")).toBe("false");
    expect(input.hasAttribute("aria-describedby")).toBe(false);
    expect((submit as HTMLButtonElement).disabled).toBe(false);
    fireEvent.submit(submit.closest("form")!);
    await waitFor(() => expect(current.register).toHaveBeenCalledOnce());
  });

  it("blocks normalized duplicates and overlaps until corrected", async () => {
    listProjectRoots.mockResolvedValue([{ id: "root", displayPath: "C:\\project" }]);
    const current = state();
    useBuildArtifacts.mockReturnValue(current);
    render(<BuildArtifactCoordinator />);
    await screen.findByRole("option", { name: "C:\\project" });
    fireEvent.change(screen.getByLabelText("Profile name"), { target: { value: "Build" } });
    fireEvent.change(screen.getByLabelText("Native executable (.exe)"), { target: { value: "C:\\cargo.exe" } });
    fireEvent.change(screen.getByLabelText("Relative path 1"), { target: { value: "Target//App/" } });
    fireEvent.click(screen.getByRole("button", { name: "Add artifact path" }));
    const second = screen.getByLabelText("Relative path 2");
    const submit = screen.getByRole("button", { name: "Review and register profile" });
    for (const path of ["target\\app", "target/app/child", "target"]) {
      fireEvent.change(second, { target: { value: path } });
      expect(second.getAttribute("aria-invalid")).toBe("true");
      expect(document.getElementById(second.getAttribute("aria-describedby")!)?.textContent).toContain("duplicate or overlap");
      fireEvent.submit(submit.closest("form")!);
      expect(current.register).not.toHaveBeenCalled();
    }
    fireEvent.change(second, { target: { value: "target/application" } });
    expect(second.getAttribute("aria-invalid")).toBe("false");
    expect(screen.getByLabelText("Relative path 1").getAttribute("aria-invalid")).toBe("false");
    fireEvent.submit(submit.closest("form")!);
    await waitFor(() => expect(current.register).toHaveBeenCalledOnce());
  });

  it("inspects saved values separately in collapsed Details without joining a shell command", async () => {
    listProjectRoots.mockResolvedValue([]);
    const current = state({ profiles: [savedProfile] });
    useBuildArtifacts.mockReturnValue(current);
    render(<BuildArtifactCoordinator />);
    await waitFor(() => expect(listProjectRoots).toHaveBeenCalled());

    const summary = screen.getByText("Details");
    const details = summary.closest("details")!;
    expect(details.open).toBe(false);
    fireEvent.click(summary);
    expect(details.open).toBe(true);
    const inspection = within(details);
    expect(inspection.getByText(savedProfile.executable).textContent).toBe(savedProfile.executable);
    expect(inspection.getByText(savedProfile.workingDirectory)).toBeTruthy();
    expect(inspection.getByText("high")).toBeTruthy();
    const argumentsList = inspection.getByRole("list", { name: "Arguments, in order" });
    expect(within(argumentsList).getAllByRole("listitem").map((item) => item.querySelector("code")?.textContent))
      .toEqual(savedProfile.argv);
    expect(within(argumentsList).getByText("Empty argument")).toBeTruthy();
    const paths = inspection.getByRole("list", { name: "Artifact paths and roles" });
    const items = within(paths).getAllByRole("listitem");
    expect(items).toHaveLength(savedProfile.artifactPaths.length);
    savedProfile.artifactPaths.forEach((artifact, index) => {
      expect(within(items[index]).getByText(artifact.relativePath)).toBeTruthy();
      expect(within(items[index]).getByText(artifact.role)).toBeTruthy();
    });
    expect(details.textContent).not.toContain(savedProfile.argv.join(" "));
    expect(details.textContent).not.toMatch(/executableIdentity|987654|123456/);
    expect(details.querySelector("tag")).toBeNull();
    expect(current.start).not.toHaveBeenCalled();
    fireEvent.click(summary);
    expect(details.open).toBe(false);
  });

  it("shows root loading failures and retries profile registration", async () => {
    listProjectRoots
      .mockRejectedValueOnce(new Error("storage unavailable"))
      .mockResolvedValueOnce([{ id: "root", displayPath: "C:\\Recovered", paused: false }]);
    useBuildArtifacts.mockReturnValue(state());

    render(<BuildArtifactCoordinator />);

    expect((await screen.findByRole("alert")).textContent).toContain("Project roots could not be loaded.");
    expect(screen.queryByText("No project roots are registered.")).toBeNull();
    expect((screen.getByRole("group", { name: "Register a build profile" }) as HTMLFieldSetElement).disabled).toBe(true);

    fireEvent.click(screen.getByRole("button", { name: "Retry loading project roots" }));

    expect(await screen.findByRole("option", { name: "C:\\Recovered" })).toBeTruthy();
    expect((screen.getByRole("group", { name: "Register a build profile" }) as HTMLFieldSetElement).disabled).toBe(false);
    expect(listProjectRoots).toHaveBeenCalledTimes(2);
  });

  it("submits dynamic arguments separately and labels protected-floor evidence", async () => {
    listProjectRoots.mockResolvedValue([{ id: "root", displayPath: "C:\\Work", paused: false }]);
    const current = state();
    useBuildArtifacts.mockReturnValue(current);
    render(<BuildArtifactCoordinator />);
    await screen.findByRole("option", { name: "C:\\Work" });
    fireEvent.change(screen.getByLabelText("Profile name"), { target: { value: "Release" } });
    fireEvent.change(screen.getByLabelText("Native executable (.exe)"), { target: { value: "C:\\Tools\\cargo.exe" } });
    fireEvent.click(screen.getByRole("button", { name: "Add argument" }));
    fireEvent.change(screen.getByLabelText("Argument 2"), { target: { value: "--release" } });
    fireEvent.change(screen.getByLabelText("Relative path 1"), { target: { value: "target/release/app.exe" } });
    fireEvent.click(screen.getByRole("button", { name: "Review and register profile" }));
    await waitFor(() => expect(current.register).toHaveBeenCalled());
    expect(current.register.mock.calls[0][0].argv).toEqual(["build", "--release"]);
    expect(current.register.mock.calls[0][0].artifactPaths[0].relativePath).toBe("target/release/app.exe");
    expect(screen.getByText(/Protected data alone uses/).textContent).toContain("limit remains unmet");
    expect(screen.getByText("Incremental build state")).toBeTruthy();
  });

  it.each(["inherit", "disabled"] as const)("previews two projects with explicit and %s limits", async (mode) => {
    listProjectRoots.mockResolvedValue([
      { id: "alpha", displayPath: "C:\\Alpha", paused: false },
      { id: "beta", displayPath: "C:\\Beta", paused: false },
    ]);
    const current = state();
    useBuildArtifacts.mockReturnValue({
      ...current,
      policy: {
        enabled: true,
        globalLimits: { maximumAllocatedBytes: 15 },
        projectOverrides: [
          { rootId: "alpha", policy: { mode: "explicit", limits: { maximumAllocatedBytes: 10 } } },
          { rootId: "beta", policy: { mode } },
        ],
      },
      preview: {
        ...current.preview,
        decision: {
          ...current.preview.decision,
          projectCurrentBytes: { beta: 70, alpha: 50 },
          projectProjectedBytes: { beta: 30, alpha: 20 },
          projectProtectedByteFloors: { beta: 30, alpha: 20 },
        },
      },
    });
    render(<BuildArtifactCoordinator />);

    const alpha = within(await screen.findByRole("region", { name: "C:\\Alpha" }));
    const beta = within(screen.getByRole("region", { name: "C:\\Beta" }));
    expect(alpha.getByText("Explicit project limits")).toBeTruthy();
    expect(alpha.getByText("50 B")).toBeTruthy();
    expect(alpha.getAllByText("20 B")).toHaveLength(2);
    expect(alpha.getByRole("status").textContent).toContain("10 B");
    expect(beta.getByText(mode === "inherit" ? "Inherit global limits" : "Disabled for this project")).toBeTruthy();
    expect(beta.getByText("70 B")).toBeTruthy();
    expect(beta.getAllByText("30 B")).toHaveLength(2);
    if (mode === "inherit") expect(beta.getByRole("status").textContent).toContain("15 B");
    else expect(beta.queryByRole("status")).toBeNull();
    expect(screen.getByText(/Protected data alone uses/)).toBeTruthy();
    expect(screen.getByText("Quarantine")).toBeTruthy();
  });

  it("labels partial budget enforcement as analysis failure with recoverable moves", () => {
    listProjectRoots.mockResolvedValue([]);
    useBuildArtifacts.mockReturnValue(state({
      run: { runId: "run", profileId: "profile", state: "analysisFailed" },
    }));

    render(<BuildArtifactCoordinator />);

    expect(screen.getByText(/budget enforcement failed; completed quarantine moves remain undoable/)).toBeTruthy();
  });

  it("disables Run and Forget for every profile while a discovered run is active", async () => {
    listProjectRoots.mockResolvedValue([]);
    const current = state({
      profiles: [
        { ...savedProfile, profileId: "active-profile", displayName: "Active" },
        { ...savedProfile, profileId: "other-profile", displayName: "Other" },
      ],
      run: { runId: "opaque-run", profileId: "active-profile", state: "running" },
      buildActionsDisabled: true,
    });
    useBuildArtifacts.mockReturnValue(current);
    render(<BuildArtifactCoordinator />);
    await waitFor(() => expect(listProjectRoots).toHaveBeenCalled());
    for (const button of screen.getAllByRole("button", { name: /^(Run|Forget)$/ })) {
      expect((button as HTMLButtonElement).disabled).toBe(true);
    }
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(current.cancel).toHaveBeenCalledOnce();
  });

  it("announces native approval failures and offers cancellation while running", async () => {
    listProjectRoots.mockResolvedValue([]);
    const current = state({
      error: "The build profile was not approved.",
      profiles: [{
        ...savedProfile,
        profileId: "profile",
        displayName: "Debug",
        executable: "C:\\Tools\\cargo.exe",
        argv: ["build"],
      }],
      run: { runId: "run", profileId: "profile", state: "running" },
    });
    useBuildArtifacts.mockReturnValue(current);
    render(<BuildArtifactCoordinator />);
    expect(screen.getByRole("alert").textContent).toContain("not approved");
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(current.cancel).toHaveBeenCalledOnce();
    expect(screen.getByText(/Latest build: running/)).toBeTruthy();
  });
});
