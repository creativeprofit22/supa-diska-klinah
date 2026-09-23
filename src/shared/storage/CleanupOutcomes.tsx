import { useState } from "react";
import type { CleanupExecutionSummary, CleanupItemOutcome, CleanupItemState } from "../cleanup/api";
import { formatBytes } from "../format";

const labels: Record<CleanupItemState, string> = {
  pending: "Not confirmed", mutating: "Not confirmed", unknown: "Uncertain",
  recycled: "Retained in Recycle Bin", quarantined: "Retained in app recovery",
  purged: "Permanently removed", restored: "Restored", failed: "Failed",
};
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
function failureDescription(item: CleanupItemOutcome): string | null {
  if (item.failure === "recovery-identity-unproven") return "Recovery identity could not be verified. The location and safety of this item are uncertain. Preserve recovery data and seek help; do not retry or move recovery files.";
  if (uncertain(item)) return "The result is not confirmed. Do not assume this item was deleted or is safely recoverable. Preserve history and inspect before further action.";
  switch (item.failure) {
    case "not-found": return "The item was already gone when cleanup reached it. Nothing was removed.";
    case "in-use": return "The item is open in another program, so it was left untouched. Close that program and scan again.";
    case "permission-denied": return "Windows denied access to the item, so it was left untouched.";
    case "permanent-remove-failed": return "Removal was refused. The item may have changed or the folder may no longer be empty. Inspect it and scan again before any new cleanup.";
    case "restore-protection-rejected": return "Restoration was blocked by current protection or browser activity checks. Keep recovery data intact; do not bypass protection.";
    case "restore-rejected":
    case "restore-collision": return "Restoration was refused because the destination or recovery checks did not pass. Inspect the original location; do not overwrite it or move recovery files.";
    case "restore-failed": return "Restoration could not be confirmed. Preserve recovery data and inspect the original location before further action.";
    default: return failed(item) ? "The operation did not succeed. Inspect the item and preserve recovery data. No automatic retry will run." : null;
  }
}
export function CleanupOutcomes({ value, expanded = false }: { value: CleanupExecutionSummary; expanded?: boolean }) {
  const [page, setPage] = useState(0);
  const [open, setOpen] = useState(expanded);
  const lastPage = Math.max(0, Math.ceil(value.items.length / 20) - 1);
  const current = Math.min(page, lastPage);
  const start = current * 20;
  const failedCount = value.items.filter(failed).length;
  const uncertainCount = value.items.filter(uncertain).length;
  const count = (n: number, label: string) => `${n} ${label} item${n === 1 ? "" : "s"}`;
  return <div>
    <p>{value.items.length} total items · {count(failedCount, "failed")} · {count(uncertainCount, "uncertain")} · {count(value.items.filter(item => !failed(item) && !uncertain(item)).length, "confirmed")}</p>
    <details open={open} onToggle={event => setOpen(event.currentTarget.open)} aria-label="Item outcomes">
      <summary>Item outcomes</summary>
      {open && <>
        <p>{formatBytes(value.accounting.reclaimedBytes)} reclaimed · {formatBytes(value.accounting.failedBytes)} failed · {formatBytes(value.accounting.occupiedBytes)} still occupied</p>
        <p>Processed: {formatBytes(value.accounting.processedBytes)}. Quarantined: {formatBytes(value.accounting.quarantinedBytes)}. Purged: {formatBytes(value.accounting.purgedBytes)}.</p>
        <p role="status">Items {value.items.length ? start + 1 : 0}–{Math.min(start + 20, value.items.length)} of {value.items.length}</p>
        <ul>{value.items.slice(start, start + 20).map(item => <li key={item.itemId}>
          <p><bdi>{item.displayPath ?? "Display location unavailable"}</bdi></p>
          <p>Item ID: {item.itemId}</p>
          <p>{uncertain(item) ? "Uncertain" : labels[item.state]} · {formatBytes(item.logicalBytes)} logical size{failed(item) && item.state !== "failed" ? " · Operation failed" : ""}</p>
          {failureDescription(item) && <p>{failureDescription(item)}</p>}
        </li>)}</ul>
        <button type="button" disabled={current === 0} onClick={() => setPage(current - 1)}>Previous items</button>
        <button type="button" disabled={current === lastPage} onClick={() => setPage(current + 1)}>Next items</button>
      </>}
    </details>
  </div>;
}
