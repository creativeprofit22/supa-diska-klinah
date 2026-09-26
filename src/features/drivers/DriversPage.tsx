import { useCallback, useEffect, useMemo, useState } from "react";
import { useStrings } from "../../shared/i18n/I18nProvider";
import { SystemChangeJournal } from "../../shared/system-change/SystemChangeJournal";
import { SystemChangeReview } from "../../shared/system-change/SystemChangeReview";
import type { SystemChange } from "../../shared/system-change/types";
import { listDriverPackages } from "./api";
import { driversStrings, type DriversStrings } from "./strings";
import type { DriverPackage } from "./types";

export const RESTORE_DESCRIPTION = driversStrings.en.restoreDescription;

function reasonNotDeletable(pkg: DriverPackage, t: DriversStrings): string {
  if (pkg.status === "inUse") return t.notDeletableInUse;
  if (pkg.status === "current") return t.notDeletableCurrent;
  return t.notDeletable;
}

export function describe(change: SystemChange, t: DriversStrings = driversStrings.en): string {
  switch (change.kind) {
    case "deleteDriverPackage": return t.describeDelete(change.publishedName);
    case "createRestorePoint": return t.describeRestorePoint(change.description);
    default: return change.kind;
  }
}

export function DriversPage() {
  const t = useStrings(driversStrings);
  const describeChange = useCallback((change: SystemChange) => describe(change, t), [t]);
  const [packages, setPackages] = useState<DriverPackage[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<string[]>([]);
  const [restoreChoice, setRestoreChoice] = useState<boolean | null>(null);
  const [journalKey, setJournalKey] = useState(0);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const next = await listDriverPackages();
      setPackages(next);
      setSelected((current) => current.filter((name) => next.some((pkg) => pkg.deletable && pkg.publishedName === name)));
    } catch {
      setError(t.loadError);
    } finally {
      setLoading(false);
    }
  }, [t]);
  useEffect(() => { void refresh(); }, [refresh]);

  // An empty selection resets the restore-point choice to its checked default.
  useEffect(() => { if (selected.length === 0) setRestoreChoice(null); }, [selected.length]);
  const createRestorePoint = selected.length > 0 && (restoreChoice ?? true);
  const changes = useMemo<SystemChange[]>(() => {
    const deletes: SystemChange[] = selected.map((publishedName) => ({ kind: "deleteDriverPackage", publishedName }));
    return createRestorePoint && deletes.length > 0
      ? [{ kind: "createRestorePoint", description: t.restoreDescription }, ...deletes]
      : deletes;
  }, [createRestorePoint, selected, t]);

  const toggle = (name: string) => setSelected((current) => current.includes(name) ? current.filter((value) => value !== name) : [...current, name]);
  const onFinished = useCallback(() => { setSelected([]); setJournalKey((n) => n + 1); void refresh(); }, [refresh]);

  return <section aria-labelledby="drivers-title">
    <header className="page-header"><div>
      <p className="eyebrow">{t.eyebrow}</p>
      <h1 id="drivers-title">{t.title}</h1>
      <p>{t.intro}</p>
    </div></header>
    <button type="button" disabled={loading} onClick={() => void refresh()}>{t.refresh}</button>
    {loading && <p role="status">{t.loading}</p>}
    {error && <p role="alert">{error}</p>}
    <p>{t.irreversible}</p>
    {packages?.length === 0 && <p>{t.none}</p>}
    {!!packages?.length && <ul className="drivers-list">{packages.map((pkg) => {
      const title = `${pkg.publishedName}${pkg.originalName ? ` (${pkg.originalName})` : ""}`;
      const meta = [pkg.provider, pkg.class, pkg.driverVersion, pkg.driverDate].filter(Boolean).join(" · ");
      return <li key={pkg.publishedName}>
        {pkg.deletable
          ? <label><input type="checkbox" checked={selected.includes(pkg.publishedName)} onChange={() => toggle(pkg.publishedName)} /> <span>{title}</span></label>
          : <span>{title}</span>}
        <p>{t.statusLine(t.status[pkg.status])}{meta ? ` · ${meta}` : ""}</p>
        {!pkg.deletable && <p>{reasonNotDeletable(pkg, t)}</p>}
      </li>;
    })}</ul>}
    <label>
      <input type="checkbox" disabled={selected.length === 0} checked={createRestorePoint} onChange={(event) => setRestoreChoice(event.target.checked)} />
      {" "}{t.createRestorePoint}
    </label>
    <SystemChangeReview changes={changes} describe={describeChange} onFinished={onFinished} />
    <SystemChangeJournal module="drivers" describe={describeChange} refreshKey={journalKey} onFinished={onFinished} />
  </section>;
}
