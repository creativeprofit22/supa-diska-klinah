import { useCallback, useEffect, useMemo, useState } from "react";
import { MAX_PLAN_CHANGES, systemChangeError } from "../../shared/system-change/api";
import { plannedMeta } from "../../shared/system-change/labels";
import { SystemChangeJournal } from "../../shared/system-change/SystemChangeJournal";
import { SystemChangeReview } from "../../shared/system-change/SystemChangeReview";
import type { SystemChange } from "../../shared/system-change/types";
import { getOptimizerProposals } from "./api";
import type { OptimizerReport, Proposal, ProposalGroup } from "./types";

const groupOrder: ProposalGroup[] = ["services", "privacy", "performance", "power"];
const groupTitle: Record<ProposalGroup, string> = {
  services: "Services", privacy: "Privacy", performance: "Performance", power: "Power",
};

interface Row { key: string; proposal: Proposal }

/** Proposals in display order: grouped by `groupOrder`, report order within each group. */
function displayRows(report: OptimizerReport | null): Row[] {
  if (!report) return [];
  const rows = report.proposals.map((proposal, index) => ({ key: String(index), proposal }));
  return groupOrder.flatMap((group) => rows.filter((row) => row.proposal.group === group));
}

const suggestedKeys = (rows: Row[]) =>
  rows.filter((row) => row.proposal.suggested).map((row) => row.key).slice(0, MAX_PLAN_CHANGES);

function fallbackDescribe(change: SystemChange): string {
  switch (change.kind) {
    case "setServiceStartType": return `Set service ${change.catalogId} to ${change.startType}`;
    case "setUserSetting":
    case "setMachineSetting": return `Change setting: ${change.settingId}`;
    case "setSystemTaskEnabled": return `${change.enabled ? "Enable" : "Disable"} task: ${change.catalogId}`;
    case "setActivePowerScheme": return `Switch power plan: ${change.scheme}`;
    case "setHibernation": return `${change.enabled ? "Enable" : "Disable"} hibernation`;
    default: return `System change: ${change.kind}`;
  }
}

export function OptimizerPage() {
  const [report, setReport] = useState<OptimizerReport | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<string[]>([]);
  const [journalKey, setJournalKey] = useState(0);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const next = await getOptimizerProposals();
      setReport(next);
      setSelected(suggestedKeys(displayRows(next)));
      setError(null);
    } catch (reason) {
      setError(systemChangeError(reason));
    } finally {
      setLoading(false);
    }
  }, []);
  useEffect(() => { void refresh(); }, [refresh]);

  const rows = useMemo(() => displayRows(report), [report]);
  const changes = useMemo(
    () => rows.filter((row) => selected.includes(row.key)).map((row) => row.proposal.planned.change),
    [rows, selected],
  );
  const labels = useMemo(
    () => new Map(rows.map((row) => [JSON.stringify(row.proposal.planned.change), row.proposal.label])),
    [rows],
  );
  const describe = useCallback(
    (change: SystemChange) => labels.get(JSON.stringify(change)) ?? fallbackDescribe(change),
    [labels],
  );
  const onFinished = useCallback(() => { setJournalKey((n) => n + 1); void refresh(); }, [refresh]);
  const toggle = (key: string) => setSelected((current) =>
    current.includes(key) ? current.filter((value) => value !== key) : [...current, key]);
  const full = selected.length >= MAX_PLAN_CHANGES;

  return <section aria-labelledby="optimizer-title">
    <header className="page-header"><div>
      <p className="eyebrow">System</p>
      <h1 id="optimizer-title">Quick optimization</h1>
      <p>Every change is listed separately. Suggested ones are pre-selected because they are low-risk and can be undone; nothing happens until you review the selection and confirm in Windows.</p>
    </div></header>
    <button type="button" onClick={() => void refresh()} disabled={loading}>Refresh</button>
    {loading && <p role="status">Checking which optimizations apply to this device…</p>}
    {error && <p role="alert">{error}</p>}
    {report && <>
      <p>{report.alreadyApplied} already applied, {report.unavailable} unavailable on this device</p>
      {rows.length === 0 && <p>No optimizations to propose.</p>}
      <div>
        <button type="button" onClick={() => setSelected(suggestedKeys(rows))}>Select suggested</button>
        <button type="button" onClick={() => setSelected([])}>Clear selection</button>
      </div>
      {full && <p>At most {MAX_PLAN_CHANGES} changes can be reviewed at a time.</p>}
      {groupOrder.map((group) => {
        const groupRows = rows.filter((row) => row.proposal.group === group);
        if (groupRows.length === 0) return null;
        const headingId = `optimizer-group-${group}`;
        return <section key={group} aria-labelledby={headingId}>
          <h2 id={headingId}>{groupTitle[group]}</h2>
          <ul>{groupRows.map(({ key, proposal }) => {
            const checked = selected.includes(key);
            return <li key={key}>
              <label>
                <input type="checkbox" checked={checked} disabled={!checked && full} onChange={() => toggle(key)} />
                <span>{proposal.label}</span>
              </label>
              <p>{proposal.planned.impact.effect}</p>
              <p className="system-change-meta">{plannedMeta(proposal.planned).join(" · ")}{proposal.suggested ? " · Suggested" : ""}</p>
            </li>;
          })}</ul>
        </section>;
      })}
    </>}
    <SystemChangeReview changes={changes} describe={describe} reviewLabel="Review selected optimizations" onFinished={onFinished} />
    <SystemChangeJournal describe={describe} refreshKey={journalKey} onFinished={onFinished} />
  </section>;
}
