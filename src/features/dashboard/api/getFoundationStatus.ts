import { invoke } from "@tauri-apps/api/core";

export interface FoundationStatus {
  platform: string;
  architecture: string;
  adapterReady: boolean;
}

export function getFoundationStatus(): Promise<FoundationStatus> {
  return invoke<FoundationStatus>("foundation_status");
}
