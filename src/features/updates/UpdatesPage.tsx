import { useCallback, useEffect, useMemo, useState } from "react";
import { SystemChangeJournal } from "../../shared/system-change/SystemChangeJournal";
import { SystemChangeReview } from "../../shared/system-change/SystemChangeReview";
import { unsupportedLabel } from "../../shared/system-change/labels";
import type { SystemChange } from "../../shared/system-change/types";
import { detectWindowsUpdates, getWindowsUpdateStatus } from "./api";
import type { Edition, PolicyStatus, UpdateStatus } from "./types";

const editionLabel: Record<Edition, string> = {
  home: "Home", pro: "Pro", education: "Education", enterprise: "Enterprise", server: "Server", other: "Other",
};

function errorText(reason: unknown, fallback: string): string {
  const message = (reason as { message?: unknown } | null)?.message;
  return typeof message === "string" && message.length > 0 && message.length <= 300 ? message : fallback;
}

const yesNo = (value: boolean | null) => value === null ? "Unknown" : value ? "Yes" : "No";
const dateText = (seconds: number | null) => seconds === null ? "Unknown" : new Date(seconds * 1000).toLocaleString();

function valueLabel(policy: PolicyStatus, value: number | null): string {
  if (value === null) return "Windows default";
  if (policy.allowed.kind === "options") return policy.allowed.options.find((option) => option.value === value)?.label ?? String(value);
  return `${value} days`;
}

/** Drafted target value per policy id; absent means "keep current". */
type Drafts = Record<string, number | null>;

export function UpdatesPage() {
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [detecting, setDetecting] = useState(false);
  const [detectStarted, setDetectStarted] = useState(false);
  const [drafts, setDrafts] = useState<Drafts>({});
  const [journalKey, setJournalKey] = useState(0);
  const [revision, setRevision] = useState(0);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      setStatus(await getWindowsUpdateStatus());
      setDrafts({});
      setRevision((value) => value + 1);
      setError(null);
    } catch (reason) {
      setError(errorText(reason, "Windows Update status could not be read."));
    } finally {
      setLoading(false);
    }
  }, []);
  useEffect(() => { void refresh(); }, [refresh]);

  const detect = async () => {
    setDetecting(true);
    setDetectStarted(false);
    try {
      await detectWindowsUpdates();
      // DetectNow only starts a background scan; the refreshed status won't reflect it for minutes.
      setDetectStarted(true);
      await refresh();
    } catch (reason) {
      setDetectStarted(false);
      setError(errorText(reason, "Windows could not start an update check."));
    } finally {
      setDetecting(false);
    }
  };

  const policies = useMemo(() => status?.policies ?? [], [status]);
  const changes = useMemo<SystemChange[]>(() => policies
    .filter((policy) => policy.id in drafts && drafts[policy.id] !== policy.current)
    .map((policy) => ({ kind: "setWindowsUpdatePolicy", settingId: policy.id, value: drafts[policy.id] })), [drafts, policies]);

  const describe = useCallback((change: SystemChange) => {
    if (change.kind !== "setWindowsUpdatePolicy") return change.kind;
    const policy = policies.find((entry) => entry.id === change.settingId);
    return policy
      ? `Set Windows Update policy: ${policy.label} → ${valueLabel(policy, change.value)}`
      : `Set Windows Update policy: ${change.settingId}`;
  }, [policies]);

  /** `undefined` drops the draft so the policy keeps its current value. */
  const setDraft = (id: string, value: number | null | undefined) => setDrafts((current) => {
    const next = { ...current };
    if (value === undefined) delete next[id]; else next[id] = value;
    return next;
  });
  const onFinished = useCallback(() => { setJournalKey((value) => value + 1); void refresh(); }, [refresh]);
  const locked = !status?.policySupported;

  return <section className="updates-page" aria-labelledby="updates-title">
    <header className="page-header"><div>
      <p className="eyebrow">System</p>
      <h1 id="updates-title">Windows Update</h1>
      <p>Shows what the Windows Update Agent reports and lets you set a few documented Group Policy values; checking for updates never installs anything by itself.</p>
    </div></header>
    <div className="updates-actions">
      <button type="button" disabled={loading} onClick={() => void refresh()}>Refresh</button>
      <button type="button" disabled={detecting || loading} onClick={() => void detect()}>Check for updates now</button>
    </div>
    {loading && <p role="status">Reading Windows Update status…</p>}
    {detecting && <p role="status">Asking Windows to check for updates…</p>}
    {detectStarted && <p role="status">Windows started checking for updates in the background. Results appear in Settings › Windows Update; refresh this page in a few minutes to see the last search time.</p>}
    {error && <p role="alert">{error}</p>}
    {status && <>
      <dl className="updates-status">
        <dt>Windows Update API available</dt><dd>{status.apiAvailable ? "Yes" : "No"}</dd>
        <dt>Automatic Updates service enabled</dt><dd>{yesNo(status.serviceEnabled)}</dd>
        <dt>Last successful search</dt><dd>{dateText(status.lastSearchSuccess)}</dd>
        <dt>Last successful install</dt><dd>{dateText(status.lastInstallSuccess)}</dd>
        <dt>Restart required</dt><dd>{yesNo(status.rebootRequired)}</dd>
        <dt>Edition</dt><dd>{editionLabel[status.edition]}</dd>
        <dt>Managed by an organization</dt><dd>{status.managed ? "Yes" : "No"}</dd>
      </dl>
      <h2>Update policies</h2>
      {locked && <p className="updates-unsupported">
        Policy changes are unavailable: {status.unsupportedReason ? unsupportedLabel[status.unsupportedReason] : "Not supported on this device"}.
      </p>}
      <ul className="updates-policies">
        {status.policies.map((policy) => <PolicyControl key={`${revision}:${policy.id}`} policy={policy} disabled={locked}
          draft={policy.id in drafts ? drafts[policy.id] : policy.current} onChange={(value) => setDraft(policy.id, value)} />)}
      </ul>
      <SystemChangeReview changes={changes} describe={describe} onFinished={onFinished} />
    </>}
    <SystemChangeJournal module="updates" describe={describe} refreshKey={journalKey} onFinished={onFinished} />
  </section>;
}

function PolicyControl({ policy, draft, disabled, onChange }: {
  policy: PolicyStatus; draft: number | null; disabled: boolean; onChange: (value: number | null | undefined) => void;
}) {
  const { allowed } = policy;
  const [text, setText] = useState(draft === null ? "" : String(draft));
  const [invalid, setInvalid] = useState(false);
  const label = `Policy value for ${policy.label}`;
  const currentText = `Current: ${valueLabel(policy, policy.current)}${policy.current !== null && !policy.applied ? " (not applied on this device)" : ""}`;
  return <li>
    <strong>{policy.label}</strong>
    <p>{policy.description}</p>
    <p>{currentText}</p>
    {allowed.kind === "options"
      ? <select aria-label={label} disabled={disabled} value={draft === null ? "default" : String(draft)}
        onChange={(event) => onChange(event.target.value === "default" ? null : Number(event.target.value))}>
        <option value="default">Windows default</option>
        {draft !== null && !allowed.options.some((option) => option.value === draft)
          && <option value={String(draft)} disabled>Unlisted value ({draft})</option>}
        {allowed.options.map((option) => <option key={option.value} value={String(option.value)}>{option.label}</option>)}
      </select>
      : <>
        <input type="number" aria-label={label} disabled={disabled} min={allowed.min} max={allowed.max} step={1}
          placeholder="Windows default" value={text}
          onChange={(event) => {
            const raw = event.target.value;
            setText(raw);
            if (raw.trim() === "") { setInvalid(false); onChange(null); return; }
            const value = Number(raw);
            // An invalid entry must not leave an earlier valid draft queued for review.
            if (!Number.isInteger(value) || value < allowed.min || value > allowed.max) { setInvalid(true); onChange(undefined); return; }
            setInvalid(false);
            onChange(value);
          }} />
        <p>Leave empty for Windows default. Allowed: {allowed.min}–{allowed.max} days.</p>
        {invalid && <p role="alert">Enter a whole number from {allowed.min} to {allowed.max}.</p>}
      </>}
  </li>;
}
