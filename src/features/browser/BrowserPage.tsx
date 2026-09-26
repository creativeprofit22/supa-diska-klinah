import { useEffect, useState } from "react";
import { useFormat, useStrings } from "../../shared/i18n/I18nProvider";
import { authorizeStorageScope, listStorageScopes, storageError } from "../../shared/storage/api";
import { storageStrings } from "../../shared/storage/strings";
import { StoragePlanReview } from "../../shared/storage/StoragePlanReview";
import { StoragePaging, StorageScanStatus } from "../../shared/storage/StorageScanStatus";
import { MAX_SELECTION, type NativeScope } from "../../shared/storage/types";
import { useStorageRoot } from "../../shared/storage/useStorageRoot";
import { useStorageScan } from "../../shared/storage/useStorageScan";
import { listBrowserPolicy, startBrowserScan } from "./api";
import { candidateId, type BrowserPolicy, type FileRow } from "./types";
import { browserStrings, type BrowserStrings } from "./strings";
import "./browser.css";

export function BrowserPage() {
  const t = useStrings(browserStrings);
  const errors = useStrings(storageStrings).errors;
  const fmt = useFormat();
  const [policy, setPolicy] = useState<BrowserPolicy | null>(null);
  const [scopes, setScopes] = useState<NativeScope[]>([]);
  const [selected, setSelected] = useState<NativeScope | null>(null);
  const [optIn, setOptIn] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    let alive = true;
    setPolicy(null); setScopes([]); setSelected(null); setError(null);
    void Promise.all([listBrowserPolicy(), listStorageScopes("browser")]).then(([p, s]) => {
      if (alive) { setPolicy(p); setScopes(s); }
    }).catch(cause => { if (alive) setError(storageError(cause, errors)); });
    return () => { alive = false; };
  }, [revision, errors]);
  return <section className="browser-page" aria-labelledby="browser-title">
    <h1 id="browser-title">{t.title}</h1>
    <p>{t.introScope}</p>
    <p>{t.introRefusal}</p>
    <button type="button" onClick={() => { setSelected(null); setRevision(v => v + 1); }}>{t.refreshScopes}</button>
    {error && <p role="alert">{error}</p>}
    {!policy && !error && <p role="status">{t.loading}</p>}
    {policy && <>
      <p>{t.minimumAge(fmt.number(policy.minimumAgeSeconds))}</p>
      <label><input type="checkbox" checked={optIn} onChange={e => setOptIn(e.target.checked)} />{t.includeServiceWorker}</label>
      <p>{policy.serviceWorkerDisclosure}</p>
      <fieldset><legend>{t.scopesLegend}</legend>{scopes.map(scope => <label className="browser-scope" key={scope.scopeId}>
        <input type="radio" name="browser-scope" checked={selected?.scopeId === scope.scopeId} disabled={!scope.available} onChange={() => setSelected(scope)} />
        {scope.label}: {scope.displayPath}{!scope.available && t.scopeUnavailable}
      </label>)}</fieldset>
      {selected && <BrowserSession key={`${selected.scopeId}:${optIn}`} scope={selected} policy={policy} optIn={optIn} t={t} />}
      <details><summary>{t.policySummary}</summary>
        <p>{t.sourceRevision(policy.source, policy.revision)}</p>
        <p>{t.riskLifecycle(policy.risk, policy.lifecycle)}</p><p>{policy.consequence}</p>
        <p>{t.exclusions(policy.exclusions.join("; "))}</p>
        <p>{t.profileCacheRoots(policy.profileCacheRoots.join("; "))}</p><p>{t.sharedCacheRoots(policy.sharedCacheRoots.join("; "))}</p>
        {policy.unsupported.map(reason => <p key={reason}>{t.unsupported(reason)}</p>)}
      </details>
    </>}
  </section>;
}
// Display only. These strings never enter authorization or scan requests.
export function cacheLabel(path: string, base: string, policy: BrowserPolicy, labels: BrowserStrings["cacheLabels"] = browserStrings.en.cacheLabels): string {
  const normalize = (value: string) => value.replaceAll("\\", "/").replace(/^\/\/\?\//, "").replace(/\/+$/, "").toLowerCase();
  const full = normalize(path), root = normalize(base);
  if (!root || !full.startsWith(root + "/")) return labels.unclassified;
  const relative = full.slice(root.length + 1);
  const under = (value: string, prefix: string) => value.startsWith(normalize(prefix) + "/");
  if (policy.sharedCacheRoots.some(prefix => under(relative, prefix))) return labels.shared;
  const parts = relative.split("/");
  if (policy.profileCacheRoots.some(prefix => under(relative, prefix))) return labels.perProfile;
  if ((parts[0] === "default" || /^profile \d+$/.test(parts[0])) && policy.profileCacheRoots.some(prefix => under(parts.slice(1).join("/"), prefix))) return labels.perProfileNamed(parts[0]);
  if (parts.length > 2 && ["cache2", "startupcache"].includes(parts[1])) return labels.perProfileNamed(parts[0]);
  return labels.unclassified;
}
function BrowserSession({ scope, policy, optIn, t }: { scope: NativeScope; policy: BrowserPolicy; optIn: boolean; t: BrowserStrings }) {
  const fmt = useFormat();
  const root = useStorageRoot("browser");
  const scan = useStorageScan<FileRow>({ module: "browser", collection: "files", candidateId, scopeKey: root.choice?.rootId ?? scope.scopeId });
  const active = ["starting", "scanning", "cancelling"].includes(scan.phase);
  return <div>
    <button type="button" disabled={root.picking || active} onClick={() => void root.acquire(() => authorizeStorageScope("browser", scope.scopeId))}>{t.authorize}</button>
    {root.error && <p role="alert">{root.error}</p>}
    {root.choice && <p>{t.authorizedScope(root.choice.displayPath)}</p>}
    <button type="button" disabled={!root.available || root.picking || active} onClick={() => void scan.start(() => root.run(id => startBrowserScan(id, optIn)))}>{t.scan}</button>
    <p>{t.singleUse}</p>
    <StorageScanStatus phase={scan.phase} status={scan.status} error={scan.error} cancel={() => void scan.cancel()} />
    {scan.phase === "ready" && <>
      <p>{t.resultsCaveat}</p>
      <p role="status">{t.selected(fmt.number(scan.selected.size), fmt.number(MAX_SELECTION))}</p>
      <button type="button" disabled={!scan.selected.size} onClick={scan.clearSelection}>{t.clearSelection}</button>
      {!scan.loadingPage && <ul className="cleanup-records" aria-label={t.resultsLabel}>{scan.page?.records.map(row => {
        const id = candidateId(row); const checked = id !== null && scan.selected.has(id);
        return <li key={row.record.recordId}><label><input type="checkbox" checked={checked} disabled={!id || (!checked && scan.selected.size >= MAX_SELECTION)} onChange={() => { if (id) scan.toggle(id); }} />{row.record.displayPath}</label>
          <p>{cacheLabel(row.record.displayPath, root.choice?.displayPath ?? "", policy, t.cacheLabels)}</p><p>{t.logical(fmt.bytes(row.record.logicalBytes))} · {row.record.allocatedBytes === null ? t.unknownAllocation : t.allocated(fmt.bytes(row.record.allocatedBytes))}</p>
          {row.record.eligibility.kind !== "eligible" && <p>{t.notSelectable(row.record.eligibility.kind === "ineligible" ? row.record.eligibility.reason : t.readOnly)}</p>}</li>;
      })}</ul>}
      {scan.page?.records.length === 0 && <p>{t.noMatches}</p>}
      <StoragePaging loading={scan.loadingPage} hasNext={!!scan.page?.nextCursor} count={scan.page?.records.length ?? 0} total={scan.page?.retainedTotal ?? 0} firstPage={() => void scan.firstPage()} nextPage={() => void scan.nextPage()} />
    </>}
    <p>{t.dispositionNote}</p>
    <StoragePlanReview selection={scan.selection} dispositions={["quarantine", "permanent"]} onExecuted={() => { void scan.reset(); }} />
  </div>;
}
