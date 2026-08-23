import { useCallback, useEffect, useState } from "react";
import {
  getFoundationStatus,
  type FoundationStatus,
} from "../api/getFoundationStatus";

interface FoundationStatusState {
  status: FoundationStatus | null;
  error: string | null;
  loading: boolean;
  retry: () => void;
}

export function useFoundationStatus(): FoundationStatusState {
  const [status, setStatus] = useState<FoundationStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [request, setRequest] = useState(0);
  const retry = useCallback(() => setRequest((value) => value + 1), []);

  useEffect(() => {
    let active = true;
    setError(null);

    getFoundationStatus().then(
      (nextStatus) => {
        if (active) setStatus(nextStatus);
      },
      (reason: unknown) => {
        if (active) {
          setError(reason instanceof Error ? reason.message : String(reason));
        }
      },
    );

    return () => {
      active = false;
    };
  }, [request]);

  return { status, error, loading: !status && !error, retry };
}
