import { invoke } from "@tauri-apps/api/core";
import { type UninstallerStrings, uninstallerStrings } from "./strings";
import { historyPage, historyRequest, type HistoryPage, type HistoryRequest } from "../../shared/cleanup/history";
export type VendorHistoryRequest = HistoryRequest;
export type VendorHistoryPage = HistoryPage<VendorJob>;
export interface ProgramQuery { nameContains: string; largestFirst: boolean }
export interface InstalledProgram {
  programId: string; name: string; publisher: string | null; version: string | null;
  installDate: string | null; estimatedSizeBytes: number | null; lastUsedAt: number | null; leftoverSupport: string;
}
export type ProgramRow = { kind: "program"; record: InstalledProgram };
export type VendorJobState = "awaitingConfirmation" | "queued" | "launching" | "completed" | "cancelledByVendorOrUAC" | "cancelledBeforeLaunch" | "failed" | "rebootRequired" | "outcomeUnknown";
export interface VendorJob {
  jobId: string; programId: string; programName: string; state: VendorJobState;
  createdAt: number; updatedAt: number; exitCode: number | null; launchError: number | null;
  persistenceError: boolean; completionMeaning: string; leftoverSupport: string;
}
function request<T>(command: string, input: object): Promise<T> {
  return invoke<T>(command, new TextEncoder().encode(JSON.stringify(input)));
}
export const startProgramInventory = (query: ProgramQuery) => request<string>("start_program_inventory", { nameContains: query.nameContains, largestFirst: query.largestFirst });
export const prepareVendorJob = (snapshotId: string, programId: string) => request<VendorJob>("prepare_vendor_job", { snapshotId, programId });
export const confirmVendorJob = (jobId: string) => request<VendorJob>("confirm_vendor_job", { jobId });
export const vendorJobStatus = (jobId: string) => request<VendorJob>("vendor_job_status", { jobId });
export const cancelVendorJob = (jobId: string) => request<VendorJob>("cancel_vendor_job", { jobId });
export const releaseVendorJob = (jobId: string) => request<void>("release_vendor_job", { jobId });
export async function vendorJobHistory(input: VendorHistoryRequest = {}): Promise<VendorHistoryPage> {
  const dto = historyRequest(input, "vendor");
  return historyPage<VendorJob>(await request<unknown>("vendor_job_history", dto), "vendor", dto.limit);
}
export const pending = (job: VendorJob) => job.state === "queued" || job.state === "launching";
export function vendorError(error: unknown, t: UninstallerStrings["errors"] = uninstallerStrings.en.errors): string {
  const code = typeof error === "object" && error !== null && "code" in error ? error.code : "";
  switch (code) {
    case "history_count_capacity": return t.historyCountCapacity;
    case "history_size_capacity": return t.historySizeCapacity;
    case "busy": return t.busy;
    case "expired": case "registry_changed": case "executable_changed": return t.expired;
    case "unsupported_command": return t.unsupported;
    default: return t.unknown;
  }
}
