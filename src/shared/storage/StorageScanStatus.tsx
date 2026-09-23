import type { ScanPhase, StorageStatus } from "./types";
import { formatBytes } from "../format";
import "./storage.css";

const labels: Record<ScanPhase, string> = {
  idle: "Choose a scope, then start a scan.",
  starting: "Starting scan…",
  scanning: "Scanning. No files are being changed.",
  ready: "Scan complete.",
  cancelling: "Requesting cancellation…",
  cancelled: "Scan cancelled. Results and selections were discarded. Start a new scan to continue.",
  failed: "Scan unavailable. Start a new scan to continue.",
};
export function StorageScanStatus({ phase, status, error, cancel, idleMessage }: {
  phase: ScanPhase; status: StorageStatus | null; error: string | null; cancel: () => void; idleMessage?: string;
}) {
  const partial = !!status?.completeness.reasons.length;
  return <section className="cleanup-state-panel storage-status" aria-label="Storage scan status">
    {/* Live text changes on transitions only, not for every progress counter. */}
    <p role="status">{phase === "idle" && idleMessage ? idleMessage : labels[phase]}{phase === "ready" && partial ? " Results are partial; totals may be incomplete." : ""}</p>
    {status && <p>{status.visitedEntries.toLocaleString()} entries checked · {status.retainedRecords.toLocaleString()} records retained</p>}
    {status?.module === "duplicates" && <p>{formatBytes(status.hashedBytes)} hashed · {status.completedHashes.toLocaleString()} hashes completed</p>}
    {error && <p role="alert">{error}</p>}
    {(phase === "starting" || phase === "scanning" || phase === "cancelling") &&
      <button type="button" disabled={phase === "cancelling"} onClick={cancel}>Cancel scan</button>}
  </section>;
}

export function StoragePaging({ loading, hasNext, count, total, firstPage, nextPage }: {
  loading: boolean; hasNext: boolean; count: number; total: number; firstPage: () => void; nextPage: () => void;
}) {
  return <nav className="storage-paging" aria-label="Scan result pages" aria-busy={loading}>
    <p role="status">{loading ? "Loading page…" : `${count.toLocaleString()} records on this page; ${total.toLocaleString()} retained in this view.`}</p>
    <button type="button" disabled={loading} onClick={firstPage}>First page</button>
    <button type="button" disabled={loading || !hasNext} onClick={nextPage}>Next page</button>
  </nav>;
}
