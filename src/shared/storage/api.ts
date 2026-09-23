import { invoke } from "@tauri-apps/api/core";
import type { CleanupDisposition, CleanupPlanSummary } from "../cleanup/api";
import type { NativeScope, PageRequest, RootChoice, ScanApi, SnapshotInput, StorageModule, StoragePage, StorageSelection, StorageStatus } from "./types";
import { MAX_SELECTION, validId } from "./types";

// Matches step 1's Raw-body boundary. Never send ordinary invoke argument objects.
function request<T>(command: string, input: object): Promise<T> {
  const bytes = new TextEncoder().encode(JSON.stringify(input));
  if (bytes.byteLength > 65_536) return Promise.reject({ code: "limit_reached" });
  return invoke<T>(command, bytes);
}
export function chooseStorageRoot(module: StorageModule): Promise<RootChoice | null> {
  return request("choose_storage_root", { module });
}
interface ScopeRequest {
  module: StorageModule;
  promise: Promise<NativeScope[]>;
  resolve: (scopes: NativeScope[]) => void;
  reject: (error: unknown) => void;
}
let activeScopes: ScopeRequest | null = null;
let queuedScopes: ScopeRequest | null = null;
function launchScopes(entry: ScopeRequest): void {
  activeScopes = entry;
  void request<NativeScope[]>("list_storage_scopes", { module: entry.module })
    .then(entry.resolve, entry.reject).finally(() => {
      activeScopes = null;
      const next = queuedScopes; queuedScopes = null;
      if (next) launchScopes(next);
    });
}
export function listStorageScopes(module: StorageModule): Promise<NativeScope[]> {
  // Native refresh invalidates older IDs. Serialize refreshes, join StrictMode's
  // duplicate request, and retain only one latest queued scope intent.
  if (queuedScopes?.module === module) return queuedScopes.promise;
  if (!queuedScopes && activeScopes?.module === module) return activeScopes.promise;
  let resolve!: ScopeRequest["resolve"]; let reject!: ScopeRequest["reject"];
  const promise = new Promise<NativeScope[]>((yes, no) => { resolve = yes; reject = no; });
  const entry = { module, promise, resolve, reject };
  if (activeScopes) {
    queuedScopes?.reject({ code: "snapshot_unavailable" });
    queuedScopes = entry;
  } else launchScopes(entry);
  return promise;
}
export function authorizeStorageScope(module: StorageModule, scopeId: string): Promise<RootChoice> {
  return request("authorize_storage_scope", { module, scopeId });
}
export function storageStatus(input: SnapshotInput): Promise<StorageStatus> {
  return request("storage_scan_status", input);
}
export function storagePage<Row>(input: PageRequest): Promise<StoragePage<Row>> {
  return request("storage_scan_page", input);
}
export function cancelStorageScan(input: SnapshotInput): Promise<void> {
  return request("cancel_storage_scan", input);
}
export function releaseStorageScan(input: SnapshotInput): Promise<void> {
  return request("release_storage_scan", input);
}
export function createStoragePlan(selection: StorageSelection, disposition: CleanupDisposition): Promise<CleanupPlanSummary> {
  if (!validId(selection.snapshotId) || !selection.candidateIds.length || selection.candidateIds.length > MAX_SELECTION ||
      selection.candidateIds.some(id => !validId(id)) || new Set(selection.candidateIds).size !== selection.candidateIds.length) {
    return Promise.reject({ code: "invalid_input" });
  }
  // Pick only the declared IDs, never spread a caller's proof/path fields.
  return request("create_storage_plan", { selection: { module: selection.module, snapshotId: selection.snapshotId, candidateIds: [...selection.candidateIds] }, disposition });
}
export function scanApi<Row>(): ScanApi<Row> {
  return { status: storageStatus, page: storagePage<Row>, cancel: cancelStorageScan, release: releaseStorageScan };
}
export function storageError(error: unknown): string {
  const code = typeof error === "object" && error !== null && "code" in error ? error.code : null;
  switch (code) {
    case "busy": return "Another scan is active. Cancel it or wait before starting again.";
    case "snapshot_unavailable": return "These results expired or were released. Start a new scan.";
    case "scope_unavailable": return "This scope is unavailable. Choose another supported scope.";
    case "invalid_cursor": return "This page is no longer available. Return to the first page or scan again.";
    case "limit_reached": return "The safety limit was reached. Narrow the scope or select fewer items.";
    case "recovery_volume_unsupported": return "App recovery is unavailable for the selected volume. No recovery plan was created.";
    case "invalid_evidence": return "The selection no longer matches the scan. Scan again before reviewing.";
    default: return "Storage could not continue. Start a new scan before retrying.";
  }
}
