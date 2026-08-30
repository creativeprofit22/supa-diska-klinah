import { invoke } from "@tauri-apps/api/core";

export type PreviewKind = "file" | "directory";

export interface PreviewRecord {
  id: string;
  ruleId: string;
  displayPath: string;
  kind: PreviewKind;
  bytes: number;
  modifiedUnixSeconds?: number | null;
}

export interface ScanDiagnostic {
  ruleId: string;
  path: string;
  reason: string;
}

export interface CleanupPreview {
  scanId: string;
  records: PreviewRecord[];
  diagnostics: ScanDiagnostic[];
}

export function previewCleanup(): Promise<CleanupPreview> {
  return invoke<CleanupPreview>("preview_cleanup");
}
