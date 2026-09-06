import { useCallback, useEffect, useRef, useState } from "react";
import {
  addProjectRoot,
  discoverProjectArtifacts,
  listProjectRoots,
  removeProjectRoot,
  setProjectRootPaused,
  type ProjectArtifactDiscovery,
  type ProjectRoot,
} from "../api/previewCleanup";

const ERROR_MESSAGES: Record<string, string> = {
  duplicateRoot: "That project root is already saved.",
  invalidInput: "Enter an existing, unprotected absolute project path.",
  notFound: "That saved project root is no longer available.",
  rootLimitReached: "Remove a project root before adding another.",
  rootPaused: "Resume this project root before scanning it.",
  persistenceFailed: "Saved project roots could not be updated.",
};

function errorMessage(error: unknown): string {
  if (typeof error === "object" && error !== null && "code" in error) {
    const code = String(error.code);
    if (ERROR_MESSAGES[code]) return ERROR_MESSAGES[code];
  }
  return "Project roots could not be updated. Try again.";
}

interface ProjectArtifactDiscoveryState {
  roots: ProjectRoot[];
  result: ProjectArtifactDiscovery | null;
  attempted: boolean;
  loadingRoots: boolean;
  scanning: boolean;
  pending: string | null;
  error: string | null;
  add: (path: string) => Promise<boolean>;
  setPaused: (rootId: string, paused: boolean) => Promise<boolean>;
  remove: (rootId: string) => Promise<boolean>;
  scan: (rootId?: string) => Promise<void>;
  reload: () => void;
}

export function useProjectArtifactDiscovery(): ProjectArtifactDiscoveryState {
  const [roots, setRoots] = useState<ProjectRoot[]>([]);
  const [result, setResult] = useState<ProjectArtifactDiscovery | null>(null);
  const [attempted, setAttempted] = useState(false);
  const [loadingRoots, setLoadingRoots] = useState(true);
  const [scanning, setScanning] = useState(false);
  const [pending, setPending] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const request = useRef(0);

  const load = useCallback(async () => {
    const current = ++request.current;
    setLoadingRoots(true);
    setError(null);
    try {
      const saved = await listProjectRoots();
      if (request.current === current) setRoots(saved);
    } catch (failure) {
      if (request.current === current) setError(errorMessage(failure));
    } finally {
      if (request.current === current) setLoadingRoots(false);
    }
  }, []);

  useEffect(() => {
    void load();
    return () => {
      request.current += 1;
    };
  }, [load]);

  const mutate = useCallback(
    async (key: string, operation: () => Promise<ProjectRoot[]>): Promise<boolean> => {
      const current = ++request.current;
      setPending(key);
      setError(null);
      setResult(null);
      setAttempted(false);
      try {
        const saved = await operation();
        if (request.current !== current) return false;
        setRoots(saved);
        return true;
      } catch (failure) {
        if (request.current === current) setError(errorMessage(failure));
        return false;
      } finally {
        if (request.current === current) setPending(null);
      }
    },
    [],
  );

  const add = useCallback(
    (path: string) => mutate("add", () => addProjectRoot(path)),
    [mutate],
  );
  const setPaused = useCallback(
    (rootId: string, paused: boolean) =>
      mutate(`pause:${rootId}`, () => setProjectRootPaused(rootId, paused)),
    [mutate],
  );
  const remove = useCallback(
    (rootId: string) => mutate(`remove:${rootId}`, () => removeProjectRoot(rootId)),
    [mutate],
  );

  const scan = useCallback(async (rootId?: string) => {
    const current = ++request.current;
    setAttempted(true);
    setScanning(true);
    setError(null);
    setResult(null);
    try {
      const discovered = await discoverProjectArtifacts(rootId);
      if (request.current === current) {
        setRoots(discovered.roots);
        setResult(discovered);
      }
    } catch (failure) {
      if (request.current === current) setError(errorMessage(failure));
    } finally {
      if (request.current === current) setScanning(false);
    }
  }, []);

  return {
    roots,
    result,
    attempted,
    loadingRoots,
    scanning,
    pending,
    error,
    add,
    setPaused,
    remove,
    scan,
    reload: () => void load(),
  };
}
