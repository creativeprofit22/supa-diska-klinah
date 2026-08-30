// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { CleanupPreviewPage } from "./CleanupPreviewPage";
import type { CleanupPreview, PreviewRecord } from "./api/previewCleanup";

const invoke = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const emptyPreview: CleanupPreview = {
  scanId: "scan-empty",
  records: [],
  diagnostics: [],
};

function record(overrides: Partial<PreviewRecord>): PreviewRecord {
  return {
    id: "record-1",
    ruleId: "temporary-caches",
    displayPath: "C:\\Temp\\cache",
    kind: "directory",
    bytes: 0,
    ...overrides,
  };
}

describe("CleanupPreviewPage", () => {
  afterEach(() => {
    cleanup();
    invoke.mockReset();
  });

  it("invokes the input-free command on mount and renders loading", () => {
    invoke.mockReturnValue(new Promise(() => {}));

    render(<CleanupPreviewPage />);

    expect(invoke).toHaveBeenCalledOnce();
    expect(invoke).toHaveBeenCalledWith("preview_cleanup");
    expect(screen.getByRole("status").textContent).toContain(
      "Scanning temporary caches",
    );
    expect((screen.getByRole("button", { name: "Scanning…" }) as HTMLButtonElement).disabled).toBe(
      true,
    );
  });

  it("renders an empty result and scans once more on retry", async () => {
    invoke.mockResolvedValue(emptyPreview);
    render(<CleanupPreviewPage />);

    expect(await screen.findByRole("heading", { name: "Nothing found" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Scan again" }));

    await waitFor(() => expect(invoke).toHaveBeenCalledTimes(2));
  });

  it("does not leak a rejected object and recovers on retry", async () => {
    invoke
      .mockRejectedValueOnce({
        code: "scanFailed",
        path: "C:\\Users\\private\\secret-cache",
        detail: "raw OS failure",
      })
      .mockResolvedValueOnce(emptyPreview);
    render(<CleanupPreviewPage />);

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("Could not complete the preview");
    expect(document.body.textContent).not.toContain("secret-cache");
    expect(document.body.textContent).not.toContain("raw OS failure");

    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByRole("heading", { name: "Nothing found" })).toBeTruthy();
    expect(invoke).toHaveBeenCalledTimes(2);
  });

  it("groups rule results and totals their bytes", async () => {
    invoke.mockResolvedValue({
      scanId: "scan-groups",
      records: [
        record({ id: "one", bytes: 1024 }),
        record({
          id: "two",
          ruleId: "other-rule",
          displayPath: "C:\\Temp\\tmp",
          bytes: 512,
          modifiedUnixSeconds: 1_700_000_000,
        }),
      ],
      diagnostics: [],
    } satisfies CleanupPreview);

    render(<CleanupPreviewPage />);

    expect(await screen.findByRole("heading", { name: "Temporary caches" })).toBeTruthy();
    expect(screen.getByRole("heading", { name: "Other rule" })).toBeTruthy();
    const summary = screen.getByRole("status");
    expect(summary.textContent).toContain("2 items");
    expect(summary.textContent).toContain("1.5 KB total");
    expect(screen.getByText(/Modified/)).toBeTruthy();
  });

  it("shows only a skipped-location count from diagnostics", async () => {
    invoke.mockResolvedValue({
      ...emptyPreview,
      diagnostics: [
        {
          ruleId: "temporary-caches",
          path: "C:\\Users\\private\\diagnostic-path",
          reason: "unreadable",
        },
      ],
    } satisfies CleanupPreview);

    render(<CleanupPreviewPage />);

    expect(await screen.findByText("1 skipped location")).toBeTruthy();
    expect(document.body.textContent).not.toContain("diagnostic-path");
    expect(document.body.textContent).not.toContain("unreadable");
  });

  it("renders records without optional timestamps", async () => {
    invoke.mockResolvedValue({
      scanId: "scan-no-time",
      records: [record({ modifiedUnixSeconds: undefined })],
      diagnostics: [],
    } satisfies CleanupPreview);

    render(<CleanupPreviewPage />);

    expect(await screen.findByText("C:\\Temp\\cache")).toBeTruthy();
    expect(screen.queryByText(/Modified/)).toBeNull();
    expect((screen.getByRole("button", { name: "Scan again" }) as HTMLButtonElement).disabled).toBe(
      false,
    );
  });
});
