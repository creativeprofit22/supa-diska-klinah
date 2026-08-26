import { invoke } from "@tauri-apps/api/core";

export interface CreateRestorePointInput {
  description: string;
}

export interface CreateSystemRestorePointResult {
  sequenceNumber: number;
}

export function createSystemRestorePoint(
  input: CreateRestorePointInput,
): Promise<CreateSystemRestorePointResult> {
  return invoke<CreateSystemRestorePointResult>("create_system_restore_point", {
    input,
  });
}
