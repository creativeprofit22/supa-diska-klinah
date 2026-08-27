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
  const code =
    typeof reason === "object" &&
    reason !== null &&
    "code" in reason &&
    typeof reason.code === "string"
      ? reason.code
      : "";

  switch (code) {
    case "invalidInput":
      return "Enter a valid restore point description.";
    case "authorizationCancelled":
      return "Administrator approval was cancelled. Try again and approve the Windows prompt.";
    case "helperUnavailable":
      return "The privileged helper is unavailable. Repair or reinstall the app, then try again.";
    case "operationTimedOut":
      return "System Restore timed out. Check Windows System Protection, then try again.";
    case "invalidRequest":
      return "The restore point request expired or was rejected. Try again.";
    case "privilegeFailure":
      return "Administrator access was not granted. Try again and approve the Windows prompt.";
    case "systemRestoreFailure":
      return "Windows System Restore failed. Check System Protection and available disk space.";
    default:
      return "Windows did not complete the restore point. This app is still open.";
  }
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
