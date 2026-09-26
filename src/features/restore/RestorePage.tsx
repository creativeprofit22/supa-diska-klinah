import { useCallback, useEffect, useState } from "react";
import { useFormat, useStrings } from "../../shared/i18n/I18nProvider";
import { SystemChangeJournal } from "../../shared/system-change/SystemChangeJournal";
import { SystemChangeReview } from "../../shared/system-change/SystemChangeReview";
import type { SystemChange } from "../../shared/system-change/types";
import { getRestoreProtection, listRestorePoints } from "./api";
import { restoreStrings, type RestoreStrings } from "./strings";
import type { RestorePointList, RestoreProtection } from "./types";

export const DEFAULT_DESCRIPTION = restoreStrings.en.defaultDescription;
/**
 * Matches the backend limit of 128 UTF-16 code units (`MAX_RESTORE_DESCRIPTION_UTF16` in cleanup-core and
 * `MAX_RESTORE_POINT_DESCRIPTION_UTF16` in windows-platform). JS `length` counts the same unit.
 */
export const MAX_DESCRIPTION = 128;

// eslint-disable-next-line no-control-regex
const CONTROL = /[\u0000-\u001f\u007f-\u009f]/;
export function descriptionError(value: string, t: RestoreStrings = restoreStrings.en): string | null {
  if (value.trim() === "") return t.enterDescription;
  if (value.length > MAX_DESCRIPTION) return t.tooLong(MAX_DESCRIPTION);
  if (CONTROL.test(value)) return t.controlChars;
  return null;
}

export function describe(change: SystemChange, t: RestoreStrings = restoreStrings.en): string {
  return change.kind === "createRestorePoint" ? t.describe(change.description) : change.kind;
}

function protectionText(protection: RestoreProtection, t: RestoreStrings): string[] {
  const lines: string[] = [];
  lines.push(protection.policyDisabled ? t.policyDisabled : t.policyNotDisabled);
  lines.push(protection.protectionEnabled === null ? t.protectionUnknown : t.protectionState(protection.protectionEnabled));
  lines.push(t.frequency(protection.creationFrequencyMinutes));
  return lines;
}

export function RestorePage() {
  const t = useStrings(restoreStrings);
  const fmt = useFormat();
  const describeChange = useCallback((change: SystemChange) => describe(change, t), [t]);
  const [list, setList] = useState<RestorePointList | null>(null);
  const [protection, setProtection] = useState<RestoreProtection | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [description, setDescription] = useState(() => t.defaultDescription);
  const [changes, setChanges] = useState<SystemChange[]>([]);
  const [journalKey, setJournalKey] = useState(0);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    const [points, prot] = await Promise.allSettled([listRestorePoints(), getRestoreProtection()]);
    if (points.status === "fulfilled") setList(points.value);
    if (prot.status === "fulfilled") setProtection(prot.value);
    if (points.status === "rejected" || prot.status === "rejected") {
      setError(points.status === "rejected" ? t.listError : t.protectionError);
    }
    setLoading(false);
  }, [t]);
  useEffect(() => { void refresh(); }, [refresh]);

  const invalid = descriptionError(description, t);
  const onFinished = useCallback(() => { setChanges([]); setJournalKey((n) => n + 1); void refresh(); }, [refresh]);

  return <section aria-labelledby="restore-title">
    <header className="page-header"><div>
      <p className="eyebrow">{t.eyebrow}</p>
      <h1 id="restore-title">{t.title}</h1>
      <p>{t.intro}</p>
    </div></header>
    <button type="button" disabled={loading} onClick={() => void refresh()}>{t.refresh}</button>
    {loading && <p role="status">{t.loading}</p>}
    {error && <p role="alert">{error}</p>}

    {protection && <section aria-label={t.protectionLabel}>
      <h2>{t.protectionTitle}</h2>
      {protectionText(protection, t).map((line) => <p key={line}>{line}</p>)}
    </section>}

    {list && <section aria-label={t.existingLabel}>
      <h2>{t.existingLabel}</h2>
      {list.status === "requiresAdministrator" && <p>{t.requiresAdministrator}</p>}
      {list.status === "unavailable" && <p>{t.unavailable}</p>}
      {list.status === "available" && list.points.length === 0 && <p>{t.noPoints}</p>}
      {list.status === "available" && list.points.length > 0 && <ul>{list.points.map((point) => <li key={point.sequenceNumber}>
        <strong>{point.description}</strong>
        <p>{(point.createdAt === null ? null : fmt.dateTime(point.createdAt)) ?? t.creationUnknown} · {t.kind[point.kind]}</p>
      </li>)}</ul>}
    </section>}

    <section aria-label={t.createLabel}>
      <h2>{t.createLabel}</h2>
      <label htmlFor="restore-description">{t.description}</label>
      <input id="restore-description" type="text" value={description} maxLength={MAX_DESCRIPTION}
        aria-invalid={invalid !== null} aria-describedby={invalid ? "restore-description-error" : undefined}
        onChange={(event) => { setDescription(event.target.value); setChanges([]); }} />
      {invalid && <p id="restore-description-error" role="alert">{invalid}</p>}
      <button type="button" disabled={invalid !== null} onClick={() => setChanges([{ kind: "createRestorePoint", description }])}>
        {t.addToReview}
      </button>
    </section>
    <SystemChangeReview changes={invalid ? [] : changes} describe={describeChange} onFinished={onFinished} />
    <SystemChangeJournal module="restore" describe={describeChange} refreshKey={journalKey} onFinished={onFinished} />
  </section>;
}
