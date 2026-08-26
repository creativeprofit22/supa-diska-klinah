import { useCallback, useState } from "react";
import {
  createSystemRestorePoint,
  type CreateRestorePointInput,
  type CreateSystemRestorePointResult,
} from "../api/createSystemRestorePoint";

interface CreateSystemRestorePointState {
  result: CreateSystemRestorePointResult | null;
  error: string | null;
  loading: boolean;
  create: (input: CreateRestorePointInput) => Promise<void>;
}

function restorePointErrorMessage(reason: unknown): string {
  if (
    typeof reason === "object" &&
    reason !== null &&
    "code" in reason &&
    reason.code === "invalidInput"
  ) {
    return "Enter a valid restore point description.";
  }

  return "Windows did not complete the restore point. This app is still open.";
}

export function useCreateSystemRestorePoint(): CreateSystemRestorePointState {
  const [result, setResult] = useState<CreateSystemRestorePointResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const create = useCallback(async (input: CreateRestorePointInput) => {
    setLoading(true);
    setResult(null);
    setError(null);

    try {
      setResult(await createSystemRestorePoint(input));
    } catch (reason: unknown) {
      setError(restorePointErrorMessage(reason));
    } finally {
      setLoading(false);
    }
  }, []);

  return { result, error, loading, create };
}
