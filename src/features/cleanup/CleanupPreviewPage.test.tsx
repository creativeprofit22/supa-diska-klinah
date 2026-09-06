// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { CleanupPreviewPage } from "./CleanupPreviewPage";

vi.mock("../build-artifacts/BuildArtifactCoordinator", () => ({
  BuildArtifactCoordinator: () => <div>Build artifact budgets</div>,
}));
import type {
  CleanupExecutionSummary,
  CleanupPreview,
  ProjectArtifactDiscovery,
  ProjectRoot,
} from "./api/previewCleanup";
import { useProjectArtifactDiscovery } from "./model/useProjectArtifactDiscovery";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const preview: CleanupPreview = {
  scanId: "a".repeat(32),
  records: [{
    id: "b".repeat(32),
    ruleId: "temporary-caches",
    displayPath: "C:\\Users\\private\\cache",
    kind: "directory",
    bytes: 1024,
  }],
  diagnostics: [],
};

const projectRoot: ProjectRoot = {
  id: "r".repeat(32),
  displayPath: "C:\\work\\app",
  paused: false,
  addedAtUnixSeconds: 1_700_000_000,
  lastScannedAtUnixSeconds: null,
};

const projectDiscovery: ProjectArtifactDiscovery = {
  roots: [{ ...projectRoot, lastScannedAtUnixSeconds: 1_700_000_100 }],
  records: [{
    id: "p".repeat(32),
    ruleId: "node-installed-dependencies",
    displayPath: "C:\\work\\app\\node_modules",
    kind: "directory",
    bytes: 1536,
    modifiedUnixSeconds: 1_700_000_000,
    ageSeconds: 100,
    activity: "inUse",
    projectName: "app",
    projectPath: "C:\\work\\app",
    artifact: {
      ecosystem: "nodeJs",
      artifactType: "installedDependencies",
      confidence: "high",
      recoverability: "rebuildable",
      rebuildConsequence: "networkDownloadRequired",
    },
    risk: "recoverable",
    defaultSelected: false,
  }],
  diagnostics: [],
  scannedAtUnixSeconds: 1_700_000_100,
};

function execution(disposition: "recycleBin" | "permanent"): CleanupExecutionSummary {
  return {
    executionId: "d".repeat(32),
    planId: "c".repeat(32),
    disposition,
    completed: true,
    items: [{ itemId: "b".repeat(32), state: disposition === "recycleBin" ? "recycled" : "purged", logicalBytes: 1024 }],
    accounting: {
      selectedBytes: 1024,
      processedBytes: 1024,
      failedBytes: 0,
      quarantinedBytes: 0,
      purgedBytes: disposition === "permanent" ? 1024 : 0,
      occupiedBytes: 4096,
      reclaimedBytes: disposition === "permanent" ? 4096 : 0,
    },
  };
}

function mockBackend(
  result: CleanupPreview = preview,
  discovery: ProjectArtifactDiscovery | Promise<ProjectArtifactDiscovery> | Error = projectDiscovery,
  roots: ProjectRoot[] = [projectRoot],
) {
  invoke.mockImplementation((command: string) => {
    if (command === "preview_cleanup") return Promise.resolve(result);
    if (command === "list_project_roots") return Promise.resolve(roots);
    if (command === "add_project_root") return Promise.resolve(roots);
    if (command === "set_project_root_paused") return Promise.resolve(roots);
    if (command === "remove_project_root") return Promise.resolve([]);
    if (command === "discover_project_artifacts") {
      return discovery instanceof Error ? Promise.reject(discovery) : Promise.resolve(discovery);
    }
    if (command === "cleanup_history") return Promise.resolve([]);
    if (command === "create_cleanup_plan") return Promise.resolve({
      planId: "c".repeat(32),
      disposition: "recycleBin",
      selectedCount: 1,
      selectedBytes: 1024,
    });
    if (command === "execute_cleanup_plan") return Promise.resolve(execution("recycleBin"));
    if (command === "execute_permanent_cleanup_plan") return Promise.resolve(execution("permanent"));
    return Promise.reject(new Error("unexpected command"));
  });
}

describe("CleanupPreviewPage", () => {
  afterEach(() => {
    cleanup();
    invoke.mockReset();
  });

  it("loads an empty root registry without scanning", async () => {
    mockBackend({ ...preview, records: [] }, projectDiscovery, []);
    render(<CleanupPreviewPage />);

    expect(screen.getByLabelText("Add an absolute project path")).toBeTruthy();
    expect(screen.getByText("Loading saved roots.")).toBeTruthy();
    expect(await screen.findByText("No roots saved. Add one absolute path to begin.")).toBeTruthy();
    expect(invoke.mock.calls.some(([command]) => command === "discover_project_artifacts")).toBe(false);
  });

  it("adds roots, preserves the list on duplicate errors, and enforces the byte bound", async () => {
    let addCount = 0;
    invoke.mockImplementation((command: string) => {
      if (command === "preview_cleanup") return Promise.resolve({ ...preview, records: [] });
      if (command === "cleanup_history") return Promise.resolve([]);
      if (command === "list_project_roots") return Promise.resolve([]);
      if (command === "add_project_root") {
        addCount += 1;
        return addCount === 1
          ? Promise.resolve([projectRoot])
          : Promise.reject({ code: "duplicateRoot", message: "backend detail" });
      }
      return Promise.reject(new Error("unexpected command"));
    });
    render(<CleanupPreviewPage />);
    await screen.findByText("No roots saved. Add one absolute path to begin.");
    const input = screen.getByLabelText("Add an absolute project path") as HTMLInputElement;
    const add = screen.getByRole("button", { name: "Add root" }) as HTMLButtonElement;

    fireEvent.change(input, { target: { value: `C:\\${"界".repeat(1_365)}` } });
    expect(screen.getByRole("alert").textContent).toBe(
      "Project root must be 4,096 UTF-8 bytes or fewer.",
    );
    expect(add.disabled).toBe(true);
    const boundaryRoot = `C:\\${"a".repeat(4_093)}`;
    fireEvent.change(input, { target: { value: boundaryRoot } });
    fireEvent.click(add);
    expect(await screen.findByText(projectRoot.displayPath)).toBeTruthy();
    expect(invoke).toHaveBeenCalledWith("add_project_root", { path: boundaryRoot });

    fireEvent.change(input, { target: { value: projectRoot.displayPath } });
    fireEvent.click(screen.getByRole("button", { name: "Add root" }));
    expect(await screen.findByText("That project root is already saved.")).toBeTruthy();
    expect(screen.getByText(projectRoot.displayPath)).toBeTruthy();
    expect(document.body.textContent).not.toContain("backend detail");
  });

  it("pauses, resumes, and forgets roots without implying file deletion", async () => {
    let roots = [projectRoot];
    invoke.mockImplementation((command: string, input?: Record<string, unknown>) => {
      if (command === "preview_cleanup") return Promise.resolve({ ...preview, records: [] });
      if (command === "cleanup_history") return Promise.resolve([]);
      if (command === "list_project_roots") return Promise.resolve(roots);
      if (command === "set_project_root_paused") {
        roots = [{ ...projectRoot, paused: Boolean(input?.paused) }];
        return Promise.resolve(roots);
      }
      if (command === "remove_project_root") return Promise.resolve([]);
      return Promise.reject(new Error("unexpected command"));
    });
    render(<CleanupPreviewPage />);
    await screen.findByText(projectRoot.displayPath);

    fireEvent.click(screen.getByRole("button", { name: `Pause ${projectRoot.displayPath}` }));
    expect(await screen.findByRole("button", { name: `Resume ${projectRoot.displayPath}` })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: `Resume ${projectRoot.displayPath}` }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("set_project_root_paused", {
      rootId: projectRoot.id,
      paused: false,
    }));
    fireEvent.click(await screen.findByRole("button", { name: `Remove ${projectRoot.displayPath}` }));
    expect(await screen.findByText("No roots saved. Add one absolute path to begin.")).toBeTruthy();
    expect(screen.getByText("Removing a root only forgets it. Project files stay unchanged.")).toBeTruthy();
    expect(invoke).toHaveBeenCalledWith("remove_project_root", { rootId: projectRoot.id });
  });

  it("scans one root or all active roots and disables root changes while pending", async () => {
    let resolveDiscovery!: (value: ProjectArtifactDiscovery) => void;
    const pending = new Promise<ProjectArtifactDiscovery>((resolve) => {
      resolveDiscovery = resolve;
    });
    mockBackend(preview, pending);
    render(<CleanupPreviewPage />);
    await screen.findByText(projectRoot.displayPath);

    fireEvent.click(screen.getByRole("button", { name: `Scan ${projectRoot.displayPath}` }));
    expect(await screen.findByText("Scanning project roots without changing files.")).toBeTruthy();
    expect((screen.getByRole("button", { name: `Pause ${projectRoot.displayPath}` }) as HTMLButtonElement).disabled).toBe(true);
    expect(invoke).toHaveBeenCalledWith("discover_project_artifacts", { rootId: projectRoot.id });
    resolveDiscovery(projectDiscovery);
    expect(await screen.findByText(/1 rebuildable artifact found/)).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Scan active roots" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("discover_project_artifacts", { rootId: undefined }));
  });

  it("ignores stale scan responses after a root mutation", async () => {
    let resolveDiscovery!: (value: ProjectArtifactDiscovery) => void;
    const pending = new Promise<ProjectArtifactDiscovery>((resolve) => {
      resolveDiscovery = resolve;
    });
    invoke.mockImplementation((command: string) => {
      if (command === "list_project_roots") return Promise.resolve([projectRoot]);
      if (command === "discover_project_artifacts") return pending;
      if (command === "remove_project_root") return Promise.resolve([]);
      return Promise.resolve(command === "cleanup_history" ? [] : { ...preview, records: [] });
    });
    function Harness() {
      const state = useProjectArtifactDiscovery();
      return (
        <div>
          <button onClick={() => void state.scan(projectRoot.id)}>Start scan</button>
          <button onClick={() => void state.remove(projectRoot.id)}>Forget now</button>
          <span>{state.result?.records[0]?.displayPath ?? "No result"}</span>
        </div>
      );
    }
    render(<Harness />);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("list_project_roots"));
    fireEvent.click(screen.getByRole("button", { name: "Start scan" }));
    fireEvent.click(screen.getByRole("button", { name: "Forget now" }));
    resolveDiscovery(projectDiscovery);
    await waitFor(() => expect(screen.getByText("No result")).toBeTruthy());
  });

  it("renders diagnostics and grouped in-use intelligence without destructive controls", async () => {
    mockBackend(preview, {
      ...projectDiscovery,
      diagnostics: [{ ruleId: "node", path: "C:\\private\\secret", reason: "overlap" }],
    });
    render(<CleanupPreviewPage />);
    await screen.findByText(projectRoot.displayPath);
    fireEvent.click(screen.getByRole("button", { name: `Scan ${projectRoot.displayPath}` }));

    const list = await screen.findByRole("list", { name: "Discovered project artifacts" });
    expect(within(list).getByRole("heading", { name: "app" })).toBeTruthy();
    for (const value of [
      "Node.js",
      "Installed dependencies",
      "1.5 KB",
      "In use",
      "High",
      "Recoverable",
      "Rebuildable",
      "Network download required",
      "Not selected",
    ]) expect(within(list).getByText(value)).toBeTruthy();
    expect(screen.getByText(/1 location was skipped or suppressed/)).toBeTruthy();
    expect(document.body.textContent).not.toContain("private\\secret");
    expect(within(list).queryByRole("checkbox")).toBeNull();
    expect(within(list).queryByRole("button")).toBeNull();
  });

  it("shows fixed scan errors without backend path details", async () => {
    mockBackend(preview, new Error("C:\\private\\secret raw OS failure"));
    render(<CleanupPreviewPage />);
    await screen.findByText(projectRoot.displayPath);
    fireEvent.click(screen.getByRole("button", { name: `Scan ${projectRoot.displayPath}` }));

    expect(await screen.findByText("Project roots could not be updated. Try again.")).toBeTruthy();
    expect(document.body.textContent).not.toContain("private\\secret");
  });

  it("loads preview and bounded history without command inputs", async () => {
    mockBackend({ ...preview, records: [] });
    render(<CleanupPreviewPage />);
    expect(await screen.findByRole("heading", { name: "Nothing found" })).toBeTruthy();
    expect(invoke).toHaveBeenCalledWith("preview_cleanup");
    expect(invoke).toHaveBeenCalledWith("cleanup_history");
  });

  it("scans again on retry without changing command inputs", async () => {
    mockBackend({ ...preview, records: [] });
    render(<CleanupPreviewPage />);
    await screen.findByRole("heading", { name: "Nothing found" });
    fireEvent.click(screen.getByRole("button", { name: "Scan again" }));
    await waitFor(() => expect(
      invoke.mock.calls.filter(([command]) => command === "preview_cleanup"),
    ).toHaveLength(2));
  });

  it("groups records and preserves optional modified metadata", async () => {
    mockBackend({
      ...preview,
      records: [
        preview.records[0],
        {
          ...preview.records[0],
          id: "e".repeat(32),
          ruleId: "other-rule",
          displayPath: "C:\\Temp\\tmp",
          bytes: 512,
          modifiedUnixSeconds: 1_700_000_000,
        },
      ],
    });
    render(<CleanupPreviewPage />);
    expect(await screen.findByRole("heading", { name: "Temporary caches" })).toBeTruthy();
    expect(screen.getByRole("heading", { name: "Other rule" })).toBeTruthy();
    expect(screen.getByText(/Modified/)).toBeTruthy();
    expect(screen.getByText("0 of 2 selected")).toBeTruthy();
  });

  it("shows only a skipped count from diagnostics", async () => {
    mockBackend({
      ...preview,
      records: [],
      diagnostics: [{
        ruleId: "temporary-caches",
        path: "C:\\Users\\private\\diagnostic-path",
        reason: "unreadable",
      }],
    });
    render(<CleanupPreviewPage />);
    expect(await screen.findByText("1 skipped location")).toBeTruthy();
    expect(document.body.textContent).not.toContain("diagnostic-path");
    expect(document.body.textContent).not.toContain("unreadable");
  });

  it("renders records when modified metadata is absent", async () => {
    mockBackend(preview);
    render(<CleanupPreviewPage />);
    expect(await screen.findByText("C:\\Users\\private\\cache")).toBeTruthy();
    expect(screen.queryByText(/Modified/)).toBeNull();
    expect((screen.getByRole("button", { name: "Scan again" }) as HTMLButtonElement).disabled).toBe(false);
  });

  it("sends opaque identifiers only through safe mutation IPC", async () => {
    mockBackend();
    render(<CleanupPreviewPage />);
    fireEvent.click(await screen.findByRole("checkbox"));
    fireEvent.click(screen.getByRole("button", { name: "Move to Recycle Bin" }));

    const dialog = await screen.findByRole("dialog");
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("create_cleanup_plan", {
      scanId: "a".repeat(32),
      candidateIds: ["b".repeat(32)],
      disposition: "recycleBin",
    }));
    expect(JSON.stringify(invoke.mock.calls.at(-1))).not.toContain("private");
    fireEvent.click(within(dialog).getByRole("button", { name: "Move to Recycle Bin" }));

    await screen.findByRole("heading", { name: "Latest cleanup" });
    expect(invoke).toHaveBeenCalledWith("execute_cleanup_plan", { planId: "c".repeat(32) });
  });

  it("uses a distinct warning and command for permanent deletion", async () => {
    mockBackend();
    invoke.mockImplementation((command: string) => {
      if (command === "preview_cleanup") return Promise.resolve(preview);
      if (command === "cleanup_history") return Promise.resolve([]);
      if (command === "create_cleanup_plan") return Promise.resolve({ planId: "c".repeat(32), disposition: "permanent", selectedCount: 1, selectedBytes: 1024 });
      if (command === "execute_permanent_cleanup_plan") return Promise.resolve(execution("permanent"));
      return Promise.reject(new Error("normal execution must not run"));
    });
    render(<CleanupPreviewPage />);
    fireEvent.click(await screen.findByRole("checkbox"));
    fireEvent.click(screen.getByRole("button", { name: "Delete permanently" }));
    const dialog = await screen.findByRole("dialog");
    expect(dialog.textContent).toContain("cannot be undone");
    fireEvent.click(within(dialog).getByRole("button", { name: "Delete permanently" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("execute_permanent_cleanup_plan", { planId: "c".repeat(32) }));
    expect(invoke.mock.calls.some(([command]) => command === "execute_cleanup_plan")).toBe(false);
  });

  it("never renders rejected backend path details", async () => {
    invoke.mockImplementation((command: string) => command === "cleanup_history"
      ? Promise.resolve([])
      : Promise.reject({ path: "C:\\Users\\private\\secret", detail: "raw OS failure" }));
    render(<CleanupPreviewPage />);
    const alerts = await screen.findAllByRole("alert");
    expect(alerts.some((alert) => alert.textContent?.includes("could not continue"))).toBe(true);
    expect(document.body.textContent).not.toContain("secret");
    expect(document.body.textContent).not.toContain("raw OS failure");
  });
});
