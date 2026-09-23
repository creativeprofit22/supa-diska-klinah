import { useMemo, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { StoragePaging, StorageScanStatus } from "../StorageScanStatus";
import { StoragePlanReview } from "../StoragePlanReview";
import { useStorageScan } from "../useStorageScan";
import type { ScanApi } from "../types";
import type { CleanupDisposition, CleanupExecutionSummary, CleanupPlanSummary } from "../../cleanup/api";
import "../../../styles.css";

// Browser-only fixture entry, never imported by the application/router. There is
// no native host. Every operation is injected; mutation attempts fail closed.
interface Row { id: string; path: string }
const id = (n: number) => n.toString(16).padStart(32,"0");
let mutationAttempts = 0;
async function forbidden(): Promise<CleanupExecutionSummary> { mutationAttempts++; throw new Error("Fixture prohibits mutation"); }
const actions = {executeCleanupPlan:forbidden,executePermanentCleanupPlan:forbidden,undoCleanup:forbidden,cleanupHistory:async()=>({records:[],nextCursor:null})};
const dispositions: readonly CleanupDisposition[] = ["recycleBin", "permanent"];
const delay = (ms: number) => new Promise<void>(resolve=>setTimeout(resolve,ms));
function StorageFixture() {
  const [mode, setMode] = useState("complete");
  const [scope, setScope] = useState(1);
  const sequence = useRef(0);
  const polling = useRef(0);
  const api = useMemo<ScanApi<Row>>(()=>({
    status: async ({snapshotId}) => {
      await delay(150);
      polling.current++;
      if(mode === "expired") throw {code:"snapshot_unavailable"};
      return {snapshotId,module:"largeFiles",phase:mode === "slow" ? "walking" : "complete",visitedEntries:8,retainedRecords:8,hashedBytes:0,completedHashes:0,completeness:{reasons:mode === "partial" ? ["entryLimit"] : []}};
    },
    page: async ({snapshotId,cursor}) => {
      await delay(200);
      const start=cursor ? 5 : 1;
      return {snapshotId,records:mode === "empty" ? [] : Array.from({length:4},(_,i)=>({id:id(start+i+10),path:`C:\\Fixture only\\Archives\\Long project name with spaces\\${"nested-folder-".repeat(5)}\\Recording ${start+i}.bin`})),nextCursor:mode === "empty" || cursor ? null : id(100),retainedTotal:mode === "empty" ? 0 : 8,completeness:{reasons:mode === "partial" ? ["entryLimit"] : []}};
    },
    cancel:async()=>{},release:async()=>{},
  }),[mode]);
  const scan=useStorageScan({module:"largeFiles",scopeKey:`fixture-${scope}:${mode}`,collection:"files",api,candidateId:(row:Row)=>row.id});
  const createPlan=async (selection: {candidateIds: readonly string[]}, disposition: CleanupDisposition):Promise<CleanupPlanSummary>=>({planId:id(200),disposition,selectedCount:selection.candidateIds.length,selectedBytes:selection.candidateIds.length*1024});
  return <main style={{maxWidth:"52rem",marginInline:"auto",padding:"1rem"}}>
    <h1>Storage component fixture</h1>
    <p>Simulated data only. Native IPC, cleanup, and undo cannot run here.</p>
    <div className="cleanup-actions">
      <label>Fixture state <select value={mode} onChange={e=>setMode(e.target.value)}>
        <option value="complete">Complete</option><option value="slow">Slow scan</option><option value="partial">Partial</option><option value="empty">Empty</option><option value="expired">Expired</option><option value="busy">Busy</option>
      </select></label>
      <button type="button" onClick={()=>setScope(s=>s+1)}>Change scope</button>
      <button type="button" onClick={()=>void scan.start(async()=>{if(mode === "busy")throw {code:"busy"};return id(++sequence.current);})}>Start fixture scan</button>
    </div>
    <StorageScanStatus phase={scan.phase} status={scan.status} error={scan.error} cancel={()=>void scan.cancel()}/>
    <p role="status">{scan.selected.size} selected</p>
    {scan.page && <ul className="cleanup-records" aria-label="Fixture scan results">
      {scan.page.records.map(row=><li key={row.id}><label className="cleanup-selection" style={{minHeight:44}}><input type="checkbox" checked={scan.selected.has(row.id)} onChange={()=>scan.toggle(row.id)}/><span className="cleanup-path">{row.path}</span></label></li>)}
      {!scan.page.records.length && <li>No eligible items found.</li>}
    </ul>}
    {scan.phase === "ready" && <StoragePaging loading={scan.loadingPage} hasNext={!!scan.page?.nextCursor} count={scan.page?.records.length??0} total={scan.page?.retainedTotal??0} firstPage={()=>void scan.firstPage()} nextPage={()=>void scan.nextPage()}/>}
    <StoragePlanReview selection={scan.selection} dispositions={dispositions} createPlan={createPlan} actions={actions} onExecuted={scan.clearSelection}/>
    <output aria-label="Fixture safety counter">Mutation attempts: {mutationAttempts}</output>
  </main>;
}
const root = document.getElementById("root");
if (!root) throw new Error("Fixture root missing");
createRoot(root).render(<StorageFixture/>);
