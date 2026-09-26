// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { I18nProvider } from "../../shared/i18n/I18nProvider";
import { CleanerPage } from "./CleanerPage";
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
const id = (n: number) => n.toString(16).padStart(32, "0");
const calls = (command: string) => invoke.mock.calls.filter(([name]) => name === command).map(([, bytes]) => JSON.parse(new TextDecoder().decode(bytes)));
let current = 1;
beforeEach(() => {
  current = 1;
  Object.defineProperty(HTMLDialogElement.prototype, "showModal", { configurable: true, value() { this.open = true; } });
  Object.defineProperty(HTMLDialogElement.prototype, "close", { configurable: true, value() { this.open = false; } });
  invoke.mockImplementation(async (command: string, bytes: Uint8Array) => {
    expect(Object.prototype.toString.call(bytes)).toBe("[object Uint8Array]");
    const input = JSON.parse(new TextDecoder().decode(bytes));
    switch (command) {
      case "list_cleaner_catalog": expect(input).toEqual({}); return { targets: [{ catalogId: "gpu", targetId: "cache:0", path: "local/Cache", source: "rules/source.ts", revision: "pinned-revision", ruleVersion: 1, minimumAgeSeconds: 86400, consequence: "Rebuild cache", exclusions: ["private data"], matcher: "single-file", unsupportedReason: "Unsupported fixture" }], unsupportedOperations: [["database", "No database mutation"]] };
      case "list_storage_scopes": return [1, 2, 3].map(n => ({ scopeId: id(n), module: "cleaner", label: `Scope ${n}`, displayPath: n < 3 ? `C:/Fixture${n}` : null, available: n < 3 }));
      case "authorize_storage_scope": current = Number.parseInt(input.scopeId, 16); return { rootId: id(current + 10), module: "cleaner", displayPath: `C:/Fixture${current}` };
      case "start_cleaner": return id(current + 20);
      case "storage_scan_status": return { snapshotId: input.snapshotId, module: "cleaner", phase: "complete", visitedEntries: 1, retainedRecords: 1, hashedBytes: 0, completedHashes: 0, completeness: { reasons: [] } };
      case "storage_scan_page": return { snapshotId: input.snapshotId, records: [{ kind: "file", record: { recordId: id(current + 30), displayPath: `C:/Fixture${current}/cache`, logicalBytes: 12, allocatedBytes: null, modifiedUnixSeconds: null, eligibility: { kind: "eligible", candidate_id: id(current + 40) } } }], nextCursor: null, retainedTotal: 1, completeness: { reasons: [] } };
      case "create_storage_plan": return { planId: id(99), disposition: input.disposition, selectedCount: 1, selectedBytes: 12 };
      case "cancel_storage_scan": case "release_storage_scan": return;
      default: throw Error(`Unexpected IPC ${command}`);
    }
  });
});
afterEach(() => { cleanup(); expect(invoke.mock.calls.some(([name]) => /execute|undo|purge/.test(name))).toBe(false); vi.clearAllMocks(); });
async function select(n = 1) { fireEvent.click(await screen.findByRole("radio", { name: new RegExp(`Scope ${n}`) })); }
async function authorize() { fireEvent.click(screen.getByRole("button", { name: "Authorize selected scope" })); await waitFor(() => expect(screen.getByRole<HTMLButtonElement>("button", { name: "Scan cleaner scope" }).disabled).toBe(false)); }
async function start(n = 1) { await select(n); await authorize(); fireEvent.click(screen.getByRole("button", { name: "Scan cleaner scope" })); await screen.findByRole("list", { name: "Cleaner results" }); }
it("shows native unavailable scopes and honest catalog metadata without default authority", async () => {
  render(<CleanerPage />); await screen.findByRole("radio", { name: /Scope 1/ });
  expect(screen.getAllByRole<HTMLInputElement>("radio").every(r => !r.checked)).toBe(true);
  expect(screen.getByRole<HTMLInputElement>("radio", { name: /Scope 3/ }).disabled).toBe(true);
  expect(calls("authorize_storage_scope")).toEqual([]); expect(calls("start_cleaner")).toEqual([]);
  fireEvent.click(screen.getByText("Catalog rules, provenance and exclusions"));
  for (const text of [/86,400 seconds/, /pinned-revision/, /private data/, /Unsupported fixture/, /not per-file rule attribution/, /not atomic/]) expect(screen.getByText(text)).toBeTruthy();
});
it("reviews explicit snapshot IDs with app recovery default, never executes", async () => {
  render(<CleanerPage />); await start();
  const file = screen.getByRole<HTMLInputElement>("checkbox", { name: "C:/Fixture1/cache" }); expect(file.checked).toBe(false);
  const review = screen.getByRole<HTMLButtonElement>("button", { name: "Review: Move to app recovery" }); expect(review.disabled).toBe(true);
  fireEvent.click(file); fireEvent.click(review); await screen.findByRole("dialog");
  expect(calls("authorize_storage_scope")).toEqual([{ module: "cleaner", scopeId: id(1) }]);
  expect(calls("start_cleaner")).toEqual([{ rootId: id(11) }]);
  expect(calls("create_storage_plan")).toEqual([{ selection: { module: "cleaner", snapshotId: id(21), candidateIds: [id(41)] }, disposition: "quarantine" }]);
  expect(screen.queryByRole("button", { name: "Review: Move to Recycle Bin" })).toBeNull();
});
it("isolates sequential roots and clears prior results and selection", async () => {
  render(<CleanerPage />); await start(); fireEvent.click(screen.getByRole("checkbox"));
  await select(2); expect(screen.queryByRole("checkbox")).toBeNull();
  expect(calls("release_storage_scan")).toContainEqual({ module: "cleaner", snapshotId: id(21) });
  await authorize(); fireEvent.click(screen.getByRole("button", { name: "Scan cleaner scope" }));
  expect((await screen.findByRole<HTMLInputElement>("checkbox", { name: "C:/Fixture2/cache" })).checked).toBe(false);
  expect(calls("start_cleaner")).toEqual([{ rootId: id(11) }, { rootId: id(12) }]);
});
it("category changes release unused authority and clear scope", async () => {
  render(<CleanerPage />); await select(); await authorize();
  fireEvent.change(screen.getByLabelText("Catalog category"), { target: { value: "gpu" } });
  expect(screen.queryByRole("button", { name: "Scan cleaner scope" })).toBeNull();
  expect(calls("release_storage_scan")).toContainEqual({ module: "cleaner", snapshotId: id(11) });
  expect(screen.getAllByRole<HTMLInputElement>("radio").every(r => !r.checked)).toBe(true);
});
it("renders Spanish (es-419) text when the language is Spanish", async () => {
  render(<I18nProvider languages={["es-MX"]}><CleanerPage /></I18nProvider>);
  expect(screen.getByRole("heading", { name: "Limpiador por reglas" })).toBeTruthy();
  await screen.findByRole("radio", { name: /Scope 1/ });
  expect(screen.getByText("Reglas del catálogo, procedencia y exclusiones")).toBeTruthy();
});
