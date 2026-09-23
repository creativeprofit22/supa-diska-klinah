import { invoke } from "@tauri-apps/api/core";
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
export const outcome: Record<VendorJobState, string> = {
  awaitingConfirmation: "Awaiting separate Windows confirmation. Nothing has launched.",
  queued: "Queued for vendor launch. Vendor UI or UAC may appear.",
  launching: "Waiting for the vendor launcher. Navigating away does not stop the installer.",
  completed: "Launcher exited successfully. This is not proof that the program was removed.",
  cancelledByVendorOrUAC: "Vendor or UAC declined/cancelled. Removal is not confirmed.",
  cancelledBeforeLaunch: "Cancelled before launch. No vendor installer was started by this job.",
  failed: "Vendor launch or operation failed. Removal is not confirmed.",
  rebootRequired: "Vendor reported a reboot is required. Removal is not yet confirmed.",
  outcomeUnknown: "Outcome unknown: timeout, interrupted waiting, or an unverified vendor exit. The installer may still be running; it was not terminated.",
};
export function vendorError(error: unknown): string {
  const code = typeof error === "object" && error !== null && "code" in error ? error.code : "";
  switch (code) {
    case "history_count_capacity": return "Retained vendor history has reached its 1,000-outcome capacity. New jobs are stopped until a future journal migration. You can still inspect retained history.";
    case "history_size_capacity": return "Retained vendor history has reached its storage capacity (8 MiB safeguard). New jobs are stopped until a future journal migration. You can still inspect retained history.";
    case "busy": return "Another operation or an unresolved vendor job is active.";
    case "expired": case "registry_changed": case "executable_changed": return "Program evidence expired or changed. Refresh inventory and review again.";
    case "unsupported_command": return "This vendor command is not supported. Use Windows installed-app settings.";
    default: return "The vendor request could not be confirmed. Check retained history before trying again.";
  }
}
