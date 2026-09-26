import { useState } from "react";
import type { CleanupExecutionSummary, CleanupItemOutcome } from "../cleanup/api";
import { useFormat, useStrings } from "../i18n/I18nProvider";
import { storageStrings, type StorageStrings } from "./strings";

function uncertain(item: CleanupItemOutcome): boolean {
  return item.failure === "recovery-identity-unproven" ||
    !["recycled", "quarantined", "purged", "restored", "failed"].includes(item.state);
}
function failed(item: CleanupItemOutcome): boolean {
  return !uncertain(item) && (item.state === "failed" || Boolean(item.failure));
}
export function cleanupSucceeded(value: CleanupExecutionSummary): boolean {
  return value.completed && value.accounting.failedBytes === 0 &&
    value.items.every(item => !uncertain(item) && !failed(item));
}
export function canUndoCleanup(value: CleanupExecutionSummary): boolean {
  // Undo targets the whole execution. Never re-offer it after a refused/uncertain restore.
  return !value.items.some(item => uncertain(item) ||
    (Boolean(item.failure) && (item.state === "recycled" || item.state === "quarantined"))) &&
    value.items.some(item => item.state === "recycled" || item.state === "quarantined");
}
function failureDescription(item: CleanupItemOutcome, t: StorageStrings["outcomes"]["failures"]): string | null {
  if (item.failure === "recovery-identity-unproven") return t.identityUnproven;
  if (uncertain(item)) return t.uncertain;
  switch (item.failure) {
    case "not-found": return t.notFound;
    case "in-use": return t.inUse;
    case "permission-denied": return t.permissionDenied;
    case "permanent-remove-failed": return t.permanentRemoveFailed;
    case "restore-protection-rejected": return t.restoreProtectionRejected;
    case "restore-rejected":
    case "restore-collision": return t.restoreRejected;
    case "restore-failed": return t.restoreFailed;
    default: return failed(item) ? t.generic : null;
  }
}
export function CleanupOutcomes({ value, expanded = false }: { value: CleanupExecutionSummary; expanded?: boolean }) {
  const t = useStrings(storageStrings).outcomes;
  const fmt = useFormat();
  const [page, setPage] = useState(0);
  const [open, setOpen] = useState(expanded);
  const lastPage = Math.max(0, Math.ceil(value.items.length / 20) - 1);
  const current = Math.min(page, lastPage);
  const start = current * 20;
  const failedCount = value.items.filter(failed).length;
  const uncertainCount = value.items.filter(uncertain).length;
  return <div>
    <p>{t.totalItems(value.items.length)} · {t.failedItems(failedCount)} · {t.uncertainItems(uncertainCount)} · {t.confirmedItems(value.items.filter(item => !failed(item) && !uncertain(item)).length)}</p>
    <details open={open} onToggle={event => setOpen(event.currentTarget.open)} aria-label={t.itemOutcomes}>
      <summary>{t.itemOutcomes}</summary>
      {open && <>
        <p>{t.accounting(fmt.bytes(value.accounting.reclaimedBytes), fmt.bytes(value.accounting.failedBytes), fmt.bytes(value.accounting.occupiedBytes))}</p>
        <p>{t.breakdown(fmt.bytes(value.accounting.processedBytes), fmt.bytes(value.accounting.quarantinedBytes), fmt.bytes(value.accounting.purgedBytes))}</p>
        <p role="status">{t.range(value.items.length ? start + 1 : 0, Math.min(start + 20, value.items.length), value.items.length)}</p>
        <ul>{value.items.slice(start, start + 20).map(item => {
          const description = failureDescription(item, t.failures);
          return <li key={item.itemId}>
            <p><bdi>{item.displayPath ?? t.displayUnavailable}</bdi></p>
            <p>{t.itemId(item.itemId)}</p>
            <p>{uncertain(item) ? t.uncertain : t.states[item.state]} · {t.logicalSize(fmt.bytes(item.logicalBytes))}{failed(item) && item.state !== "failed" ? ` · ${t.operationFailed}` : ""}</p>
            {description && <p>{description}</p>}
          </li>;
        })}</ul>
        <button type="button" disabled={current === 0} onClick={() => setPage(current - 1)}>{t.previousItems}</button>
        <button type="button" disabled={current === lastPage} onClick={() => setPage(current + 1)}>{t.nextItems}</button>
      </>}
    </details>
  </div>;
}
