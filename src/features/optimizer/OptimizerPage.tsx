import { useCallback, useEffect, useMemo, useState } from "react";
import { useFormat, useStrings } from "../../shared/i18n/I18nProvider";
import { MAX_PLAN_CHANGES, systemChangeError } from "../../shared/system-change/api";
import { useSystemChangeLabels } from "../../shared/system-change/labels";
import { SystemChangeJournal } from "../../shared/system-change/SystemChangeJournal";
import { SystemChangeReview } from "../../shared/system-change/SystemChangeReview";
import type { SystemChange } from "../../shared/system-change/types";
import { getOptimizerProposals } from "./api";
import { optimizerStrings, type OptimizerStrings } from "./strings";
import type { OptimizerReport, Proposal, ProposalGroup } from "./types";

const groupOrder: ProposalGroup[] = ["services", "privacy", "performance", "power"];

interface Row { key: string; proposal: Proposal }

/** Proposals in display order: grouped by `groupOrder`, report order within each group. */
function displayRows(report: OptimizerReport | null): Row[] {
  if (!report) return [];
  const rows = report.proposals.map((proposal, index) => ({ key: String(index), proposal }));
  return groupOrder.flatMap((group) => rows.filter((row) => row.proposal.group === group));
}

const suggestedKeys = (rows: Row[]) =>
  rows.filter((row) => row.proposal.suggested).map((row) => row.key).slice(0, MAX_PLAN_CHANGES);

function fallbackDescribe(change: SystemChange, t: OptimizerStrings): string {
  const d = t.describe;
  switch (change.kind) {
    case "setServiceStartType": return d.service(change.catalogId, t.startType[change.startType]);
    case "setUserSetting":
    case "setMachineSetting": return d.setting(change.settingId);
    case "setSystemTaskEnabled": return d.task(change.enabled, change.catalogId);
    case "setActivePowerScheme": return d.powerPlan(change.scheme);
    case "setHibernation": return d.hibernation(change.enabled);
    default: return d.other(change.kind);
  }
}

export function OptimizerPage() {
  const sc = useSystemChangeLabels();
  const t = useStrings(optimizerStrings);
  const fmt = useFormat();
  const errors = sc.t.errors;
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
      setError(systemChangeError(reason, errors));
    } finally {
      setLoading(false);
    }
  }, [errors]);
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
    (change: SystemChange) => labels.get(JSON.stringify(change)) ?? fallbackDescribe(change, t),
    [labels, t],
  );
  const onFinished = useCallback(() => { setJournalKey((n) => n + 1); void refresh(); }, [refresh]);
  const toggle = (key: string) => setSelected((current) =>
    current.includes(key) ? current.filter((value) => value !== key) : [...current, key]);
  const full = selected.length >= MAX_PLAN_CHANGES;

  return <section aria-labelledby="optimizer-title">
    <header className="page-header"><div>
      <p className="eyebrow">{t.eyebrow}</p>
      <h1 id="optimizer-title">{t.title}</h1>
      <p>{t.intro}</p>
    </div></header>
    <button type="button" onClick={() => void refresh()} disabled={loading}>{t.refresh}</button>
    {loading && <p role="status">{t.loading}</p>}
    {error && <p role="alert">{error}</p>}
    {report && <>
      <p>{t.summary(fmt.number(report.alreadyApplied), fmt.number(report.unavailable))}</p>
      {rows.length === 0 && <p>{t.none}</p>}
      <div>
        <button type="button" onClick={() => setSelected(suggestedKeys(rows))}>{t.selectSuggested}</button>
        <button type="button" onClick={() => setSelected([])}>{t.clearSelection}</button>
      </div>
      {full && <p>{t.maxChanges(MAX_PLAN_CHANGES)}</p>}
      {groupOrder.map((group) => {
        const groupRows = rows.filter((row) => row.proposal.group === group);
        if (groupRows.length === 0) return null;
        const headingId = `optimizer-group-${group}`;
        return <section key={group} aria-labelledby={headingId}>
          <h2 id={headingId}>{t.group[group]}</h2>
          <ul>{groupRows.map(({ key, proposal }) => {
            const checked = selected.includes(key);
            return <li key={key}>
              <label>
                <input type="checkbox" checked={checked} disabled={!checked && full} onChange={() => toggle(key)} />
                <span>{proposal.label}</span>
              </label>
              <p>{proposal.planned.impact.effect}</p>
              <p className="system-change-meta">{sc.plannedMeta(proposal.planned).join(" · ")}{proposal.suggested ? t.suggested : ""}</p>
            </li>;
          })}</ul>
        </section>;
      })}
    </>}
    <SystemChangeReview changes={changes} describe={describe} reviewLabel={t.reviewLabel} onFinished={onFinished} />
    <SystemChangeJournal describe={describe} refreshKey={journalKey} onFinished={onFinished} />
  </section>;
}
