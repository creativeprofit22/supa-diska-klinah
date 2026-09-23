import { beforeEach, expect, it, vi } from "vitest";
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
import { vendorJobHistory as load } from "./api";
beforeEach(() => invoke.mockReset());
it("vendor bounded raw page API", async () => {
  invoke.mockResolvedValue({ records: [], nextCursor: null });
  expect(await load()).toEqual({ records: [], nextCursor: null });
  expect(JSON.parse(new TextDecoder().decode(invoke.mock.calls[0][1]))).toEqual({ cursor: null, limit: 64 });
  for (const limit of [0, 65, 1.5]) await expect(load({ limit })).rejects.toThrow();
  const cursor = JSON.stringify({ version: 1, kind: "vendor", timestamp: 1, id: "a".repeat(32) });
  invoke.mockResolvedValue({ records: [], nextCursor: cursor });
  expect((await load({ cursor, limit: 1 })).nextCursor).toBe(cursor);
  for (const id of ["", "a", "g".repeat(32), "a".repeat(33)]) {
    await expect(load({ cursor: JSON.stringify({ version: 1, kind: "vendor", timestamp: 1, id }) })).rejects.toThrow();
  }
  for (const page of [[], { records: [] }, { records: [1, 2], nextCursor: null }, { records: [], nextCursor: "x".repeat(257) }, { records: [], nextCursor: cursor.replace("vendor", "cleanup") }]) {
    invoke.mockResolvedValue(page);
    await expect(load({ limit: 1 })).rejects.toThrow();
  }
});
