import { useCallback, useEffect, useMemo, useState } from "react";
import { SystemChangeJournal } from "../../shared/system-change/SystemChangeJournal";
import { SystemChangeReview } from "../../shared/system-change/SystemChangeReview";
import type { HostsLineOp, SystemChange } from "../../shared/system-change/types";
import { getHostsReport } from "./api";
import type { HostsFindingKind, HostsLineKind, HostsLineView, HostsReport } from "./types";

export const MAX_LINE_OPS = 64;

const kindLabel: Record<HostsLineKind, string> = {
  blank: "Blank", comment: "Comment", mapping: "Mapping", appDisabled: "Disabled by this app", invalid: "Not a valid entry",
};
const findingLabel: Record<HostsFindingKind, string> = {
  redirect: "Redirect", securityBlocked: "Security site blocked", customMapping: "Custom mapping", invalid: "Invalid line",
};

/** Lines are zero-based in the report and in `lineOps`; people read them one-based. */
const lineNumber = (index: number) => index + 1;

export function describe(change: SystemChange): string {
  if (change.kind !== "editHosts") return change.kind;
  const parts = change.lineOps.map((op) => `${op.action === "disable" ? "disable" : "restore"} line ${lineNumber(op.line)}`);
  return `Edit hosts file: ${parts.join(", ")}`;
}

function actionFor(line: HostsLineView): HostsLineOp["action"] | null {
  if (line.kind === "mapping") return "disable";
  if (line.kind === "appDisabled") return "restore";
  return null;
}

function loadError(reason: unknown): string {
  const message = (reason as { message?: unknown } | null)?.message;
  return typeof message === "string" && message.length <= 300 ? message : "The hosts file could not be read.";
}

/** Hosts file findings and lines, with explicit per-line disable/restore. */
export function HostsPage() {
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
      setError(loadError(reason));
    } finally {
      setLoading(false);
    }
  }, []);
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
        <p className="eyebrow">System</p>
        <h1 id="hosts-title">Hosts file</h1>
        <p>Lists what the Windows hosts file redirects or blocks; entries are only commented out or restored, never deleted.</p>
      </div>
      <button type="button" disabled={loading} onClick={() => void refresh()}>Refresh</button>
    </header>
    {loading && <p role="status">Reading the hosts file…</p>}
    {error && <p role="alert">{error}</p>}
    {report && <>
      <p>{report.path} · {report.totalLines} lines · {report.sizeBytes} bytes{report.linesTruncated ? ` · only the first ${report.lines.length} lines are shown` : ""}</p>
      <p>Before editing, a backup of the hosts file is made. The file is only changed if it is still exactly as it was when you reviewed it; otherwise nothing is written and you need to refresh.</p>

      <section aria-labelledby="hosts-findings">
        <h2 id="hosts-findings">Findings</h2>
        {report.findings.length === 0 && <p>No findings.</p>}
        <ul>{report.findings.map((finding, index) => <li key={`${finding.line}-${index}`}>
          <span>{finding.severity === "high" ? "High" : "Low"}</span> · <strong>{findingLabel[finding.kind]}</strong> · Line {lineNumber(finding.line)}: {finding.detail}
        </li>)}</ul>
      </section>

      <section aria-labelledby="hosts-lines">
        <h2 id="hosts-lines">Lines</h2>
        {full && <p>At most {MAX_LINE_OPS} lines can be changed at a time.</p>}
        <ol className="hosts-lines">{report.lines.map((line) => {
          const action = actionFor(line);
          const n = lineNumber(line.index);
          const checked = selected.has(line.index);
          return <li key={line.index} value={n}>
            {action
              ? <label>
                <input type="checkbox" checked={checked} disabled={!checked && full} onChange={() => toggle(line.index, action)} />
                {`${action === "disable" ? "Disable" : "Restore"} line ${n}`}
              </label>
              : <span>Line {n}</span>}
            {" "}<span>({kindLabel[line.kind]})</span> <code>{line.text}</code>
          </li>;
        })}</ol>
      </section>
    </>}
    <SystemChangeReview changes={changes} describe={describe} onFinished={onFinished} />
    <SystemChangeJournal module="hosts" describe={describe} refreshKey={journalKey} onFinished={onFinished} />
  </section>;
}
