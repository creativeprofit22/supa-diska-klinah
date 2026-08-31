import { useEffect } from "react";
import type { CleanupExecutionSummary, PreviewRecord } from "./api/previewCleanup";
import { formatBytes, formatModified } from "./format";
import { useCleanupPreview } from "./model/useCleanupPreview";
import { ProjectArtifactDiscovery } from "./ProjectArtifactDiscovery";

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
  return (
    <dl className="cleanup-accounting">
      <div><dt>Selected</dt><dd>{formatBytes(accounting.selectedBytes)}</dd></div>
      <div><dt>Processed</dt><dd>{formatBytes(accounting.processedBytes)}</dd></div>
      <div><dt>Failed</dt><dd>{formatBytes(accounting.failedBytes)}</dd></div>
      <div><dt>Quarantined</dt><dd>{formatBytes(accounting.quarantinedBytes)}</dd></div>
      <div><dt>Purged</dt><dd>{formatBytes(accounting.purgedBytes)}</dd></div>
      <div><dt>Occupied</dt><dd>{formatBytes(accounting.occupiedBytes)}</dd></div>
      <div><dt>Reclaimed</dt><dd>{formatBytes(accounting.reclaimedBytes)}</dd></div>
    </dl>
  );
}

export function CleanupPreviewPage() {
  const state = useCleanupPreview();
  const records = state.result?.records ?? [];
  const groups = groupRecords(records);
  const selectedCount = state.selectedIds.size;

  useEffect(() => {
    document.title = "Cleanup | Supa Diska Klinah";
  }, []);

  return (
    <section aria-labelledby="cleanup-heading">
      <header className="page-header cleanup-page-header">
        <div>
          <p className="kicker">Temporary files</p>
          <h1 id="cleanup-heading">Cleanup</h1>
          <p>Review every item first. Cleanup uses the Windows Recycle Bin by default.</p>
        </div>
        <button type="button" disabled={state.loading || state.busy} onClick={state.retry}>
          {state.loading ? "Scanning…" : state.error ? "Try again" : "Scan again"}
        </button>
      </header>

      {state.loading && <div className="cleanup-state-panel" role="status"><h2>Scanning temporary caches</h2><p>Checking fixed Windows locations without changing files.</p></div>}
      {state.error && <div className="cleanup-state-panel error-state" role="alert"><h2>Cleanup could not continue</h2><p>Nothing else was changed. Scan again before retrying.</p></div>}
      {state.result && records.length === 0 && <div className="cleanup-state-panel"><h2>Nothing found</h2><p>No cache or tmp directories are currently eligible.</p></div>}
      {state.result && state.result.diagnostics.length > 0 && <p className="cleanup-diagnostics">{state.result.diagnostics.length} skipped {state.result.diagnostics.length === 1 ? "location" : "locations"}</p>}

      {state.result && records.length > 0 && (
        <div className="cleanup-results">
          <div className="cleanup-summary" role="status">
            <strong>{selectedCount} of {records.length} selected</strong>
            <span>{formatBytes(state.selectedBytes)} selected</span>
            <button type="button" disabled={state.busy} onClick={state.selectAll}>
              {selectedCount === records.length ? "Clear selection" : "Select all"}
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
                      <span><strong className="cleanup-path">{record.displayPath}</strong><small>{record.kind === "directory" ? "Directory" : "File"} · {formatBytes(record.bytes)}{record.modifiedUnixSeconds != null ? ` · Modified ${formatModified(record.modifiedUnixSeconds)}` : ""}</small></span>
                    </label>
                  </li>
                ))}
              </ul>
            </section>
          ))}
          <div className="cleanup-actions">
            <button type="button" disabled={!selectedCount || state.busy} onClick={() => void state.prepare("recycleBin")}>Move to Recycle Bin</button>
            <button className="danger-button" type="button" disabled={!selectedCount || state.busy} onClick={() => void state.prepare("permanent")}>Delete permanently</button>
          </div>
        </div>
      )}

      {state.plan && (
        <dialog open aria-labelledby="cleanup-confirm-title" aria-modal="true">
          <h2 id="cleanup-confirm-title">{state.plan.disposition === "permanent" ? "Permanently delete these items?" : "Move these items to the Recycle Bin?"}</h2>
          <p>{state.plan.selectedCount} {state.plan.selectedCount === 1 ? "item" : "items"} · {formatBytes(state.plan.selectedBytes)}</p>
          {state.plan.disposition === "permanent" && <p className="error-text">This cannot be undone by Supa Diska Klinah.</p>}
          <div className="dialog-actions">
            <button type="button" disabled={state.busy} onClick={state.cancelPlan}>Cancel</button>
            <button className={state.plan.disposition === "permanent" ? "danger-button" : undefined} type="button" disabled={state.busy} onClick={() => void state.confirmPlan()}>{state.busy ? "Working…" : state.plan.disposition === "permanent" ? "Delete permanently" : "Move to Recycle Bin"}</button>
          </div>
        </dialog>
      )}

      {state.execution && (
        <section className="cleanup-outcomes" aria-labelledby="latest-cleanup-heading">
          <h2 id="latest-cleanup-heading">Latest cleanup</h2>
          <Accounting execution={state.execution} />
          <ul>{state.execution.items.map((item) => <li key={item.itemId}><span>{item.state}</span><strong>{formatBytes(item.logicalBytes)}</strong></li>)}</ul>
          {state.execution.items.some((item) => item.state === "recycled" || item.state === "quarantined") && <button type="button" disabled={state.busy} onClick={() => void state.undo(state.execution!.executionId)}>Undo cleanup</button>}
        </section>
      )}

      {state.history.length > 0 && (
        <section className="cleanup-history" aria-labelledby="cleanup-history-heading">
          <h2 id="cleanup-history-heading">Cleanup history</h2>
          <ul>{state.history.slice(0, 20).map((item) => <li key={item.executionId}><span>{item.disposition}</span><span>{formatBytes(item.accounting.reclaimedBytes)} reclaimed</span></li>)}</ul>
        </section>
      )}
      <ProjectArtifactDiscovery />
    </section>
  );
}
