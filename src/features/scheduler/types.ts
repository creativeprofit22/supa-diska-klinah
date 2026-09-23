import type { ScheduleCadence } from "../../shared/system-change/types";

export type OrphanReason = "foreignExecutable" | "malformedArguments" | "unrecognizedDefinition";
/** Mirrors `windows_platform::scheduler::ScanSchedule`. Run times are local `YYYY-MM-DDTHH:MM:SS`. */
export interface ScanSchedule {
  id: string;
  cadence: ScheduleCadence | null;
  enabled: boolean;
  lastRun: string | null;
  nextRun: string | null;
  orphaned: boolean;
  orphanReason: OrphanReason | null;
}
/** Mirrors `windows_platform::scheduler::scan_run::ScheduledScanSummary`. `finishedAt` is Unix seconds. */
export interface ScheduledScanSummary {
  scheduleId: string;
  finishedAt: number;
  reclaimableBytes: number;
  itemCount: number;
  diagnosticCount: number;
  succeeded: boolean;
}
