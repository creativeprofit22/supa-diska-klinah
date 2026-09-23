// @vitest-environment jsdom
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useStorageScan } from "./useStorageScan";
import type { ScanApi, StoragePage, StorageStatus } from "./types";
import { MAX_SELECTION, PAGE_SIZE } from "./types";

type Row = { id: string };
const id = (n: number) => n.toString(16).padStart(32, "0");
const status = (snapshotId = id(1), phase: StorageStatus["phase"] = "complete"): StorageStatus => ({ snapshotId, module: "largeFiles", phase, visitedEntries: 2, retainedRecords: 2, hashedBytes: 0, completedHashes: 0, completeness: { reasons: [] } });
const page = (snapshotId = id(1), row = id(10), cursor: string | null = id(2)): StoragePage<Row> => ({ snapshotId, records: [{ id: row }], nextCursor: cursor, retainedTotal: 2, completeness: { reasons: [] } });
function deferred<T>() { let resolve!: (value: T) => void; let reject!: (error: unknown) => void; const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; }
function api() {
  return { status: vi.fn<ScanApi<Row>["status"]>().mockResolvedValue(status()), page: vi.fn<ScanApi<Row>["page"]>().mockResolvedValue(page()), cancel: vi.fn<ScanApi<Row>["cancel"]>().mockResolvedValue(), release: vi.fn<ScanApi<Row>["release"]>().mockResolvedValue() };
}
function setup(service = api()) {
  const hook = renderHook(({ scopeKey }) => useStorageScan({ module: "largeFiles", scopeKey, collection: "files", candidateId: (r: Row) => r.id, api: service }), { initialProps: { scopeKey: "scope-a" } });
  return { ...hook, service };
}
afterEach(() => { cleanup(); vi.useRealTimers(); });
async function flush() { await act(async () => { await Promise.resolve(); }); }

describe("shared storage lifecycle", () => {
  it("replaces pages, sends snapshot-bound cursors, and keeps only bounded selected IDs", async () => {
    const { result, service } = setup();
    await act(async () => { await result.current.start(async () => id(1)); });
    await waitFor(() => expect(result.current.page?.records).toEqual([{ id: id(10) }]));
    act(() => result.current.toggle(id(10)));
    service.page.mockResolvedValueOnce(page(id(1), id(11), null));
    await act(async () => { await result.current.nextPage(); });
    expect(service.page).toHaveBeenLastCalledWith(expect.objectContaining({ snapshotId: id(1), cursor: id(2), pageSize: PAGE_SIZE }));
    expect(result.current.page?.records).toEqual([{ id: id(11) }]);
    expect([...result.current.selected]).toEqual([id(10)]);
    act(() => result.current.toggle(id(999)));
    expect([...result.current.selected]).toEqual([id(10)]);
    await act(async () => { await result.current.firstPage(); });
    expect(service.page).toHaveBeenLastCalledWith(expect.objectContaining({ cursor: undefined }));
  });
  it("cancels synchronously, ignores a late status, and releases the snapshot", async () => {
    const service = api(); const pending = deferred<StorageStatus>();
    service.status.mockReturnValueOnce(pending.promise);
    const { result } = setup(service);
    await act(async () => { await result.current.start(async () => id(1)); });
    await act(async () => { await result.current.cancel(); });
    await act(async () => { pending.resolve(status()); });
    expect(result.current.phase).toBe("cancelled");
    expect(result.current.selection).toBeNull(); expect(service.page).not.toHaveBeenCalled();
    expect(service.cancel).toHaveBeenCalledWith({ module: "largeFiles", snapshotId: id(1) });
    expect(service.release).toHaveBeenCalledTimes(1);
  });
  it("does not grant authority when cancel fails", async () => {
    const service = api(); service.status.mockResolvedValue(status(id(1), "walking")); service.cancel.mockRejectedValue(new Error("private path"));
    const { result } = setup(service);
    await act(async () => { await result.current.start(async () => id(1)); });
    await act(async () => { await result.current.cancel(); });
    expect(result.current.phase).toBe("failed"); expect(result.current.error).toContain("could not be confirmed");
    expect(result.current.selection).toBeNull(); expect(service.release).toHaveBeenCalledTimes(1);
  });
  it("releases a start response that arrives after cancellation or unmount", async () => {
    const pending = deferred<string>(); const { result, service, unmount } = setup();
    act(() => { void result.current.start(() => pending.promise); });
    await flush();
    await act(async () => { await result.current.cancel(); });
    unmount();
    await act(async () => { pending.resolve(id(7)); });
    expect(service.release).toHaveBeenCalledWith({ module: "largeFiles", snapshotId: id(7) });
    expect(service.status).not.toHaveBeenCalled();
  });
  it("clears selection on a new scan and drops late pages from the previous scan", async () => {
    const { result, service } = setup();
    await act(async () => { await result.current.start(async () => id(1)); });
    act(() => result.current.toggle(id(10)));
    expect(result.current.selection?.candidateIds).toEqual([id(10)]);
    const pending = deferred<StoragePage<Row>>(); service.page.mockReturnValueOnce(pending.promise);
    act(() => { void result.current.nextPage(); });
    service.status.mockResolvedValue(status(id(3))); service.page.mockResolvedValue(page(id(3), id(30)));
    await act(async () => { await result.current.start(async () => id(3)); });
    await act(async () => { pending.resolve(page(id(1), id(99))); });
    expect(result.current.page?.snapshotId).toBe(id(3));
    expect(result.current.selected.size).toBe(0); expect(result.current.selection).toBeNull();
    expect(service.release).toHaveBeenCalledWith({ module: "largeFiles", snapshotId: id(1) });
  });
  it("ignores a selection callback retained from the previous scan", async () => {
    const { result, service } = setup();
    await act(async () => { await result.current.start(async () => id(1)); });
    const oldToggle = result.current.toggle;
    service.status.mockResolvedValue(status(id(3))); service.page.mockResolvedValue(page(id(3), id(30)));
    await act(async () => { await result.current.start(async () => id(3)); });
    act(() => oldToggle(id(10)));
    expect(result.current.selected.size).toBe(0);
  });
  it("scope/filter changes clear authority and ignore pending responses", async () => {
    const { result, service, rerender } = setup();
    await act(async () => { await result.current.start(async () => id(1)); });
    act(() => result.current.toggle(id(10)));
    rerender({ scopeKey: "scope-a:new-filter" });
    expect(result.current.phase).toBe("idle"); expect(result.current.page).toBeNull(); expect(result.current.selection).toBeNull();
    expect(service.release).toHaveBeenCalledTimes(1);
  });
  it("ignores out-of-order page transitions", async () => {
    const { result, service } = setup();
    await act(async () => { await result.current.start(async () => id(1)); });
    const late = deferred<StoragePage<Row>>(); service.page.mockReturnValueOnce(late.promise);
    act(() => { void result.current.nextPage(); });
    service.page.mockResolvedValueOnce(page(id(1), id(44)));
    await act(async () => { await result.current.firstPage(); });
    await act(async () => { late.resolve(page(id(1), id(99))); });
    expect(result.current.page?.records).toEqual([{ id: id(44) }]);
  });
  it("polls serially and stops timers on completion and unmount", async () => {
    vi.useFakeTimers();
    const service = api(); const pending = deferred<StorageStatus>(); service.status.mockReturnValueOnce(pending.promise);
    const { result, unmount } = setup(service);
    await act(async () => { await result.current.start(async () => id(1)); });
    await act(async () => { await vi.advanceTimersByTimeAsync(5000); });
    expect(service.status).toHaveBeenCalledTimes(1);
    await act(async () => { pending.resolve(status(id(1), "walking")); });
    await act(async () => { await vi.advanceTimersByTimeAsync(500); });
    expect(service.status).toHaveBeenCalledTimes(2);
    await act(async () => { await vi.advanceTimersByTimeAsync(5000); });
    expect(service.status).toHaveBeenCalledTimes(2);
    unmount(); expect(vi.getTimerCount()).toBe(0); expect(service.release).toHaveBeenCalledTimes(1);
  });
  it("bounds pages and selections, rejecting mismatched snapshots", async () => {
    const { result, service } = setup();
    service.page.mockResolvedValue({ ...page(), records: Array.from({length: PAGE_SIZE}, (_, n) => ({id:id(n+10)})) });
    await act(async () => { await result.current.start(async () => id(1)); });
    for(let batch=0; batch<11; batch++) {
      service.page.mockResolvedValue({ ...page(), records: Array.from({length: PAGE_SIZE}, (_, n) => ({id:id(batch*PAGE_SIZE+n+10)})) });
      await act(async () => { await result.current.firstPage(); });
      act(() => { for(const row of result.current.page!.records) result.current.toggle(row.id); });
    }
    expect(result.current.selected.size).toBe(MAX_SELECTION);
    service.page.mockResolvedValue(page(id(99)));
    await act(async () => { await result.current.firstPage(); });
    expect(result.current.selection).toBeNull(); expect(result.current.error).toContain("no longer matches");
    service.page.mockResolvedValue({ ...page(), records: Array.from({length: PAGE_SIZE+1}, () => ({id:id(10)})) });
    await act(async () => { await result.current.firstPage(); });
    expect(result.current.page).toBeNull();
  });
  it("distinguishes busy, expired, and partial results without exposing native errors", async () => {
    const { result, service } = setup();
    await act(async () => { await result.current.start(async () => { throw {code:"busy", path:"private"}; }); });
    expect(result.current.error).toContain("Another scan");
    service.status.mockRejectedValueOnce({code:"snapshot_unavailable"});
    await act(async () => { await result.current.start(async () => id(1)); });
    expect(result.current.error).toContain("expired");
    service.status.mockResolvedValue({...status(), completeness:{reasons:["entryLimit"]}});
    await act(async () => { await result.current.start(async () => id(1)); });
    expect(result.current.status?.completeness.reasons).toEqual(["entryLimit"]);
  });
});
