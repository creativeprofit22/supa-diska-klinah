import { useCallback, useEffect, useMemo, useState } from "react";
import {
  cleanupHistory,
  type CleanupDisposition,
  type CleanupExecutionSummary,
  type CleanupPlanSummary,
  type CleanupPreview,
  createCleanupPlan,
  executeCleanupPlan,
  executePermanentCleanupPlan,
  previewCleanup,
  undoCleanup,
} from "../api/previewCleanup";

interface CleanupPreviewState {
  result: CleanupPreview | null;
  error: boolean;
  loading: boolean;
  busy: boolean;
  selectedIds: Set<string>;
  selectedBytes: number;
  plan: CleanupPlanSummary | null;
  execution: CleanupExecutionSummary | null;
  history: CleanupExecutionSummary[];
  retry: () => void;
  toggle: (id: string) => void;
  selectAll: () => void;
  prepare: (disposition: CleanupDisposition) => Promise<void>;
  cancelPlan: () => void;
  confirmPlan: () => Promise<void>;
  undo: (executionId: string) => Promise<void>;
}

export function useCleanupPreview(): CleanupPreviewState {
  const [result, setResult] = useState<CleanupPreview | null>(null);
  const [error, setError] = useState(false);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [request, setRequest] = useState(0);
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [plan, setPlan] = useState<CleanupPlanSummary | null>(null);
  const [execution, setExecution] = useState<CleanupExecutionSummary | null>(null);
  const [history, setHistory] = useState<CleanupExecutionSummary[]>([]);
  const retry = useCallback(() => setRequest((value) => value + 1), []);

  useEffect(() => {
    let active = true;
    setResult(null);
    setSelectedIds(new Set());
    setError(false);
    setLoading(true);
    Promise.all([previewCleanup(), cleanupHistory()]).then(
      ([preview, executions]) => {
        if (active) {
          setResult(preview);
          setHistory(executions);
          setLoading(false);
        }
      },
      () => {
        if (active) {
          setError(true);
          setLoading(false);
        }
      },
    );
    return () => {
      active = false;
    };
  }, [request]);

  const selectedBytes = useMemo(
    () =>
      result?.records
        .filter((record) => selectedIds.has(record.id))
        .reduce((total, record) => total + record.bytes, 0) ?? 0,
    [result, selectedIds],
  );

  const toggle = useCallback((id: string) => {
    setSelectedIds((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const selectAll = useCallback(() => {
    setSelectedIds((current) => {
      const ids = result?.records.map((record) => record.id) ?? [];
      return current.size === ids.length ? new Set() : new Set(ids);
    });
  }, [result]);

  const prepare = useCallback(
    async (disposition: CleanupDisposition) => {
      if (!result || selectedIds.size === 0) return;
      setBusy(true);
      setError(false);
      try {
        setPlan(await createCleanupPlan(result.scanId, [...selectedIds], disposition));
      } catch {
        setError(true);
      } finally {
        setBusy(false);
      }
    },
    [result, selectedIds],
  );

  const confirmPlan = useCallback(async () => {
    if (!plan) return;
    setBusy(true);
    setError(false);
    try {
      const completed =
        plan.disposition === "permanent"
          ? await executePermanentCleanupPlan(plan.planId)
          : await executeCleanupPlan(plan.planId);
      setExecution(completed);
      setHistory((current) => [completed, ...current.filter((item) => item.executionId !== completed.executionId)]);
      setPlan(null);
      setSelectedIds(new Set());
    } catch {
      setError(true);
    } finally {
      setBusy(false);
    }
  }, [plan]);

  const undo = useCallback(async (executionId: string) => {
    setBusy(true);
    setError(false);
    try {
      const updated = await undoCleanup(executionId);
      setExecution(updated);
      setHistory((current) => current.map((item) => (item.executionId === executionId ? updated : item)));
    } catch {
      setError(true);
    } finally {
      setBusy(false);
    }
  }, []);

  return {
    result,
    error,
    loading,
    busy,
    selectedIds,
    selectedBytes,
    plan,
    execution,
    history,
    retry,
    toggle,
    selectAll,
    prepare,
    cancelPlan: () => setPlan(null),
    confirmPlan,
    undo,
  };
}
