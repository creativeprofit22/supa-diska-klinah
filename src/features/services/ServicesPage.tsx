import { useCallback, useEffect, useMemo, useState } from "react";
import { useStrings } from "../../shared/i18n/I18nProvider";
import { useSystemChangeLabels } from "../../shared/system-change/labels";
import { SystemChangeJournal } from "../../shared/system-change/SystemChangeJournal";
import { SystemChangeReview } from "../../shared/system-change/SystemChangeReview";
import { systemChangeError } from "../../shared/system-change/api";
import type { ServiceStartType, SystemChange } from "../../shared/system-change/types";
import { listServices } from "./api";
import { servicesStrings, type ServicesStrings } from "./strings";
import type { ServiceItem } from "./types";

const startTypes: ServiceStartType[] = ["automatic", "manual", "disabled"];

function lockedReason(item: ServiceItem, t: ServicesStrings): string | null {
  if (!item.installed) return t.lockedNotInstalled;
  if (item.start === null) return t.lockedUnreadable;
  if (item.start === "boot" || item.start === "system") return t.lockedBootSystem;
  return null;
}

function currentLabel(item: ServiceItem, t: ServicesStrings): string {
  if (item.start === null) return t.unknown;
  return `${t.start[item.start]}${item.start === "automatic" && item.delayedAutoStart ? t.delayed : ""}`;
}

function describeWith(items: ServiceItem[] | null, t: ServicesStrings) {
  return (change: SystemChange): string => {
    if (change.kind !== "setServiceStartType") return change.kind;
    const label = items?.find((item) => item.id === change.catalogId)?.label ?? change.catalogId;
    return t.describe(t.start[change.startType], label);
  };
}

export function ServicesPage() {
  const sc = useSystemChangeLabels();
  const t = useStrings(servicesStrings);
  const errors = sc.t.errors;
  const [items, setItems] = useState<ServiceItem[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [choices, setChoices] = useState<Record<string, ServiceStartType>>({});
  const [journalKey, setJournalKey] = useState(0);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      setItems(await listServices());
      setChoices({});
      setError(null);
    } catch (reason) {
      setError(systemChangeError(reason, errors));
    } finally {
      setLoading(false);
    }
  }, [errors]);
  useEffect(() => { void refresh(); }, [refresh]);

  const changes = useMemo<SystemChange[]>(() => (items ?? []).flatMap((item) => {
    const startType = choices[item.id];
    if (!startType || lockedReason(item, t) || startType === item.start) return [];
    return [{ kind: "setServiceStartType", catalogId: item.id, startType }];
  }), [items, choices, t]);

  const choose = (id: string, value: string) => setChoices((current) => {
    const next = { ...current };
    const picked = startTypes.find((type) => type === value);
    if (picked) next[id] = picked; else delete next[id];
    return next;
  });
  const describe = useMemo(() => describeWith(items, t), [items, t]);
  const onFinished = useCallback(() => { setJournalKey((n) => n + 1); void refresh(); }, [refresh]);

  return <section aria-labelledby="services-title">
    <header className="page-header">
      <div>
        <p className="eyebrow">{t.eyebrow}</p>
        <h1 id="services-title">{t.title}</h1>
        <p>{t.intro}</p>
      </div>
    </header>
    <button type="button" disabled={loading} onClick={() => void refresh()}>{t.refresh}</button>
    {loading && <p role="status">{t.loading}</p>}
    {error && <p role="alert">{error}</p>}
    {items?.length === 0 && <p>{t.none}</p>}
    {!!items?.length && <ul className="services-list">
      {items.map((item) => {
        const locked = lockedReason(item, t);
        const selectId = `service-start-${item.id}`;
        return <li key={item.id}>
          <strong>{item.label}</strong> <span>({item.serviceName})</span>
          <p>{item.description}</p>
          <p>{sc.risk[item.risk]} · {t.category[item.category]}{t.recommended(t.start[item.recommended])}{t.current(item.installed ? currentLabel(item, t) : t.notInstalled)}{item.installed && item.start !== null ? (item.running ? t.running : t.notRunning) : ""}</p>
          <label htmlFor={selectId}>{t.startTypeFor(item.label)}</label>{" "}
          <select id={selectId} disabled={locked !== null} value={choices[item.id] ?? ""} onChange={(event) => choose(item.id, event.target.value)}>
            <option value="">{t.keepCurrent}</option>
            {startTypes.map((type) => <option key={type} value={type}>{t.start[type]}</option>)}
          </select>
          {locked && <p>{locked}</p>}
        </li>;
      })}
    </ul>}
    <SystemChangeReview changes={changes} describe={describe} onFinished={onFinished} />
    <SystemChangeJournal module="services" describe={describe} refreshKey={journalKey} onFinished={onFinished} />
  </section>;
}
