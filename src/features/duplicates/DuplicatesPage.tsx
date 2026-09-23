import { useEffect, useState } from "react";
import { formatBytes } from "../../shared/format";
import { chooseStorageRoot } from "../../shared/storage/api";
import { StoragePaging, StorageScanStatus } from "../../shared/storage/StorageScanStatus";
import { StoragePlanReview } from "../../shared/storage/StoragePlanReview";
import { MAX_SELECTION } from "../../shared/storage/types";
import { useStorageRoot } from "../../shared/storage/useStorageRoot";
import { useStorageScan } from "../../shared/storage/useStorageScan";
import { parseFields, startDuplicates } from "./api";
import { selectableMember, type DuplicateGroup, type DuplicateRow } from "./types";
import "./duplicates.css";
export function DuplicatesPage() {
  const root = useStorageRoot("duplicates");
  const [depth, setDepth] = useState("20"), [minimum, setMinimum] = useState("1");
  const [maximum, setMaximum] = useState(""), [extensions, setExtensions] = useState("");
  const [group, setGroup] = useState<DuplicateGroup | null>(null);
  const [keeper, setKeeper] = useState<string | null>(null);
  const scope = JSON.stringify([root.choice?.rootId, depth, minimum, maximum, extensions]);
  const scan = useStorageScan<DuplicateRow>({ module: "duplicates", scopeKey: scope,
    collection: group ? "duplicateMembers" : "duplicateGroups", parentId: group?.groupId,
    candidateId: row => selectableMember(row, group?.groupId, keeper) });
  useEffect(() => { setGroup(null); setKeeper(null); }, [scope]);
  useEffect(() => {
    const first = scan.page?.records[0];
    if (group && !keeper && !scan.loadingPage && first?.kind === "duplicateMember" && first.record.groupId === group.groupId) setKeeper(first.record.file.recordId);
  }, [group, keeper, scan.page, scan.loadingPage]);
  const navigate = (next: DuplicateGroup | null) => { scan.clearSelection(); setKeeper(null); setGroup(next); };
  const active = ["starting", "scanning", "cancelling"].includes(scan.phase);
  const config = parseFields(depth, minimum, maximum, extensions);
  return <section className="duplicates-page" aria-labelledby="duplicates-title">
    <header className="page-header"><div><p className="eyebrow">Personal-file cleanup</p><h1 id="duplicates-title">Duplicate files</h1><p>Review independently verified copies. Nothing is selected automatically.</p></div></header>
    <button disabled={active || root.picking} onClick={() => void root.acquire(() => chooseStorageRoot("duplicates"))}>Choose folder</button>
    {root.choice && <p>Selected folder: {root.choice.displayPath}</p>}
    {root.choice && !root.available && !active && <p>Choose a folder again to scan. Folder authorizations are single-use.</p>}
    {root.error && <p role="alert">{root.error}</p>}
    <form onSubmit={e => { e.preventDefault(); if (config && root.available && !active) { navigate(null); void scan.start(() => root.run(id => startDuplicates(id, config.depth, config.minimumBytes, config.maximumBytes, config.extensions))); } }}>
      <fieldset disabled={active || root.picking}><legend>Scan filters</legend>
        <label>Minimum size (MiB)<input type="number" min="0" step="any" required value={minimum} onChange={e => setMinimum(e.target.value)} /></label>
        <label>Maximum size (MiB, blank for no maximum)<input type="number" min="0" step="any" value={maximum} onChange={e => setMaximum(e.target.value)} /></label>
        <label>Extensions (comma-separated, blank for all)<input type="text" maxLength={2112} value={extensions} onChange={e => setExtensions(e.target.value)} /></label>
        <label>Scan depth<input type="number" min="0" max="64" step="1" required value={depth} onChange={e => setDepth(e.target.value)} /></label>
        {!config && <p role="alert">Enter depth 0–64, valid minimum/maximum sizes, and at most 64 alphanumeric extensions.</p>}
        <button disabled={!config || !root.available}>Scan for duplicates</button>
      </fieldset>
    </form>
    <StorageScanStatus phase={scan.phase} status={scan.status} error={scan.error} cancel={() => void scan.cancel()} />
    {scan.phase === "ready" && <>
      <p>Results are bounded and may be incomplete. Hard links count as one independent copy. Native checks revalidate content and the retained copy before cleanup.</p>
      <p>Changing groups clears selection. The first member on the first page is reserved as an unselectable keeper across all member pages.</p>
      {group && <><button onClick={() => navigate(null)}>Back to groups (clear selection)</button><h2>{group.independentCopies} independent copies</h2><p>{group.memberCount} members · {formatBytes(group.bytesPerCopy)} per copy</p>{!keeper && <p>Waiting for the first-page keeper. Selection is disabled.</p>}</>}
      <ul className="cleanup-records" aria-label={group ? "Duplicate members" : "Duplicate groups"}>
        {!scan.loadingPage && scan.page?.records.map(row => {
          if (row.kind === "duplicateGroup") return !group && <li key={row.record.groupId}><p>{row.record.independentCopies} independent copies · {row.record.memberCount} members · {formatBytes(row.record.bytesPerCopy)} per copy</p><button onClick={() => navigate(row.record)}>Review group</button></li>;
          if (row.record.groupId !== group?.groupId) return null;
          const file = row.record.file, id = selectableMember(row, group.groupId, keeper), selected = !!id && scan.selected.has(id);
          return <li key={file.recordId}><label><input type="checkbox" checked={selected} disabled={!id || (!selected && scan.selected.size >= MAX_SELECTION)} onChange={() => { if (id) scan.toggle(id); }} />{file.displayPath}</label><p>{formatBytes(file.logicalBytes)} logical{file.recordId === keeper ? " · Keeper (retained)" : ""}</p></li>;
        })}
      </ul>
      {scan.page && !scan.loadingPage && !scan.page.records.length && <p>No matches on this page.</p>}
      <StoragePaging loading={scan.loadingPage || (!!group && !keeper)} hasNext={!!scan.page?.nextCursor} count={scan.page?.records.length ?? 0} total={scan.page?.retainedTotal ?? 0} firstPage={() => void scan.firstPage()} nextPage={() => void scan.nextPage()} />
      <p role="status">{scan.selected.size} selected (maximum {MAX_SELECTION}). App recovery keeps copies on the same volume for undo and does not free disk space. Permanent deletion is nonundoable and requires a separate Windows confirmation.</p>
      <button disabled={!scan.selected.size} onClick={scan.clearSelection}>Clear selection</button>
    </>}
    <StoragePlanReview selection={scan.selection} dispositions={["quarantine", "permanent"]} onExecuted={() => { navigate(null); void scan.reset(); }} />
  </section>;
}
