import { useState } from "react";
import { chooseStorageRoot } from "../../shared/storage/api";
import { StoragePaging, StorageScanStatus } from "../../shared/storage/StorageScanStatus";
import { StoragePlanReview } from "../../shared/storage/StoragePlanReview";
import { MAX_SELECTION } from "../../shared/storage/types";
import { useStorageRoot } from "../../shared/storage/useStorageRoot";
import { useStorageScan } from "../../shared/storage/useStorageScan";
import { useStrings } from "../../shared/i18n/I18nProvider";
import { startEmptyFolders } from "./api";
import { emptyFoldersStrings } from "./strings";
import { candidateId, type EmptyFolderRow } from "./types";
import "./empty-folders.css";
export function EmptyFoldersPage() {
  const t = useStrings(emptyFoldersStrings);
  const root = useStorageRoot("emptyFolders");
  const [depth, setDepth] = useState("20");
  const valid = !!depth.trim() && Number.isInteger(Number(depth)) && Number(depth) >= 0 && Number(depth) <= 64;
  const scan = useStorageScan<EmptyFolderRow>({ module: "emptyFolders", collection: "emptyFolders", candidateId, scopeKey: JSON.stringify([root.choice?.rootId, depth]) });
  const active = ["starting", "scanning", "cancelling"].includes(scan.phase);
  return <section className="empty-folders-page" aria-labelledby="empty-folders-title">
    <header className="page-header"><div><p className="eyebrow">{t.eyebrow}</p><h1 id="empty-folders-title">{t.title}</h1><p>{t.intro}</p></div></header>
    <p>{t.rootRetained}</p>
    <p role="note">{t.permanentNote}</p>
    <button disabled={active || root.picking} onClick={() => void root.acquire(() => chooseStorageRoot("emptyFolders"))}>{t.chooseFolder}</button>
    {root.choice && <p>{t.selectedFolder(root.choice.displayPath)}</p>}
    {root.choice && !root.available && !active && <p>{t.chooseAgain}</p>}
    {root.error && <p role="alert">{root.error}</p>}
    <form onSubmit={e => { e.preventDefault(); if (valid && root.available && !active) void scan.start(() => root.run(id => startEmptyFolders(id, Number(depth)))); }}>
      <fieldset disabled={active || root.picking}><legend>{t.filters}</legend><label>{t.depth}<input type="number" min="0" max="64" step="1" required value={depth} onChange={e => setDepth(e.target.value)} /></label>
        {!valid && <p role="alert">{t.depthInvalid}</p>}<button disabled={!valid || !root.available}>{t.scan}</button></fieldset>
    </form>
    <StorageScanStatus phase={scan.phase} status={scan.status} error={scan.error} cancel={() => void scan.cancel()} />
    {scan.phase === "ready" && <>
      <p>{t.resultsNote}</p>
      <p role="status">{t.selectedCount(scan.selected.size, MAX_SELECTION)}</p>
      <button disabled={!scan.selected.size} onClick={scan.clearSelection}>{t.clearSelection}</button>
      <ul className="cleanup-records" aria-label={t.resultsLabel}>{!scan.loadingPage && scan.page?.records.map(row => {
        const id = candidateId(row), selected = !!id && scan.selected.has(id);
        return <li key={row.record.recordId}><label><input type="checkbox" checked={selected} disabled={!id || (!selected && scan.selected.size >= MAX_SELECTION)} onChange={() => { if (id) scan.toggle(id); }} />{row.record.displayPath}</label><p>{t.rowDetail(row.record.depth, row.record.descendantDirectories)}</p></li>;
      })}</ul>
      {scan.page && !scan.loadingPage && !scan.page.records.length && <p>{t.noResults}</p>}
      <StoragePaging loading={scan.loadingPage} hasNext={!!scan.page?.nextCursor} count={scan.page?.records.length ?? 0} total={scan.page?.retainedTotal ?? 0} firstPage={() => void scan.firstPage()} nextPage={() => void scan.nextPage()} />
    </>}
    <StoragePlanReview selection={scan.selection} dispositions={["permanent"]} onExecuted={() => { void scan.reset(); }} />
  </section>;
}
