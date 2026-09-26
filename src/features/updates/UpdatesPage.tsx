import { useCallback, useEffect, useMemo, useState } from "react";
import { useFormat, useStrings } from "../../shared/i18n/I18nProvider";
import { SystemChangeJournal } from "../../shared/system-change/SystemChangeJournal";
import { SystemChangeReview } from "../../shared/system-change/SystemChangeReview";
import { useSystemChangeLabels } from "../../shared/system-change/labels";
import type { SystemChange } from "../../shared/system-change/types";
import { detectWindowsUpdates, getWindowsUpdateStatus } from "./api";
import { updatesStrings, type UpdatesStrings } from "./strings";
import type { PolicyStatus, UpdateStatus } from "./types";

function errorText(reason: unknown, fallback: string): string {
  const message = (reason as { message?: unknown } | null)?.message;
  return typeof message === "string" && message.length > 0 && message.length <= 300 ? message : fallback;
}

const yesNo = (value: boolean | null, t: UpdatesStrings) => value === null ? t.unknown : value ? t.yes : t.no;

function valueLabel(policy: PolicyStatus, value: number | null, t: UpdatesStrings): string {
  if (value === null) return t.windowsDefault;
  if (policy.allowed.kind === "options") return policy.allowed.options.find((option) => option.value === value)?.label ?? String(value);
  return t.days(value);
}

/** Drafted target value per policy id; absent means "keep current". */
type Drafts = Record<string, number | null>;

export function UpdatesPage() {
  const sc = useSystemChangeLabels();
  const t = useStrings(updatesStrings);
  const fmt = useFormat();
  const dateText = (seconds: number | null) => (seconds === null ? null : fmt.dateTime(seconds)) ?? t.unknown;
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
      setError(errorText(reason, t.loadError));
    } finally {
      setLoading(false);
    }
  }, [t]);
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
      setError(errorText(reason, t.detectError));
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
      ? t.describePolicy(policy.label, valueLabel(policy, change.value, t))
      : t.describePolicyId(change.settingId);
  }, [policies, t]);

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
      <p className="eyebrow">{t.eyebrow}</p>
      <h1 id="updates-title">{t.title}</h1>
      <p>{t.intro}</p>
    </div></header>
    <div className="updates-actions">
      <button type="button" disabled={loading} onClick={() => void refresh()}>{t.refresh}</button>
      <button type="button" disabled={detecting || loading} onClick={() => void detect()}>{t.checkNow}</button>
    </div>
    {loading && <p role="status">{t.loading}</p>}
    {detecting && <p role="status">{t.detecting}</p>}
    {detectStarted && <p role="status">{t.detectStarted}</p>}
    {error && <p role="alert">{error}</p>}
    {status && <>
      <dl className="updates-status">
        <dt>{t.apiAvailable}</dt><dd>{status.apiAvailable ? t.yes : t.no}</dd>
        <dt>{t.serviceEnabled}</dt><dd>{yesNo(status.serviceEnabled, t)}</dd>
        <dt>{t.lastSearch}</dt><dd>{dateText(status.lastSearchSuccess)}</dd>
        <dt>{t.lastInstall}</dt><dd>{dateText(status.lastInstallSuccess)}</dd>
        <dt>{t.rebootRequired}</dt><dd>{yesNo(status.rebootRequired, t)}</dd>
        <dt>{t.editionLabel}</dt><dd>{t.edition[status.edition]}</dd>
        <dt>{t.managed}</dt><dd>{status.managed ? t.yes : t.no}</dd>
      </dl>
      <h2>{t.policiesTitle}</h2>
      {locked && <p className="updates-unsupported">
        {t.policiesUnavailable(status.unsupportedReason ? sc.unsupported[status.unsupportedReason] : sc.t.notSupportedOnDevice)}
      </p>}
      <ul className="updates-policies">
        {status.policies.map((policy) => <PolicyControl key={`${revision}:${policy.id}`} t={t} policy={policy} disabled={locked}
          draft={policy.id in drafts ? drafts[policy.id] : policy.current} onChange={(value) => setDraft(policy.id, value)} />)}
      </ul>
      <SystemChangeReview changes={changes} describe={describe} onFinished={onFinished} />
    </>}
    <SystemChangeJournal module="updates" describe={describe} refreshKey={journalKey} onFinished={onFinished} />
  </section>;
}

function PolicyControl({ t, policy, draft, disabled, onChange }: {
  t: UpdatesStrings; policy: PolicyStatus; draft: number | null; disabled: boolean; onChange: (value: number | null | undefined) => void;
}) {
  const { allowed } = policy;
  const [text, setText] = useState(draft === null ? "" : String(draft));
  const [invalid, setInvalid] = useState(false);
  const label = t.policyValueFor(policy.label);
  const currentText = `${t.current(valueLabel(policy, policy.current, t))}${policy.current !== null && !policy.applied ? t.notApplied : ""}`;
  return <li>
    <strong>{policy.label}</strong>
    <p>{policy.description}</p>
    <p>{currentText}</p>
    {allowed.kind === "options"
      ? <select aria-label={label} disabled={disabled} value={draft === null ? "default" : String(draft)}
        onChange={(event) => onChange(event.target.value === "default" ? null : Number(event.target.value))}>
        <option value="default">{t.windowsDefault}</option>
        {draft !== null && !allowed.options.some((option) => option.value === draft)
          && <option value={String(draft)} disabled>{t.unlisted(draft)}</option>}
        {allowed.options.map((option) => <option key={option.value} value={String(option.value)}>{option.label}</option>)}
      </select>
      : <>
        <input type="number" aria-label={label} disabled={disabled} min={allowed.min} max={allowed.max} step={1}
          placeholder={t.windowsDefault} value={text}
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
        <p>{t.rangeHint(allowed.min, allowed.max)}</p>
        {invalid && <p role="alert">{t.rangeError(allowed.min, allowed.max)}</p>}
      </>}
  </li>;
}
