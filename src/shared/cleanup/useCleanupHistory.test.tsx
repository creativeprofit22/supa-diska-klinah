// @vitest-environment jsdom
import { useEffect } from "react";
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useCleanupHistory } from "./useCleanupHistory";
import type { CleanupExecutionSummary } from "./api";
import type { HistoryPage } from "./history";

afterEach(cleanup);
const cursor = JSON.stringify({version:1,kind:"cleanup",timestamp:1,id:"a".repeat(32)});
const row = {executionId:"old"} as CleanupExecutionSummary;
const page = {records:[row],nextCursor:cursor};
function deferred() {
  let resolve!: (page: HistoryPage<CleanupExecutionSummary>) => void;
  let reject!: (cause: unknown) => void;
  const promise = new Promise<HistoryPage<CleanupExecutionSummary>>((yes,no) => {resolve=yes;reject=no;});
  return {promise,resolve,reject};
}
it("retains only one page, preserves it on errors and rejects oversized envelopes", async () => {
  const fetch = vi.fn().mockResolvedValueOnce(page).mockRejectedValueOnce(new Error("private"))
    .mockResolvedValueOnce({records:Array(21).fill(row),nextCursor:null}).mockResolvedValue({records:[],nextCursor:null});
  const {result} = renderHook(() => useCleanupHistory(fetch));
  expect(result.current.loaded).toBe(false);
  await act(() => result.current.refresh());
  await act(() => result.current.older());
  expect(result.current.records).toEqual([row]);
  expect(result.current.error).not.toContain("private");
  await act(() => result.current.older());
  expect(result.current.error).toBeTruthy();
  await act(() => result.current.older());
  expect(result.current.records).toEqual([]);
  expect(result.current.currentCursor).toBe(cursor);
  expect(result.current.nextCursor).toBeNull();
});
it("ignores stale success and failure after a newer request", async () => {
  const first = deferred(); const second = deferred();
  const fetch = vi.fn().mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise).mockResolvedValue(page);
  const {result} = renderHook(() => useCleanupHistory(fetch));
  act(() => {void result.current.refresh(); void result.current.refresh();});
  await act(async () => second.resolve(page));
  await act(async () => first.reject(new Error("stale")));
  expect(result.current.error).toBeNull();
  expect(result.current.records).toEqual([row]);
  const late = deferred(); fetch.mockReturnValueOnce(late.promise);
  act(() => {void result.current.older();});
  await act(() => result.current.refresh());
  await act(async () => late.resolve({records:[],nextCursor:null}));
  expect(result.current.records).toEqual([row]);
  expect(result.current.currentCursor).toBeNull();
});
it("survives StrictMode effect replay and invalidates unmounted requests", async () => {
  const first = deferred(); const second = deferred(); const last = deferred();
  const fetch = vi.fn().mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise).mockReturnValue(last.promise);
  const {result,unmount} = renderHook(() => {
    const history = useCleanupHistory(fetch);
    useEffect(() => {void history.refresh();}, [history.refresh]);
    return history;
  }, {reactStrictMode:true});
  expect(fetch).toHaveBeenCalledTimes(2);
  await act(async () => second.resolve(page));
  await act(async () => first.resolve({records:[],nextCursor:null}));
  expect(result.current.records).toEqual([row]);
  act(() => {void result.current.older();});
  const before = result.current;
  unmount();
  await act(async () => last.resolve({records:[],nextCursor:null}));
  expect(result.current).toBe(before);
});
it("Undo patches only a matching visible immutable ID and invalidates pre-Undo reads", async () => {
  const late = deferred();
  const fetch = vi.fn().mockResolvedValueOnce(page).mockReturnValue(late.promise);
  const {result} = renderHook(() => useCleanupHistory(fetch));
  await act(() => result.current.refresh());
  act(() => {void result.current.refresh(); result.current.updateVisible({...row,completed:true});});
  await act(async () => late.resolve(page));
  expect(result.current.records[0].completed).toBe(true);
  act(() => result.current.updateVisible({...row,executionId:"not-visible"}));
  expect(result.current.records).toHaveLength(1);
  expect(result.current.records[0].executionId).toBe("old");
});
