import { useCallback, useLayoutEffect, useRef, useState } from "react";
import type { CleanupExecutionSummary } from "./api";
import { historyPage, type HistoryPage, type HistoryRequest } from "./history";

/** One live page, never an accumulated history or a stack of prior cursors. */
export function useCleanupHistory(fetchPage: (input?: HistoryRequest) => Promise<HistoryPage<CleanupExecutionSummary>>) {
  const mounted = useRef(false);
  const generation = useRef(0);
  const [records, setRecords] = useState<CleanupExecutionSummary[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [currentCursor, setCurrentCursor] = useState<string | null>(null);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  useLayoutEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; generation.current++; };
  }, []);
  const load = useCallback(async (cursor: string | null) => {
    if (!mounted.current) return;
    const request = ++generation.current;
    setLoading(true); setError(null);
    try {
      const page = historyPage<CleanupExecutionSummary>(await fetchPage({ cursor, limit: 20 }), "cleanup", 20);
      if (!mounted.current || request !== generation.current) return;
      setRecords(page.records); setCurrentCursor(cursor); setNextCursor(page.nextCursor); setLoaded(true);
    } catch {
      if (mounted.current && request === generation.current) setError("History could not be loaded. Try loading history again.");
    } finally {
      if (mounted.current && request === generation.current) setLoading(false);
    }
  }, [fetchPage]);
  const refresh = useCallback(() => load(null), [load]);
  const older = useCallback(async () => { if (nextCursor !== null) await load(nextCursor); }, [load, nextCursor]);
  const updateVisible = useCallback((updated: CleanupExecutionSummary) => {
    if (!mounted.current) return;
    // A pre-Undo read must not overwrite the confirmed outcome.
    generation.current++; setLoading(false);
    setRecords(current => current.map(row => row.executionId === updated.executionId ? updated : row));
  }, []);
  return { records, loaded, loading, error, currentCursor, nextCursor, refresh, older, updateVisible };
}
