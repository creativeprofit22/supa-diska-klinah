import { useCallback, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useStrings } from "../i18n/I18nProvider";
import { scanApi, storageError } from "./api";
import { storageStrings } from "./strings";
import { MAX_SELECTION, PAGE_SIZE, validId } from "./types";
import type { PageCollection, ScanApi, ScanPhase, SnapshotInput, StorageModule, StoragePage, StorageSelection, StorageStatus } from "./types";

interface Options<Row> {
  module: StorageModule;
  /** Stable scope + filter identity. Changing it invalidates all retained authority. */
  scopeKey: string;
  collection: PageCollection;
  parentId?: string;
  candidateId?: (row: Row) => string | null;
  api?: ScanApi<Row>;
}
interface State<Row> {
  phase: ScanPhase;
  status: StorageStatus | null;
  page: StoragePage<Row> | null;
  loadingPage: boolean;
  error: string | null;
  selected: ReadonlySet<string>;
}
function empty<Row>(): State<Row> {
  return { phase: "idle", status: null, page: null, loadingPage: false, error: null, selected: new Set() };
}
export function useStorageScan<Row>({ module, scopeKey, collection, parentId, candidateId, api: provided }: Options<Row>) {
  const fallback = useMemo(() => scanApi<Row>(), []);
  const api = provided ?? fallback;
  const [state, setState] = useState<State<Row>>(empty);
  const generation = useRef(0);
  const pageGeneration = useRef(0);
  const job = useRef<SnapshotInput | null>(null);
  const pageReady = useRef(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const view = useRef({ collection, parentId });
  view.current = { collection, parentId };
  // Read at failure time so a locale change never re-creates callbacks or reloads pages.
  const t = useStrings(storageStrings);
  const strings = useRef(t);
  strings.current = t;

  const stopTimer = () => { if (timer.current !== null) clearTimeout(timer.current); timer.current = null; };
  const release = useCallback((input: SnapshotInput) => api.release(input).catch(() => {
    // Native authorizations also expire. A failed release never keeps UI authority.
  }), [api]);
  const invalidate = useCallback(() => {
    generation.current++;
    pageGeneration.current++;
    stopTimer();
    const previous = job.current;
    job.current = null;
    pageReady.current = false;
    return previous ? release(previous) : Promise.resolve();
  }, [release]);

  const reset = useCallback(() => {
    const retired = invalidate();
    setState(empty());
    return retired;
  }, [invalidate]);

  const loadPage = useCallback(async (cursor?: string) => {
    const input = job.current;
    if (!input || !pageReady.current) return;
    const current = generation.current;
    const pageVersion = ++pageGeneration.current;
    setState(s => ({ ...s, page: null, loadingPage: true, error: null }));
    try {
      const result = await api.page({ ...input, ...view.current, cursor, pageSize: PAGE_SIZE });
      if (current !== generation.current || pageVersion !== pageGeneration.current) return;
      if (result.snapshotId !== input.snapshotId || result.records.length > PAGE_SIZE ||
          (result.nextCursor !== null && !validId(result.nextCursor))) throw { code: "invalid_evidence" };
      setState(s => ({ ...s, page: result, loadingPage: false }));
    } catch (error) {
      if (current === generation.current && pageVersion === pageGeneration.current) {
        setState(s => ({ ...s, page: null, loadingPage: false, selected: new Set(), error: storageError(error, strings.current.errors) }));
      }
    }
  }, [api]);

  useLayoutEffect(() => {
    setState(empty());
    return () => { void invalidate(); };
  }, [module, scopeKey, invalidate]);

  useLayoutEffect(() => {
    // Navigation replaces rows, not the snapshot. No cursor history accumulates.
    if (job.current) void loadPage();
  }, [collection, parentId, loadPage]);

  const start = async (startNativeScan: () => Promise<string>) => {
    const retired = invalidate();
    const current = generation.current;
    setState({ ...empty(), phase: "starting" });
    await retired;
    if (current !== generation.current) return;
    try {
      // Feature-owned start callback; never a renderer-selected native runner name.
      const snapshotId = await startNativeScan();
      if (!validId(snapshotId)) throw { code: "invalid_evidence" };
      const input = { module, snapshotId };
      if (current !== generation.current) { await release(input); return; }
      job.current = input;
      setState(s => ({ ...s, phase: "scanning" }));
      const poll = async () => {
        try {
          const status = await api.status(input);
          if (current !== generation.current) return;
          if (status.snapshotId !== snapshotId || status.module !== module) throw { code: "invalid_evidence" };
          if (status.phase === "cancelled" || status.phase === "failed") {
            job.current = null;
            void release(input);
            setState(s => ({ ...s, status, phase: status.phase === "cancelled" ? "cancelled" : "failed", selected: new Set() }));
          } else if (status.phase === "complete") {
            pageReady.current = true;
            setState(s => ({ ...s, status, phase: "ready" }));
            await loadPage();
          } else {
            setState(s => ({ ...s, status }));
            // One outstanding poll, scheduled only after the previous one settles.
            timer.current = setTimeout(() => { void poll(); }, 500);
          }
        } catch (error) {
          if (current !== generation.current) return;
          job.current = null;
          void release(input);
          setState(s => ({ ...s, phase: "failed", error: storageError(error, strings.current.errors), selected: new Set() }));
        }
      };
      void poll();
    } catch (error) {
      if (current === generation.current) setState(s => ({ ...s, phase: "failed", error: storageError(error, strings.current.errors) }));
    }
  };
  const cancel = async () => {
    const input = job.current;
    job.current = null;
    pageReady.current = false;
    generation.current++;
    pageGeneration.current++;
    stopTimer();
    const current = generation.current;
    setState({ ...empty(), phase: "cancelling" });
    try {
      if (input) await api.cancel(input);
      if (current === generation.current) setState({ ...empty(), phase: "cancelled" });
    } catch {
      if (current === generation.current) setState({ ...empty(), phase: "failed", error: strings.current.scan.cancellationUnconfirmed });
    } finally {
      if (input) await release(input);
    }
  };
  const toggle = (id: string) => {
    if (state.phase !== "ready" || state.loadingPage || !state.page || !validId(id) ||
        !candidateId || !state.page.records.some(row => candidateId(row) === id)) return;
    setState(s => {
      if (s.page !== state.page || job.current?.snapshotId !== s.page?.snapshotId || s.loadingPage || s.phase !== "ready") return s;
      const selected = new Set(s.selected);
      if (selected.has(id)) selected.delete(id);
      else if (selected.size < MAX_SELECTION) selected.add(id);
      return { ...s, selected };
    });
  };
  const selection: StorageSelection | null = useMemo(() => {
    if (state.phase !== "ready" || state.loadingPage || !state.page || !state.selected.size) return null;
    return { module, snapshotId: state.page.snapshotId, candidateIds: [...state.selected] };
  }, [state.phase, state.loadingPage, state.page, state.selected, module]);
  return {
    ...state, selection, start, cancel, toggle, reset,
    clearSelection: () => setState(s => ({ ...s, selected: new Set() })),
    firstPage: () => loadPage(),
    nextPage: () => state.page?.nextCursor && job.current?.snapshotId === state.page.snapshotId
      ? loadPage(state.page.nextCursor) : Promise.resolve(),
  };
}
