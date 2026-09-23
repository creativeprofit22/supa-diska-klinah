import { useLayoutEffect, useRef, useState } from "react";
import { formatBytes } from "../../shared/format";
import { StoragePaging, StorageScanStatus } from "../../shared/storage/StorageScanStatus";
import { useStorageScan } from "../../shared/storage/useStorageScan";
import { outcome, pending, startProgramInventory, type ProgramRow, type VendorJob } from "./api";
import { useVendorJobs } from "./useVendorJobs";
import "./uninstaller.css";

function JobDetails({ job }: { job: VendorJob }) {
  return <><h3>{job.programName}</h3><p>Job ID: {job.jobId}</p><p role="status">{outcome[job.state]}</p>
    {job.exitCode !== null && <p>Launcher exit code: {job.exitCode}</p>}
    {job.launchError !== null && <p>Windows launch error code: {job.launchError}</p>}
    {job.persistenceError && <p role="alert">Journal persistence failed. The outcome may not survive restart; do not assume removal or retry automatically.</p>}
    <p>Leftovers: unknown ownership; informational only, not selectable.</p></>;
}
export function UninstallerPage() {
  const [nameContains, setName] = useState("");
  const [largestFirst, setLargest] = useState(false);
  const key = JSON.stringify([nameContains, largestFirst]);
  const scan = useStorageScan<ProgramRow>({ module: "uninstaller", collection: "programs", scopeKey: key });
  const jobs = useVendorJobs(JSON.stringify([key, scan.status?.snapshotId ?? null]));
  const review = useRef<HTMLElement>(null);
  const page = useRef<HTMLElement>(null);
  const focusRequest = useRef<string | null>(null);
  useLayoutEffect(() => {
    if (focusRequest.current !== key) { focusRequest.current = null; return; }
    if (jobs.busy) return;
    focusRequest.current = null;
    (review.current ?? page.current)?.focus();
  }, [key, jobs.busy, jobs.job, jobs.error]);
  const active = scan.phase === "starting" || scan.phase === "scanning" || scan.phase === "cancelling";
  const valid = nameContains.length <= 128 && !/[\u0000-\u001f\u007f-\u009f]/u.test(nameContains);
  return <section ref={page} tabIndex={-1} className="uninstaller-page" aria-labelledby="uninstaller-title">
    <header className="page-header"><div><p className="eyebrow">Separate vendor operations</p><h1 id="uninstaller-title">Installed programs</h1>
      <p>Read-only registry inventory. Select one program explicitly to review a separate vendor uninstall job. No programs are selected automatically.</p></div></header>
    <form onSubmit={e => { e.preventDefault(); if (valid && !active) void scan.start(() => startProgramInventory({ nameContains, largestFirst })); }}>
      <label>Program name<input maxLength={128} value={nameContains} onChange={e => setName(e.target.value)} /></label>
      <label>Sort<select value={largestFirst ? "size" : "name"} onChange={e => setLargest(e.target.value === "size")}><option value="name">Name</option><option value="size">Estimated size, largest first</option></select></label>
      {!valid && <p role="alert">Name must be at most 128 UTF-16 units, without control characters.</p>}
      <button type="submit" disabled={active || !valid}>Refresh inventory</button>
    </form>
    <StorageScanStatus idleMessage="Refresh inventory to list installed programs." phase={scan.phase} status={scan.status} error={scan.error} cancel={() => void scan.cancel()} />
    <p>Sizes are vendor registry estimates, not measured disk use or reclaimed space. Install dates are vendor-reported; last use is unknown when unavailable. Refresh inventory to observe changes; it does not prove removal.</p>
    {scan.phase === "ready" && <>
      {!!scan.page?.completeness.reasons.length && <p role="note">Partial inventory: bounded enumeration or unavailable registry entries may omit programs.</p>}
      <ul aria-label="Installed program results">{!scan.loadingPage && scan.page?.records.map(({ record: p }) => <li key={p.programId}>
        <h2>{p.name}</h2><p>Publisher: {p.publisher ?? "Unknown"} · Version: {p.version ?? "Unknown"}</p>
        <p>Estimated size: {p.estimatedSizeBytes === null ? "Unknown" : formatBytes(p.estimatedSizeBytes)} · Vendor-reported install date: {p.installDate ?? "Unknown"} · Last use: {p.lastUsedAt === null ? "Unknown" : new Date(p.lastUsedAt * 1000).toLocaleString()}</p>
        <p>Leftovers: unknown ownership; informational only, not selectable.</p>
        <button type="button" disabled={jobs.busy || !!jobs.job} onClick={() => { if (scan.page) { focusRequest.current = key; void jobs.prepare(scan.page.snapshotId, p.programId); } }}>Review vendor uninstall for {p.name}</button>
      </li>)}</ul>
      {scan.page?.records.length === 0 && <p>No matching programs on this page.</p>}
      <StoragePaging loading={scan.loadingPage} hasNext={!!scan.page?.nextCursor} count={scan.page?.records.length ?? 0} total={scan.page?.retainedTotal ?? 0} firstPage={() => void scan.firstPage()} nextPage={() => void scan.nextPage()} />
    </>}
    {jobs.error && <p role="alert">{jobs.error}</p>}
    {jobs.busy && <p role="status">Waiting for the vendor request or Windows confirmation…</p>}
    {jobs.job && <section ref={review} tabIndex={-1} aria-label="Immutable vendor job review">{jobs.reviewUnavailable ? <><h3>{jobs.job.programName}</h3><p>Job ID: {jobs.job.jobId}</p><p role="status">Current vendor evidence unavailable.</p></> : <JobDetails job={jobs.job} />}
      <p>Vendor uninstall cannot be undone here. Windows confirmation will show the backend-resolved command; vendor UI or UAC may follow.</p>
      {!jobs.reviewUnavailable && jobs.job.state === "awaitingConfirmation" && <button type="button" disabled={jobs.busy || jobs.submitted} onClick={() => { focusRequest.current = key; void jobs.confirm(); }}>Continue to Windows confirmation</button>}
      {!jobs.reviewUnavailable && (jobs.job.state === "awaitingConfirmation" || pending(jobs.job)) && <button type="button" disabled={jobs.busy} onClick={() => { focusRequest.current = key; void jobs.cancel(); }}>{jobs.job.state === "awaitingConfirmation" ? "Cancel prepared job" : "Stop waiting (does not stop installer)"}</button>}
    </section>}
    <section aria-label="Retained vendor job history"><h2>Retained vendor jobs</h2>
      <p>Up to 64 retained jobs per page, newest first. Navigation stops polling, not the vendor installer. Unknown outcomes cannot be erased or replayed.</p>
      <p>Up to 1,000 outcomes are retained, subject to an 8 MiB storage limit. At capacity, new jobs stop until a future journal migration; retained history remains available for inspection.</p>
      <button type="button" disabled={jobs.historyLoading} onClick={() => void jobs.refreshHistory()}>Refresh retained history</button>
      <button type="button" disabled={jobs.historyLoading || !jobs.historyNextCursor} onClick={() => void jobs.olderHistory()}>Older retained jobs</button>
      {jobs.historyLoading && <p role="status">Loading retained history…</p>}
      {jobs.historyError && <p role="alert">{jobs.historyError}</p>}
      {!jobs.historyLoaded && !jobs.historyLoading && !jobs.historyError && <p>Retained history has not been loaded.</p>}
      {jobs.historyLoaded && !jobs.historyLoading && !jobs.historyError && jobs.history.length === 0 && jobs.historyCursor === null && <p>No retained jobs reported.</p>}
      {jobs.historyLoaded && !jobs.historyLoading && !jobs.historyError && !jobs.historyNextCursor && (jobs.history.length > 0 || jobs.historyCursor !== null) && <p>End of retained history.</p>}
      <ul>{jobs.history.map(job => <li key={job.jobId}><JobDetails job={job} /></li>)}</ul>
    </section>
  </section>;
}
