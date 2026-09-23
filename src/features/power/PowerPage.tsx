import { useCallback, useEffect, useMemo, useState } from "react";
import { formatBytes } from "../../shared/format";
import { systemChangeError } from "../../shared/system-change/api";
import { SystemChangeJournal } from "../../shared/system-change/SystemChangeJournal";
import { SystemChangeReview } from "../../shared/system-change/SystemChangeReview";
import type { SystemChange } from "../../shared/system-change/types";
import { getPowerStatus } from "./api";
import type { PowerStatus } from "./types";

export function PowerPage() {
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
      setError(systemChangeError(reason));
    } finally {
      setLoading(false);
    }
  }, []);

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
      case "setHibernation": return `Turn hibernation ${change.enabled ? "on" : "off"}`;
      case "setActivePowerScheme": {
        const name = status?.schemes.find((entry) => entry.id === change.scheme)?.name ?? change.scheme;
        return `Switch power plan: ${name}`;
      }
      default: return change.kind;
    }
  }, [status]);

  const onFinished = useCallback(() => { setJournalKey((n) => n + 1); void refresh(); }, [refresh]);
  const checkedScheme = scheme ?? activeId;

  return <section className="power-page" aria-labelledby="power-title">
    <header className="page-header"><div>
      <p className="eyebrow">System</p>
      <h1 id="power-title">Power and hibernation</h1>
      <p>See the hibernation state and the active power plan; changes only happen after you select them and confirm in Windows.</p>
    </div></header>
    <button type="button" disabled={loading} onClick={() => void refresh()}>Refresh</button>
    {loading && <p role="status">Reading power settings…</p>}
    {error && <p role="alert">{error}</p>}
    {status && hibernation && <>
      <section aria-labelledby="power-hibernation">
        <h2 id="power-hibernation">Hibernation</h2>
        {hibernation.supported ? <>
          <p>
            Hibernation is {hibernation.enabled ? "on" : "off"}.
            {hibernation.hiberfileBytes !== null && ` The hibernation file uses ${formatBytes(hibernation.hiberfileBytes)}.`}
          </p>
          <p>Turning hibernation off also disables Fast Startup and removes the hibernation file.</p>
          <label>
            <input type="checkbox" checked={toggleHibernation} onChange={() => setToggleHibernation((value) => !value)} />
            <span>Turn hibernation {hibernation.enabled ? "off" : "on"}</span>
          </label>
        </> : <p>Hibernation is not supported on this device (the firmware or Windows does not offer it), so it cannot be changed here.</p>}
      </section>
      <fieldset>
        <legend>Power plans</legend>
        {status.schemes.length === 0 && <p>No power plans were reported.</p>}
        {status.schemes.map((entry) => <label key={entry.id}>
          <input type="radio" name="power-scheme" value={entry.id} checked={checkedScheme === entry.id} onChange={() => setScheme(entry.id)} />
          <span>{entry.name}{entry.active ? " (active)" : ""}</span>
        </label>)}
      </fieldset>
    </>}
    <SystemChangeReview changes={changes} describe={describe} onFinished={onFinished} />
    <SystemChangeJournal module="power" describe={describe} refreshKey={journalKey} onFinished={onFinished} />
  </section>;
}
