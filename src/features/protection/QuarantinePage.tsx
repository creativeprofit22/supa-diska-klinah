import { useCallback, useEffect, useState } from "react";
import { useFormat, useStrings } from "../../shared/i18n/I18nProvider";
import { deleteQuarantined, listQuarantine, restoreQuarantined } from "./api";
import { errorMessage, isCancelled, isCollision } from "./labels";
import { protectionStrings } from "./strings";
import type { QuarantineEntry } from "./types";

/** English collision help; the page shows the active locale's `quarantine.collision`. */
export const COLLISION_HELP = protectionStrings.en.quarantine.collision;

export function QuarantinePage() {
  const strings = useStrings(protectionStrings);
  const t = strings.quarantine;
  const fmt = useFormat();
  const [entries, setEntries] = useState<QuarantineEntry[] | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setEntries(await listQuarantine());
    } catch (reason) {
      setError(errorMessage(reason, t.loadFailed));
    }
  }, [t]);
  useEffect(() => { void refresh(); }, [refresh]);

  const act = async (entry: QuarantineEntry, action: "restore" | "delete") => {
    setBusyId(entry.id);
    setError(null);
    setMessage(null);
    try {
      if (action === "restore") setMessage(t.restored(await restoreQuarantined(entry.id)));
      else { await deleteQuarantined(entry.id); setMessage(t.deleted); }
    } catch (reason) {
      if (isCollision(reason)) setError(t.collision);
      else if (!isCancelled(reason)) setError(errorMessage(reason, strings.errors.actionFailed));
    } finally {
      setBusyId(null);
      await refresh();
    }
  };

  return <section aria-labelledby="quarantine-title">
    <h2 id="quarantine-title">{t.title}</h2>
    <p>{t.intro}</p>
    {error && <p role="alert">{error}</p>}
    {message && <p role="status">{message}</p>}
    {entries === null && !error && <p role="status">{strings.loading}</p>}
    {entries?.length === 0 && <p>{t.empty}</p>}
    {entries && entries.length > 0 && <ul className="protection-findings">{entries.map((entry) => <li key={entry.id}>
      <code>{entry.originalPath ?? t.unknownLocation}</code>
      {entry.damaged
        ? <p>{t.damaged}</p>
        : <p>{entry.finding} · {t.sizeBytes(fmt.number(entry.size ?? 0))} · {t.quarantinedAt((entry.quarantinedAt ? fmt.dateTime(entry.quarantinedAt) : null) ?? t.unknownTime)}</p>}
      <div className="protection-actions">
        {!entry.damaged && <button type="button" disabled={busyId !== null} onClick={() => void act(entry, "restore")}>{t.restore}</button>}
        <button type="button" disabled={busyId !== null} onClick={() => void act(entry, "delete")}>{t.delete}</button>
      </div>
    </li>)}</ul>}
  </section>;
}
