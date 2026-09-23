import { useCallback, useLayoutEffect, useRef, useState } from "react";
import { cancelVendorJob, confirmVendorJob, pending, prepareVendorJob, releaseVendorJob, vendorError, vendorJobHistory, vendorJobStatus, type VendorJob } from "./api";

async function discard(job: VendorJob) {
  if (job.state !== "awaitingConfirmation") return;
  try {
    const result = await cancelVendorJob(job.jobId);
    if (result.state === "cancelledBeforeLaunch") await releaseVendorJob(job.jobId);
  } catch { /* Backend expiry retires abandoned consent; never infer successful cancellation. */ }
}
export function useVendorJobs(scopeKey: string) {
  const [job, setJob] = useState<VendorJob | null>(null);
  const [history, setHistory] = useState<VendorJob[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const generation = useRef(0);
  const current = useRef<VendorJob | null>(null);
  const submitted = useRef(false);
  const locked = useRef(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [historyLoading, setHistoryLoading] = useState(false);
  const [historyLoaded, setHistoryLoaded] = useState(false);
  const [historyError, setHistoryError] = useState<string | null>(null);
  const [historyCursor, setHistoryCursor] = useState<string | null>(null);
  const [historyNextCursor, setHistoryNextCursor] = useState<string | null>(null);
  const historyGeneration = useRef(0);
  const historyMounted = useRef(false);
  const pollVersion = useRef(0);
  const polls = useRef(0);
  const unavailable = useRef(false);
  const [reviewUnavailable, setReviewUnavailable] = useState(false);
  const historyFlight = useRef<Promise<void> | null>(null);
  const historyFlightVersion = useRef(0);
  const loadHistory = useCallback((cursor: string | null, reconcile?: (next: VendorJob) => void): Promise<void> => {
    if (!historyMounted.current) return Promise.resolve();
    if (historyFlight.current) return historyFlightVersion.current === historyGeneration.current
      ? historyFlight.current
      : historyFlight.current.then(() => loadHistory(cursor, reconcile));
    const version = ++historyGeneration.current;
    historyFlightVersion.current = version;
    const scope = generation.current;
    const poll = pollVersion.current;
    const selected = locked.current ? null : current.current;
    const isCurrent = () => historyMounted.current && version === historyGeneration.current;
    const isSelected = () => isCurrent() && scope === generation.current && poll === pollVersion.current && selected === current.current && !locked.current;
    setHistoryLoading(true); setHistoryError(null);
    const request = (async () => {
      try {
        const result = await vendorJobHistory({ cursor, limit: 64 });
        if (!isCurrent()) return;
        setHistory(result.records); setHistoryCursor(cursor); setHistoryNextCursor(result.nextCursor);
        setHistoryLoaded(true);
        if (reconcile && selected && isSelected()) {
          try {
            const next = result.records.find(row => row.jobId === selected.jobId && row.programId === selected.programId)
              ?? await vendorJobStatus(selected.jobId);
            if (!isSelected()) return;
            if (next.jobId !== selected.jobId || next.programId !== selected.programId) throw new Error("Mismatched vendor job");
            // A submitted or retired receipt must never regain confirmation authority.
            if (next.state === "awaitingConfirmation" && (submitted.current || unavailable.current || selected.state !== "awaitingConfirmation")) throw new Error("Retired review");
            reconcile(next);
            if (!submitted.current && next.state === "cancelledBeforeLaunch") setError("Program evidence expired or was retired. Refresh inventory and review again.");
          } catch {
            if (!isSelected()) return;
            pollVersion.current++;
            if (timer.current !== null) clearTimeout(timer.current);
            timer.current = null;
            unavailable.current = true; setReviewUnavailable(true);
            setError(submitted.current
              ? "The submitted job outcome is unknown. Refresh retained history to check again; do not retry the vendor operation."
              : "Program evidence is unavailable or expired. Refresh inventory and review again.");
          }
        }
      } catch {
        if (isCurrent()) setHistoryError("Retained history could not be loaded. Refresh retained history to try again. History browsing does not start vendor jobs.");
      } finally {
        historyFlight.current = null;
        if (isCurrent()) setHistoryLoading(false);
      }
    })();
    historyFlight.current = request;
    return request;
  }, []);
  const olderHistory = () => historyNextCursor !== null && !historyLoading ? loadHistory(historyNextCursor) : Promise.resolve();
  useLayoutEffect(() => {
    historyMounted.current = true;
    void loadHistory(null);
    return () => { historyMounted.current = false; historyGeneration.current++; };
  }, [loadHistory]);
  useLayoutEffect(() => {
    setJob(null); setBusy(false); setError(null); locked.current = false; polls.current = 0;
    unavailable.current = false; setReviewUnavailable(false);
    return () => {
      generation.current++;
      if (timer.current !== null) clearTimeout(timer.current);
      timer.current = null;
      const old = current.current; current.current = null;
      // After submission a native dialog/launch may already be in progress. Never
      // send cancellation from navigation that could cancel waiting on a vendor.
      if (old && !submitted.current && !unavailable.current) void discard(old);
      submitted.current = false;
    };
  }, [scopeKey]);
  const accept = (next: VendorJob, version: number, refresh = true) => {
    if (version !== generation.current) return;
    if (timer.current !== null) clearTimeout(timer.current);
    timer.current = null;
    const poll = ++pollVersion.current;
    current.current = next; setJob(next); setError(null);
    unavailable.current = false; setReviewUnavailable(false);
    if (!pending(next)) {
      if (refresh && next.state !== "awaitingConfirmation") void loadHistory(null);
      return;
    }
    if (++polls.current > 2400) { setError("Polling limit reached. The vendor was not stopped. Refresh retained history for its outcome."); return; }
    timer.current = setTimeout(() => {
      timer.current = null;
      void vendorJobStatus(next.jobId).then(result => {
        if (version !== generation.current || poll !== pollVersion.current) return;
        if (result.jobId !== next.jobId || result.programId !== next.programId || result.state === "awaitingConfirmation") throw new Error("Mismatched job");
        accept(result, version);
      }).catch(e => { if (version === generation.current && poll === pollVersion.current) setError(vendorError(e)); });
    }, 750);
  };
  const refreshHistory = () => {
    const version = generation.current;
    return loadHistory(null, next => accept(next, version, false));
  };
  const prepare = async (snapshotId: string, programId: string) => {
    if (locked.current || current.current) return;
    locked.current = true; setBusy(true); setError(null);
    const version = generation.current;
    try {
      const result = await prepareVendorJob(snapshotId, programId);
      if (version !== generation.current) { await discard(result); return; }
      if (result.programId !== programId || result.state !== "awaitingConfirmation") { await discard(result); throw new Error("Mismatched review"); }
      accept(result, version);
    } catch (e) { if (version === generation.current) setError(vendorError(e)); }
    finally { if (version === generation.current) { locked.current = false; setBusy(false); } }
  };
  const confirm = async () => {
    const review = current.current;
    if (locked.current || unavailable.current || !review || review.state !== "awaitingConfirmation" || submitted.current) return;
    locked.current = true; submitted.current = true; pollVersion.current++; setBusy(true); setError(null);
    const version = generation.current;
    try {
      const result = await confirmVendorJob(review.jobId);
      if (result.jobId !== review.jobId || result.programId !== review.programId) throw new Error("Mismatched vendor job");
      accept(result, version);
    }
    catch (e) { if (version === generation.current) { setError(vendorError(e)); void loadHistory(null); } }
    finally { if (version === generation.current) { locked.current = false; setBusy(false); } }
  };
  const cancel = async () => {
    const review = current.current;
    if (!review || locked.current || unavailable.current) return;
    locked.current = true; setBusy(true);
    pollVersion.current++;
    if (timer.current !== null) clearTimeout(timer.current);
    timer.current = null;
    const version = generation.current;
    try {
      const result = await cancelVendorJob(review.jobId);
      if (result.jobId !== review.jobId || result.programId !== review.programId) throw new Error("Mismatched vendor job");
      accept(result, version);
    }
    catch (e) { if (version === generation.current) setError(vendorError(e)); }
    finally { if (version === generation.current) { locked.current = false; setBusy(false); } }
  };
  return { job, reviewUnavailable, history, historyLoading, historyLoaded, historyError, historyCursor, historyNextCursor, olderHistory, busy, error, prepare, confirm, cancel, refreshHistory, submitted: submitted.current };
}
