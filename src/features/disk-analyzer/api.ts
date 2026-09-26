import { invoke } from "@tauri-apps/api/core";
import { validId } from "../../shared/storage/types";

export function startDiskAnalyzer(rootId: string, displayedDepth: number): Promise<string> {
  if (!validId(rootId) || !Number.isInteger(displayedDepth) || displayedDepth < 0 || displayedDepth > 64) {
    return Promise.reject({ code: "invalid_input" });
  }
  return invoke<string>("start_disk_analyzer", new TextEncoder().encode(JSON.stringify({ rootId, displayedDepth })));
}
