// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { LargeFilesPage } from "./LargeFilesPage";
import type { LargeFileRow } from "./types";
import { I18nProvider } from "../../shared/i18n/I18nProvider";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
const id = (n: number) => n.toString(16).padStart(32, "0");
const choice = { rootId: id(10), module: "largeFiles", displayPath: "C:\\Fixture" };
const row = (n: number, eligibility: LargeFileRow["record"]["eligibility"] = { kind: "eligible", candidate_id: id(n + 1000) }): LargeFileRow => ({ kind: "file", record: { recordId: id(n), displayPath: `C:\\Fixture\\file-${n}.zip`, logicalBytes: 123, allocatedBytes: null, modifiedUnixSeconds: null, eligibility } });
const status = (phase = "complete") => ({ snapshotId: id(1), module: "largeFiles", phase, visitedEntries: 200, retainedRecords: 103, hashedBytes: 0, completedHashes: 0, completeness: { reasons: [] as string[] } });
const page = (records = [row(1), row(2, { kind: "readOnly" }), row(3, { kind: "ineligible", reason: "Protected file" })], nextCursor: string | null = id(99)) => ({ snapshotId: id(1), records, nextCursor, retainedTotal: 103, completeness: { reasons: [] as string[] } });
let chooser: () => Promise<typeof choice | null>;
let getStatus: () => Promise<ReturnType<typeof status>>;
let getPage: (input: { cursor?: string; pageSize: number }) => Promise<ReturnType<typeof page>>;
let getPlan: (input: { disposition: string }) => Promise<object>;
const calls = (command: string) => invoke.mock.calls.filter(([name]) => name === command).map(([, bytes]) => JSON.parse(new TextDecoder().decode(bytes)));
beforeEach(() => {
  chooser = async () => choice;
  getStatus = async () => status();
  getPage = async input => { expect(input.pageSize).toBe(100); return input.cursor ? page([row(4)], null) : page(); };
  getPlan = async input => ({ planId: id(30), disposition: input.disposition, selectedCount: 1, selectedBytes: 123 });
  Object.defineProperty(HTMLDialogElement.prototype, "showModal", { configurable: true, value() { this.open = true; } });
  Object.defineProperty(HTMLDialogElement.prototype, "close", { configurable: true, value() { this.open = false; } });
  invoke.mockImplementation(async (command: string, bytes: Uint8Array) => {
    expect(Object.prototype.toString.call(bytes)).toBe("[object Uint8Array]");
    const input = JSON.parse(new TextDecoder().decode(bytes));
    switch (command) {
      case "choose_storage_root": expect(input).toEqual({ module: "largeFiles" }); return chooser();
      case "start_large_files": return id(1);
      case "storage_scan_status": return getStatus();
      case "storage_scan_page": return getPage(input);
      case "create_storage_plan": return getPlan(input);
      case "release_storage_scan": case "cancel_storage_scan": return;
      default: throw new Error(`Forbidden/unexpected IPC: ${command}`);
    }
  });
});
afterEach(() => {
  cleanup();
  expect(invoke.mock.calls.some(([name]) => /execute|undo|purge/.test(name))).toBe(false);
  vi.clearAllMocks();
});
async function choose() {
  fireEvent.click(screen.getByRole("button", { name: "Choose folder" }));
  await waitFor(() => expect(screen.getByRole<HTMLButtonElement>("button", { name: "Scan for large files" }).disabled).toBe(false));
}
async function start() { await choose(); fireEvent.click(screen.getByRole("button", { name: "Scan for large files" })); await screen.findByRole("list", { name: "Large file results" }); }
const review = () => screen.getByRole<HTMLButtonElement>("button", { name: "Review: Move to app recovery" });
const file = (n: number) => screen.getByRole<HTMLInputElement>("checkbox", { name: `C:\\Fixture\\file-${n}.zip` });

// Pinned Kudu db09e051d0615121e659db187e3799438acbc9e6,
// large-file-finder.ipc.ts safeOptions and descending result order.
it("uses pinned Kudu large-file default threshold, depth and ordering", () => {
  render(<LargeFilesPage />);
  expect(screen.getByLabelText<HTMLInputElement>("Minimum size (MiB)").value).toBe("10");
  expect(screen.getByLabelText<HTMLInputElement>("Scan depth").value).toBe("20");
  expect(screen.getByLabelText<HTMLSelectElement>("Sort by").value).toBe("size");
  expect(screen.getByLabelText<HTMLInputElement>("Descending order").checked).toBe(true);
});
it("never auto-selects and reviews immutable native candidate IDs without executing", async () => {
  render(<LargeFilesPage />); await start();
  expect(file(1).checked).toBe(false); expect(file(2).disabled).toBe(true); expect(file(3).disabled).toBe(true);
  expect(review().disabled).toBe(true); expect(calls("create_storage_plan")).toEqual([]);
  expect(screen.getAllByText(/Unknown allocation/)).toHaveLength(3);
  fireEvent.click(file(1)); fireEvent.click(review());
  await screen.findByRole("dialog", { name: "Move to app recovery" });
  expect(calls("create_storage_plan")).toEqual([{ selection: { module: "largeFiles", snapshotId: id(1), candidateIds: [id(1001)] }, disposition: "quarantine" }]);
  expect(screen.getByText(/selected, not reclaimed/)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Cancel review" }));
  expect(screen.queryByRole("dialog")).toBeNull();
});
it("does not offer the unsupported Windows Recycle Bin operation", async () => {
  render(<LargeFilesPage />); await start(); fireEvent.click(file(1));
  expect(screen.queryByRole("button", { name: "Review: Move to Recycle Bin" })).toBeNull();
  expect(screen.getByText(/Windows Recycle Bin is not supported/)).toBeTruthy();
  expect(calls("create_storage_plan")).toEqual([]);
});
it("offers permanent review with a separate native confirmation, never auto-execution", async () => {
  render(<LargeFilesPage />); await start(); fireEvent.click(file(1));
  fireEvent.click(screen.getByRole("button", { name: "Review: Delete permanently" }));
  await screen.findByRole("dialog");
  expect(calls("create_storage_plan")[0].disposition).toBe("permanent");
  expect(screen.getByRole("button", { name: "Continue to Windows confirmation" })).toBeTruthy();
});
it("replaces pages while preserving only explicit IDs; partial results do not claim globally largest files", async () => {
  getStatus = async () => ({ ...status(), completeness: { reasons: ["retentionLimit"] } });
  render(<LargeFilesPage />); await start(); fireEvent.click(file(1));
  expect(screen.getByText(/not necessarily the globally largest/)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Next page" })); await screen.findByRole("checkbox", { name: /file-4/ });
  expect(screen.queryByRole("checkbox", { name: /file-1\.zip/ })).toBeNull(); expect(file(4).checked).toBe(false);
  expect(screen.getByText(/1 selected \(maximum/)).toBeTruthy();
  fireEvent.click(review()); await screen.findByRole("dialog");
  expect(calls("create_storage_plan")[0].selection.candidateIds).toEqual([id(1001)]);
});
it.each(["Minimum size (MiB)", "Maximum size (MiB, optional)", "Scan depth", "Extensions", "Category", "Sort by", "Descending order"])("clears stale selection and results on %s change", async label => {
  render(<LargeFilesPage />); await start(); fireEvent.click(file(1));
  const values: Record<string, string> = { "Minimum size (MiB)": "1", "Maximum size (MiB, optional)": "200", "Scan depth": "2", Extensions: "ZIP", Category: "archives", "Sort by": "path" };
  if (label === "Descending order") fireEvent.click(screen.getByLabelText(label));
  else fireEvent.change(screen.getByLabelText(label), { target: { value: values[label] } });
  expect(review().disabled).toBe(true); expect(screen.queryByRole("list", { name: "Large file results" })).toBeNull();
  expect(calls("release_storage_scan")).toContainEqual({ module: "largeFiles", snapshotId: id(1) });
});
it("preserves existing selection on picker cancellation but clears it for replacement scope", async () => {
  render(<LargeFilesPage />); await start(); fireEvent.click(file(1));
  chooser = async () => null; fireEvent.click(screen.getByRole("button", { name: "Choose folder" }));
  await waitFor(() => expect(screen.getByRole<HTMLButtonElement>("button", { name: "Choose folder" }).disabled).toBe(false));
  expect(file(1).checked).toBe(true);
  chooser = async () => ({ ...choice, rootId: id(20), displayPath: "C:\\Second" }); await choose();
  expect(review().disabled).toBe(true); expect(screen.queryByRole("list", { name: "Large file results" })).toBeNull();
});
it("ignores late plan and page replies after filters change", async () => {
  let resolvePlan!: (value: object) => void;
  getPlan = () => new Promise(resolve => { resolvePlan = resolve; });
  render(<LargeFilesPage />); await start(); fireEvent.click(file(1)); fireEvent.click(review());
  fireEvent.change(screen.getByLabelText("Scan depth"), { target: { value: "3" } });
  await act(async () => resolvePlan({ planId: id(30), disposition: "quarantine", selectedCount: 1, selectedBytes: 123 }));
  expect(screen.queryByRole("dialog")).toBeNull();
  await start();
  let resolvePage!: (value: ReturnType<typeof page>) => void;
  getPage = () => new Promise(resolve => { resolvePage = resolve; });
  fireEvent.click(screen.getByRole("button", { name: "Next page" }));
  fireEvent.change(screen.getByLabelText("Extensions"), { target: { value: "pdf" } });
  await act(async () => resolvePage(page([row(99)])));
  expect(screen.queryByRole("checkbox", { name: /file-99/ })).toBeNull(); expect(review().disabled).toBe(true);
});
it("cancels and discards late status without granting selection authority", async () => {
  let resolve!: (value: ReturnType<typeof status>) => void;
  getStatus = () => new Promise(r => { resolve = r; });
  render(<LargeFilesPage />); await choose(); fireEvent.click(screen.getByRole("button", { name: "Scan for large files" }));
  await waitFor(() => expect(calls("storage_scan_status")).toHaveLength(1));
  fireEvent.click(screen.getByRole("button", { name: "Cancel scan" })); await screen.findByText(/Scan cancelled/);
  await act(async () => resolve(status()));
  expect(calls("cancel_storage_scan")).toHaveLength(1); expect(calls("storage_scan_page")).toHaveLength(0); expect(review().disabled).toBe(true);
});
it("clears selection when paging fails with expired evidence", async () => {
  render(<LargeFilesPage />); await start(); fireEvent.click(file(1));
  getPage = async () => { throw { code: "snapshot_unavailable" }; };
  fireEvent.click(screen.getByRole("button", { name: "Next page" })); await screen.findByText(/These results expired/);
  expect(review().disabled).toBe(true); expect(screen.queryByRole("list", { name: "Large file results" })).toBeNull();
});

it("renders Spanish copy for es-MX", () => {
  render(<I18nProvider languages={["es-MX"]}><LargeFilesPage /></I18nProvider>);
  expect(screen.getByRole("heading", { name: "Archivos grandes" })).toBeTruthy();
  expect(screen.getByLabelText<HTMLInputElement>("Tamaño mínimo (MiB)").value).toBe("10");
  expect(screen.getByRole("option", { name: "Fecha de modificación" })).toBeTruthy();
  expect(screen.getByRole("button", { name: "Buscar archivos grandes" })).toBeTruthy();
});
