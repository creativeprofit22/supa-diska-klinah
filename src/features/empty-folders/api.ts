import { invoke } from "@tauri-apps/api/core";
import { validId } from "../../shared/storage/types";
export function startEmptyFolders(rootId: string, depth: number): Promise<string> {
  if (!validId(rootId) || !Number.isInteger(depth) || depth < 0 || depth > 64) return Promise.reject({ code: "invalid_input" });
  return invoke("start_empty_folders", new TextEncoder().encode(JSON.stringify({ rootId, depth })));
}
