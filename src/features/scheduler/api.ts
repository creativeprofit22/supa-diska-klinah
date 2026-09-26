import { invoke } from "@tauri-apps/api/core";
import type { ScanSchedule, ScheduledScanSummary } from "./types";

export const listScanSchedules = () => invoke<ScanSchedule[]>("list_scan_schedules");
export const listScheduledScanSummaries = () => invoke<ScheduledScanSummary[]>("list_scheduled_scan_summaries");
