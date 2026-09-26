import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent, type MouseEvent } from "react";
import { BuildArtifactCoordinator } from "../build-artifacts/BuildArtifactCoordinator";
import type { CleanupExecutionSummary, PreviewRecord } from "./api/previewCleanup";
import { useFormat, useStrings } from "../../shared/i18n/I18nProvider";
import { formatModified } from "./format";
import { useCleanupPreview } from "./model/useCleanupPreview";
import { ProjectArtifactDiscovery } from "./ProjectArtifactDiscovery";
import { cleanupStrings } from "./strings";
// Shared pager styles (`.storage-paging`) for the paginated preview list.
import "../../shared/storage/storage.css";

/** Preview records rendered per page; the full set stays selectable via "Select all". */
export const PREVIEW_PAGE_SIZE = 100;

function ruleLabel(ruleId: string): string {
  // simplification: Catalog-provided localized labels replace this fallback later.
  const label = ruleId.replaceAll("-", " ");
  return label.charAt(0).toUpperCase() + label.slice(1);
}

function groupRecords(records: PreviewRecord[]): [string, PreviewRecord[]][] {
  const groups = new Map<string, PreviewRecord[]>();
  for (const record of records) {
    groups.set(record.ruleId, [...(groups.get(record.ruleId) ?? []), record]);
  }
  return [...groups.entries()].sort(([left], [right]) => left.localeCompare(right));
}

function Accounting({ execution }: { execution: CleanupExecutionSummary }) {
  const { accounting } = execution;
  const t = useStrings(cleanupStrings).accounting;
  const fmt = useFormat();
  return (
    <dl className="cleanup-accounting">
      <div><dt>{t.selected}</dt><dd>{fmt.bytes(accounting.selectedBytes)}</dd></div>
      <div><dt>{t.processed}</dt><dd>{fmt.bytes(accounting.processedBytes)}</dd></div>
      <div><dt>{t.failed}</dt><dd>{fmt.bytes(accounting.failedBytes)}</dd></div>
      <div><dt>{t.quarantined}</dt><dd>{fmt.bytes(accounting.quarantinedBytes)}</dd></div>
      <div><dt>{t.purged}</dt><dd>{fmt.bytes(accounting.purgedBytes)}</dd></div>
      <div><dt>{t.occupied}</dt><dd>{fmt.bytes(accounting.occupiedBytes)}</dd></div>
      <div><dt>{t.reclaimed}</dt><dd>{fmt.bytes(accounting.reclaimedBytes)}</dd></div>
    </dl>
  );
}

export function CleanupPreviewPage() {
  const state = useCleanupPreview();
  const records = state.result?.records ?? [];
  const [page, setPage] = useState(0);
  const lastPage = Math.max(0, Math.ceil(records.length / PREVIEW_PAGE_SIZE) - 1);
  const currentPage = Math.min(page, lastPage);
  const pageStart = currentPage * PREVIEW_PAGE_SIZE;
  const groups = groupRecords(records.slice(pageStart, pageStart + PREVIEW_PAGE_SIZE));
  const selectedCount = state.selectedIds.size;
  const t = useStrings(cleanupStrings);
  const fmt = useFormat();

  useEffect(() => {
    document.title = t.documentTitle;
  }, [t.documentTitle]);

  // The confirmation dialog is modal: focus moves in, Tab is trapped, Escape
  // cancels, and focus returns to the button that opened it.
  const dialogRef = useRef<HTMLDialogElement>(null);
  const trigger = useRef<HTMLElement | null>(null);
  const planOpen = state.plan !== null;
  useLayoutEffect(() => {
    if (planOpen) {
      dialogRef.current?.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
    } else if (trigger.current) {
      const previous = trigger.current;
      trigger.current = null;
      if (previous.isConnected && !previous.matches(":disabled")) previous.focus();
      else document.getElementById("main-content")?.focus();
    }
  }, [planOpen]);
  const rememberTrigger = (event: MouseEvent<HTMLElement>) => { trigger.current = event.currentTarget; };
  const onDialogKeyDown = (event: KeyboardEvent<HTMLDialogElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      if (!state.busy) state.cancelPlan();
      return;
    }
    if (event.key !== "Tab") return;
    const buttons = event.currentTarget.querySelectorAll<HTMLButtonElement>("button:not(:disabled)");
    const first = buttons[0];
    const last = buttons[buttons.length - 1];
    if (!first) { event.preventDefault(); return; }
    if (event.shiftKey && (document.activeElement === first || !event.currentTarget.contains(document.activeElement))) { event.preventDefault(); last.focus(); }
    else if (!event.shiftKey && (document.activeElement === last || !event.currentTarget.contains(document.activeElement))) { event.preventDefault(); first.focus(); }
  };

  return (
    <section aria-labelledby="cleanup-heading">
      <header className="page-header cleanup-page-header">
        <div>
          <p className="kicker">{t.kicker}</p>
          <h1 id="cleanup-heading">{t.heading}</h1>
          <p>{t.intro}</p>
        </div>
        <button type="button" disabled={state.loading || state.busy} onClick={state.retry}>
          {state.loading ? t.scanning : state.error ? t.tryAgain : t.scanAgain}
        </button>
      </header>

      {state.loading && <div className="cleanup-state-panel" role="status"><h2>{t.loadingTitle}</h2><p>{t.loadingBody}</p></div>}
      {state.error && <div className="cleanup-state-panel error-state" role="alert"><h2>{t.errorTitle}</h2><p>{t.errorBody}</p></div>}
      {state.result && records.length === 0 && <div className="cleanup-state-panel"><h2>{t.emptyTitle}</h2><p>{t.emptyBody}</p></div>}
      {state.result && state.result.diagnostics.length > 0 && <p className="cleanup-diagnostics">{t.skippedLocations(state.result.diagnostics.length)}</p>}

      {state.result && records.length > 0 && (
        <div className="cleanup-results">
          <div className="cleanup-summary" role="status">
            <strong>{t.selectedOf(selectedCount, records.length)}</strong>
            <span>{t.bytesSelected(fmt.bytes(state.selectedBytes))}</span>
            <button type="button" disabled={state.busy} onClick={state.selectAll}>
              {selectedCount === records.length ? t.clearSelection : t.selectAll}
            </button>
          </div>
          {groups.map(([ruleId, group], index) => (
            <section className="cleanup-group" aria-labelledby={`cleanup-group-${index}`} key={ruleId}>
              <h2 id={`cleanup-group-${index}`}>{ruleLabel(ruleId)}</h2>
              <ul className="cleanup-records">
                {group.map((record) => (
                  <li key={record.id}>
                    <label className="cleanup-selection">
                      <input type="checkbox" checked={state.selectedIds.has(record.id)} disabled={state.busy} onChange={() => state.toggle(record.id)} />
                      <span><strong className="cleanup-path">{record.displayPath}</strong><small>{record.kind === "directory" ? t.directory : t.file} · {fmt.bytes(record.bytes)}{record.modifiedUnixSeconds != null ? t.modified(formatModified(record.modifiedUnixSeconds, fmt.locale)) : ""}</small></span>
                    </label>
                  </li>
                ))}
              </ul>
            </section>
          ))}
          {records.length > PREVIEW_PAGE_SIZE && (
            <nav className="storage-paging" aria-label={t.recordPagesLabel}>
              <p>{t.recordRange(pageStart + 1, Math.min(pageStart + PREVIEW_PAGE_SIZE, records.length), records.length)}</p>
              <button type="button" disabled={currentPage === 0} onClick={() => setPage(currentPage - 1)}>{t.previousRecords}</button>
              <button type="button" disabled={currentPage === lastPage} onClick={() => setPage(currentPage + 1)}>{t.nextRecords}</button>
            </nav>
          )}
          <div className="cleanup-actions">
            <button type="button" disabled={!selectedCount || state.busy} onClick={(event) => { rememberTrigger(event); void state.prepare("recycleBin"); }}>{t.moveToRecycleBin}</button>
            <button className="danger-button" type="button" disabled={!selectedCount || state.busy} onClick={(event) => { rememberTrigger(event); void state.prepare("permanent"); }}>{t.deletePermanently}</button>
          </div>
        </div>
      )}

      {state.plan && (
        <dialog ref={dialogRef} open aria-labelledby="cleanup-confirm-title" aria-modal="true" onKeyDown={onDialogKeyDown}>
          <h2 id="cleanup-confirm-title">{state.plan.disposition === "permanent" ? t.confirmPermanentTitle : t.confirmRecycleTitle}</h2>
          <p>{t.itemCount(state.plan.selectedCount)} · {fmt.bytes(state.plan.selectedBytes)}</p>
          {state.plan.disposition === "permanent" && <p className="error-text">{t.permanentWarning}</p>}
          <div className="dialog-actions">
            <button type="button" disabled={state.busy} onClick={state.cancelPlan}>{t.cancel}</button>
            <button className={state.plan.disposition === "permanent" ? "danger-button" : undefined} type="button" disabled={state.busy} onClick={() => void state.confirmPlan()}>{state.busy ? t.working : state.plan.disposition === "permanent" ? t.deletePermanently : t.moveToRecycleBin}</button>
          </div>
        </dialog>
      )}

      {state.execution && (
        <section className="cleanup-outcomes" aria-labelledby="latest-cleanup-heading">
          <h2 id="latest-cleanup-heading">{t.latestCleanup}</h2>
          <Accounting execution={state.execution} />
          <ul>{state.execution.items.map((item) => <li key={item.itemId}><span>{item.state}</span><strong>{fmt.bytes(item.logicalBytes)}</strong></li>)}</ul>
          {state.execution.items.some((item) => item.state === "recycled" || item.state === "quarantined") && <button type="button" disabled={state.busy} onClick={() => void state.undo(state.execution!.executionId)}>{t.undoCleanup}</button>}
        </section>
      )}

      {(
        <section className="cleanup-history" aria-labelledby="cleanup-history-heading">
          <h2 id="cleanup-history-heading">{t.history.title}</h2>
          <button type="button" disabled={state.busy || state.history.loading} onClick={() => void state.history.refresh()}>{t.history.refresh}</button>
          <button type="button" disabled={state.busy || state.history.loading || state.history.nextCursor === null} onClick={() => void state.history.older()}>{t.history.older}</button>
          <p>{t.history.pageSize}</p>
          {state.history.loading && <p role="status">{t.history.loading}</p>}
          {state.history.error && <p role="alert">{state.history.error}</p>}
          {!state.history.loaded && !state.history.loading && !state.history.error && <p>{t.history.notLoaded}</p>}
          {state.history.loaded && !state.history.loading && !state.history.error && (state.history.records.length === 0 && state.history.currentCursor === null ? <p>{t.history.empty}</p> : state.history.nextCursor === null && <p>{t.history.end}</p>)}
          <ul>{state.history.records.map((item) => <li key={item.executionId}><span>{item.disposition}</span><span>{t.history.reclaimed(fmt.bytes(item.accounting.reclaimedBytes))}</span>{item.items.some(outcome => outcome.state === "recycled" || outcome.state === "quarantined") && <button type="button" disabled={state.busy} onClick={() => void state.undo(item.executionId)}>{t.history.undo}</button>}</li>)}</ul>
        </section>
      )}
      <ProjectArtifactDiscovery />
      <BuildArtifactCoordinator />
    </section>
  );
}
