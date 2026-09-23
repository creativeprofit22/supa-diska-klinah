import { useCallback, useEffect, useState } from "react";
import { SystemChangeJournal } from "../../shared/system-change/SystemChangeJournal";
import { SystemChangeReview } from "../../shared/system-change/SystemChangeReview";
import type { SystemChange } from "../../shared/system-change/types";
import { getRestoreProtection, listRestorePoints } from "./api";
import type { RestorePointKind, RestorePointList, RestoreProtection } from "./types";

export const DEFAULT_DESCRIPTION = "Supa Diska Klinah manual restore point";
/**
 * Matches the backend limit of 128 UTF-16 code units (`MAX_RESTORE_DESCRIPTION_UTF16` in cleanup-core and
 * `MAX_RESTORE_POINT_DESCRIPTION_UTF16` in windows-platform). JS `length` counts the same unit.
 */
export const MAX_DESCRIPTION = 128;

const kindLabel: Record<RestorePointKind, string> = {
  applicationInstall: "Application install",
  applicationUninstall: "Application uninstall",
  deviceDriverInstall: "Device driver install",
  modifySettings: "Settings change",
  cancelledOperation: "Cancelled operation",
  other: "Other",
};

// eslint-disable-next-line no-control-regex
const CONTROL = /[\u0000-\u001f\u007f-\u009f]/;
export function descriptionError(value: string): string | null {
  if (value.trim() === "") return "Enter a description.";
  if (value.length > MAX_DESCRIPTION) return `Use at most ${MAX_DESCRIPTION} characters.`;
  if (CONTROL.test(value)) return "Remove control characters (such as tabs or line breaks).";
  return null;
}

export function describe(change: SystemChange): string {
  return change.kind === "createRestorePoint" ? `Create restore point: ${change.description}` : change.kind;
}

function protectionText(protection: RestoreProtection): string[] {
  const lines: string[] = [];
  lines.push(protection.policyDisabled
    ? "System Restore is disabled by policy on this device; Windows will not create restore points."
    : "System Restore is not disabled by policy.");
  lines.push(protection.protectionEnabled === null
    ? "Windows has not recorded whether system drive protection is on."
    : `System drive protection is ${protection.protectionEnabled ? "on" : "off"}.`);
  lines.push(`Creation frequency: ${protection.creationFrequencyMinutes} minutes. Windows may skip a new restore point if one was created within that window.`);
  return lines;
}

export function RestorePage() {
  const [list, setList] = useState<RestorePointList | null>(null);
  const [protection, setProtection] = useState<RestoreProtection | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [description, setDescription] = useState(DEFAULT_DESCRIPTION);
  const [changes, setChanges] = useState<SystemChange[]>([]);
  const [journalKey, setJournalKey] = useState(0);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    const [points, prot] = await Promise.allSettled([listRestorePoints(), getRestoreProtection()]);
    if (points.status === "fulfilled") setList(points.value);
    if (prot.status === "fulfilled") setProtection(prot.value);
    if (points.status === "rejected" || prot.status === "rejected") {
      setError(points.status === "rejected" ? "Restore points could not be listed." : "Restore protection status could not be read.");
    }
    setLoading(false);
  }, []);
  useEffect(() => { void refresh(); }, [refresh]);

  const invalid = descriptionError(description);
  const onFinished = useCallback(() => { setChanges([]); setJournalKey((n) => n + 1); void refresh(); }, [refresh]);

  return <section aria-labelledby="restore-title">
    <header className="page-header"><div>
      <p className="eyebrow">System</p>
      <h1 id="restore-title">Restore points</h1>
      <p>Shows Windows System Restore status and existing points, and can ask Windows to create a new one; it cannot restore or delete points.</p>
    </div></header>
    <button type="button" disabled={loading} onClick={() => void refresh()}>Refresh</button>
    {loading && <p role="status">Loading restore points…</p>}
    {error && <p role="alert">{error}</p>}

    {protection && <section aria-label="Protection status">
      <h2>Protection</h2>
      {protectionText(protection).map((line) => <p key={line}>{line}</p>)}
    </section>}

    {list && <section aria-label="Existing restore points">
      <h2>Existing restore points</h2>
      {list.status === "requiresAdministrator" && <p>Listing restore points requires administrator permission. Run the app as administrator to see them.</p>}
      {list.status === "unavailable" && <p>System Restore is unavailable on this device, so restore points cannot be listed.</p>}
      {list.status === "available" && list.points.length === 0 && <p>No restore points found.</p>}
      {list.status === "available" && list.points.length > 0 && <ul>{list.points.map((point) => <li key={point.sequenceNumber}>
        <strong>{point.description}</strong>
        <p>{point.createdAt === null ? "Creation time unknown" : new Date(point.createdAt * 1000).toLocaleString()} · {kindLabel[point.kind]}</p>
      </li>)}</ul>}
    </section>}

    <section aria-label="Create a restore point">
      <h2>Create a restore point</h2>
      <label htmlFor="restore-description">Description</label>
      <input id="restore-description" type="text" value={description} maxLength={MAX_DESCRIPTION}
        aria-invalid={invalid !== null} aria-describedby={invalid ? "restore-description-error" : undefined}
        onChange={(event) => { setDescription(event.target.value); setChanges([]); }} />
      {invalid && <p id="restore-description-error" role="alert">{invalid}</p>}
      <button type="button" disabled={invalid !== null} onClick={() => setChanges([{ kind: "createRestorePoint", description }])}>
        Add restore point to review
      </button>
    </section>
    <SystemChangeReview changes={invalid ? [] : changes} describe={describe} onFinished={onFinished} />
    <SystemChangeJournal module="restore" describe={describe} refreshKey={journalKey} onFinished={onFinished} />
  </section>;
}
