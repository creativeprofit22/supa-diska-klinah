// @vitest-environment jsdom
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useStorageRoot } from "./useStorageRoot";
import type { RootChoice, StorageModule } from "./types";

const invoke = vi.hoisted(() => vi.fn(async () => undefined));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
const root = (n: number, module: StorageModule = "largeFiles"): RootChoice => ({ rootId: n.toString(16).padStart(32, "0"), module, displayPath: "Fixture" });
const released = () => invoke.mock.calls.map(call => JSON.parse(new TextDecoder().decode((call as unknown as [string, Uint8Array])[1])));
afterEach(() => { cleanup(); vi.clearAllMocks(); });

it("preserves scope on cancellation, releases replacement and unmount authorizations", async () => {
  const { result, unmount } = renderHook(() => useStorageRoot("largeFiles"));
  await act(async () => result.current.acquire(async () => root(1)));
  await act(async () => result.current.acquire(async () => null));
  expect(result.current.choice).toEqual(root(1)); expect(result.current.available).toBe(true);
  expect(released()).toEqual([]);
  await act(async () => result.current.acquire(async () => root(2)));
  expect(released()).toContainEqual({ module: "largeFiles", snapshotId: root(1).rootId });
  unmount();
  expect(released()).toContainEqual({ module: "largeFiles", snapshotId: root(2).rootId });
});
it.each([false, true])("retires a run authorization after success/failure (%s), without permitting reuse", async fail => {
  const { result } = renderHook(() => useStorageRoot("largeFiles"));
  await act(async () => result.current.acquire(async () => root(1)));
  const consume = vi.fn(async (id: string) => { expect(id).toBe(root(1).rootId); if (fail) throw new Error("busy"); return "snapshot"; });
  await act(async () => {
    if (fail) await expect(result.current.run(consume)).rejects.toThrow("busy");
    else expect(await result.current.run(consume)).toBe("snapshot");
  });
  expect(result.current.available).toBe(false);
  await expect(result.current.run(consume)).rejects.toEqual({ code: "scope_unavailable" });
  expect(consume).toHaveBeenCalledTimes(1);
  expect(released()).toEqual([{ module: "largeFiles", snapshotId: root(1).rootId }]);
});
it("ignores and releases late picker replies after unmount", async () => {
  let resolve!: (choice: RootChoice) => void;
  const { result, unmount } = renderHook(() => useStorageRoot("largeFiles"));
  let pending!: Promise<void>;
  act(() => { pending = result.current.acquire(() => new Promise(r => { resolve = r; })); });
  unmount();
  await act(async () => { resolve(root(1)); await pending; });
  expect(released()).toEqual([{ module: "largeFiles", snapshotId: root(1).rootId }]);
});
it("invalidates old module replies without unlocking a newer picker", async () => {
  let resolve!: (choice: RootChoice) => void;
  const { result, rerender } = renderHook(({ module }: { module: StorageModule }) => useStorageRoot(module), { initialProps: { module: "largeFiles" } });
  let pending!: Promise<void>;
  act(() => { pending = result.current.acquire(() => new Promise(r => { resolve = r; })); });
  rerender({ module: "cleaner" });
  await act(async () => result.current.acquire(async () => root(2, "cleaner")));
  await act(async () => { resolve(root(1)); await pending; });
  expect(result.current.choice).toEqual(root(2, "cleaner"));
  expect(result.current.available).toBe(true);
  expect(released()).toContainEqual({ module: "largeFiles", snapshotId: root(1).rootId });
});
it("reports sanitized picker errors while retaining a prior authorization", async () => {
  const { result } = renderHook(() => useStorageRoot("largeFiles"));
  await act(async () => result.current.acquire(async () => root(1)));
  await act(async () => result.current.acquire(async () => { throw { code: "scope_unavailable", message: "private path" }; }));
  expect(result.current.error).toMatch(/scope is unavailable/);
  expect(result.current.available).toBe(true);
  expect(result.current.choice).toEqual(root(1));
});
