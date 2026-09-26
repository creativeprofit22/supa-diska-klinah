import type { ScanPhase, StorageStatus } from "./types";
import { useFormat, useStrings } from "../i18n/I18nProvider";
import { storageStrings } from "./strings";
import "./storage.css";

export function StorageScanStatus({ phase, status, error, cancel, idleMessage }: {
  phase: ScanPhase; status: StorageStatus | null; error: string | null; cancel: () => void; idleMessage?: string;
}) {
  const t = useStrings(storageStrings).scan;
  const fmt = useFormat();
  const partial = !!status?.completeness.reasons.length;
  return <section className="cleanup-state-panel storage-status" aria-label={t.statusLabel}>
    {/* Live text changes on transitions only, not for every progress counter. */}
    <p role="status">{phase === "idle" && idleMessage ? idleMessage : t.phases[phase]}{phase === "ready" && partial ? ` ${t.partial}` : ""}</p>
    {status && <p>{t.progress(fmt.number(status.visitedEntries), fmt.number(status.retainedRecords))}</p>}
    {status?.module === "duplicates" && <p>{t.hashing(fmt.bytes(status.hashedBytes), fmt.number(status.completedHashes))}</p>}
    {error && <p role="alert">{error}</p>}
    {(phase === "starting" || phase === "scanning" || phase === "cancelling") &&
      <button type="button" disabled={phase === "cancelling"} onClick={cancel}>{t.cancelScan}</button>}
  </section>;
}

export function StoragePaging({ loading, hasNext, count, total, firstPage, nextPage }: {
  loading: boolean; hasNext: boolean; count: number; total: number; firstPage: () => void; nextPage: () => void;
}) {
  const t = useStrings(storageStrings).scan;
  const fmt = useFormat();
  return <nav className="storage-paging" aria-label={t.pagesLabel} aria-busy={loading}>
    <p role="status">{loading ? t.loadingPage : t.pageSummary(fmt.number(count), fmt.number(total))}</p>
    <button type="button" disabled={loading} onClick={firstPage}>{t.firstPage}</button>
    <button type="button" disabled={loading || !hasNext} onClick={nextPage}>{t.nextPage}</button>
  </nav>;
}
