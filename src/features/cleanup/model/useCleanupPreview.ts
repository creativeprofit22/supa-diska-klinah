import { useCallback, useEffect, useState } from "react";
import {
  type CleanupPreview,
  previewCleanup,
} from "../api/previewCleanup";

interface CleanupPreviewState {
  result: CleanupPreview | null;
  error: boolean;
  loading: boolean;
  retry: () => void;
}

export function useCleanupPreview(): CleanupPreviewState {
  const [result, setResult] = useState<CleanupPreview | null>(null);
  const [error, setError] = useState(false);
  const [loading, setLoading] = useState(true);
  const [request, setRequest] = useState(0);
  const retry = useCallback(() => setRequest((value) => value + 1), []);

  useEffect(() => {
    let active = true;
    setResult(null);
    setError(false);
    setLoading(true);

    previewCleanup().then(
      (preview) => {
        if (active) {
          setResult(preview);
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

  return { result, error, loading, retry };
}
