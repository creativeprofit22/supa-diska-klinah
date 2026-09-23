import { useCallback, useEffect, useId, useRef, useState } from "react";
import { MAX_PLAN_CHANGES, systemChangeActions, systemChangeError, type SystemChangeActions } from "./api";
import { outcomeLabel, reversibilityLabel } from "./labels";
import type { ExecutionReport, JournalView, RollbackStatus, SystemChange } from "./types";
import { ExecutionResults, PlannedChangeList } from "./SystemChangeReview";
import { useSystemChangePlan } from "./useSystemChangePlan";
import "./system-change.css";

const rollbackLabel: Record<RollbackStatus, string> = {
  available: "Can be undone",
  alreadyRolledBack: "Already undone",
  irreversible: "Cannot be undone",
  interrupted: "Interrupted; check the setting manually",
  nothingApplied: "Nothing to undo",
};

/** Change history for one module (or all), with explicit per-entry undo. */
export function SystemChangeJournal({ module, describe, actions = systemChangeActions, refreshKey = 0, onFinished }: {
  module?: string;
  describe: (change: SystemChange) => string;
  actions?: SystemChangeActions;
  refreshKey?: number;
  /** Called once per completed undo so the page can reload its inventory. */
  onFinished?: (report: ExecutionReport) => void;
}) {
  const [entries, setEntries] = useState<JournalView[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<string[]>([]);
  const plan = useSystemChangePlan(actions);
  const headingId = useId();
  const load = useCallback(async () => {
    try {
      const all = await actions.journal();
      setEntries(all.filter((view) => !module || kindModule(view.entry.change) === module));
      setError(null);
    } catch (reason) { setError(systemChangeError(reason)); }
  }, [actions, module]);
  useEffect(() => { void load(); }, [load, refreshKey, plan.report]);
  const handledReport = useRef<ExecutionReport | null>(null);
  useEffect(() => {
    if (plan.phase !== "done" || !plan.report || handledReport.current === plan.report) return;
    handledReport.current = plan.report;
    onFinished?.(plan.report);
  }, [onFinished, plan.phase, plan.report]);
  const toggle = (id: string) => setSelected((current) => current.includes(id) ? current.filter((value) => value !== id) : [...current, id].slice(0, MAX_PLAN_CHANGES));
  return <section className="system-change-journal" aria-labelledby={headingId}>
    <h2 id={headingId}>Change history</h2>
    {error && <p role="alert">{error}</p>}
    {entries?.length === 0 && <p>No changes recorded yet.</p>}
    {!!entries?.length && <ul>{entries.map(({ entry, rollback }) => <li key={entry.id}>
      <label>
        <input type="checkbox" disabled={rollback !== "available" || plan.phase !== "idle"} checked={selected.includes(entry.id)} onChange={() => toggle(entry.id)} />
        <span>{describe(entry.change)}</span>
      </label>
      <p className="system-change-meta">{new Date(entry.recordedAt * 1000).toLocaleString()} · {outcomeLabel(entry.outcome)} · {reversibilityLabel(entry.reversibility)} · {rollbackLabel[rollback]}</p>
    </li>)}</ul>}
    <button type="button" disabled={selected.length === 0 || plan.phase !== "idle"} onClick={() => { void plan.reviewRollback(selected); setSelected([]); }}>Review undo ({selected.length})</button>
    {plan.error && <p role="alert">{plan.error}</p>}
    {plan.ticket && <>
      <p>Undo restores the state recorded before each change. Windows will ask you to confirm.</p>
      <PlannedChangeList ticket={plan.ticket} />
      <button type="button" onClick={() => void plan.apply()}>Continue to Windows confirmation</button>
      <button type="button" onClick={plan.discard}>Discard review</button>
    </>}
    {plan.report && <ExecutionResults report={plan.report} describe={describe} />}
  </section>;
}

const moduleByKind: Record<SystemChange["kind"], string> = {
  setStartupEntry: "startup", setServiceStartType: "services", setUserSetting: "privacy", setMachineSetting: "privacy",
  setSystemTaskEnabled: "privacy", setWindowsUpdatePolicy: "updates", setFirewallRuleEnabled: "firewall",
  setFirewallProfileEnabled: "firewall", setHibernation: "power", setActivePowerScheme: "power",
  deleteDriverPackage: "drivers", editHosts: "hosts", createRestorePoint: "restore",
  upsertScanSchedule: "scheduler", removeScanSchedule: "scheduler",
};
export const kindModule = (change: SystemChange) => moduleByKind[change.kind];
