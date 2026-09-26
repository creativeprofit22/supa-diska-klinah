import { useEffect, useState } from "react";
import { useFormat, useStrings } from "../../shared/i18n/I18nProvider";
import { authorizeStorageScope, listStorageScopes, storageError } from "../../shared/storage/api";
import { storageStrings } from "../../shared/storage/strings";
import { StoragePlanReview } from "../../shared/storage/StoragePlanReview";
import { StoragePaging, StorageScanStatus } from "../../shared/storage/StorageScanStatus";
import { MAX_SELECTION, type NativeScope } from "../../shared/storage/types";
import { useStorageRoot } from "../../shared/storage/useStorageRoot";
import { useStorageScan } from "../../shared/storage/useStorageScan";
import { listCleanerCatalog, startCleaner } from "./api";
import { candidateId, type CleanerCatalog, type FileRow } from "./types";
import { cleanerStrings, type CleanerStrings } from "./strings";
import "./cleaner.css";

export function CleanerPage() {
  const t = useStrings(cleanerStrings);
  const errors = useStrings(storageStrings).errors;
  const fmt = useFormat();
  const [catalog, setCatalog] = useState<CleanerCatalog | null>(null);
  const [scopes, setScopes] = useState<NativeScope[]>([]);
  const [selected, setSelected] = useState<NativeScope | null>(null);
  const [category, setCategory] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [revision, setRevision] = useState(0);
  const [scopePage, setScopePage] = useState(0);
  const [catalogPage, setCatalogPage] = useState(0);
  const rules = catalog?.targets.filter(target => !category || target.catalogId === category) ?? [];
  useEffect(() => {
    let alive = true;
    setCatalog(null); setScopes([]); setSelected(null); setError(null);
    setScopePage(0); setCatalogPage(0);
    void Promise.all([listCleanerCatalog(), listStorageScopes("cleaner")]).then(([inventory, roots]) => {
      if (alive) { setCatalog(inventory); setScopes(roots); }
    }).catch(cause => { if (alive) setError(storageError(cause, errors)); });
    return () => { alive = false; };
  }, [revision, errors]);
  return <section className="cleaner-page" aria-labelledby="cleaner-title">
    <header className="page-header"><div><p className="eyebrow">{t.eyebrow}</p><h1 id="cleaner-title">{t.title}</h1>
      <p>{t.intro}</p></div></header>
    <button type="button" onClick={() => { setSelected(null); setRevision(value => value + 1); }}>{t.refreshScopes}</button>
    {error && <p role="alert">{error}</p>}
    {!catalog && !error && <p role="status">{t.loading}</p>}
    <label>{t.categoryLabel}<select value={category} onChange={e => { setCategory(e.target.value); setSelected(null); setCatalogPage(0); }}><option value="">{t.allCategories}</option>
      {[...new Set(catalog?.targets.map(target => target.catalogId))].map(id => <option key={id}>{id}</option>)}</select></label>
    <p>{t.categoryNote}</p>
    <fieldset><legend>{t.scopesLegend}</legend><p>{t.scopesNote}</p>
      {scopes.slice(scopePage * 20, scopePage * 20 + 20).map(scope => <label className="cleaner-scope" key={scope.scopeId}><input type="radio" name="cleaner-scope" checked={selected?.scopeId === scope.scopeId} disabled={!scope.available} onChange={() => setSelected(scope)} />
        <span>{scope.label}{scope.displayPath && <span>: {scope.displayPath}</span>}{!scope.available && `: ${t.scopeUnavailable}`}</span></label>)}
    </fieldset>
    <nav className="cleanup-actions" aria-label={t.scopePagesLabel}>
      <button type="button" disabled={scopePage === 0} onClick={() => setScopePage(page => page - 1)}>{t.previousScopes}</button>
      <button type="button" disabled={(scopePage + 1) * 20 >= scopes.length} onClick={() => setScopePage(page => page + 1)}>{t.nextScopes}</button>
      <span>{t.scopeCount(fmt.number(scopes.length))}</span>
    </nav>
    {selected && <p>{t.selectedScope(selected.label)}</p>}
    {selected && <CleanerSession key={selected.scopeId} scope={selected} t={t} />}
    <details><summary>{t.catalogSummary}</summary>
      <p>{t.catalogNote}</p>
      <ul className="cleanup-records">{rules.slice(catalogPage * 20, catalogPage * 20 + 20).map(rule => <li key={`${rule.catalogId}:${rule.targetId}:${rule.path}`}>
        <h2>{rule.catalogId} · {rule.targetId}</h2><p>{t.pathTemplate(rule.path)}</p><p>{t.ruleFacts(fmt.number(rule.minimumAgeSeconds), String(rule.ruleVersion), rule.matcher)}</p>
        <p>{t.sourceRevision(rule.source, rule.revision)}</p><p>{t.consequence(rule.consequence)}</p><p>{t.exclusions(rule.exclusions.join("; "))}</p>
        {rule.unsupportedReason && <p>{t.unsupported(rule.unsupportedReason)}</p>}
      </li>)}</ul>
      <nav className="cleanup-actions" aria-label={t.rulePagesLabel}>
        <button type="button" disabled={catalogPage === 0} onClick={() => setCatalogPage(page => page - 1)}>{t.previousRules}</button>
        <button type="button" disabled={(catalogPage + 1) * 20 >= rules.length} onClick={() => setCatalogPage(page => page + 1)}>{t.nextRules}</button>
        <span>{t.ruleCount(fmt.number(rules.length))}</span>
      </nav>
      {catalog?.unsupportedOperations.map(([operation, reason]) => <p key={operation}>{t.unsupportedOperation(operation, reason)}</p>)}
    </details>
  </section>;
}
function CleanerSession({ scope, t }: { scope: NativeScope; t: CleanerStrings }) {
  const fmt = useFormat();
  const root = useStorageRoot("cleaner");
  const scan = useStorageScan<FileRow>({ module: "cleaner", collection: "files", candidateId, scopeKey: root.choice?.rootId ?? scope.scopeId });
  const active = ["starting", "scanning", "cancelling"].includes(scan.phase);
  return <div>
    <button type="button" disabled={root.picking || active} onClick={() => void root.acquire(() => authorizeStorageScope("cleaner", scope.scopeId))}>{t.authorize}</button>
    {root.error && <p role="alert">{root.error}</p>}
    {root.choice && <p>{t.authorizedScope(root.choice.displayPath)}</p>}
    <button type="button" disabled={!root.available || root.picking || active} onClick={() => void scan.start(() => root.run(startCleaner))}>{t.scan}</button>
    <p>{t.singleUse}</p>
    <StorageScanStatus phase={scan.phase} status={scan.status} error={scan.error} cancel={() => void scan.cancel()} />
    {scan.phase === "ready" && <>
      <p>{t.resultsCaveat}</p>
      <p role="status">{t.selected(fmt.number(scan.selected.size), fmt.number(MAX_SELECTION))}</p>
      <button type="button" disabled={!scan.selected.size} onClick={scan.clearSelection}>{t.clearSelection}</button>
      {!scan.loadingPage && <ul className="cleanup-records" aria-label={t.resultsLabel}>{scan.page?.records.map(row => {
        const id = candidateId(row); const checked = id !== null && scan.selected.has(id);
        return <li key={row.record.recordId}><label><input type="checkbox" checked={checked} disabled={!id || (!checked && scan.selected.size >= MAX_SELECTION)} onChange={() => { if (id) scan.toggle(id); }} />{row.record.displayPath}</label>
          <p>{t.logical(fmt.bytes(row.record.logicalBytes))} · {row.record.allocatedBytes === null ? t.unknownAllocation : t.allocated(fmt.bytes(row.record.allocatedBytes))}</p>
          {row.record.eligibility.kind !== "eligible" && <p>{t.notSelectable(row.record.eligibility.kind === "ineligible" ? row.record.eligibility.reason : t.readOnly)}</p>}</li>;
      })}</ul>}
      {scan.page?.records.length === 0 && <p>{t.noMatches}</p>}
      <StoragePaging loading={scan.loadingPage} hasNext={!!scan.page?.nextCursor} count={scan.page?.records.length ?? 0} total={scan.page?.retainedTotal ?? 0} firstPage={() => void scan.firstPage()} nextPage={() => void scan.nextPage()} />
    </>}
    <p>{t.dispositionNote}</p>
    <StoragePlanReview selection={scan.selection} dispositions={["quarantine", "permanent"]} onExecuted={() => { void scan.reset(); }} />
  </div>;
}
