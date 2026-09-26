import { useId, useLayoutEffect, useRef, useState } from "react";
import { cleanupActions, type CleanupDisposition, type CleanupExecutionSummary, type CleanupPlanSummary } from "../cleanup/api";
import { useCleanupHistory } from "../cleanup/useCleanupHistory";
import { useFormat, useStrings } from "../i18n/I18nProvider";
import { CleanupOutcomes, cleanupSucceeded as successful, canUndoCleanup } from "./CleanupOutcomes";
import { createStoragePlan, storageError } from "./api";
import { storageStrings } from "./strings";
import type { StorageSelection } from "./types";
import "./storage.css";

const defaultDispositions: readonly CleanupDisposition[] = ["recycleBin"];
export function StoragePlanReview({ selection, dispositions = defaultDispositions, createPlan = createStoragePlan, actions = cleanupActions, onExecuted }: {
  selection: StorageSelection | null;
  /** Feature must explicitly opt into only the dispositions its native runner supports. */
  dispositions?: readonly CleanupDisposition[];
  createPlan?: typeof createStoragePlan;
  actions?: typeof cleanupActions;
  onExecuted: () => void;
}) {
  const strings = useStrings(storageStrings);
  const t = strings.review;
  const dispositionLabels = t.dispositions;
  const fmt = useFormat();
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
        setError(storageError(cause, strings.errors));
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
      if (mounted.current) setError(t.operationUnconfirmed);
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

  return <section ref={region} tabIndex={-1} className="storage-review" aria-label={t.regionLabel}>
    <div className="cleanup-actions">
      {dispositions.map(disposition => <button key={disposition} type="button" disabled={!selection || busy || (disposition === "quarantine" && recoveryUnavailable)} onClick={() => void prepare(disposition)}>
        {t.reviewAction(dispositionLabels[disposition])}
      </button>)}
      <button type="button" disabled={busy || history.loading} onClick={() => void history.refresh()}>{history.loaded ? t.refreshHistory : t.loadHistory}</button>
    </div>
    {busy && <p role="status">{t.waiting}</p>}
    {error && <p role="alert">{error}</p>}
    {recoveryUnavailable && dispositions.includes("permanent") && <p>{t.permanentSeparate}</p>}
    <dialog ref={dialog} className="storage-plan-dialog" aria-labelledby={headingId} aria-describedby={summaryId}
      onCancel={event => { event.preventDefault(); dismiss(); }}
      onKeyDown={event => {
        // Browsers raise `cancel` for Escape; handling it here keeps the same path
        // when the platform does not (preventDefault avoids a second cancel).
        if (event.key === "Escape") { event.preventDefault(); dismiss(); return; }
        if (event.key !== "Tab") return;
        const buttons = event.currentTarget.querySelectorAll<HTMLButtonElement>("button:not(:disabled)");
        const first = buttons[0]; const last = buttons[buttons.length - 1];
        if (first && event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
        else if (last && !event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
      }}>
      <h2 id={headingId}>{visible ? dispositionLabels[visible.disposition] : t.reviewCleanup}</h2>
      <p id={summaryId}>{t.planSummary(visible?.selectedCount, fmt.bytes(visible?.selectedBytes ?? 0))}</p>
      <p>{t.planFixed}</p>
      {visible?.disposition === "permanent" ? <p>{t.permanentWarning}</p> :
        <p>{visible?.disposition === "quarantine" ? t.quarantineNote : t.recycleNote} {t.undoNote}</p>}
      <div className="dialog-actions">
        <button ref={cancelButton} type="button" disabled={busy} onClick={dismiss}>{t.cancelReview}</button>
        <button type="button" className={visible?.disposition === "permanent" ? "danger-button" : undefined} disabled={busy || !visible} onClick={confirm}>{visible?.disposition === "permanent" ? t.continueToWindows : visible ? dispositionLabels[visible.disposition] : t.confirm}</button>
      </div>
    </dialog>
    {execution && <section aria-label={t.latestOutcomeLabel} className="cleanup-state-panel">
      <h3>{successful(execution) ? t.finished : t.needsAttention}</h3>
      <CleanupOutcomes key={execution.executionId} value={execution} expanded/>
    </section>}
    {<section aria-label={t.historyLabel}><h3>{t.recentHistory}</h3>
      <ul className="storage-history">{history.records.map(item => <li key={item.executionId}>
        <span>{t.historyRow(dispositionLabels[item.disposition], fmt.bytes(item.accounting.reclaimedBytes))}{successful(item) ? "" : ` · ${t.rowNeedsAttention}`}</span>
        <CleanupOutcomes value={item}/>
        {canUndoCleanup(item) && <button type="button" disabled={busy} onClick={() => void perform(() => actions.undoCleanup(item.executionId), true)}>{t.undoCleanup}</button>}
      </li>)}</ul>
      <p>{t.historyPaging}</p>
      {history.loading && <p role="status">{t.loadingHistory}</p>}
      {history.error && <p role="alert">{history.error}</p>}
      {!history.loaded && !history.loading && !history.error && <p>{t.historyNotLoaded}</p>}
      {history.loaded && !history.loading && !history.error && (history.records.length === 0 && history.currentCursor === null ? <p>{t.noHistory}</p> : history.nextCursor === null && <p>{t.endOfHistory}</p>)}
      <button type="button" disabled={busy || history.loading || history.nextCursor === null} onClick={() => void history.older()}>{t.olderHistory}</button>
    </section>}
  </section>;
}
