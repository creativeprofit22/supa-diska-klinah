import { useCallback, useEffect, useMemo, useState } from "react";
import { useStrings } from "../../shared/i18n/I18nProvider";
import { SystemChangeJournal } from "../../shared/system-change/SystemChangeJournal";
import { SystemChangeReview } from "../../shared/system-change/SystemChangeReview";
import { systemChangeError } from "../../shared/system-change/api";
import { systemChangeStrings } from "../../shared/system-change/strings";
import type { SystemChange } from "../../shared/system-change/types";
import { listStartupItems } from "./api";
import { startupStrings, type StartupStrings } from "./strings";
import type { StartupItem } from "./types";

function readOnlyReason(item: StartupItem, t: StartupStrings): string {
  if (item.source === "runOnce") return t.readOnlyRunOnce;
  if (item.source === "logonTask") return t.readOnlyLogonTask;
  return t.readOnly;
}

const itemKey = (item: StartupItem) => `${item.scope}|${item.source}|${item.location ?? ""}|${item.name}`;

function describe(change: SystemChange, t: StartupStrings): string {
  if (change.kind === "setStartupEntry") return t.describe(change.enabled, change.entry.name);
  return change.kind;
}

export function StartupPage() {
  const t = useStrings(startupStrings);
  const errors = useStrings(systemChangeStrings).errors;
  const describeChange = useCallback((change: SystemChange) => describe(change, t), [t]);
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
      setError(systemChangeError(reason, errors));
    } finally {
      setLoading(false);
    }
  }, [errors]);
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
        <p className="eyebrow">{t.eyebrow}</p>
        <h1 id="startup-title">{t.title}</h1>
        <p>{t.intro}</p>
      </div>
    </header>
    <p>{t.noDelete}</p>
    <button type="button" disabled={loading} onClick={() => void refresh()}>{t.refresh}</button>
    {loading && <p role="status">{t.loading}</p>}
    {error && <p role="alert">{error}</p>}
    {items?.length === 0 && <p>{t.none}</p>}
    {!!items?.length && <ul className="startup-list">
      {items.map((item) => {
        const key = itemKey(item);
        const scope = item.scope === "user" ? t.yourAccount : t.allUsers;
        return <li key={key}>
          <strong>{item.name}</strong>
          <p>{scope} · {t.source[item.source]}{t.currently(item.enabled)}</p>
          <p><code>{item.command}</code></p>
          {item.toggleable && item.location !== null
            ? <label>
                <input type="checkbox" aria-label={t.changeItem(item.name, !item.enabled)} checked={selected.includes(key)} onChange={() => toggle(key)} />
                {" "}{t.changeTo(!item.enabled)}
              </label>
            : <p>{readOnlyReason(item, t)}</p>}
        </li>;
      })}
    </ul>}
    <SystemChangeReview changes={changes} describe={describeChange} onFinished={onFinished} />
    <SystemChangeJournal module="startup" describe={describeChange} refreshKey={journalKey} onFinished={onFinished} />
  </section>;
}
