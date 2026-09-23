import { useEffect, useState } from "react";
import { formatBytes } from "../../shared/format";
import { authorizeStorageScope, listStorageScopes, storageError } from "../../shared/storage/api";
import { StoragePlanReview } from "../../shared/storage/StoragePlanReview";
import { StoragePaging, StorageScanStatus } from "../../shared/storage/StorageScanStatus";
import { MAX_SELECTION, type NativeScope } from "../../shared/storage/types";
import { useStorageRoot } from "../../shared/storage/useStorageRoot";
import { useStorageScan } from "../../shared/storage/useStorageScan";
import { listBrowserPolicy, startBrowserScan } from "./api";
import { candidateId, type BrowserPolicy, type FileRow } from "./types";
import "./browser.css";

export function BrowserPage() {
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
    }).catch(cause => { if (alive) setError(storageError(cause)); });
    return () => { alive = false; };
  }, [revision]);
  return <section className="browser-page" aria-labelledby="browser-title">
    <h1 id="browser-title">Browser caches</h1>
    <p>Native cache scopes only. No automatic browser closing or cache cleanup. Cookies, logins, history, bookmarks and site databases are excluded; private profiles are preserved.</p>
    <p>Active or unknown browser activity is refused. Close browsers yourself and authorize again to retry. Partial results may omit inaccessible, protected, recent or changing files. Unavailable scopes cannot be authorized; refresh after the browser installation is available.</p>
    <button type="button" onClick={() => { setSelected(null); setRevision(v => v + 1); }}>Refresh native scopes</button>
    {error && <p role="alert">{error}</p>}
    {!policy && !error && <p role="status">Loading native browser policy and scopes...</p>}
    {policy && <>
      <p>Minimum age: {policy.minimumAgeSeconds.toLocaleString()} seconds</p>
      <label><input type="checkbox" checked={optIn} onChange={e => setOptIn(e.target.checked)} />Include service-worker caches</label>
      <p>{policy.serviceWorkerDisclosure}</p>
      <fieldset><legend>Native browser scopes</legend>{scopes.map(scope => <label className="browser-scope" key={scope.scopeId}>
        <input type="radio" name="browser-scope" checked={selected?.scopeId === scope.scopeId} disabled={!scope.available} onChange={() => setSelected(scope)} />
        {scope.label}: {scope.displayPath}{!scope.available && " (unavailable or unsupported)"}
      </label>)}</fieldset>
      {selected && <BrowserSession key={`${selected.scopeId}:${optIn}`} scope={selected} policy={policy} optIn={optIn} />}
      <details><summary>Native policy and provenance</summary>
        <p>Source: {policy.source} | Revision: {policy.revision}</p>
        <p>Risk: {policy.risk} | Lifecycle: {policy.lifecycle}</p><p>{policy.consequence}</p>
        <p>Exclusions: {policy.exclusions.join("; ")}</p>
        <p>Profile cache roots: {policy.profileCacheRoots.join("; ")}</p><p>Shared cache roots: {policy.sharedCacheRoots.join("; ")}</p>
        {policy.unsupported.map(reason => <p key={reason}>Unsupported: {reason}</p>)}
      </details>
    </>}
  </section>;
}
// Display only. These strings never enter authorization or scan requests.
export function cacheLabel(path: string, base: string, policy: BrowserPolicy): string {
  const normalize = (value: string) => value.replaceAll("\\", "/").replace(/^\/\/\?\//, "").replace(/\/+$/, "").toLowerCase();
  const full = normalize(path), root = normalize(base);
  if (!root || !full.startsWith(root + "/")) return "Unclassified cache";
  const relative = full.slice(root.length + 1);
  const under = (value: string, prefix: string) => value.startsWith(normalize(prefix) + "/");
  if (policy.sharedCacheRoots.some(prefix => under(relative, prefix))) return "Shared cache";
  const parts = relative.split("/");
  if (policy.profileCacheRoots.some(prefix => under(relative, prefix))) return "Per-profile cache";
  if ((parts[0] === "default" || /^profile \d+$/.test(parts[0])) && policy.profileCacheRoots.some(prefix => under(parts.slice(1).join("/"), prefix))) return `Per-profile cache: ${parts[0]}`;
  if (parts.length > 2 && ["cache2", "startupcache"].includes(parts[1])) return `Per-profile cache: ${parts[0]}`;
  return "Unclassified cache";
}
function BrowserSession({ scope, policy, optIn }: { scope: NativeScope; policy: BrowserPolicy; optIn: boolean }) {
  const root = useStorageRoot("browser");
  const scan = useStorageScan<FileRow>({ module: "browser", collection: "files", candidateId, scopeKey: root.choice?.rootId ?? scope.scopeId });
  const active = ["starting", "scanning", "cancelling"].includes(scan.phase);
  return <div>
    <button type="button" disabled={root.picking || active} onClick={() => void root.acquire(() => authorizeStorageScope("browser", scope.scopeId))}>Authorize selected scope</button>
    {root.error && <p role="alert">{root.error}</p>}
    {root.choice && <p>Authorized scope: {root.choice.displayPath}</p>}
    <button type="button" disabled={!root.available || root.picking || active} onClick={() => void scan.start(() => root.run(id => startBrowserScan(id, optIn)))}>Scan browser scope</button>
    <p>Authorization is single-use. Authorize again for another scan. Switching scopes releases old authority and results.</p>
    <StorageScanStatus phase={scan.phase} status={scan.status} error={scan.error} cancel={() => void scan.cancel()} />
    {scan.phase === "ready" && <>
      <p>Results may omit protected, inaccessible, changing or limit-excluded files. Byte totals are not reclaimed space.</p>
      <p role="status">{scan.selected.size} selected (maximum {MAX_SELECTION.toLocaleString()}).</p>
      <button type="button" disabled={!scan.selected.size} onClick={scan.clearSelection}>Clear selection</button>
      {!scan.loadingPage && <ul className="cleanup-records" aria-label="Browser results">{scan.page?.records.map(row => {
        const id = candidateId(row); const checked = id !== null && scan.selected.has(id);
        return <li key={row.record.recordId}><label><input type="checkbox" checked={checked} disabled={!id || (!checked && scan.selected.size >= MAX_SELECTION)} onChange={() => { if (id) scan.toggle(id); }} />{row.record.displayPath}</label>
          <p>{cacheLabel(row.record.displayPath, root.choice?.displayPath ?? "", policy)}</p><p>{formatBytes(row.record.logicalBytes)} logical · {row.record.allocatedBytes === null ? "Unknown allocation" : `${formatBytes(row.record.allocatedBytes)} allocated`}</p>
          {row.record.eligibility.kind !== "eligible" && <p>Not selectable: {row.record.eligibility.kind === "ineligible" ? row.record.eligibility.reason : "Read-only"}</p>}</li>;
      })}</ul>}
      {scan.page?.records.length === 0 && <p>No matching files on this page.</p>}
      <StoragePaging loading={scan.loadingPage} hasNext={!!scan.page?.nextCursor} count={scan.page?.records.length ?? 0} total={scan.page?.retainedTotal ?? 0} firstPage={() => void scan.firstPage()} nextPage={() => void scan.nextPage()} />
    </>}
    <p>App recovery is the default: same-volume storage for manual undo, no space reclaimed and no automatic purge. Permanent deletion is optional and requires Windows confirmation. Windows Recycle Bin is unsupported; there is no path-based fallback.</p>
    <StoragePlanReview selection={scan.selection} dispositions={["quarantine", "permanent"]} onExecuted={() => { void scan.reset(); }} />
  </div>;
}
