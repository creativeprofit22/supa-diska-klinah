import { useId, useLayoutEffect, useRef, useState } from "react";
import { cleanupActions, type CleanupDisposition, type CleanupExecutionSummary, type CleanupPlanSummary } from "../cleanup/api";
import { useCleanupHistory } from "../cleanup/useCleanupHistory";
import { formatBytes } from "../format";
import { CleanupOutcomes, cleanupSucceeded as successful, canUndoCleanup } from "./CleanupOutcomes";
import { createStoragePlan, storageError } from "./api";
import type { StorageSelection } from "./types";
import "./storage.css";

const dispositionLabels: Record<CleanupDisposition, string> = {
  recycleBin: "Move to Recycle Bin", quarantine: "Move to app recovery", permanent: "Delete permanently",
};
const defaultDispositions: readonly CleanupDisposition[] = ["recycleBin"];
export function StoragePlanReview({ selection, dispositions = defaultDispositions, createPlan = createStoragePlan, actions = cleanupActions, onExecuted }: {
  selection: StorageSelection | null;
  /** Feature must explicitly opt into only the dispositions its native runner supports. */
  dispositions?: readonly CleanupDisposition[];
  createPlan?: typeof createStoragePlan;
  actions?: typeof cleanupActions;
  onExecuted: () => void;
}) {
  const key = JSON.stringify(selection);
  const currentKey = useRef(key);
  const mounted = useRef(false);
  const version = useRef(0);
  const lock = useRef(false);
  const [busy, setBusy] = useState(false);
  const [plan, setPlan] = useState<{ key: string; summary: Readonly<CleanupPlanSummary> } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [recoveryUnavailable, setRecoveryUnavailable] = useState(false);
  const [execution, setExecution] = useState<CleanupExecutionSummary | null>(null);
  const history = useCleanupHistory(actions.cleanupHistory);
  const region = useRef<HTMLElement>(null);
  const returnFocus = useRef<HTMLElement | null>(null);
  const dialog = useRef<HTMLDialogElement>(null);
  const cancelButton = useRef<HTMLButtonElement>(null);
  const headingId = useId();
  const summaryId = useId();
  const visible = plan?.key === key ? plan.summary : null;

  useLayoutEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; version.current++; };
  }, []);
  useLayoutEffect(() => {
    currentKey.current = key;
    version.current++;
    setPlan(null);
    setError(null);
    setRecoveryUnavailable(false);
  }, [key]);
  useLayoutEffect(() => {
    const node = dialog.current;
    if (!node) return;
    if (visible) {
      node.showModal();
      cancelButton.current?.focus();
    } else if (node.open) {
      // Restore after React's DOM commit, not effect cleanup: React can otherwise
      // restore focus back into the now-closed dialog during the same commit.
      node.close();
      const previous = returnFocus.current;
      if (previous?.isConnected && !previous.matches(":disabled")) previous.focus();
      else region.current?.focus();
    }
  }, [visible]);

  const prepare = async (disposition: CleanupDisposition) => {
    if (!selection || lock.current || !dispositions.includes(disposition)) return;
    returnFocus.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    lock.current = true; setBusy(true); setError(null);
    const requestVersion = version.current;
    // Copy only immutable IDs. Scope or selection changes discard the late result.
    const captured = { module: selection.module, snapshotId: selection.snapshotId, candidateIds: [...selection.candidateIds] };
    try {
      const result = await createPlan(captured, disposition);
      if (!mounted.current || requestVersion !== version.current || currentKey.current !== key) return;
      if (result.disposition !== disposition || result.selectedCount < 1 || result.selectedCount > captured.candidateIds.length ||
          !Number.isSafeInteger(result.selectedCount) || !Number.isFinite(result.selectedBytes) || result.selectedBytes < 0) throw { code: "invalid_evidence" };
      setPlan({ key, summary: Object.freeze({ ...result }) });
    } catch (cause) {
      if (mounted.current && requestVersion === version.current) {
        setPlan(null);
        setError(storageError(cause));
        if (disposition === "quarantine" && typeof cause === "object" && cause !== null &&
            "code" in cause && cause.code === "recovery_volume_unsupported") setRecoveryUnavailable(true);
      }
    } finally { lock.current = false; if (mounted.current) setBusy(false); }
  };
  const perform = async (operation: () => Promise<CleanupExecutionSummary>, undo = false) => {
    if (lock.current) return;
    lock.current = true; setBusy(true); setError(null);
    try {
      const result = await operation();
      if (!mounted.current) return;
      setExecution(result);
      if (undo) history.updateVisible(result);
      else void history.refresh();
      onExecuted();
    } catch {
      if (mounted.current) setError("The operation was not confirmed or could not finish. Load history to check outcomes before retrying.");
    } finally {
      lock.current = false;
      if (mounted.current) { setBusy(false); setPlan(null); }
    }
  };
  const confirm = () => {
    if (!visible || currentKey.current !== plan?.key) return;
    const captured = visible;
    // Permanent execution keeps its existing, separate native-confirmed command.
    void perform(() => captured.disposition === "permanent"
      ? actions.executePermanentCleanupPlan(captured.planId)
      : actions.executeCleanupPlan(captured.planId));
  };
  const dismiss = () => { if (!lock.current) { version.current++; setPlan(null); } };

  return <section ref={region} tabIndex={-1} className="storage-review" aria-label="Storage cleanup review">
    <div className="cleanup-actions">
      {dispositions.map(disposition => <button key={disposition} type="button" disabled={!selection || busy || (disposition === "quarantine" && recoveryUnavailable)} onClick={() => void prepare(disposition)}>
        Review: {dispositionLabels[disposition]}
      </button>)}
      <button type="button" disabled={busy || history.loading} onClick={() => void history.refresh()}>{history.loaded ? "Refresh newest cleanup history" : "Load cleanup history"}</button>
    </div>
    {busy && <p role="status">Waiting for the native operation…</p>}
    {error && <p role="alert">{error}</p>}
    {recoveryUnavailable && dispositions.includes("permanent") && <p>Permanent deletion remains a separate choice: select Review: Delete permanently, then confirm in Windows. It cannot be undone.</p>}
    <dialog ref={dialog} className="storage-plan-dialog" aria-labelledby={headingId} aria-describedby={summaryId}
      onCancel={event => { event.preventDefault(); dismiss(); }}
      onKeyDown={event => {
        if (event.key !== "Tab") return;
        const buttons = event.currentTarget.querySelectorAll<HTMLButtonElement>("button:not(:disabled)");
        const first = buttons[0]; const last = buttons[buttons.length - 1];
        if (first && event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
        else if (last && !event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
      }}>
      <h2 id={headingId}>{visible ? dispositionLabels[visible.disposition] : "Review cleanup"}</h2>
      <p id={summaryId}>{visible?.selectedCount} {visible?.selectedCount === 1 ? "item" : "items"} · {formatBytes(visible?.selectedBytes ?? 0)} selected, not reclaimed.</p>
      <p>This plan is fixed to the reviewed scan and selection. Changed files can be refused at execution.</p>
      {visible?.disposition === "permanent" ? <p>This cannot be undone. Continuing opens a separate Windows confirmation.</p> :
        <p>{visible?.disposition === "quarantine" ? "Files are held in app recovery storage on the same volume, without automatic purge. This does not free disk space." : "Files go to the Windows Recycle Bin where supported."} Undo is available only for items successfully retained for recovery.</p>}
      <div className="dialog-actions">
        <button ref={cancelButton} type="button" disabled={busy} onClick={dismiss}>Cancel review</button>
        <button type="button" className={visible?.disposition === "permanent" ? "danger-button" : undefined} disabled={busy || !visible} onClick={confirm}>{visible?.disposition === "permanent" ? "Continue to Windows confirmation" : visible ? dispositionLabels[visible.disposition] : "Confirm"}</button>
      </div>
    </dialog>
    {execution && <section aria-label="Latest storage cleanup outcome" className="cleanup-state-panel">
      <h3>{successful(execution) ? "Cleanup finished" : "Cleanup needs attention"}</h3>
      <CleanupOutcomes key={execution.executionId} value={execution} expanded/>
    </section>}
    {<section aria-label="Storage cleanup history"><h3>Recent cleanup history</h3>
      <ul className="storage-history">{history.records.map(item => <li key={item.executionId}>
        <span>{dispositionLabels[item.disposition]} · {formatBytes(item.accounting.reclaimedBytes)} reclaimed{successful(item) ? "" : " · Needs attention"}</span>
        <CleanupOutcomes value={item}/>
        {canUndoCleanup(item) && <button type="button" disabled={busy} onClick={() => void perform(() => actions.undoCleanup(item.executionId), true)}>Undo cleanup</button>}
      </li>)}</ul>
      <p>Up to 20 executions per page. Undo can fail if retained files have changed.</p>
      {history.loading && <p role="status">Loading cleanup history…</p>}
      {history.error && <p role="alert">{history.error}</p>}
      {!history.loaded && !history.loading && !history.error && <p>Cleanup history has not been loaded.</p>}
      {history.loaded && !history.loading && !history.error && (history.records.length === 0 && history.currentCursor === null ? <p>No cleanup history yet.</p> : history.nextCursor === null && <p>End of cleanup history.</p>)}
      <button type="button" disabled={busy || history.loading || history.nextCursor === null} onClick={() => void history.older()}>Older cleanup history</button>
    </section>}
  </section>;
}
