import { invoke } from "@tauri-apps/api/core";
import { historyPage, historyRequest, type HistoryPage, type HistoryRequest } from "./history";
export type { HistoryRequest, HistoryPage } from "./history";
export type CleanupHistoryRequest = HistoryRequest;
export type CleanupHistoryPage = HistoryPage<CleanupExecutionSummary>;

export type CleanupDisposition = "recycleBin" | "quarantine" | "permanent";
export type CleanupItemState = "pending" | "mutating" | "recycled" | "quarantined" | "purged" | "restored" | "failed" | "unknown";
export interface CleanupPlanSummary {
  planId: string;
  disposition: CleanupDisposition;
  selectedCount: number;
  selectedBytes: number;
}
export interface CleanupItemOutcome {
  itemId: string;
  /** Bounded native display text from the immutable plan; never mutation authority. */
  displayPath?: string | null;
  state: CleanupItemState;
  logicalBytes: number;
  failure?: string | null;
}
export interface ByteAccounting {
  selectedBytes: number;
  processedBytes: number;
  failedBytes: number;
  quarantinedBytes: number;
  purgedBytes: number;
  occupiedBytes: number;
  reclaimedBytes: number;
}
export interface CleanupExecutionSummary {
  executionId: string;
  planId: string;
  disposition: CleanupDisposition;
  completed: boolean;
  purgeAfter?: number | null;
  items: CleanupItemOutcome[];
  accounting: ByteAccounting;
}
export function executeCleanupPlan(planId: string): Promise<CleanupExecutionSummary> {
  return invoke<CleanupExecutionSummary>("execute_cleanup_plan", { planId });
}
export function executePermanentCleanupPlan(planId: string): Promise<CleanupExecutionSummary> {
  return invoke<CleanupExecutionSummary>("execute_permanent_cleanup_plan", { planId });
}
export function undoCleanup(executionId: string): Promise<CleanupExecutionSummary> {
  return invoke<CleanupExecutionSummary>("undo_cleanup", { executionId });
}
export async function cleanupHistory(input: CleanupHistoryRequest = {}): Promise<CleanupHistoryPage> {
  const request = historyRequest(input, "cleanup");
  const page = await invoke<unknown>("cleanup_history", new TextEncoder().encode(JSON.stringify(request)));
  return historyPage<CleanupExecutionSummary>(page, "cleanup", request.limit);
}
export const cleanupActions = { executeCleanupPlan, executePermanentCleanupPlan, undoCleanup, cleanupHistory };
