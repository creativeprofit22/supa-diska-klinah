import { useState } from "react";
import { useFormat, useStrings } from "../../shared/i18n/I18nProvider";
import { chooseStorageRoot } from "../../shared/storage/api";
import { StoragePaging, StorageScanStatus } from "../../shared/storage/StorageScanStatus";
import { StoragePlanReview } from "../../shared/storage/StoragePlanReview";
import { MAX_SELECTION } from "../../shared/storage/types";
import { useStorageRoot } from "../../shared/storage/useStorageRoot";
import { useStorageScan } from "../../shared/storage/useStorageScan";
import { defaultFields, parseLargeFileFields, startLargeFiles, type LargeFileFields } from "./api";
import { largeFilesStrings } from "./strings";
import { candidateId, categories, sorts, type LargeFileRow } from "./types";
import "./large-files.css";

export function LargeFilesPage() {
  const t = useStrings(largeFilesStrings);
  const fmt = useFormat();
  const modified = (seconds: number | null): string =>
    (seconds === null ? null : fmt.dateTime(seconds)) ?? t.unknownModified;
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
    <header className="page-header"><div><p className="eyebrow">{t.eyebrow}</p><h1 id="large-files-title">{t.heading}</h1>
      <p>{t.intro}</p></div></header>
    <button type="button" disabled={root.picking || active} onClick={() => void root.acquire(() => chooseStorageRoot("largeFiles"))}>{root.picking ? t.choosingFolder : t.chooseFolder}</button>
    {root.choice && <p>{t.selectedFolder(root.choice.displayPath)}</p>}
    {root.choice && !root.available && !active && <p>{t.chooseAgain}</p>}
    {root.error && <p role="alert">{root.error}</p>}
    <form onSubmit={event => { event.preventDefault(); start(); }}>
      <fieldset disabled={active || root.picking} aria-describedby="large-files-filter-help">
        <legend>{t.filters}</legend>
        <div className="large-files-controls">
          <label>{t.minimumSize}<input type="number" min="0" step="any" required value={fields.minimumMiB} onChange={e => change("minimumMiB", e.target.value)} /></label>
          <label>{t.maximumSize}<input type="number" min="0" step="any" value={fields.maximumMiB} onChange={e => change("maximumMiB", e.target.value)} /></label>
          <label>{t.depth}<input type="number" min="0" max="64" step="1" required value={fields.depth} onChange={e => change("depth", e.target.value)} /></label>
          <label>{t.extensions}<input type="text" maxLength={2112} value={fields.extensions} onChange={e => change("extensions", e.target.value)} placeholder={t.extensionsPlaceholder} /></label>
          <label>{t.category}<select value={fields.category} onChange={e => change("category", e.target.value as LargeFileFields["category"])}>{categories.map(category => <option key={category} value={category}>{t.categories[category]}</option>)}</select></label>
          <label>{t.sortBy}<select value={fields.sort} onChange={e => change("sort", e.target.value as LargeFileFields["sort"])}>{sorts.map(sort => <option key={sort} value={sort}>{t.sorts[sort]}</option>)}</select></label>
          <label className="large-files-check"><input type="checkbox" checked={fields.descending} onChange={e => change("descending", e.target.checked)} />{t.descending}</label>
        </div>
        <p id="large-files-filter-help">{t.filterHelp}</p>
        {!config && <p role="alert">{t.invalidFilters}</p>}
        <button type="submit" disabled={!config || !root.available}>{t.scan}</button>
      </fieldset>
    </form>
    <StorageScanStatus phase={scan.phase} status={scan.status} error={scan.error} cancel={() => void scan.cancel()} />
    {scan.phase === "ready" && <>
      {partial && <p role="note">{t.partial}</p>}
      <p>{t.bytesNote}</p>
      <p role="status">{t.selection(scan.selected.size, fmt.number(scan.selected.size), fmt.number(MAX_SELECTION))}</p>
      <button type="button" disabled={!scan.selected.size} onClick={scan.clearSelection}>{t.clearSelection}</button>
      {scan.page && !scan.loadingPage && <>
        {!scan.page.records.length && <p>{t.noMatches}</p>}
        <ul className="cleanup-records large-files-records" aria-label={t.resultsLabel}>{scan.page.records.map(row => {
          const record = row.record;
          const id = candidateId(row);
          const selected = id !== null && scan.selected.has(id);
          return <li key={record.recordId}>
            <label className="large-files-check"><input type="checkbox" checked={selected} disabled={!id || (!selected && scan.selected.size >= MAX_SELECTION)} onChange={() => { if (id) scan.toggle(id); }} /><span>{record.displayPath}</span></label>
            <p>{t.fileRow(fmt.bytes(record.logicalBytes), record.allocatedBytes === null ? t.unknownAllocation : t.allocated(fmt.bytes(record.allocatedBytes)), modified(record.modifiedUnixSeconds))}</p>
            {record.eligibility.kind === "readOnly" && <p>{t.readOnly}</p>}
            {record.eligibility.kind === "ineligible" && <p>{t.notEligible(record.eligibility.reason)}</p>}
          </li>;
        })}</ul>
      </>}
      <StoragePaging loading={scan.loadingPage} hasNext={!!scan.page?.nextCursor} count={scan.page?.records.length ?? 0} total={scan.page?.retainedTotal ?? 0} firstPage={() => void scan.firstPage()} nextPage={() => void scan.nextPage()} />
    </>}
    <StoragePlanReview selection={scan.selection} dispositions={["quarantine", "permanent"]} onExecuted={() => { void scan.reset(); }} />
  </section>;
}
