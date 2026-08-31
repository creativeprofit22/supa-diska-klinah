import { useCallback, useEffect, useRef, useState } from "react";
import {
  discoverProjectArtifacts,
  type ProjectArtifactDiscovery,
} from "../api/previewCleanup";

interface ProjectArtifactDiscoveryState {
  result: ProjectArtifactDiscovery | null;
  attempted: boolean;
  loading: boolean;
  error: boolean;
  scan: (root: string) => Promise<void>;
  retry: () => void;
}

export function useProjectArtifactDiscovery(): ProjectArtifactDiscoveryState {
  const [result, setResult] = useState<ProjectArtifactDiscovery | null>(null);
  const [attempted, setAttempted] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState(false);
  const request = useRef(0);
  const lastRoot = useRef("");

  useEffect(() => () => {
    request.current += 1;
  }, []);

  const scan = useCallback(async (root: string) => {
    const current = ++request.current;
    lastRoot.current = root;
    setAttempted(true);
    setLoading(true);
    setError(false);
    setResult(null);
    try {
      const discovered = await discoverProjectArtifacts(root);
      if (request.current === current) setResult(discovered);
    } catch {
      if (request.current === current) setError(true);
    } finally {
      if (request.current === current) setLoading(false);
    }
  }, []);

  const retry = useCallback(() => {
    if (lastRoot.current) void scan(lastRoot.current);
  }, [scan]);

  return { result, attempted, loading, error, scan, retry };
}
