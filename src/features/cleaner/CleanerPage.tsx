import { useEffect, useState } from "react";
import { formatBytes } from "../../shared/format";
import { authorizeStorageScope, listStorageScopes, storageError } from "../../shared/storage/api";
import { StoragePlanReview } from "../../shared/storage/StoragePlanReview";
import { StoragePaging, StorageScanStatus } from "../../shared/storage/StorageScanStatus";
import { MAX_SELECTION, type NativeScope } from "../../shared/storage/types";
import { useStorageRoot } from "../../shared/storage/useStorageRoot";
import { useStorageScan } from "../../shared/storage/useStorageScan";
import { listCleanerCatalog, startCleaner } from "./api";
import { candidateId, type CleanerCatalog, type FileRow } from "./types";
import "./cleaner.css";

export function CleanerPage() {
  const [catalog, setCatalog] = useState<CleanerCatalog | null>(null);
  const [scopes, setScopes] = useState<NativeScope[]>([]);
  const [selected, setSelected] = useState<NativeScope | null>(null);
  const [category, setCategory] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [revision, setRevision] = useState(0);
  const [scopePage, setScopePage] = useState(0);
  const [catalogPage, setCatalogPage] = useState(0);
  const rules = catalog?.targets.filter(t => !category || t.catalogId === category) ?? [];
  useEffect(() => {
    let alive = true;
    setCatalog(null); setScopes([]); setSelected(null); setError(null);
    setScopePage(0); setCatalogPage(0);
    void Promise.all([listCleanerCatalog(), listStorageScopes("cleaner")]).then(([inventory, roots]) => {
      if (alive) { setCatalog(inventory); setScopes(roots); }
    }).catch(cause => { if (alive) setError(storageError(cause)); });
    return () => { alive = false; };
  }, [revision]);
  return <section className="cleaner-page" aria-labelledby="cleaner-title">
    <header className="page-header"><div><p className="eyebrow">Catalog-based cleanup</p><h1 id="cleaner-title">Rule cleaner</h1>
      <p>Choose one native scope, authorize it, then scan. Nothing is selected automatically. Work across roots is sequential, with separate snapshots and plans, not atomic.</p></div></header>
    <button type="button" onClick={() => { setSelected(null); setRevision(value => value + 1); }}>Refresh native scopes</button>
    {error && <p role="alert">{error}</p>}
    {!catalog && !error && <p role="status">Loading native catalog and scopes…</p>}
    <label>Catalog category<select value={category} onChange={e => { setCategory(e.target.value); setSelected(null); setCatalogPage(0); }}><option value="">All categories</option>
      {[...new Set(catalog?.targets.map(t => t.catalogId))].map(id => <option key={id}>{id}</option>)}</select></label>
    <p>This filter changes the catalog display only, not native scan rules. Changing it clears the current scope and results.</p>
    <fieldset><legend>Native scopes</legend><p>Unavailable or unsupported targets cannot be selected. Paths are display-only; no typed paths are accepted.</p>
      {scopes.slice(scopePage * 20, scopePage * 20 + 20).map(scope => <label className="cleaner-scope" key={scope.scopeId}><input type="radio" name="cleaner-scope" checked={selected?.scopeId === scope.scopeId} disabled={!scope.available} onChange={() => setSelected(scope)} />
        <span>{scope.label}{scope.displayPath && <span>: {scope.displayPath}</span>}{!scope.available && ": Unavailable or unsupported"}</span></label>)}
    </fieldset>
    <nav className="cleanup-actions" aria-label="Native scope pages">
      <button type="button" disabled={scopePage === 0} onClick={() => setScopePage(page => page - 1)}>Previous scopes</button>
      <button type="button" disabled={(scopePage + 1) * 20 >= scopes.length} onClick={() => setScopePage(page => page + 1)}>Next scopes</button>
      <span>{scopes.length} native scope entries; up to 20 shown</span>
    </nav>
    {selected && <p>Selected scope: {selected.label}</p>}
    {selected && <CleanerSession key={selected.scopeId} scope={selected} />}
    <details><summary>Catalog rules, provenance and exclusions</summary>
      <p>This is the compiled catalog inventory, not per-file rule attribution or a claim that each target exists. Scan rows do not provide rule metadata.</p>
      <ul className="cleanup-records">{rules.slice(catalogPage * 20, catalogPage * 20 + 20).map(t => <li key={`${t.catalogId}:${t.targetId}:${t.path}`}>
        <h2>{t.catalogId} · {t.targetId}</h2><p>Path template: {t.path}</p><p>Minimum age: {t.minimumAgeSeconds.toLocaleString()} seconds · Rule version {t.ruleVersion} · Matcher: {t.matcher}</p>
        <p>Source: {t.source} · Revision: {t.revision}</p><p>Consequence: {t.consequence}</p><p>Exclusions: {t.exclusions.join("; ")}</p>
        {t.unsupportedReason && <p>Unsupported: {t.unsupportedReason}</p>}
      </li>)}</ul>
      <nav className="cleanup-actions" aria-label="Catalog rule pages">
        <button type="button" disabled={catalogPage === 0} onClick={() => setCatalogPage(page => page - 1)}>Previous rules</button>
        <button type="button" disabled={(catalogPage + 1) * 20 >= rules.length} onClick={() => setCatalogPage(page => page + 1)}>Next rules</button>
        <span>{rules.length} catalog rules; up to 20 shown</span>
      </nav>
      {catalog?.unsupportedOperations.map(([operation, reason]) => <p key={operation}>Unsupported operation: {operation}: {reason}</p>)}
    </details>
  </section>;
}
function CleanerSession({ scope }: { scope: NativeScope }) {
  const root = useStorageRoot("cleaner");
  const scan = useStorageScan<FileRow>({ module: "cleaner", collection: "files", candidateId, scopeKey: root.choice?.rootId ?? scope.scopeId });
  const active = ["starting", "scanning", "cancelling"].includes(scan.phase);
  return <div>
    <button type="button" disabled={root.picking || active} onClick={() => void root.acquire(() => authorizeStorageScope("cleaner", scope.scopeId))}>Authorize selected scope</button>
    {root.error && <p role="alert">{root.error}</p>}
    {root.choice && <p>Authorized scope: {root.choice.displayPath}</p>}
    <button type="button" disabled={!root.available || root.picking || active} onClick={() => void scan.start(() => root.run(startCleaner))}>Scan cleaner scope</button>
    <p>Authorization is single-use. Authorize again for another scan. Switching scopes releases old authority and results.</p>
    <StorageScanStatus phase={scan.phase} status={scan.status} error={scan.error} cancel={() => void scan.cancel()} />
    {scan.phase === "ready" && <>
      <p>Results may omit protected, inaccessible, changing or limit-excluded files. Byte totals are not reclaimed space.</p>
      <p role="status">{scan.selected.size} selected (maximum {MAX_SELECTION.toLocaleString()}).</p>
      <button type="button" disabled={!scan.selected.size} onClick={scan.clearSelection}>Clear selection</button>
      {!scan.loadingPage && <ul className="cleanup-records" aria-label="Cleaner results">{scan.page?.records.map(row => {
        const id = candidateId(row); const checked = id !== null && scan.selected.has(id);
        return <li key={row.record.recordId}><label><input type="checkbox" checked={checked} disabled={!id || (!checked && scan.selected.size >= MAX_SELECTION)} onChange={() => { if (id) scan.toggle(id); }} />{row.record.displayPath}</label>
          <p>{formatBytes(row.record.logicalBytes)} logical · {row.record.allocatedBytes === null ? "Unknown allocation" : `${formatBytes(row.record.allocatedBytes)} allocated`}</p>
          {row.record.eligibility.kind !== "eligible" && <p>Not selectable: {row.record.eligibility.kind === "ineligible" ? row.record.eligibility.reason : "Read-only"}</p>}</li>;
      })}</ul>}
      {scan.page?.records.length === 0 && <p>No matching files on this page.</p>}
      <StoragePaging loading={scan.loadingPage} hasNext={!!scan.page?.nextCursor} count={scan.page?.records.length ?? 0} total={scan.page?.retainedTotal ?? 0} firstPage={() => void scan.firstPage()} nextPage={() => void scan.nextPage()} />
    </>}
    <p>App recovery is the default: same-volume storage for manual undo, no space reclaimed and no automatic purge. Permanent deletion is optional and requires Windows confirmation. Windows Recycle Bin is unsupported; there is no path-based fallback.</p>
    <StoragePlanReview selection={scan.selection} dispositions={["quarantine", "permanent"]} onExecuted={() => { void scan.reset(); }} />
  </div>;
}
