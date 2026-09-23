import { useCallback, useLayoutEffect, useRef, useState } from "react";
import { releaseStorageScan, storageError } from "./api";
import { validId, type RootChoice, type StorageModule } from "./types";

/** Owns unused native authorizations, never paths as authority. */
export function useStorageRoot(module: StorageModule) {
  const [choice, setChoice] = useState<RootChoice | null>(null);
  const [picking, setPicking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [available, setAvailable] = useState(false);
  const unused = useRef<RootChoice | null>(null);
  const generation = useRef(0);
  const mounted = useRef(false);
  const locked = useRef(false);
  const release = useCallback((root: RootChoice) => releaseStorageScan({ module: root.module, snapshotId: root.rootId }).catch(() => {
    // Native expiry remains a fallback; failed release never restores UI authority.
  }), []);

  useLayoutEffect(() => {
    mounted.current = true;
    locked.current = false;
    setChoice(null); setPicking(false); setError(null); setAvailable(false);
    return () => {
      mounted.current = false;
      generation.current++;
      const root = unused.current;
      unused.current = null;
      if (root) void release(root);
    };
  }, [module, release]);

  const acquire = async (choose: () => Promise<RootChoice | null>) => {
    if (!mounted.current || locked.current) return;
    locked.current = true; setPicking(true); setError(null);
    const version = generation.current;
    try {
      const root = await choose();
      if (!root) return; // Cancellation preserves the previous scope and its availability.
      if (!mounted.current || version !== generation.current) { await release(root); return; }
      if (root.module !== module || !validId(root.rootId)) {
        await release(root);
        throw { code: "invalid_evidence" };
      }
      const previous = unused.current;
      unused.current = root;
      setChoice(root); setAvailable(true);
      if (previous && previous.rootId !== root.rootId) await release(previous);
    } catch (cause) {
      if (mounted.current && version === generation.current) setError(storageError(cause));
    } finally {
      if (mounted.current && version === generation.current) { locked.current = false; setPicking(false); }
    }
  };

  const run = async <T,>(consume: (rootId: string) => Promise<T>): Promise<T> => {
    const root = unused.current;
    if (!mounted.current || locked.current || !root || root.module !== module) throw { code: "scope_unavailable" };
    unused.current = null;
    setAvailable(false);
    try { return await consume(root.rootId); }
    finally { await release(root); } // Retire also when native start was busy or failed.
  };
  return { choice, picking, error, available, acquire, run };
}
