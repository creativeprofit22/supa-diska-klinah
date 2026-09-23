import { invoke } from "@tauri-apps/api/core";
import { validId } from "../../shared/storage/types";
import { categories, sorts, type LargeFileFilter } from "./types";

const validBytes = (value: number) => Number.isSafeInteger(value) && value >= 0;
export function validLargeFileFilter(depth: number, filter: LargeFileFilter): boolean {
  return Number.isInteger(depth) && depth >= 0 && depth <= 64 &&
    validBytes(filter.minimumBytes) && (filter.maximumBytes === null ||
      (validBytes(filter.maximumBytes) && filter.maximumBytes >= filter.minimumBytes)) &&
    filter.extensions.length <= 64 && filter.extensions.every(extension => /^[a-z0-9]{1,32}$/.test(extension)) &&
    categories.includes(filter.category) && sorts.includes(filter.sort) && typeof filter.descending === "boolean";
}
export function startLargeFiles(rootId: string, depth: number, filter: LargeFileFilter): Promise<string> {
  if (!validId(rootId) || !validLargeFileFilter(depth, filter)) return Promise.reject({ code: "invalid_input" });
  return invoke("start_large_files", new TextEncoder().encode(JSON.stringify({ rootId, depth, filter: {
    minimumBytes: filter.minimumBytes, maximumBytes: filter.maximumBytes,
    extensions: [...filter.extensions], category: filter.category, sort: filter.sort, descending: filter.descending,
  } })));
}

export interface LargeFileFields {
  depth: string;
  minimumMiB: string;
  maximumMiB: string;
  extensions: string;
  category: LargeFileFilter["category"];
  sort: LargeFileFilter["sort"];
  descending: boolean;
}
export const defaultFields: LargeFileFields = {
  depth: "20", minimumMiB: "10", maximumMiB: "", extensions: "", category: "any", sort: "size", descending: true,
};
/** MiB may be fractional only when the resulting bytes are an exact safe integer. */
export function parseLargeFileFields(fields: LargeFileFields): { depth: number; filter: LargeFileFilter } | null {
  if (!fields.depth.trim() || !fields.minimumMiB.trim()) return null;
  const depth = Number(fields.depth);
  const filter: LargeFileFilter = {
    minimumBytes: Number(fields.minimumMiB) * 1_048_576,
    maximumBytes: fields.maximumMiB.trim() ? Number(fields.maximumMiB) * 1_048_576 : null,
    extensions: fields.extensions.trim() ? fields.extensions.trim().toLowerCase().split(/[\s,]+/) : [],
    category: fields.category, sort: fields.sort, descending: fields.descending,
  };
  return validLargeFileFilter(depth, filter) ? { depth, filter } : null;
}
