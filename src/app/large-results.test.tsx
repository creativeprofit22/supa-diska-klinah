// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { createMemoryRouter, RouterProvider } from "react-router-dom";
import { afterEach, expect, it, vi } from "vitest";
import { PREVIEW_PAGE_SIZE } from "../features/cleanup/CleanupPreviewPage";
import { LargeFilesPage } from "../features/large-files/LargeFilesPage";
import { I18nProvider } from "../shared/i18n/I18nProvider";
import { PAGE_SIZE } from "../shared/storage/types";
import { appRoutes } from "./router";
import { respond } from "./routeFixtures";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
afterEach(() => { cleanup(); invoke.mockReset(); });

const TOTAL = 100_000;
const id = (n: number) => n.toString(16).padStart(32, "0");
const statusesMentioning = (pattern: RegExp) => screen.queryAllByRole("status").filter((node) => pattern.test(node.textContent ?? ""));

it("storage scan results render one page of 100 000 retained records and announce the total once", async () => {
  // Native snapshot of 100 000 rows; the UI may only ever ask for one page.
  const rows = Array.from({ length: TOTAL }, (_, n) => ({
    kind: "file", record: { recordId: id(n + 1), displayPath: `C:\\Big\\file-${n}.bin`, logicalBytes: 1, allocatedBytes: null, modifiedUnixSeconds: null,
      eligibility: { kind: "eligible", candidate_id: id(n + 1 + TOTAL) } },
  }));
  const pageSizes: number[] = [];
  invoke.mockImplementation(async (command: string, bytes: Uint8Array) => {
    const input = JSON.parse(new TextDecoder().decode(bytes)) as { cursor?: string; pageSize?: number };
    switch (command) {
      case "choose_storage_root": return { rootId: id(TOTAL * 3), module: "largeFiles", displayPath: "C:\\Big" };
      case "start_large_files": return id(1);
      case "storage_scan_status": return { snapshotId: id(1), module: "largeFiles", phase: "complete", visitedEntries: TOTAL, retainedRecords: TOTAL, hashedBytes: 0, completedHashes: 0, completeness: { reasons: [] } };
      case "storage_scan_page": {
        pageSizes.push(input.pageSize!);
        const start = input.cursor ? Number.parseInt(input.cursor, 16) : 0;
        const end = start + input.pageSize!;
        return { snapshotId: id(1), records: rows.slice(start, end), nextCursor: end < TOTAL ? id(end) : null, retainedTotal: TOTAL, completeness: { reasons: [] } };
      }
      case "release_storage_scan": case "cancel_storage_scan": return undefined;
      default: throw new Error(`Unexpected IPC: ${command}`);
    }
  });
  render(<I18nProvider languages={["en-US"]}><LargeFilesPage /></I18nProvider>);
  fireEvent.click(screen.getByRole("button", { name: "Choose folder" }));
  await waitFor(() => expect(screen.getByRole<HTMLButtonElement>("button", { name: "Scan for large files" }).disabled).toBe(false));
  fireEvent.click(screen.getByRole("button", { name: "Scan for large files" }));
  const list = await screen.findByRole("list", { name: "Large file results" });

  expect(within(list).getAllByRole("listitem").length).toBeLessThanOrEqual(PAGE_SIZE);
  expect(pageSizes.every((size) => size === PAGE_SIZE)).toBe(true);
  expect(statusesMentioning(/100,000/)).toHaveLength(1);
  expect(screen.getAllByRole("status").length).toBeLessThan(10);

  fireEvent.click(screen.getByRole("button", { name: "Next page" }));
  const next = await screen.findByRole("list", { name: "Large file results" });
  expect(within(next).getAllByRole("listitem")).toHaveLength(PAGE_SIZE);
  expect(within(next).getByText(`C:\\Big\\file-${PAGE_SIZE}.bin`)).toBeTruthy();
  expect(statusesMentioning(/100,000/)).toHaveLength(1);
});

it("cleanup preview paginates 100 000 records and announces the total once", async () => {
  const records = Array.from({ length: TOTAL }, (_, n) => ({ id: id(n + 1), ruleId: n % 2 ? "temporary-caches" : "tmp-directories", displayPath: `C:\\tmp\\item-${n}`, kind: "file", bytes: 1 }));
  invoke.mockImplementation((command: string, bytes?: unknown) => command === "preview_cleanup"
    ? Promise.resolve({ scanId: "a".repeat(32), records, diagnostics: [] })
    : respond(command, bytes));
  const router = createMemoryRouter(appRoutes, { initialEntries: ["/cleanup"] });
  render(<I18nProvider languages={["en-US"]}><RouterProvider router={router} /></I18nProvider>);
  await screen.findByRole("heading", { level: 1 });
  await screen.findByRole("button", { name: "Select all" });
  await act(async () => { for (let i = 0; i < 5; i++) await new Promise((done) => setTimeout(done, 0)); });

  expect(screen.getAllByRole("checkbox").length).toBeLessThanOrEqual(PREVIEW_PAGE_SIZE);
  expect(statusesMentioning(/100000|100,000/)).toHaveLength(1);
  const pages = screen.getByRole("navigation", { name: "Cleanup result pages" });
  expect(within(pages).getByText("Showing 1–100 of 100000")).toBeTruthy();

  fireEvent.click(within(pages).getByRole("button", { name: "Next results" }));
  expect(screen.getByText("C:\\tmp\\item-100")).toBeTruthy();
  expect(screen.getAllByRole("checkbox").length).toBeLessThanOrEqual(PREVIEW_PAGE_SIZE);

  // Selection still covers the whole preview, not just the visible page.
  fireEvent.click(screen.getByRole("button", { name: "Select all" }));
  expect(statusesMentioning(/100000 of 100000 selected/)).toHaveLength(1);
});
