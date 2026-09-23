import { invoke } from "@tauri-apps/api/core";
import { validId } from "../../shared/storage/types";
export function parseFields(depthText: string, minimumText = "0", maximumText = "", extensionText = "") {
  const depth = Number(depthText), minimumBytes = Number(minimumText) * 1_048_576;
  const maximumBytes = maximumText.trim() ? Number(maximumText) * 1_048_576 : null;
  const extensions = [...new Set(extensionText.split(",").map(value => value.trim().replace(/^\./, "").toLowerCase()).filter(Boolean))];
  if (!depthText.trim() || !minimumText.trim() || !Number.isInteger(depth) || depth < 0 || depth > 64 || !Number.isSafeInteger(minimumBytes) || minimumBytes < 0 ||
      (maximumBytes !== null && (!Number.isSafeInteger(maximumBytes) || maximumBytes < minimumBytes)) ||
      extensions.length > 64 || extensions.some(value => !/^[a-z0-9]{1,32}$/.test(value))) return null;
  return { depth, minimumBytes, maximumBytes, extensions };
}
export function startDuplicates(rootId: string, depth: number, minimumBytes: number, maximumBytes: number | null = null, extensions: readonly string[] = []): Promise<string> {
  if (!validId(rootId) || !Number.isInteger(depth) || depth < 0 || depth > 64 || !Number.isSafeInteger(minimumBytes) || minimumBytes < 0 ||
      (maximumBytes !== null && (!Number.isSafeInteger(maximumBytes) || maximumBytes < minimumBytes)) ||
      extensions.length > 64 || extensions.some(value => !/^[a-z0-9]{1,32}$/.test(value))) return Promise.reject({ code: "invalid_input" });
  return invoke("start_duplicates", new TextEncoder().encode(JSON.stringify({ rootId, depth, minimumBytes, maximumBytes, extensions: [...extensions] })));
}
