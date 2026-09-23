import { useEffect, useRef, useState } from "react";
import { chooseStorageRoot, releaseStorageScan, storageError } from "../../shared/storage/api";
import { StoragePaging, StorageScanStatus } from "../../shared/storage/StorageScanStatus";
import { useStorageScan } from "../../shared/storage/useStorageScan";
import type { RootChoice } from "../../shared/storage/types";
import { formatBytes } from "../../shared/format";
import { startDiskAnalyzer } from "./api";
import type { AnalyzerRow, DirectorySummary } from "./types";
import "./disk-analyzer.css";

function folderName(path: string): string { return path.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || path; }
function allocation(bytes: number | null): string { return bytes === null ? "Unknown allocation" : formatBytes(bytes); }

export function DiskAnalyzerPage() {
  const [choice, setChoice] = useState<RootChoice | null>(null);
  const [rootAvailable, setRootAvailable] = useState(false);
  const [picking, setPicking] = useState(false);
  const [pickerError, setPickerError] = useState<string | null>(null);
  const [depth, setDepth] = useState("3");
  const [view, setView] = useState<"tree" | "extensions">("tree");
  const [trail, setTrail] = useState<DirectorySummary[]>([]);
  const [metadataSnapshot, setMetadataSnapshot] = useState<string | null>(null);
  const unusedRoot = useRef<string | null>(null);
  const pickerVersion = useRef(0);
  const pickerLock = useRef(false);
  const headings = useRef<HTMLHeadingElement>(null);
  const current = trail[trail.length - 1];
  const displayedDepth = Number(depth);
  const validDepth = depth.trim() !== "" && Number.isInteger(displayedDepth) && displayedDepth >= 0 && displayedDepth <= 64;
  const scan = useStorageScan<AnalyzerRow>({module:"diskAnalyzer",scopeKey:`${choice?.rootId ?? "none"}:${depth}`,collection:view,parentId:view === "tree" ? current?.nodeId : undefined});
  const active = scan.phase === "starting" || scan.phase === "scanning" || scan.phase === "cancelling";
  const hasTotals = scan.phase === "ready" && !scan.error && scan.status?.snapshotId === metadataSnapshot && !!current;
  const releaseRoot = (id: string) => releaseStorageScan({module:"diskAnalyzer",snapshotId:id}).catch(()=>{});

  useEffect(() => () => {
    pickerVersion.current++;
    if (unusedRoot.current) void releaseRoot(unusedRoot.current);
    unusedRoot.current = null;
  }, []);
  useEffect(() => {
    // The first tree page contains only the root. Retain bounded breadcrumb
    // summaries, not previous pages; every navigation thereafter is snapshot-bound.
    if (scan.phase !== "ready" || !scan.page || trail.length || view !== "tree") return;
    const root = scan.page.records.find(row=>row.kind === "directory" && row.record.parentId === null);
    if (root?.kind === "directory") { setTrail([root.record]); setMetadataSnapshot(scan.page.snapshotId); }
  }, [scan.phase, scan.page, trail.length, view]);

  const choose = async () => {
    if (pickerLock.current || active) return;
    pickerLock.current = true; setPicking(true); setPickerError(null);
    const version = pickerVersion.current;
    try {
      const root = await chooseStorageRoot("diskAnalyzer");
      if (version !== pickerVersion.current) { if(root) await releaseRoot(root.rootId); return; }
      if (!root) return; // Native cancellation leaves scope and results unchanged.
      if (unusedRoot.current) await releaseRoot(unusedRoot.current);
      if (version !== pickerVersion.current) { await releaseRoot(root.rootId); return; }
      unusedRoot.current = root.rootId;
      setChoice(root); setRootAvailable(true); setTrail([]); setMetadataSnapshot(null); setView("tree");
    } catch (error) { if(version === pickerVersion.current) setPickerError(storageError(error)); }
    finally { pickerLock.current=false; if(version === pickerVersion.current) setPicking(false); }
  };
  const start = () => {
    const rootId=unusedRoot.current;
    if(!rootId || !validDepth || active || picking) return;
    unusedRoot.current=null; setRootAvailable(false); setTrail([]); setMetadataSnapshot(null); setView("tree");
    void scan.start(async()=>{
      try { return await startDiskAnalyzer(rootId,displayedDepth); }
      finally { await releaseRoot(rootId); } // Also retire an authorization if start was busy or failed.
    });
  };
  const navigate = (next: DirectorySummary[]) => { setTrail(next); headings.current?.focus(); };

  return <section className="analyzer-page" aria-labelledby="analyzer-title">
    <header className="page-header">
      <div><p className="eyebrow">Read-only storage analysis</p><h1 id="analyzer-title">Disk analyzer</h1><p>Inspect folder totals and file extensions. This page cannot delete or move files.</p></div>
    </header>
    <div className="analyzer-controls">
      <button type="button" disabled={picking || active} onClick={()=>void choose()}>{picking ? "Choosing folder…" : "Choose folder"}</button>
      <label>Displayed folder depth <input type="number" min={0} max={64} step={1} value={depth} disabled={active || picking} aria-describedby="depth-help" aria-invalid={!validDepth} onChange={e=>{setDepth(e.target.value);setTrail([]);setMetadataSnapshot(null);setView("tree");}}/></label>
      <button type="button" disabled={!rootAvailable || !validDepth || picking || active} onClick={start}>Analyze folder</button>
    </div>
    <p id="depth-help">Depth 0–64 limits displayed folders, not subtree totals. Scans still have native traversal and retention limits.</p>
    {!validDepth && <p role="alert">Enter a whole number from 0 to 64.</p>}
    {choice && <p className="analyzer-path">Selected folder: {choice.displayPath}</p>}
    {choice && !rootAvailable && !active && <p>Choose a folder again to start another scan. Folder authorizations are single-use.</p>}
    {pickerError && <p role="alert">{pickerError}</p>}
    <StorageScanStatus phase={scan.phase} status={scan.status} error={scan.error} cancel={()=>void scan.cancel()}/>
    {hasTotals && <>
      <section className="cleanup-state-panel" aria-label="Scanned root totals">
        <h2>Scanned root totals</h2><p className="analyzer-path">{trail[0].displayPath}</p>
        <dl className="analyzer-totals"><div><dt>Logical size</dt><dd>{formatBytes(trail[0].logicalBytes)}</dd></div><div><dt>Allocated size</dt><dd>{allocation(trail[0].allocatedBytes)}</dd></div><div><dt>Independent files</dt><dd>{trail[0].independentFiles.toLocaleString()}</dd></div><div><dt>Additional hard-link entries</dt><dd>{trail[0].hardLinkEntries.toLocaleString()}</dd></div></dl>
        {!!trail[0].completeness.reasons.length && <p role="note">Incomplete totals: inaccessible, changing, protected, or bounded-out entries may be omitted.</p>}
      </section>
      <p>Logical size is not reclaimable space. Hard links are counted once per subtree or extension; sibling totals may not add to the root.</p>
      <div className="cleanup-actions" aria-label="Analysis view">
        <button type="button" aria-pressed={view === "tree"} onClick={()=>setView("tree")}>Folders</button>
        <button type="button" aria-pressed={view === "extensions"} onClick={()=>setView("extensions")}>Extensions</button>
      </div>
      {view === "tree" && <nav aria-label="Folder breadcrumbs"><ol className="analyzer-breadcrumbs">{trail.map((node,index)=><li key={node.nodeId}><button type="button" aria-current={index === trail.length-1 ? "location" : undefined} onClick={()=>navigate(trail.slice(0,index+1))}>{index === 0 ? "Scanned root" : folderName(node.displayPath)}</button></li>)}</ol></nav>}
      <h2 ref={headings} tabIndex={-1}>{view === "tree" ? `Child folders of ${folderName(current.displayPath)}` : "Extensions across the scanned root"}</h2>
      {view === "tree" && <p>Current subtree: {formatBytes(current.logicalBytes)} logical · {allocation(current.allocatedBytes)} allocated</p>}
    </>}
    {hasTotals && scan.page && !scan.loadingPage && <>
      {!scan.page.records.length && <p role="status">{view === "tree" ? "No child folders within the displayed depth. Totals can still include deeper files." : "No file extensions found."}</p>}
      <ul className="cleanup-records analyzer-records" aria-label={view === "tree" ? "Child folders" : "File extensions"}>
        {scan.page.records.map(row=>row.kind === "directory" ? <li key={row.record.nodeId}>
          <button className="analyzer-folder" type="button" disabled={trail.length > 64} onClick={()=>navigate([...trail,row.record])}>{folderName(row.record.displayPath)}</button>
          <p>{formatBytes(row.record.logicalBytes)} logical · {allocation(row.record.allocatedBytes)} allocated · {row.record.independentFiles.toLocaleString()} independent files</p>
          {!!row.record.completeness.reasons.length && <p>Partial subtree totals</p>}
        </li> : <li key={row.record.extension}><strong>{row.record.extension ? `.${row.record.extension}` : "No extension"}</strong><p>{row.record.fileCount.toLocaleString()} {row.record.fileCount === 1 ? "file" : "files"} · {formatBytes(row.record.logicalBytes)} logical · {allocation(row.record.allocatedBytes)} allocated</p>{!!row.record.completeness.reasons.length && <p>Partial extension totals</p>}</li>)}
      </ul>
    </>}
    {hasTotals && <StoragePaging loading={scan.loadingPage} hasNext={!!scan.page?.nextCursor} count={scan.page?.records.length??0} total={scan.page?.retainedTotal??0} firstPage={()=>void scan.firstPage()} nextPage={()=>void scan.nextPage()}/>}
  </section>;
}
