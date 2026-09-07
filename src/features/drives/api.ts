import { invoke } from "@tauri-apps/api/core";

export interface DriveSummary {
  driveId: string;
  label: string;
  filesystem: string;
  totalBytes: number;
  freeBytes: number;
  usedBytes: number;
  system: boolean | null;
}

export interface DriveInventory {
  drives: DriveSummary[];
  partial: boolean;
  warnings: { drive: string | null; code: "drive_unavailable" | "inventory_partial" }[];
}

// Share an outstanding read across remounts, including React StrictMode.
let pending: Promise<DriveInventory> | undefined;
export function listDriveInventory(): Promise<DriveInventory> {
  return pending ??= invoke<DriveInventory>("list_drive_inventory").finally(() => {
    pending = undefined;
  });
}

export function driveInventoryError(reason: unknown): string {
  const code = typeof reason === "object" && reason !== null && "code" in reason
    ? reason.code : null;
  if (code === "busy") return "A drive inventory is already running. Try again shortly.";
  if (code === "timeout") return "Windows took too long to report drive information. Try again.";
  return "Drive information is unavailable. Open the Windows app and try again.";
}
