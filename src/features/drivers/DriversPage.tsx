import { useCallback, useEffect, useMemo, useState } from "react";
import { SystemChangeJournal } from "../../shared/system-change/SystemChangeJournal";
import { SystemChangeReview } from "../../shared/system-change/SystemChangeReview";
import type { SystemChange } from "../../shared/system-change/types";
import { listDriverPackages } from "./api";
import type { DriverPackage, DriverPackageStatus } from "./types";

export const RESTORE_DESCRIPTION = "Before removing driver packages";

const statusLabel: Record<DriverPackageStatus, string> = {
  inUse: "In use by a present device",
  current: "Not in use, but no newer package replaces it",
  superseded: "Superseded by a newer package",
};

function reasonNotDeletable(pkg: DriverPackage): string {
  if (pkg.status === "inUse") return "Cannot be removed: a present device uses it.";
  if (pkg.status === "current") return "Cannot be removed: it is the newest package from this provider.";
  return "Cannot be removed.";
}

export function describe(change: SystemChange): string {
  switch (change.kind) {
    case "deleteDriverPackage": return `Remove driver package: ${change.publishedName}`;
    case "createRestorePoint": return `Create restore point: ${change.description}`;
    default: return change.kind;
  }
}

export function DriversPage() {
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
      setError("Driver packages could not be listed.");
    } finally {
      setLoading(false);
    }
  }, []);
  useEffect(() => { void refresh(); }, [refresh]);

  // An empty selection resets the restore-point choice to its checked default.
  useEffect(() => { if (selected.length === 0) setRestoreChoice(null); }, [selected.length]);
  const createRestorePoint = selected.length > 0 && (restoreChoice ?? true);
  const changes = useMemo<SystemChange[]>(() => {
    const deletes: SystemChange[] = selected.map((publishedName) => ({ kind: "deleteDriverPackage", publishedName }));
    return createRestorePoint && deletes.length > 0
      ? [{ kind: "createRestorePoint", description: RESTORE_DESCRIPTION }, ...deletes]
      : deletes;
  }, [createRestorePoint, selected]);

  const toggle = (name: string) => setSelected((current) => current.includes(name) ? current.filter((value) => value !== name) : [...current, name]);
  const onFinished = useCallback(() => { setSelected([]); setJournalKey((n) => n + 1); void refresh(); }, [refresh]);

  return <section aria-labelledby="drivers-title">
    <header className="page-header"><div>
      <p className="eyebrow">System</p>
      <h1 id="drivers-title">Driver packages</h1>
      <p>Lists driver packages in the Windows driver store; only superseded packages not used by any device can be removed.</p>
    </div></header>
    <button type="button" disabled={loading} onClick={() => void refresh()}>Refresh</button>
    {loading && <p role="status">Loading driver packages…</p>}
    {error && <p role="alert">{error}</p>}
    <p>Removing a driver package from the driver store cannot be undone by this app. Driver update installation is not offered.</p>
    {packages?.length === 0 && <p>No driver packages reported.</p>}
    {!!packages?.length && <ul className="drivers-list">{packages.map((pkg) => {
      const title = `${pkg.publishedName}${pkg.originalName ? ` (${pkg.originalName})` : ""}`;
      const meta = [pkg.provider, pkg.class, pkg.driverVersion, pkg.driverDate].filter(Boolean).join(" · ");
      return <li key={pkg.publishedName}>
        {pkg.deletable
          ? <label><input type="checkbox" checked={selected.includes(pkg.publishedName)} onChange={() => toggle(pkg.publishedName)} /> <span>{title}</span></label>
          : <span>{title}</span>}
        <p>Status: {statusLabel[pkg.status]}{meta ? ` · ${meta}` : ""}</p>
        {!pkg.deletable && <p>{reasonNotDeletable(pkg)}</p>}
      </li>;
    })}</ul>}
    <label>
      <input type="checkbox" disabled={selected.length === 0} checked={createRestorePoint} onChange={(event) => setRestoreChoice(event.target.checked)} />
      {" "}Create a restore point first
    </label>
    <SystemChangeReview changes={changes} describe={describe} onFinished={onFinished} />
    <SystemChangeJournal module="drivers" describe={describe} refreshKey={journalKey} onFinished={onFinished} />
  </section>;
}
