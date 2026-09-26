// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { I18nProvider } from "../../shared/i18n/I18nProvider";
import { BrowserPage, cacheLabel } from "./BrowserPage";
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
const id = (n: number) => n.toString(16).padStart(32, "0");
const calls = (command: string) => invoke.mock.calls.filter(([name]) => name === command).map(([, bytes]) => JSON.parse(new TextDecoder().decode(bytes)));
const policy = { source: "rules/win32/browsers.json", revision: "pinned-revision", lifecycle: "candidate", risk: "highImpact", consequence: "Rebuild caches", minimumAgeSeconds: 3600, serviceWorkerDisclosure: "Offline content may be removed", unsupported: ["Unsupported fixture"], exclusions: ["Cookies"], profileCacheRoots: ["Cache/Cache_Data"], sharedCacheRoots: ["ShaderCache"] };
let current = 1;
beforeEach(() => {
  current = 1;
  Object.defineProperty(HTMLDialogElement.prototype, "showModal", { configurable: true, value() { this.open = true; } });
  Object.defineProperty(HTMLDialogElement.prototype, "close", { configurable: true, value() { this.open = false; } });
  invoke.mockImplementation(async (command: string, bytes: Uint8Array) => {
    expect(Object.prototype.toString.call(bytes)).toBe("[object Uint8Array]");
    const input = JSON.parse(new TextDecoder().decode(bytes));
    switch (command) {
      case "list_browser_policy": expect(input).toEqual({}); return policy;
      case "list_storage_scopes": return [1, 2, 3].map(n => ({ scopeId: id(n), module: "browser", label: `Scope ${n}`, displayPath: n < 3 ? `C:/Fixture${n}` : null, available: n < 3 }));
      case "authorize_storage_scope": current = Number.parseInt(input.scopeId, 16); return { rootId: id(current + 10), module: "browser", displayPath: `C:/Fixture${current}` };
      case "start_browser_scan": return id(current + 20);
      case "storage_scan_status": return { snapshotId: input.snapshotId, module: "browser", phase: "complete", visitedEntries: 1, retainedRecords: 1, hashedBytes: 0, completedHashes: 0, completeness: { reasons: [] } };
      case "storage_scan_page": return { snapshotId: input.snapshotId, records: [{ kind: "file", record: { recordId: id(current + 30), displayPath: `C:/Fixture${current}/Default/Cache/Cache_Data/file`, logicalBytes: 12, allocatedBytes: null, modifiedUnixSeconds: null, eligibility: { kind: "eligible", candidate_id: id(current + 40) } } }], nextCursor: null, retainedTotal: 1, completeness: { reasons: [] } };
      case "create_storage_plan": return { planId: id(99), disposition: input.disposition, selectedCount: 1, selectedBytes: 12 };
      case "cancel_storage_scan": case "release_storage_scan": return;
      default: throw Error(`Unexpected IPC ${command}`);
    }
  });
});
afterEach(() => { cleanup(); expect(invoke.mock.calls.some(([name]) => /execute|undo|purge/.test(name))).toBe(false); vi.clearAllMocks(); });
async function select(n = 1) { fireEvent.click(await screen.findByRole("radio", { name: new RegExp(`Scope ${n}`) })); }
async function authorize() { fireEvent.click(screen.getByRole("button", { name: "Authorize selected scope" })); await waitFor(() => expect(screen.getByRole<HTMLButtonElement>("button", { name: "Scan browser scope" }).disabled).toBe(false)); }
async function start(n = 1) { await select(n); await authorize(); fireEvent.click(screen.getByRole("button", { name: "Scan browser scope" })); await screen.findByRole("list", { name: "Browser results" }); }
it("shows native unavailable scopes and honest catalog metadata without default authority", async () => {
  render(<BrowserPage />); await screen.findByRole("radio", { name: /Scope 1/ });
  expect(screen.getAllByRole<HTMLInputElement>("radio").every(r => !r.checked)).toBe(true);
  expect(screen.getByRole<HTMLInputElement>("radio", { name: /Scope 3/ }).disabled).toBe(true);
  expect(calls("authorize_storage_scope")).toEqual([]); expect(calls("start_browser_scan")).toEqual([]);
  expect(screen.getByRole<HTMLInputElement>("checkbox", { name: "Include service-worker caches" }).checked).toBe(false);
  for (const text of [/3,600 seconds/, /pinned-revision/, /Offline content may be removed/]) expect(screen.getByText(text)).toBeTruthy();
});
it("reviews explicit snapshot IDs with app recovery default, never executes", async () => {
  render(<BrowserPage />); await start();
  const file = screen.getByRole<HTMLInputElement>("checkbox", { name: "C:/Fixture1/Default/Cache/Cache_Data/file" }); expect(file.checked).toBe(false);
  const review = screen.getByRole<HTMLButtonElement>("button", { name: "Review: Move to app recovery" }); expect(review.disabled).toBe(true);
  fireEvent.click(file); fireEvent.click(review); await screen.findByRole("dialog");
  expect(calls("authorize_storage_scope")).toEqual([{ module: "browser", scopeId: id(1) }]);
  expect(calls("start_browser_scan")).toEqual([{ rootId: id(11), serviceWorkerOptIn: false }]);
  expect(calls("create_storage_plan")).toEqual([{ selection: { module: "browser", snapshotId: id(21), candidateIds: [id(41)] }, disposition: "quarantine" }]);
  expect(screen.queryByRole("button", { name: "Review: Move to Recycle Bin" })).toBeNull();
});
it("isolates sequential roots and clears prior results and selection", async () => {
  render(<BrowserPage />); await start(); fireEvent.click(screen.getByRole("checkbox", { name: "C:/Fixture1/Default/Cache/Cache_Data/file" }));
  await select(2); expect(screen.queryByRole("checkbox", { name: "C:/Fixture1/Default/Cache/Cache_Data/file" })).toBeNull();
  expect(calls("release_storage_scan")).toContainEqual({ module: "browser", snapshotId: id(21) });
  await authorize(); fireEvent.click(screen.getByRole("button", { name: "Scan browser scope" }));
  expect((await screen.findByRole<HTMLInputElement>("checkbox", { name: "C:/Fixture2/Default/Cache/Cache_Data/file" })).checked).toBe(false);
  expect(calls("start_browser_scan")).toEqual([{ rootId: id(11), serviceWorkerOptIn: false }, { rootId: id(12), serviceWorkerOptIn: false }]);
});
it("opt-in changes release scans and stale selection, requiring fresh authorization", async () => {
  render(<BrowserPage />); await start();
  fireEvent.click(screen.getByRole("checkbox", { name: "C:/Fixture1/Default/Cache/Cache_Data/file" }));
  fireEvent.click(screen.getByRole("checkbox", { name: "Include service-worker caches" }));
  expect(screen.queryByRole("list", { name: "Browser results" })).toBeNull();
  expect(calls("release_storage_scan")).toContainEqual({ module: "browser", snapshotId: id(21) });
  expect(screen.getByRole<HTMLButtonElement>("button", { name: "Scan browser scope" }).disabled).toBe(true);
  await authorize(); fireEvent.click(screen.getByRole("button", { name: "Scan browser scope" }));
  await screen.findByRole("list", { name: "Browser results" });
  expect(calls("start_browser_scan").at(-1)).toEqual({ rootId: id(11), serviceWorkerOptIn: true });
  expect(screen.getByRole<HTMLInputElement>("checkbox", { name: "C:/Fixture1/Default/Cache/Cache_Data/file" }).checked).toBe(false);
});
it.each(["Browser active", "Browser activity unknown"])("shows native refusal: %s", async message => {
  const original = invoke.getMockImplementation()!;
  invoke.mockImplementation(async (command, bytes) => { if (command === "start_browser_scan") throw { message }; return original(command, bytes); });
  render(<BrowserPage />); await select(); await authorize(); fireEvent.click(screen.getByRole("button", { name: "Scan browser scope" }));
  await screen.findByRole("alert"); expect(screen.queryByRole("list", { name: "Browser results" })).toBeNull();
  expect(calls("create_storage_plan")).toEqual([]);
});
it("labels normalized profile/shared paths only for display and leaves unknown paths unclassified", () => {
  expect(cacheLabel(String.raw`\\?\C:\FIXTURE\Default\Cache\Cache_Data\file`, "c:/fixture", policy)).toBe("Per-profile cache: default");
  expect(cacheLabel("C:/Fixture/ShaderCache/file", "c:/fixture", policy)).toBe("Shared cache");
  expect(cacheLabel("C:/Fixture/odd/file", "c:/fixture", policy)).toBe("Unclassified cache");
  expect(cacheLabel("C:/Fixture-other/ShaderCache/file", "c:/fixture", policy)).toBe("Unclassified cache");
});
it("renders Spanish (es-419) text when the language is Spanish", async () => {
  render(<I18nProvider languages={["es-MX"]}><BrowserPage /></I18nProvider>);
  expect(screen.getByRole("heading", { name: "Cachés del navegador" })).toBeTruthy();
  await screen.findByRole("radio", { name: /Scope 1/ });
  expect(screen.getByRole("button", { name: "Actualizar alcances nativos" })).toBeTruthy();
});
