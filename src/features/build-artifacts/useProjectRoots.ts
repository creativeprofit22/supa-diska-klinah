import { useCallback, useEffect, useRef, useState } from "react";
import { useStrings } from "../../shared/i18n/I18nProvider";
import { listProjectRoots, type ProjectRoot } from "../cleanup/api/previewCleanup";
import { buildArtifactsStrings } from "./strings";

export type ProjectRootLoadStatus = "loading" | "ready" | "error";

export function useProjectRoots() {
  const t = useStrings(buildArtifactsStrings);
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
      setError(t.rootsLoadFailed);
      setStatus("error");
    }
  }, [t]);

  useEffect(() => {
    void reload();
    return () => {
      request.current += 1;
    };
  }, [reload]);

  return { roots, status, error, reload };
}
