export interface HistoryRequest { cursor?: string | null; limit?: number }
export interface HistoryPage<T> { records: T[]; nextCursor: string | null }
export type HistoryKind = "cleanup" | "vendor";

function object(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
export function validateHistoryCursor(value: unknown, kind: HistoryKind): asserts value is string | null {
  if (value === null) return;
  if (typeof value !== "string" || new TextEncoder().encode(value).length > 256) throw new Error("Invalid history cursor");
  let cursor: unknown;
  try { cursor = JSON.parse(value); } catch { throw new Error("Invalid history cursor"); }
  // Never reserialize the cursor: native u64 timestamps may exceed JS safe integers.
  if (!object(cursor) || Object.keys(cursor).sort().join(",") !== "id,kind,timestamp,version" ||
      cursor.version !== 1 || cursor.kind !== kind || typeof cursor.id !== "string" || !/^[a-fA-F0-9]{32}$/.test(cursor.id) ||
      typeof cursor.timestamp !== "number" || !Number.isInteger(cursor.timestamp) || cursor.timestamp < 0 || cursor.timestamp > 18446744073709551615) {
    throw new Error("Invalid history cursor");
  }
}
export function historyRequest(input: HistoryRequest, kind: HistoryKind): Required<HistoryRequest> {
  const max = kind === "cleanup" ? 100 : 64;
  if (!object(input) || Object.keys(input).some(key => key !== "cursor" && key !== "limit")) throw new Error("Invalid history request");
  const cursor = input.cursor === undefined ? null : input.cursor;
  const limit = input.limit === undefined ? (kind === "cleanup" ? 20 : 64) : input.limit;
  validateHistoryCursor(cursor, kind);
  if (typeof limit !== "number" || !Number.isInteger(limit) || limit < 1 || limit > max) throw new Error("Invalid history limit");
  return { cursor, limit };
}
export function historyPage<T>(value: unknown, kind: HistoryKind, limit: number): HistoryPage<T> {
  if (!object(value) || Object.keys(value).sort().join(",") !== "nextCursor,records" || !Array.isArray(value.records) || value.records.length > limit) {
    throw new Error("Invalid history page");
  }
  validateHistoryCursor(value.nextCursor, kind);
  return { records: value.records as T[], nextCursor: value.nextCursor };
}
