// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { CleanupPreviewPage } from "./CleanupPreviewPage";
import type {
  CleanupExecutionSummary,
  CleanupPreview,
  ProjectArtifactDiscovery,
} from "./api/previewCleanup";

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

const projectDiscovery: ProjectArtifactDiscovery = {
  records: [{
    id: "p".repeat(32),
    ruleId: "node-installed-dependencies",
    displayPath: "C:\\work\\app\\node_modules",
    kind: "directory",
    bytes: 1536,
    modifiedUnixSeconds: 1_700_000_000,
    projectName: "app",
    projectPath: "C:\\work\\app",
    artifact: {
      ecosystem: "nodeJs",
      artifactType: "installedDependencies",
      recoverability: "rebuildable",
      rebuildConsequence: "networkDownloadRequired",
    },
    risk: "recoverable",
    defaultSelected: false,
  }],
  diagnostics: [],
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
) {
  invoke.mockImplementation((command: string) => {
    if (command === "preview_cleanup") return Promise.resolve(result);
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

  it("shows project discovery first-use state without invoking discovery", async () => {
    mockBackend({ ...preview, records: [] });
    render(<CleanupPreviewPage />);

    expect(screen.getByLabelText("Project root")).toBeTruthy();
    expect(screen.getByText("Paste one explicit Windows project path to inspect it.")).toBeTruthy();
    await screen.findByRole("heading", { name: "Nothing found" });
    expect(invoke.mock.calls.some(([command]) => command === "discover_project_artifacts")).toBe(false);
  });

  it("submits the native project form and disables duplicates while pending", async () => {
    let resolveDiscovery!: (value: ProjectArtifactDiscovery) => void;
    const pending = new Promise<ProjectArtifactDiscovery>((resolve) => {
      resolveDiscovery = resolve;
    });
    mockBackend(preview, pending);
    render(<CleanupPreviewPage />);
    fireEvent.change(screen.getByLabelText("Project root"), { target: { value: "C:\\work\\app" } });
    const submit = screen.getByRole("button", { name: "Scan project root" });
    fireEvent.submit(submit.closest("form")!);

    expect(await screen.findByText("Scanning the project root without changing files.")).toBeTruthy();
    const pendingButton = screen.getByRole("button", { name: "Scanning…" }) as HTMLButtonElement;
    expect(pendingButton.disabled).toBe(true);
    fireEvent.click(pendingButton);
    expect(invoke.mock.calls.filter(([command]) => command === "discover_project_artifacts")).toHaveLength(1);
    expect(invoke).toHaveBeenCalledWith("discover_project_artifacts", { root: "C:\\work\\app" });

    resolveDiscovery(projectDiscovery);
    expect(await screen.findByText("1 rebuildable artifact found.")).toBeTruthy();
  });

  it("rejects oversized UTF-8 roots but accepts the 4,096-byte boundary", async () => {
    mockBackend();
    render(<CleanupPreviewPage />);
    const input = screen.getByLabelText("Project root") as HTMLInputElement;
    const submit = screen.getByRole("button", { name: "Scan project root" }) as HTMLButtonElement;
    const form = submit.closest("form")!;

    fireEvent.change(input, { target: { value: `C:\\${"界".repeat(1_365)}` } });
    expect(screen.getByRole("alert").textContent).toBe(
      "Project root must be 4,096 UTF-8 bytes or fewer.",
    );
    expect(input.getAttribute("aria-invalid")).toBe("true");
    expect(submit.disabled).toBe(true);
    fireEvent.submit(form);
    expect(invoke.mock.calls.some(([command]) => command === "discover_project_artifacts")).toBe(false);

    const boundaryRoot = `C:\\${"a".repeat(4_093)}`;
    fireEvent.change(input, { target: { value: boundaryRoot } });
    expect(screen.queryByText("Project root must be 4,096 UTF-8 bytes or fewer.")).toBeNull();
    expect(submit.disabled).toBe(false);
    fireEvent.submit(form);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith(
      "discover_project_artifacts",
      { root: boundaryRoot },
    ));
  });

  it("renders empty and bounded diagnostic project states", async () => {
    mockBackend(preview, {
      records: [],
      diagnostics: [{ ruleId: "node", path: "C:\\private\\secret", reason: "unreadable" }],
    });
    render(<CleanupPreviewPage />);
    fireEvent.change(screen.getByLabelText("Project root"), { target: { value: "C:\\work\\empty" } });
    fireEvent.click(screen.getByRole("button", { name: "Scan project root" }));

    expect(await screen.findByText("No marker-backed Node.js dependency folders were found.")).toBeTruthy();
    expect(screen.getByText("1 location was skipped.")).toBeTruthy();
    expect(document.body.textContent).not.toContain("private\\secret");
    expect(document.body.textContent).not.toContain("unreadable");
  });

  it("renders read-only project intelligence without destructive controls", async () => {
    mockBackend();
    render(<CleanupPreviewPage />);
    fireEvent.change(screen.getByLabelText("Project root"), { target: { value: "C:\\work\\app" } });
    fireEvent.click(screen.getByRole("button", { name: "Scan project root" }));

    const list = await screen.findByRole("list", { name: "Discovered project artifacts" });
    expect(within(list).getByRole("heading", { name: "app" })).toBeTruthy();
    expect(within(list).getByText("C:\\work\\app")).toBeTruthy();
    for (const value of [
      "Node.js",
      "Installed dependencies",
      "1.5 KB",
      "Recoverable",
      "Rebuildable",
      "Network download required",
    ]) expect(within(list).getByText(value)).toBeTruthy();
    expect(within(list).queryByRole("checkbox")).toBeNull();
    expect(within(list).queryByRole("button")).toBeNull();
  });

  it("shows a fixed project error and retries the same root", async () => {
    mockBackend(preview, new Error("C:\\private\\secret raw OS failure"));
    render(<CleanupPreviewPage />);
    fireEvent.change(screen.getByLabelText("Project root"), { target: { value: "C:\\work\\app" } });
    fireEvent.click(screen.getByRole("button", { name: "Scan project root" }));

    expect(await screen.findByText("Project artifacts could not be scanned. Check the root and try again.")).toBeTruthy();
    expect(document.body.textContent).not.toContain("private\\secret");
    fireEvent.click(screen.getByRole("button", { name: "Try project scan again" }));
    await waitFor(() => expect(
      invoke.mock.calls.filter(([command]) => command === "discover_project_artifacts"),
    ).toHaveLength(2));
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
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("could not continue");
    expect(document.body.textContent).not.toContain("secret");
    expect(document.body.textContent).not.toContain("raw OS failure");
  });
});
