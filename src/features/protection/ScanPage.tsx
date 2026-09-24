import { useEffect, useMemo, useRef, useState } from "react";
import { allowFinding, cancelScan, lastScan, quarantineFinding, scanStatus, startScan } from "./api";
import { EvidenceBadge, errorMessage, evidenceDetail, evidenceKindLabel, isCancelled, summaryLine } from "./labels";
import type { Evidence, FindingView, ScanReport, ScanScope, ScanStatus } from "./types";

const ORDER: Evidence["kind"][] = ["deterministic", "heuristic", "external", "unavailable"];
export const MAX_VISIBLE_FINDINGS = 500;
const STATUS_POLL_MS = 1000;
const IDLE: ScanStatus = { running: false, filesScanned: 0, bytesHashed: 0 };

export function ScanPage() {
  const [report, setReport] = useState<ScanReport | null>(null);
  // A scan started here, or one still running in the backend after this page was left.
  const [localRunning, setLocalRunning] = useState(false);
  const [status, setStatus] = useState<ScanStatus>(IDLE);
  const running = localRunning || status.running;
  const wasRunning = useRef(false);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void lastScan().then(setReport).catch(() => undefined);
    void scanStatus().then(setStatus).catch(() => undefined);
  }, []);

  useEffect(() => {
    if (!running) return undefined;
    const timer = setInterval(() => { void scanStatus().then(setStatus).catch(() => undefined); }, STATUS_POLL_MS);
    return () => clearInterval(timer);
  }, [running]);

  // Show results of a scan that finished while this page was not driving it.
  useEffect(() => {
    if (wasRunning.current && !running) void lastScan().then(setReport).catch(() => undefined);
    wasRunning.current = running;
  }, [running]);

  const run = async (scope: ScanScope) => {
    setLocalRunning(true);
    setError(null);
    setMessage(null);
    try {
      setReport(await startScan(scope));
    } catch (reason) {
      if (!isCancelled(reason)) setError(errorMessage(reason, "The scan could not run."));
    } finally {
      setStatus(IDLE);
      setLocalRunning(false);
    }
  };

  const act = async (finding: FindingView, action: "quarantine" | "allow") => {
    setBusyId(finding.id);
    setError(null);
    try {
      if (action === "quarantine") {
        await quarantineFinding(finding.id);
        setMessage(`Moved to quarantine: ${finding.path}`);
      } else {
        await allowFinding(finding.id);
        setMessage("Heuristic findings for this exact file content will be hidden in future scans.");
      }
      setReport(await lastScan());
    } catch (reason) {
      if (!isCancelled(reason)) setError(errorMessage(reason, "The action could not be completed."));
    } finally {
      setBusyId(null);
    }
  };

  const groups = useMemo(() => {
    const findings = report?.findings ?? [];
    return ORDER.map((kind) => [kind, findings.filter((finding) => finding.evidence.kind === kind)] as const)
      .filter(([, items]) => items.length > 0);
  }, [report]);

  return <>
    <section aria-labelledby="scan-start">
      <h2 id="scan-start">Start a scan</h2>
      <p>Quick scan checks startup folders, Temp, Downloads and running programs. Folder scan checks one folder you choose. Links and junctions are never followed.</p>
      <div className="protection-actions">
        <button type="button" disabled={running} onClick={() => void run("quick")}>Quick scan</button>
        <button type="button" disabled={running} onClick={() => void run("folder")}>Scan a folder…</button>
        {running && <button type="button" onClick={() => void cancelScan()}>Cancel scan</button>}
      </div>
      {running && <p role="status">Scanning…{status.running ? ` ${status.filesScanned.toLocaleString()} files examined` : ""}</p>}
    </section>
    {error && <p role="alert">{error}</p>}
    {message && <p role="status">{message}</p>}
    {report && <section aria-labelledby="scan-results">
      <h2 id="scan-results">Results</h2>
      <p>{summaryLine(report.summary)}{report.summary.allowlisted > 0 ? ` · ${report.summary.allowlisted} hidden by your allowlist` : ""}{report.summary.reparsePointsSkipped > 0 ? ` · ${report.summary.reparsePointsSkipped} links or junctions skipped (not followed)` : ""}</p>
      {report.summary.cancelled && <p role="note">The scan was cancelled; results are partial.</p>}
      {report.summary.truncated && <p role="note">The scan stopped at its size limit; results are partial.</p>}
      <p className="protection-hint">A file with no findings has only been compared with the current rules and heuristics. Detection is limited; see the Rules section.</p>
      {groups.map(([kind, items]) => <section key={kind} aria-labelledby={`scan-${kind}`}>
        <h3 id={`scan-${kind}`}>{evidenceKindLabel[kind]} ({items.length})</h3>
        <ul className="protection-findings">{items.slice(0, MAX_VISIBLE_FINDINGS).map((finding) => <li key={finding.id}>
          <EvidenceBadge evidence={finding.evidence} /> <code>{finding.path}</code>
          <p>{evidenceDetail(finding.evidence)}</p>
          {finding.evidence.kind === "heuristic" && <p className="protection-hint">False positives: {finding.evidence.falsePositiveNote}</p>}
          <div className="protection-actions">
            {finding.canQuarantine && <button type="button" disabled={busyId !== null} onClick={() => void act(finding, "quarantine")}>Quarantine…</button>}
            {finding.evidence.kind === "heuristic" && finding.sha256 && <button type="button" disabled={busyId !== null} onClick={() => void act(finding, "allow")}>Hide for this file</button>}
          </div>
        </li>)}</ul>
        {items.length > MAX_VISIBLE_FINDINGS && <p>Showing the first {MAX_VISIBLE_FINDINGS} of {items.length}.</p>}
      </section>)}
    </section>}
  </>;
}
