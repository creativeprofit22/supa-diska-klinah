import { useCallback, useEffect, useMemo, useState } from "react";
import { useFormat, useStrings } from "../../shared/i18n/I18nProvider";
import { systemChangeError } from "../../shared/system-change/api";
import { systemChangeStrings } from "../../shared/system-change/strings";
import { SystemChangeJournal } from "../../shared/system-change/SystemChangeJournal";
import { SystemChangeReview } from "../../shared/system-change/SystemChangeReview";
import type { SystemChange } from "../../shared/system-change/types";
import { getPowerStatus } from "./api";
import { powerStrings } from "./strings";
import type { PowerStatus } from "./types";

export function PowerPage() {
  const t = useStrings(powerStrings);
  const errors = useStrings(systemChangeStrings).errors;
  const fmt = useFormat();
  const [status, setStatus] = useState<PowerStatus | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [toggleHibernation, setToggleHibernation] = useState(false);
  const [scheme, setScheme] = useState<string | null>(null);
  const [journalKey, setJournalKey] = useState(0);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setStatus(await getPowerStatus());
      setToggleHibernation(false);
      setScheme(null);
    } catch (reason) {
      setError(systemChangeError(reason, errors));
    } finally {
      setLoading(false);
    }
  }, [errors]);

  useEffect(() => { void refresh(); }, [refresh]);

  const activeId = status?.schemes.find((entry) => entry.active)?.id ?? null;
  const hibernation = status?.hibernation;

  const changes = useMemo(() => {
    const list: SystemChange[] = [];
    if (toggleHibernation && hibernation?.supported) list.push({ kind: "setHibernation", enabled: !hibernation.enabled });
    if (scheme !== null && scheme !== activeId) list.push({ kind: "setActivePowerScheme", scheme });
    return list;
  }, [activeId, hibernation, scheme, toggleHibernation]);

  const describe = useCallback((change: SystemChange): string => {
    switch (change.kind) {
      case "setHibernation": return t.describeHibernation(change.enabled);
      case "setActivePowerScheme": {
        const name = status?.schemes.find((entry) => entry.id === change.scheme)?.name ?? change.scheme;
        return t.describeScheme(name);
      }
      default: return change.kind;
    }
  }, [status, t]);

  const onFinished = useCallback(() => { setJournalKey((n) => n + 1); void refresh(); }, [refresh]);
  const checkedScheme = scheme ?? activeId;

  return <section className="power-page" aria-labelledby="power-title">
    <header className="page-header"><div>
      <p className="eyebrow">{t.eyebrow}</p>
      <h1 id="power-title">{t.title}</h1>
      <p>{t.intro}</p>
    </div></header>
    <button type="button" disabled={loading} onClick={() => void refresh()}>{t.refresh}</button>
    {loading && <p role="status">{t.loading}</p>}
    {error && <p role="alert">{error}</p>}
    {status && hibernation && <>
      <section aria-labelledby="power-hibernation">
        <h2 id="power-hibernation">{t.hibernationTitle}</h2>
        {hibernation.supported ? <>
          <p>
            {t.hibernationState(hibernation.enabled)}
            {hibernation.hiberfileBytes !== null && t.hiberfile(fmt.bytes(hibernation.hiberfileBytes))}
          </p>
          <p>{t.offWarning}</p>
          <label>
            <input type="checkbox" checked={toggleHibernation} onChange={() => setToggleHibernation((value) => !value)} />
            <span>{t.toggleHibernation(!hibernation.enabled)}</span>
          </label>
        </> : <p>{t.unsupported}</p>}
      </section>
      <fieldset>
        <legend>{t.plansTitle}</legend>
        {status.schemes.length === 0 && <p>{t.noPlans}</p>}
        {status.schemes.map((entry) => <label key={entry.id}>
          <input type="radio" name="power-scheme" value={entry.id} checked={checkedScheme === entry.id} onChange={() => setScheme(entry.id)} />
          <span>{entry.name}{entry.active ? t.active : ""}</span>
        </label>)}
      </fieldset>
    </>}
    <SystemChangeReview changes={changes} describe={describe} onFinished={onFinished} />
    <SystemChangeJournal module="power" describe={describe} refreshKey={journalKey} onFinished={onFinished} />
  </section>;
}
