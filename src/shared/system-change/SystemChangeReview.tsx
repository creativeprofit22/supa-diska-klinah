import { useEffect, useId, useRef } from "react";
import { MAX_PLAN_CHANGES, systemChangeActions, type SystemChangeActions } from "./api";
import { outcomeLabel, plannedMeta, succeeded } from "./labels";
import type { ExecutionReport, PlanTicket, SystemChange } from "./types";
import { useSystemChangePlan } from "./useSystemChangePlan";
import "./system-change.css";

export function PlannedChangeList({ ticket }: { ticket: PlanTicket }) {
  return <ol className="system-change-list">
    {ticket.changes.map((planned, index) => <li key={index}>
      <strong>{planned.impact.component}</strong>
      <p>{planned.impact.effect}</p>
      <p className="system-change-meta">{plannedMeta(planned).join(" · ")}</p>
    </li>)}
  </ol>;
}

export function ExecutionResults({ report, describe }: { report: ExecutionReport; describe: (change: SystemChange) => string }) {
  const allOk = report.results.every((result) => succeeded(result.outcome));
  return <section aria-label="System change results" className="system-change-results">
    {allOk
      ? <p role="status">All changes finished.</p>
      : <p role="alert">Some changes did not finish. Each result below says what happened; nothing else was changed silently.</p>}
    <ul>{report.results.map((result, index) => <li key={index} data-outcome={result.outcome.status}>
      <span>{describe(result.change)}</span> — <span>{outcomeLabel(result.outcome)}</span>
    </li>)}</ul>
  </section>;
}

/**
 * Review → Windows confirmation → apply for an explicit list of changes.
 * Changes are never applied without the native confirmation, and each one
 * is listed with its impact and reversibility first.
 */
export function SystemChangeReview({ changes, describe, actions = systemChangeActions, onFinished, reviewLabel = "Review selected changes" }: {
  changes: SystemChange[];
  describe: (change: SystemChange) => string;
  actions?: SystemChangeActions;
  onFinished?: (report: ExecutionReport) => void;
  reviewLabel?: string;
}) {
  const plan = useSystemChangePlan(actions);
  const region = useRef<HTMLElement>(null);
  const headingId = useId();
  const changesKey = JSON.stringify(changes);
  const lastKey = useRef(changesKey);
  const { discard, phase, report } = plan;
  useEffect(() => {
    if (lastKey.current !== changesKey && phase === "reviewing") discard();
    lastKey.current = changesKey;
  }, [changesKey, discard, phase]);
  useEffect(() => { if (phase === "reviewing" || phase === "done") region.current?.focus(); }, [phase]);
  useEffect(() => { if (phase === "done" && report) onFinished?.(report); }, [onFinished, phase, report]);
  const tooMany = changes.length > MAX_PLAN_CHANGES;
  const busy = phase === "planning" || phase === "applying";
  return <section className="system-change-review" aria-labelledby={headingId} ref={region} tabIndex={-1}>
    <h2 id={headingId}>Review and apply</h2>
    {phase !== "reviewing" && <button type="button" disabled={busy || changes.length === 0 || tooMany} onClick={() => void plan.reviewChanges(changes)}>
      {reviewLabel} ({changes.length})
    </button>}
    {tooMany && <p role="alert">Select at most {MAX_PLAN_CHANGES} changes at a time.</p>}
    {plan.error && <p role="alert">{plan.error}</p>}
    {phase === "planning" && <p role="status">Checking the current state of each change…</p>}
    {plan.ticket && <>
      <p>These changes will be made in order. Windows will ask you to confirm; {plan.ticket.requiresHelper ? "administrator permission (UAC) will be requested once." : "no administrator permission is needed."} This review expires in {plan.ticket.expiresInSeconds} seconds.</p>
      <PlannedChangeList ticket={plan.ticket} />
      <div className="system-change-actions">
        <button type="button" disabled={busy} onClick={() => void plan.apply()}>Continue to Windows confirmation</button>
        <button type="button" disabled={busy} onClick={plan.discard}>Discard review</button>
      </div>
    </>}
    {phase === "applying" && <p role="status">Waiting for Windows confirmation and applying…</p>}
    {plan.report && <ExecutionResults report={plan.report} describe={describe} />}
  </section>;
}
