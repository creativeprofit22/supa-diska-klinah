import { useCallback, useEffect, useRef, useState } from "react";
import { listProjectRoots, type ProjectRoot } from "../cleanup/api/previewCleanup";

export type ProjectRootLoadStatus = "loading" | "ready" | "error";

export function useProjectRoots() {
  const [roots, setRoots] = useState<ProjectRoot[]>([]);
  const [status, setStatus] = useState<ProjectRootLoadStatus>("loading");
  const [error, setError] = useState<string | null>(null);
  const request = useRef(0);

  const reload = useCallback(async () => {
    const current = ++request.current;
    setStatus("loading");
    setError(null);
    try {
      const saved = await listProjectRoots();
      if (request.current !== current) return;
      setRoots(saved);
      setStatus("ready");
    } catch {
      if (request.current !== current) return;
      setRoots([]);
      setError("Project roots could not be loaded. Profile registration and project overrides are unavailable.");
      setStatus("error");
    }
  }, []);

  useEffect(() => {
    void reload();
    return () => {
      request.current += 1;
    };
  }, [reload]);

  return { roots, status, error, reload };
}
