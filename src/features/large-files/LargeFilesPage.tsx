import { useState } from "react";
import { formatBytes } from "../../shared/format";
import { chooseStorageRoot } from "../../shared/storage/api";
import { StoragePaging, StorageScanStatus } from "../../shared/storage/StorageScanStatus";
import { StoragePlanReview } from "../../shared/storage/StoragePlanReview";
import { MAX_SELECTION } from "../../shared/storage/types";
import { useStorageRoot } from "../../shared/storage/useStorageRoot";
import { useStorageScan } from "../../shared/storage/useStorageScan";
import { defaultFields, parseLargeFileFields, startLargeFiles, type LargeFileFields } from "./api";
import { candidateId, categories, sorts, type LargeFileRow } from "./types";
import "./large-files.css";

function modified(seconds: number | null): string {
  if (seconds === null) return "Unknown modification time";
  const date = new Date(seconds * 1_000);
  return Number.isFinite(date.getTime()) ? date.toLocaleString() : "Unknown modification time";
}
export function LargeFilesPage() {
  const root = useStorageRoot("largeFiles");
  const [fields, setFields] = useState<LargeFileFields>(defaultFields);
  const config = parseLargeFileFields(fields);
  const scan = useStorageScan<LargeFileRow>({ module: "largeFiles", collection: "files", candidateId,
    scopeKey: JSON.stringify([root.choice?.rootId ?? null, fields]) });
  const active = scan.phase === "starting" || scan.phase === "scanning" || scan.phase === "cancelling";
  const change = <K extends keyof LargeFileFields,>(key: K, value: LargeFileFields[K]) => setFields(previous => ({ ...previous, [key]: value }));
  const start = () => {
    if (!config || !root.available || root.picking || active) return;
    void scan.start(() => root.run(rootId => startLargeFiles(rootId, config.depth, config.filter)));
  };
  const partial = !!scan.status?.completeness.reasons.length || !!scan.page?.completeness.reasons.length;

  return <section className="large-files-page" aria-labelledby="large-files-title">
    <header className="page-header"><div><p className="eyebrow">Personal-file cleanup</p><h1 id="large-files-title">Large files</h1>
      <p>Find large files in a chosen folder, then explicitly select files to review. Nothing is selected automatically.</p></div></header>
    <button type="button" disabled={root.picking || active} onClick={() => void root.acquire(() => chooseStorageRoot("largeFiles"))}>{root.picking ? "Choosing folder…" : "Choose folder"}</button>
    {root.choice && <p>Selected folder: {root.choice.displayPath}</p>}
    {root.choice && !root.available && !active && <p>Choose a folder again to scan. Folder authorizations are single-use.</p>}
    {root.error && <p role="alert">{root.error}</p>}
    <form onSubmit={event => { event.preventDefault(); start(); }}>
      <fieldset disabled={active || root.picking} aria-describedby="large-files-filter-help">
        <legend>Scan filters</legend>
        <div className="large-files-controls">
          <label>Minimum size (MiB)<input type="number" min="0" step="any" required value={fields.minimumMiB} onChange={e => change("minimumMiB", e.target.value)} /></label>
          <label>Maximum size (MiB, optional)<input type="number" min="0" step="any" value={fields.maximumMiB} onChange={e => change("maximumMiB", e.target.value)} /></label>
          <label>Scan depth<input type="number" min="0" max="64" step="1" required value={fields.depth} onChange={e => change("depth", e.target.value)} /></label>
          <label>Extensions<input type="text" maxLength={2112} value={fields.extensions} onChange={e => change("extensions", e.target.value)} placeholder="pdf, zip" /></label>
          <label>Category<select value={fields.category} onChange={e => change("category", e.target.value as LargeFileFields["category"])}>{categories.map(category => <option key={category} value={category}>{category === "any" ? "Any category" : category[0].toUpperCase() + category.slice(1)}</option>)}</select></label>
          <label>Sort by<select value={fields.sort} onChange={e => change("sort", e.target.value as LargeFileFields["sort"])}>{sorts.map(sort => <option key={sort} value={sort}>{sort === "modified" ? "Modification time" : sort === "size" ? "Size" : "Path"}</option>)}</select></label>
          <label className="large-files-check"><input type="checkbox" checked={fields.descending} onChange={e => change("descending", e.target.checked)} />Descending order</label>
        </div>
        <p id="large-files-filter-help">Depth 0–64. Sizes must convert to safe whole-byte values; maximum must be at least minimum. Leave maximum blank for no maximum. Extensions: up to 64 comma- or space-separated bare alphanumeric names, at most 32 characters each; uppercase is normalized. Leave blank for all extensions.</p>
        {!config && <p role="alert">Enter valid sizes, depth and extensions before scanning.</p>}
        <button type="submit" disabled={!config || !root.available}>Scan for large files</button>
      </fieldset>
    </form>
    <StorageScanStatus phase={scan.phase} status={scan.status} error={scan.error} cancel={() => void scan.cancel()} />
    {scan.phase === "ready" && <>
      {partial && <p role="note">Partial results: traversal or retention limits, protected, inaccessible or changing files may omit matches. These are not necessarily the globally largest files; narrow the scope and scan again.</p>}
      <p>Logical and allocated bytes describe files, not reclaimed space. Hard links and backend safety checks can affect cleanup outcomes.</p>
      <p role="status">{scan.selected.size} selected (maximum {MAX_SELECTION.toLocaleString()}). App recovery keeps files for undo on the same volume without freeing disk space. Windows Recycle Bin is not supported for these identity-checked actions. Permanent deletion requires a separate Windows confirmation.</p>
      <button type="button" disabled={!scan.selected.size} onClick={scan.clearSelection}>Clear selection</button>
      {scan.page && !scan.loadingPage && <>
        {!scan.page.records.length && <p>No matching files on this page.</p>}
        <ul className="cleanup-records large-files-records" aria-label="Large file results">{scan.page.records.map(row => {
          const record = row.record;
          const id = candidateId(row);
          const selected = id !== null && scan.selected.has(id);
          return <li key={record.recordId}>
            <label className="large-files-check"><input type="checkbox" checked={selected} disabled={!id || (!selected && scan.selected.size >= MAX_SELECTION)} onChange={() => { if (id) scan.toggle(id); }} /><span>{record.displayPath}</span></label>
            <p>{formatBytes(record.logicalBytes)} logical · {record.allocatedBytes === null ? "Unknown allocation" : `${formatBytes(record.allocatedBytes)} allocated`} · {modified(record.modifiedUnixSeconds)}</p>
            {record.eligibility.kind === "readOnly" && <p>Read-only result: not selectable.</p>}
            {record.eligibility.kind === "ineligible" && <p>Not eligible: {record.eligibility.reason}</p>}
          </li>;
        })}</ul>
      </>}
      <StoragePaging loading={scan.loadingPage} hasNext={!!scan.page?.nextCursor} count={scan.page?.records.length ?? 0} total={scan.page?.retainedTotal ?? 0} firstPage={() => void scan.firstPage()} nextPage={() => void scan.nextPage()} />
    </>}
    <StoragePlanReview selection={scan.selection} dispositions={["quarantine", "permanent"]} onExecuted={() => { void scan.reset(); }} />
  </section>;
}
