import { useLayoutEffect, useRef, useState } from "react";
import { useFormat, useStrings } from "../../shared/i18n/I18nProvider";
import { StoragePaging, StorageScanStatus } from "../../shared/storage/StorageScanStatus";
import { useStorageScan } from "../../shared/storage/useStorageScan";
import { pending, startProgramInventory, type ProgramRow, type VendorJob } from "./api";
import { uninstallerStrings } from "./strings";
import { useVendorJobs } from "./useVendorJobs";
import "./uninstaller.css";

function JobDetails({ job }: { job: VendorJob }) {
  const t = useStrings(uninstallerStrings);
  return <><h3>{job.programName}</h3><p>{t.jobId(job.jobId)}</p><p role="status">{t.outcome[job.state]}</p>
    {job.exitCode !== null && <p>{t.exitCode(job.exitCode)}</p>}
    {job.launchError !== null && <p>{t.launchError(job.launchError)}</p>}
    {job.persistenceError && <p role="alert">{t.persistenceError}</p>}
    <p>{t.leftovers}</p></>;
}
export function UninstallerPage() {
  const t = useStrings(uninstallerStrings);
  const fmt = useFormat();
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
    <header className="page-header"><div><p className="eyebrow">{t.eyebrow}</p><h1 id="uninstaller-title">{t.heading}</h1>
      <p>{t.intro}</p></div></header>
    <form onSubmit={e => { e.preventDefault(); if (valid && !active) void scan.start(() => startProgramInventory({ nameContains, largestFirst })); }}>
      <label>{t.programName}<input maxLength={128} value={nameContains} onChange={e => setName(e.target.value)} /></label>
      <label>{t.sort}<select value={largestFirst ? "size" : "name"} onChange={e => setLargest(e.target.value === "size")}><option value="name">{t.sortName}</option><option value="size">{t.sortSize}</option></select></label>
      {!valid && <p role="alert">{t.nameInvalid}</p>}
      <button type="submit" disabled={active || !valid}>{t.refreshInventory}</button>
    </form>
    <StorageScanStatus idleMessage={t.idle} phase={scan.phase} status={scan.status} error={scan.error} cancel={() => void scan.cancel()} />
    <p>{t.sizesNote}</p>
    {scan.phase === "ready" && <>
      {!!scan.page?.completeness.reasons.length && <p role="note">{t.partialInventory}</p>}
      <ul aria-label={t.resultsLabel}>{!scan.loadingPage && scan.page?.records.map(({ record: p }) => <li key={p.programId}>
        <h2>{p.name}</h2><p>{t.publisherVersion(p.publisher ?? t.unknown, p.version ?? t.unknown)}</p>
        <p>{t.programDetails(p.estimatedSizeBytes === null ? t.unknown : fmt.bytes(p.estimatedSizeBytes), p.installDate ?? t.unknown, (p.lastUsedAt === null ? null : fmt.dateTime(p.lastUsedAt)) ?? t.unknown)}</p>
        <p>{t.leftovers}</p>
        <button type="button" disabled={jobs.busy || !!jobs.job} onClick={() => { if (scan.page) { focusRequest.current = key; void jobs.prepare(scan.page.snapshotId, p.programId); } }}>{t.reviewUninstall(p.name)}</button>
      </li>)}</ul>
      {scan.page?.records.length === 0 && <p>{t.noMatches}</p>}
      <StoragePaging loading={scan.loadingPage} hasNext={!!scan.page?.nextCursor} count={scan.page?.records.length ?? 0} total={scan.page?.retainedTotal ?? 0} firstPage={() => void scan.firstPage()} nextPage={() => void scan.nextPage()} />
    </>}
    {jobs.error && <p role="alert">{jobs.error}</p>}
    {jobs.busy && <p role="status">{t.waiting}</p>}
    {jobs.job && <section ref={review} tabIndex={-1} aria-label={t.reviewLabel}>{jobs.reviewUnavailable ? <><h3>{jobs.job.programName}</h3><p>{t.jobId(jobs.job.jobId)}</p><p role="status">{t.evidenceUnavailable}</p></> : <JobDetails job={jobs.job} />}
      <p>{t.cannotUndo}</p>
      {!jobs.reviewUnavailable && jobs.job.state === "awaitingConfirmation" && <button type="button" disabled={jobs.busy || jobs.submitted} onClick={() => { focusRequest.current = key; void jobs.confirm(); }}>{t.continueConfirmation}</button>}
      {!jobs.reviewUnavailable && (jobs.job.state === "awaitingConfirmation" || pending(jobs.job)) && <button type="button" disabled={jobs.busy} onClick={() => { focusRequest.current = key; void jobs.cancel(); }}>{jobs.job.state === "awaitingConfirmation" ? t.cancelPrepared : t.stopWaiting}</button>}
    </section>}
    <section aria-label={t.historyLabel}><h2>{t.historyHeading}</h2>
      <p>{t.historyPaging}</p>
      <p>{t.historyCapacity}</p>
      <button type="button" disabled={jobs.historyLoading} onClick={() => void jobs.refreshHistory()}>{t.refreshHistory}</button>
      <button type="button" disabled={jobs.historyLoading || !jobs.historyNextCursor} onClick={() => void jobs.olderHistory()}>{t.olderHistory}</button>
      {jobs.historyLoading && <p role="status">{t.historyLoading}</p>}
      {jobs.historyError && <p role="alert">{jobs.historyError}</p>}
      {!jobs.historyLoaded && !jobs.historyLoading && !jobs.historyError && <p>{t.historyNotLoaded}</p>}
      {jobs.historyLoaded && !jobs.historyLoading && !jobs.historyError && jobs.history.length === 0 && jobs.historyCursor === null && <p>{t.historyEmpty}</p>}
      {jobs.historyLoaded && !jobs.historyLoading && !jobs.historyError && !jobs.historyNextCursor && (jobs.history.length > 0 || jobs.historyCursor !== null) && <p>{t.historyEnd}</p>}
      <ul>{jobs.history.map(job => <li key={job.jobId}><JobDetails job={job} /></li>)}</ul>
    </section>
  </section>;
}
