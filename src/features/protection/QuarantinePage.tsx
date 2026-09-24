import { useCallback, useEffect, useState } from "react";
import { deleteQuarantined, listQuarantine, restoreQuarantined } from "./api";
import { errorMessage, isCancelled, isCollision } from "./labels";
import type { QuarantineEntry } from "./types";

export const COLLISION_HELP = "A file already exists at the original location, so nothing was restored or overwritten. Move or rename that file, then try again.";

export function QuarantinePage() {
  const [entries, setEntries] = useState<QuarantineEntry[] | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setEntries(await listQuarantine());
    } catch (reason) {
      setError(errorMessage(reason, "The quarantine could not be read."));
    }
  }, []);
  useEffect(() => { void refresh(); }, [refresh]);

  const act = async (entry: QuarantineEntry, action: "restore" | "delete") => {
    setBusyId(entry.id);
    setError(null);
    setMessage(null);
    try {
      if (action === "restore") setMessage(`Restored to ${await restoreQuarantined(entry.id)}`);
      else { await deleteQuarantined(entry.id); setMessage("Deleted permanently."); }
    } catch (reason) {
      if (isCollision(reason)) setError(COLLISION_HELP);
      else if (!isCancelled(reason)) setError(errorMessage(reason, "The action could not be completed."));
    } finally {
      setBusyId(null);
      await refresh();
    }
  };

  return <section aria-labelledby="quarantine-title">
    <h2 id="quarantine-title">Quarantine</h2>
    <p>Quarantined files are stored in a scrambled form so they cannot run. Restoring never overwrites an existing file. Each action asks for confirmation in a Windows dialog.</p>
    {error && <p role="alert">{error}</p>}
    {message && <p role="status">{message}</p>}
    {entries === null && !error && <p role="status">Loading…</p>}
    {entries?.length === 0 && <p>The quarantine is empty.</p>}
    {entries && entries.length > 0 && <ul className="protection-findings">{entries.map((entry) => <li key={entry.id}>
      <code>{entry.originalPath ?? "Unknown original location"}</code>
      {entry.damaged
        ? <p>This entry is damaged and cannot be restored. It can only be deleted.</p>
        : <p>{entry.finding} · {entry.size ?? 0} bytes · quarantined {entry.quarantinedAt ? new Date(entry.quarantinedAt * 1000).toLocaleString() : "at an unknown time"}</p>}
      <div className="protection-actions">
        {!entry.damaged && <button type="button" disabled={busyId !== null} onClick={() => void act(entry, "restore")}>Restore…</button>}
        <button type="button" disabled={busyId !== null} onClick={() => void act(entry, "delete")}>Delete permanently…</button>
      </div>
    </li>)}</ul>}
  </section>;
}
