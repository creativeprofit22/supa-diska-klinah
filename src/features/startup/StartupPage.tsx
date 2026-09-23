import { useCallback, useEffect, useMemo, useState } from "react";
import { SystemChangeJournal } from "../../shared/system-change/SystemChangeJournal";
import { SystemChangeReview } from "../../shared/system-change/SystemChangeReview";
import { systemChangeError } from "../../shared/system-change/api";
import type { SystemChange } from "../../shared/system-change/types";
import { listStartupItems } from "./api";
import type { StartupItem, StartupSource } from "./types";

const sourceLabel: Record<StartupSource, string> = {
  run: "Run key", run32: "32-bit Run key", startupFolder: "Startup folder", runOnce: "RunOnce key", logonTask: "Scheduled task (at sign-in)",
};

function readOnlyReason(item: StartupItem): string {
  if (item.source === "runOnce") return "Read-only: RunOnce entries run a single time and Windows removes them afterwards, so they cannot be turned on or off here.";
  if (item.source === "logonTask") return "Read-only: this is a scheduled task with a sign-in trigger. Manage it in Task Scheduler.";
  return "Read-only: this entry cannot be turned on or off here.";
}

const itemKey = (item: StartupItem) => `${item.scope}|${item.source}|${item.location ?? ""}|${item.name}`;

function describe(change: SystemChange): string {
  if (change.kind === "setStartupEntry") return `${change.enabled ? "Enable" : "Disable"} startup item: ${change.entry.name}`;
  return change.kind;
}

export function StartupPage() {
  const [items, setItems] = useState<StartupItem[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<string[]>([]);
  const [journalKey, setJournalKey] = useState(0);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      setItems(await listStartupItems());
      setSelected([]);
      setError(null);
    } catch (reason) {
      setError(systemChangeError(reason));
    } finally {
      setLoading(false);
    }
  }, []);
  useEffect(() => { void refresh(); }, [refresh]);

  const changes = useMemo<SystemChange[]>(() => (items ?? []).flatMap((item) => {
    if (!item.toggleable || item.location === null || !selected.includes(itemKey(item))) return [];
    return [{ kind: "setStartupEntry", entry: { scope: item.scope, location: item.location, name: item.name }, enabled: !item.enabled }];
  }), [items, selected]);

  const toggle = (key: string) => setSelected((current) => current.includes(key) ? current.filter((value) => value !== key) : [...current, key]);
  const onFinished = useCallback(() => { setJournalKey((n) => n + 1); void refresh(); }, [refresh]);

  return <section aria-labelledby="startup-title">
    <header className="page-header">
      <div>
        <p className="eyebrow">System</p>
        <h1 id="startup-title">Startup apps</h1>
        <p>Choose which programs start when you sign in; turning one off only marks it disabled, like Task Manager does.</p>
      </div>
    </header>
    <p>Deleting startup entries is not offered. Turning an item off keeps it listed here so you can turn it back on later.</p>
    <button type="button" disabled={loading} onClick={() => void refresh()}>Refresh</button>
    {loading && <p role="status">Loading startup items…</p>}
    {error && <p role="alert">{error}</p>}
    {items?.length === 0 && <p>No startup items were found.</p>}
    {!!items?.length && <ul className="startup-list">
      {items.map((item) => {
        const key = itemKey(item);
        const scope = item.scope === "user" ? "Your account" : "All users";
        return <li key={key}>
          <strong>{item.name}</strong>
          <p>{scope} · {sourceLabel[item.source]} · Currently {item.enabled ? "enabled" : "disabled"}</p>
          <p><code>{item.command}</code></p>
          {item.toggleable && item.location !== null
            ? <label>
                <input type="checkbox" aria-label={`Change ${item.name} to ${item.enabled ? "disabled" : "enabled"}`} checked={selected.includes(key)} onChange={() => toggle(key)} />
                {" "}Change to {item.enabled ? "disabled" : "enabled"}
              </label>
            : <p>{readOnlyReason(item)}</p>}
        </li>;
      })}
    </ul>}
    <SystemChangeReview changes={changes} describe={describe} onFinished={onFinished} />
    <SystemChangeJournal module="startup" describe={describe} refreshKey={journalKey} onFinished={onFinished} />
  </section>;
}
