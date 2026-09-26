import { useCallback, useEffect, useMemo, useState } from "react";
import { SystemChangeJournal } from "../../shared/system-change/SystemChangeJournal";
import { SystemChangeReview } from "../../shared/system-change/SystemChangeReview";
import { useFormat, useStrings } from "../../shared/i18n/I18nProvider";
import type { HostsLineOp, SystemChange } from "../../shared/system-change/types";
import { getHostsReport } from "./api";
import { hostsStrings, type HostsStrings } from "./strings";
import type { HostsLineView, HostsReport } from "./types";

export const MAX_LINE_OPS = 64;

/** Lines are zero-based in the report and in `lineOps`; people read them one-based. */
const lineNumber = (index: number) => index + 1;

export function describe(change: SystemChange, t: HostsStrings = hostsStrings.en): string {
  if (change.kind !== "editHosts") return change.kind;
  const parts = change.lineOps.map((op) => t.describePart(op.action === "disable", lineNumber(op.line)));
  return t.describe(parts.join(", "));
}

function actionFor(line: HostsLineView): HostsLineOp["action"] | null {
  if (line.kind === "mapping") return "disable";
  if (line.kind === "appDisabled") return "restore";
  return null;
}

function loadError(reason: unknown, t: HostsStrings): string {
  const message = (reason as { message?: unknown } | null)?.message;
  return typeof message === "string" && message.length <= 300 ? message : t.loadError;
}

/** Hosts file findings and lines, with explicit per-line disable/restore. */
export function HostsPage() {
  const t = useStrings(hostsStrings);
  const fmt = useFormat();
  const describeChange = useCallback((change: SystemChange) => describe(change, t), [t]);
  const [report, setReport] = useState<HostsReport | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<Map<number, HostsLineOp["action"]>>(() => new Map());
  const [journalKey, setJournalKey] = useState(0);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      setReport(await getHostsReport());
      setSelected(new Map());
      setError(null);
    } catch (reason) {
      setError(loadError(reason, t));
    } finally {
      setLoading(false);
    }
  }, [t]);
  useEffect(() => { void refresh(); }, [refresh]);

  const toggle = (line: number, action: HostsLineOp["action"]) => setSelected((current) => {
    const next = new Map(current);
    if (next.has(line)) next.delete(line);
    else if (next.size < MAX_LINE_OPS) next.set(line, action);
    return next;
  });
  const changes = useMemo<SystemChange[]>(() => {
    if (selected.size === 0) return [];
    const lineOps = [...selected.entries()].sort(([a], [b]) => a - b).slice(0, MAX_LINE_OPS).map(([line, action]) => ({ line, action }));
    return [{ kind: "editHosts", lineOps }];
  }, [selected]);
  const onFinished = useCallback(() => { setJournalKey((n) => n + 1); void refresh(); }, [refresh]);
  const full = selected.size >= MAX_LINE_OPS;

  return <section aria-labelledby="hosts-title">
    <header className="page-header">
      <div>
        <p className="eyebrow">{t.eyebrow}</p>
        <h1 id="hosts-title">{t.title}</h1>
        <p>{t.intro}</p>
      </div>
      <button type="button" disabled={loading} onClick={() => void refresh()}>{t.refresh}</button>
    </header>
    {loading && <p role="status">{t.loading}</p>}
    {error && <p role="alert">{error}</p>}
    {report && <>
      <p>{report.path}{t.summary(fmt.number(report.totalLines), fmt.number(report.sizeBytes), report.linesTruncated ? fmt.number(report.lines.length) : null)}</p>
      <p>{t.backupNote}</p>

      <section aria-labelledby="hosts-findings">
        <h2 id="hosts-findings">{t.findingsTitle}</h2>
        {report.findings.length === 0 && <p>{t.noFindings}</p>}
        <ul>{report.findings.map((finding, index) => <li key={`${finding.line}-${index}`}>
          <span>{t.severity[finding.severity]}</span> · <strong>{t.finding[finding.kind]}</strong>{t.findingLine(lineNumber(finding.line), finding.detail)}
        </li>)}</ul>
      </section>

      <section aria-labelledby="hosts-lines">
        <h2 id="hosts-lines">{t.linesTitle}</h2>
        {full && <p>{t.maxLines(MAX_LINE_OPS)}</p>}
        <ol className="hosts-lines">{report.lines.map((line) => {
          const action = actionFor(line);
          const n = lineNumber(line.index);
          const checked = selected.has(line.index);
          return <li key={line.index} value={n}>
            {action
              ? <label>
                <input type="checkbox" checked={checked} disabled={!checked && full} onChange={() => toggle(line.index, action)} />
                {t.toggleLine(action === "disable", n)}
              </label>
              : <span>{t.line(n)}</span>}
            {" "}<span>({t.kind[line.kind]})</span> <code>{line.text}</code>
          </li>;
        })}</ol>
      </section>
    </>}
    <SystemChangeReview changes={changes} describe={describeChange} onFinished={onFinished} />
    <SystemChangeJournal module="hosts" describe={describeChange} refreshKey={journalKey} onFinished={onFinished} />
  </section>;
}
