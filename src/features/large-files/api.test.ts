import { afterEach, expect, it, vi } from "vitest";
import { defaultFields, parseLargeFileFields, startLargeFiles, validLargeFileFilter } from "./api";
import { categories, sorts, type LargeFileFilter } from "./types";
const invoke = vi.hoisted(() => vi.fn(async () => "a".repeat(32)));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
afterEach(() => vi.clearAllMocks());
const filter: LargeFileFilter = { minimumBytes: 0, maximumBytes: null, extensions: [], category: "any", sort: "size", descending: true };
it("serializes only the native Raw contract and normalizes UI extensions", async () => {
  const config = parseLargeFileFields({ ...defaultFields, minimumMiB: "0.5", maximumMiB: "2", extensions: "PDF, ZIP mp3", depth: "0" })!;
  expect(config).toEqual({ depth: 0, filter: { ...filter, minimumBytes: 524288, maximumBytes: 2097152, extensions: ["pdf", "zip", "mp3"] } });
  await startLargeFiles("a".repeat(32), config.depth, config.filter);
  const [command, bytes] = invoke.mock.calls[0] as unknown as [string, Uint8Array];
  expect(command).toBe("start_large_files"); expect(bytes).toBeInstanceOf(Uint8Array);
  expect(JSON.parse(new TextDecoder().decode(bytes))).toEqual({ rootId: "a".repeat(32), ...config });
});
it.each(["", "-1", "65", "0.5", "NaN"])("rejects invalid depth %s", depth => {
  expect(parseLargeFileFields({ ...defaultFields, depth })).toBeNull();
});
it.each(["", "-1", "NaN", "Infinity", "9007199254740991", "0.00000001"])("rejects invalid minimum MiB %s", minimumMiB => {
  expect(parseLargeFileFields({ ...defaultFields, minimumMiB })).toBeNull();
});
it("enforces extension, category, sort, size and safe integer bounds before IPC", async () => {
  const invalid: LargeFileFilter[] = [
    { ...filter, minimumBytes: Number.MAX_SAFE_INTEGER + 1 }, { ...filter, minimumBytes: 0.5 },
    { ...filter, maximumBytes: -1 }, { ...filter, minimumBytes: 2, maximumBytes: 1 },
    { ...filter, maximumBytes: Infinity }, { ...filter, maximumBytes: Number.MAX_SAFE_INTEGER + 1 },
    ...[[".zip"], ["ZIP"], ["a-b"], ["a".repeat(33)], Array.from({ length: 65 }, () => "zip")].map(extensions => ({ ...filter, extensions })),
    { ...filter, category: "bad" as LargeFileFilter["category"] }, { ...filter, sort: "bad" as LargeFileFilter["sort"] },
  ];
  for (const bad of invalid) await expect(startLargeFiles("a".repeat(32), 64, bad)).rejects.toEqual({ code: "invalid_input" });
  await expect(startLargeFiles("C:\\Fixture", 1, filter)).rejects.toEqual({ code: "invalid_input" });
  expect(invoke).not.toHaveBeenCalled();
  expect(parseLargeFileFields({ ...defaultFields, maximumMiB: "1" })).toBeNull();
});
it("accepts all native categories/sorts and inclusive bounds", () => {
  for (const category of categories) for (const sort of sorts) expect(validLargeFileFilter(64, { ...filter, category, sort, minimumBytes: Number.MAX_SAFE_INTEGER, maximumBytes: Number.MAX_SAFE_INTEGER, extensions: Array.from({ length: 64 }, () => "a".repeat(32)) })).toBe(true);
});
