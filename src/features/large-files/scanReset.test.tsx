// @vitest-environment jsdom
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useStorageScan } from "../../shared/storage/useStorageScan";
import { candidateId, type LargeFileRow } from "./types";
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
const id = (n: number) => n.toString(16).padStart(32, "0");
afterEach(() => { cleanup(); vi.clearAllMocks(); });
it("reset clears outcome-stale selections, releases the snapshot and ignores an outstanding page", async () => {
  let resolvePage!: (value: object) => void;
  let late = false;
  const page = { snapshotId: id(1), records: [{ kind: "file", record: { recordId: id(2), displayPath: "Fixture", logicalBytes: 1, allocatedBytes: null, modifiedUnixSeconds: null, eligibility: { kind: "eligible", candidate_id: id(3) } } }], nextCursor: id(4), retainedTotal: 2, completeness: { reasons: [] } };
  invoke.mockImplementation(async (command: string) => {
    if (command === "storage_scan_status") return { snapshotId: id(1), module: "largeFiles", phase: "complete", completeness: { reasons: [] } };
    if (command === "storage_scan_page") return late ? new Promise(resolve => { resolvePage = resolve; }) : page;
    if (command === "release_storage_scan") return;
    throw new Error(`Forbidden IPC: ${command}`);
  });
  const { result } = renderHook(() => useStorageScan<LargeFileRow>({ module: "largeFiles", scopeKey: "fixture", collection: "files", candidateId }));
  await act(async () => result.current.start(async () => id(1)));
  await waitFor(() => expect(result.current.page).not.toBeNull());
  act(() => result.current.toggle(id(3)));
  expect(result.current.selection?.candidateIds).toEqual([id(3)]);
  late = true;
  let pending!: Promise<void>;
  act(() => { pending = result.current.nextPage(); });
  await act(async () => result.current.reset());
  expect(result.current.phase).toBe("idle"); expect(result.current.status).toBeNull(); expect(result.current.page).toBeNull(); expect(result.current.selected.size).toBe(0); expect(result.current.selection).toBeNull();
  await act(async () => { resolvePage(page); await pending; });
  expect(result.current.page).toBeNull();
  expect(invoke.mock.calls.filter(([command]) => command === "release_storage_scan")).toHaveLength(1);
});
it("reset releases a late start reply without polling it", async () => {
  invoke.mockImplementation(async (command: string) => { if (command !== "release_storage_scan") throw new Error(`Forbidden IPC: ${command}`); });
  let resolve!: (value: string) => void;
  const { result } = renderHook(() => useStorageScan<LargeFileRow>({ module: "largeFiles", scopeKey: "fixture", collection: "files", candidateId }));
  let pending!: Promise<void>;
  await act(async () => { pending = result.current.start(() => new Promise(r => { resolve = r; })); });
  await act(async () => result.current.reset());
  await act(async () => { resolve(id(1)); await pending; });
  expect(result.current.phase).toBe("idle"); expect(result.current.selection).toBeNull();
  expect(invoke.mock.calls.map(([command]) => command)).toEqual(["release_storage_scan"]);
});
it("retains at most 100 rows and 1000 explicitly selected candidate IDs across pages", async () => {
  let offset = 0;
  invoke.mockImplementation(async (command: string, bytes: Uint8Array) => {
    if (command === "storage_scan_status") return { snapshotId: id(1), module: "largeFiles", phase: "complete", completeness: { reasons: [] } };
    if (command === "storage_scan_page") {
      expect(JSON.parse(new TextDecoder().decode(bytes)).pageSize).toBe(100);
      const records = Array.from({ length: 100 }, (_, i) => ({ kind: "file", record: { recordId: id(offset + i + 100), displayPath: "Fixture", logicalBytes: 1, allocatedBytes: null, modifiedUnixSeconds: null, eligibility: { kind: "eligible", candidate_id: id(offset + i + 100) } } }));
      offset += 100;
      return { snapshotId: id(1), records, nextCursor: id(offset), retainedTotal: 1100, completeness: { reasons: [] } };
    }
    if (command === "release_storage_scan") return;
    throw new Error(`Forbidden IPC: ${command}`);
  });
  const { result } = renderHook(() => useStorageScan<LargeFileRow>({ module: "largeFiles", scopeKey: "fixture", collection: "files", candidateId }));
  await act(async () => result.current.start(async () => id(1)));
  await waitFor(() => expect(result.current.page).not.toBeNull());
  expect(result.current.selected.size).toBe(0);
  for (let page = 0; page < 11; page++) {
    expect(result.current.page?.records).toHaveLength(100);
    act(() => { for (const row of result.current.page!.records) result.current.toggle(candidateId(row)!); });
    expect(result.current.selected.size).toBe(Math.min((page + 1) * 100, 1000));
    if (page < 10) await act(async () => result.current.nextPage());
  }
  expect(result.current.selection?.candidateIds).toHaveLength(1000);
});
