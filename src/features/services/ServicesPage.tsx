import { useCallback, useEffect, useMemo, useState } from "react";
import { SystemChangeJournal } from "../../shared/system-change/SystemChangeJournal";
import { SystemChangeReview } from "../../shared/system-change/SystemChangeReview";
import { systemChangeError } from "../../shared/system-change/api";
import type { RiskLevel, ServiceStartState, ServiceStartType, SystemChange } from "../../shared/system-change/types";
import { listServices } from "./api";
import type { ServiceCategory, ServiceItem } from "./types";

const startLabel: Record<ServiceStartState, string> = {
  boot: "Boot", system: "System", automatic: "Automatic", manual: "Manual", disabled: "Disabled",
};
const riskLabel: Record<RiskLevel, string> = { low: "Low risk", medium: "Medium risk", high: "High risk" };
const categoryLabel: Record<ServiceCategory, string> = {
  telemetry: "Telemetry", gaming: "Gaming", legacy: "Legacy", performance: "Performance", other: "Other",
};
const startTypes: ServiceStartType[] = ["automatic", "manual", "disabled"];

function lockedReason(item: ServiceItem): string | null {
  if (!item.installed) return "Not installed on this PC, so it cannot be changed.";
  if (item.start === null) return "Its current configuration could not be read, so it cannot be changed.";
  if (item.start === "boot" || item.start === "system") return "It is a boot or system driver start type, which this app never changes.";
  return null;
}

function currentLabel(item: ServiceItem): string {
  if (item.start === null) return "Unknown";
  return `${startLabel[item.start]}${item.start === "automatic" && item.delayedAutoStart ? " (delayed)" : ""}`;
}

function describeWith(items: ServiceItem[] | null) {
  return (change: SystemChange): string => {
    if (change.kind !== "setServiceStartType") return change.kind;
    const label = items?.find((item) => item.id === change.catalogId)?.label ?? change.catalogId;
    return `Set service start type to ${startLabel[change.startType]}: ${label}`;
  };
}

export function ServicesPage() {
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
      setError(systemChangeError(reason));
    } finally {
      setLoading(false);
    }
  }, []);
  useEffect(() => { void refresh(); }, [refresh]);

  const changes = useMemo<SystemChange[]>(() => (items ?? []).flatMap((item) => {
    const startType = choices[item.id];
    if (!startType || lockedReason(item) || startType === item.start) return [];
    return [{ kind: "setServiceStartType", catalogId: item.id, startType }];
  }), [items, choices]);

  const choose = (id: string, value: string) => setChoices((current) => {
    const next = { ...current };
    const picked = startTypes.find((type) => type === value);
    if (picked) next[id] = picked; else delete next[id];
    return next;
  });
  const describe = useMemo(() => describeWith(items), [items]);
  const onFinished = useCallback(() => { setJournalKey((n) => n + 1); void refresh(); }, [refresh]);

  return <section aria-labelledby="services-title">
    <header className="page-header">
      <div>
        <p className="eyebrow">System</p>
        <h1 id="services-title">Windows services</h1>
        <p>Change how selected Windows services start; running services are not stopped or started.</p>
      </div>
    </header>
    <button type="button" disabled={loading} onClick={() => void refresh()}>Refresh</button>
    {loading && <p role="status">Loading services…</p>}
    {error && <p role="alert">{error}</p>}
    {items?.length === 0 && <p>No services are listed.</p>}
    {!!items?.length && <ul className="services-list">
      {items.map((item) => {
        const locked = lockedReason(item);
        const selectId = `service-start-${item.id}`;
        return <li key={item.id}>
          <strong>{item.label}</strong> <span>({item.serviceName})</span>
          <p>{item.description}</p>
          <p>{riskLabel[item.risk]} · {categoryLabel[item.category]} · Recommended: {startLabel[item.recommended]} · Current: {item.installed ? currentLabel(item) : "Not installed"}{item.installed && item.start !== null ? ` · ${item.running ? "Running" : "Not running"}` : ""}</p>
          <label htmlFor={selectId}>Start type for {item.label}</label>{" "}
          <select id={selectId} disabled={locked !== null} value={choices[item.id] ?? ""} onChange={(event) => choose(item.id, event.target.value)}>
            <option value="">Keep current</option>
            {startTypes.map((type) => <option key={type} value={type}>{startLabel[type]}</option>)}
          </select>
          {locked && <p>{locked}</p>}
        </li>;
      })}
    </ul>}
    <SystemChangeReview changes={changes} describe={describe} onFinished={onFinished} />
    <SystemChangeJournal module="services" describe={describe} refreshKey={journalKey} onFinished={onFinished} />
  </section>;
}
