import { useEffect, useMemo, useRef, useState } from "react";
import { useFormat, useStrings } from "../../shared/i18n/I18nProvider";
import { allowFinding, cancelScan, lastScan, quarantineFinding, scanStatus, startScan } from "./api";
import { EvidenceBadge, errorMessage, evidenceDetail, evidenceKindLabel, isCancelled, summaryLine } from "./labels";
import { protectionStrings } from "./strings";
import type { Evidence, FindingView, ScanReport, ScanScope, ScanStatus } from "./types";

const ORDER: Evidence["kind"][] = ["deterministic", "heuristic", "external", "unavailable"];
export const MAX_VISIBLE_FINDINGS = 500;
const STATUS_POLL_MS = 1000;
const IDLE: ScanStatus = { running: false, filesScanned: 0, bytesHashed: 0 };

export function ScanPage() {
  const strings = useStrings(protectionStrings);
  const t = strings.scan;
  const fmt = useFormat();
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
      if (!isCancelled(reason)) setError(errorMessage(reason, t.failed));
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
        setMessage(t.quarantined(finding.path));
      } else {
        await allowFinding(finding.id);
        setMessage(t.allowed);
      }
      setReport(await lastScan());
    } catch (reason) {
      if (!isCancelled(reason)) setError(errorMessage(reason, strings.errors.actionFailed));
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
      <h2 id="scan-start">{t.startTitle}</h2>
      <p>{t.startIntro}</p>
      <div className="protection-actions">
        <button type="button" disabled={running} onClick={() => void run("quick")}>{t.quick}</button>
        <button type="button" disabled={running} onClick={() => void run("folder")}>{t.folder}</button>
        {running && <button type="button" onClick={() => void cancelScan()}>{t.cancel}</button>}
      </div>
      {running && <p role="status">{t.scanning}{status.running ? t.filesExamined(fmt.number(status.filesScanned)) : ""}</p>}
    </section>
    {error && <p role="alert">{error}</p>}
    {message && <p role="status">{message}</p>}
    {report && <section aria-labelledby="scan-results">
      <h2 id="scan-results">{t.resultsTitle}</h2>
      <p>{summaryLine(report.summary, strings, fmt.number)}{report.summary.allowlisted > 0 ? t.allowlisted(fmt.number(report.summary.allowlisted)) : ""}{report.summary.reparsePointsSkipped > 0 ? t.reparseSkipped(fmt.number(report.summary.reparsePointsSkipped)) : ""}</p>
      {report.summary.cancelled && <p role="note">{t.cancelled}</p>}
      {report.summary.truncated && <p role="note">{t.truncated}</p>}
      <p className="protection-hint">{t.limitsHint}</p>
      {groups.map(([kind, items]) => <section key={kind} aria-labelledby={`scan-${kind}`}>
        <h3 id={`scan-${kind}`}>{strings.evidenceKind[kind]} ({items.length})</h3>
        <ul className="protection-findings">{items.slice(0, MAX_VISIBLE_FINDINGS).map((finding) => <li key={finding.id}>
          <EvidenceBadge evidence={finding.evidence} /> <code>{finding.path}</code>
          <p>{evidenceDetail(finding.evidence, strings)}</p>
          {finding.evidence.kind === "heuristic" && <p className="protection-hint">{t.falsePositives(finding.evidence.falsePositiveNote)}</p>}
          <div className="protection-actions">
            {finding.canQuarantine && <button type="button" disabled={busyId !== null} onClick={() => void act(finding, "quarantine")}>{t.quarantine}</button>}
            {finding.evidence.kind === "heuristic" && finding.sha256 && <button type="button" disabled={busyId !== null} onClick={() => void act(finding, "allow")}>{t.hide}</button>}
          </div>
        </li>)}</ul>
        {items.length > MAX_VISIBLE_FINDINGS && <p>{t.showingFirst(fmt.number(MAX_VISIBLE_FINDINGS), fmt.number(items.length))}</p>}
      </section>)}
    </section>}
  </>;
}
