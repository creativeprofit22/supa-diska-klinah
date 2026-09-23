import { useState } from "react";
import { chooseStorageRoot } from "../../shared/storage/api";
import { StoragePaging, StorageScanStatus } from "../../shared/storage/StorageScanStatus";
import { StoragePlanReview } from "../../shared/storage/StoragePlanReview";
import { MAX_SELECTION } from "../../shared/storage/types";
import { useStorageRoot } from "../../shared/storage/useStorageRoot";
import { useStorageScan } from "../../shared/storage/useStorageScan";
import { startEmptyFolders } from "./api";
import { candidateId, type EmptyFolderRow } from "./types";
import "./empty-folders.css";
export function EmptyFoldersPage() {
  const root = useStorageRoot("emptyFolders");
  const [depth, setDepth] = useState("20");
  const valid = !!depth.trim() && Number.isInteger(Number(depth)) && Number(depth) >= 0 && Number(depth) <= 64;
  const scan = useStorageScan<EmptyFolderRow>({ module: "emptyFolders", collection: "emptyFolders", candidateId, scopeKey: JSON.stringify([root.choice?.rootId, depth]) });
  const active = ["starting", "scanning", "cancelling"].includes(scan.phase);
  return <section className="empty-folders-page" aria-labelledby="empty-folders-title">
    <header className="page-header"><div><p className="eyebrow">Personal-file cleanup</p><h1 id="empty-folders-title">Empty folders</h1><p>Explicitly select empty directories for review. Nothing is selected automatically.</p></div></header>
    <p>The chosen root is always retained. Blocked, non-empty, hidden, protected and incomplete parents are absent from results.</p>
    <p role="note">Permanent deletion only, with no undo. Exact empty-only atomic directory removal cannot safely quarantine a directory that gains a child. A new child blocks removal. No app recovery or Windows Recycle Bin is offered.</p>
    <button disabled={active || root.picking} onClick={() => void root.acquire(() => chooseStorageRoot("emptyFolders"))}>Choose folder</button>
    {root.choice && <p>Selected folder: {root.choice.displayPath}</p>}
    {root.choice && !root.available && !active && <p>Choose a folder again to scan. Folder authorizations are single-use.</p>}
    {root.error && <p role="alert">{root.error}</p>}
    <form onSubmit={e => { e.preventDefault(); if (valid && root.available && !active) void scan.start(() => root.run(id => startEmptyFolders(id, Number(depth)))); }}>
      <fieldset disabled={active || root.picking}><legend>Scan filters</legend><label>Scan depth<input type="number" min="0" max="64" step="1" required value={depth} onChange={e => setDepth(e.target.value)} /></label>
        {!valid && <p role="alert">Enter a whole depth from 0 to 64.</p>}<button disabled={!valid || !root.available}>Scan for empty folders</button></fieldset>
    </form>
    <StorageScanStatus phase={scan.phase} status={scan.status} error={scan.error} cancel={() => void scan.cancel()} />
    {scan.phase === "ready" && <>
      <p>Results are bounded. Only complete empty subtrees are eligible. Permanent deletion requires separate Windows confirmation.</p>
      <p role="status">{scan.selected.size} selected (maximum {MAX_SELECTION}).</p>
      <button disabled={!scan.selected.size} onClick={scan.clearSelection}>Clear selection</button>
      <ul className="cleanup-records" aria-label="Empty folder results">{!scan.loadingPage && scan.page?.records.map(row => {
        const id = candidateId(row), selected = !!id && scan.selected.has(id);
        return <li key={row.record.recordId}><label><input type="checkbox" checked={selected} disabled={!id || (!selected && scan.selected.size >= MAX_SELECTION)} onChange={() => { if (id) scan.toggle(id); }} />{row.record.displayPath}</label><p>Depth {row.record.depth} · {row.record.descendantDirectories} descendant directories</p></li>;
      })}</ul>
      {scan.page && !scan.loadingPage && !scan.page.records.length && <p>No empty folders on this page.</p>}
      <StoragePaging loading={scan.loadingPage} hasNext={!!scan.page?.nextCursor} count={scan.page?.records.length ?? 0} total={scan.page?.retainedTotal ?? 0} firstPage={() => void scan.firstPage()} nextPage={() => void scan.nextPage()} />
    </>}
    <StoragePlanReview selection={scan.selection} dispositions={["permanent"]} onExecuted={() => { void scan.reset(); }} />
  </section>;
}
