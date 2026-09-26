import { useEffect, useState } from "react";
import { useFormat, useStrings } from "../../shared/i18n/I18nProvider";
import { chooseStorageRoot } from "../../shared/storage/api";
import { StoragePaging, StorageScanStatus } from "../../shared/storage/StorageScanStatus";
import { StoragePlanReview } from "../../shared/storage/StoragePlanReview";
import { MAX_SELECTION } from "../../shared/storage/types";
import { useStorageRoot } from "../../shared/storage/useStorageRoot";
import { useStorageScan } from "../../shared/storage/useStorageScan";
import { parseFields, startDuplicates } from "./api";
import { duplicatesStrings } from "./strings";
import { selectableMember, type DuplicateGroup, type DuplicateRow } from "./types";
import "./duplicates.css";
export function DuplicatesPage() {
  const t = useStrings(duplicatesStrings);
  const fmt = useFormat();
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
    <header className="page-header"><div><p className="eyebrow">{t.eyebrow}</p><h1 id="duplicates-title">{t.title}</h1><p>{t.intro}</p></div></header>
    <button disabled={active || root.picking} onClick={() => void root.acquire(() => chooseStorageRoot("duplicates"))}>{t.chooseFolder}</button>
    {root.choice && <p>{t.selectedFolder(root.choice.displayPath)}</p>}
    {root.choice && !root.available && !active && <p>{t.chooseAgain}</p>}
    {root.error && <p role="alert">{root.error}</p>}
    <form onSubmit={e => { e.preventDefault(); if (config && root.available && !active) { navigate(null); void scan.start(() => root.run(id => startDuplicates(id, config.depth, config.minimumBytes, config.maximumBytes, config.extensions))); } }}>
      <fieldset disabled={active || root.picking}><legend>{t.filters}</legend>
        <label>{t.minimum}<input type="number" min="0" step="any" required value={minimum} onChange={e => setMinimum(e.target.value)} /></label>
        <label>{t.maximum}<input type="number" min="0" step="any" value={maximum} onChange={e => setMaximum(e.target.value)} /></label>
        <label>{t.extensions}<input type="text" maxLength={2112} value={extensions} onChange={e => setExtensions(e.target.value)} /></label>
        <label>{t.depth}<input type="number" min="0" max="64" step="1" required value={depth} onChange={e => setDepth(e.target.value)} /></label>
        {!config && <p role="alert">{t.invalid}</p>}
        <button disabled={!config || !root.available}>{t.scan}</button>
      </fieldset>
    </form>
    <StorageScanStatus phase={scan.phase} status={scan.status} error={scan.error} cancel={() => void scan.cancel()} />
    {scan.phase === "ready" && <>
      <p>{t.resultsNote}</p>
      <p>{t.keeperNote}</p>
      {group && <><button onClick={() => navigate(null)}>{t.backToGroups}</button><h2>{t.independentCopies(group.independentCopies)}</h2><p>{t.groupSummary(group.memberCount, fmt.bytes(group.bytesPerCopy))}</p>{!keeper && <p>{t.waitingKeeper}</p>}</>}
      <ul className="cleanup-records" aria-label={group ? t.membersLabel : t.groupsLabel}>
        {!scan.loadingPage && scan.page?.records.map(row => {
          if (row.kind === "duplicateGroup") return !group && <li key={row.record.groupId}><p>{t.groupRow(row.record.independentCopies, row.record.memberCount, fmt.bytes(row.record.bytesPerCopy))}</p><button onClick={() => navigate(row.record)}>{t.reviewGroup}</button></li>;
          if (row.record.groupId !== group?.groupId) return null;
          const file = row.record.file, id = selectableMember(row, group.groupId, keeper), selected = !!id && scan.selected.has(id);
          return <li key={file.recordId}><label><input type="checkbox" checked={selected} disabled={!id || (!selected && scan.selected.size >= MAX_SELECTION)} onChange={() => { if (id) scan.toggle(id); }} />{file.displayPath}</label><p>{t.logical(fmt.bytes(file.logicalBytes))}{file.recordId === keeper ? t.keeper : ""}</p></li>;
        })}
      </ul>
      {scan.page && !scan.loadingPage && !scan.page.records.length && <p>{t.noMatches}</p>}
      <StoragePaging loading={scan.loadingPage || (!!group && !keeper)} hasNext={!!scan.page?.nextCursor} count={scan.page?.records.length ?? 0} total={scan.page?.retainedTotal ?? 0} firstPage={() => void scan.firstPage()} nextPage={() => void scan.nextPage()} />
      <p role="status">{t.selectionStatus(scan.selected.size, MAX_SELECTION)}</p>
      <button disabled={!scan.selected.size} onClick={scan.clearSelection}>{t.clearSelection}</button>
    </>}
    <StoragePlanReview selection={scan.selection} dispositions={["quarantine", "permanent"]} onExecuted={() => { navigate(null); void scan.reset(); }} />
  </section>;
}
