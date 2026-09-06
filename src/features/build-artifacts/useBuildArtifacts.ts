import { useCallback, useEffect, useRef, useState } from "react";
import {
  cancelBuildRun,
  getArtifactBudgetPolicy,
  getBuildRun,
  getActiveBuildRun,
  isBuildArtifactCommandError,
  listBuildProfiles,
  previewArtifactBudgets,
  registerBuildProfile,
  removeBuildProfile,
  setArtifactBudgetPolicy,
  startBuildRun,
  type ArtifactBudgetPolicy,
  type ArtifactBudgetPreview,
  type BuildProfile,
  type BuildRun,
  type RegisterBuildProfileInput,
} from "./api";

const TERMINAL = new Set<BuildRun["state"]>([
  "succeeded",
  "failed",
  "cancelled",
  "analysisFailed",
]);

function message(error: unknown): string {
  if (error instanceof Error || isBuildArtifactCommandError(error)) return error.message;
  return "The build artifact operation could not be completed.";
}

export function useBuildArtifacts() {
  const [profiles, setProfiles] = useState<BuildProfile[]>([]);
  const [policy, setPolicy] = useState<ArtifactBudgetPolicy | null>(null);
  const [preview, setPreview] = useState<ArtifactBudgetPreview | null>(null);
  const [run, setRun] = useState<BuildRun | null>(null);
  const [loading, setLoading] = useState(true);
  const [runKnown, setRunKnown] = useState(false);
  const [pending, setPending] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const request = useRef(0);

  const load = useCallback(async (preserveError: boolean) => {
    const current = ++request.current;
    setLoading(true);
    if (!preserveError) setError(null);
    const [profilesResult, policyResult, previewResult, activeRunResult] = await Promise.allSettled([
      listBuildProfiles(),
      getArtifactBudgetPolicy(),
      previewArtifactBudgets(),
      getActiveBuildRun(),
    ]);
    if (request.current !== current) return;
    if (profilesResult.status === "fulfilled") setProfiles(profilesResult.value);
    if (policyResult.status === "fulfilled") setPolicy(policyResult.value);
    if (previewResult.status === "fulfilled") setPreview(previewResult.value);
    setRunKnown(activeRunResult.status === "fulfilled");
    if (activeRunResult.status === "fulfilled") {
      setRun((previous) => activeRunResult.value ?? (previous && TERMINAL.has(previous.state) ? previous : null));
    }
    const failure = [profilesResult, policyResult, previewResult, activeRunResult].find(
      (result) => result.status === "rejected",
    );
    if (failure?.status === "rejected") setError(message(failure.reason));
    setLoading(false);
  }, []);

  const reload = useCallback(() => load(false), [load]);

  useEffect(() => {
    void reload();
    return () => {
      request.current += 1;
    };
  }, [reload]);

  useEffect(() => {
    if (!run || TERMINAL.has(run.state)) return;
    let cancelled = false;
    const timer = window.setInterval(() => {
      void getBuildRun(run.runId)
        .then((next) => {
          if (cancelled) return;
          setRun(next);
          if (TERMINAL.has(next.state)) void reload();
        })
        .catch((failure: unknown) => {
          if (!cancelled) setError(message(failure));
        });
    }, 500);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [reload, run]);

  const perform = useCallback(async <T,>(key: string, operation: () => Promise<T>) => {
    setPending(key);
    setError(null);
    try {
      return await operation();
    } catch (failure) {
      setError(message(failure));
      return null;
    } finally {
      setPending(null);
    }
  }, []);

  const register = useCallback(
    async (input: RegisterBuildProfileInput) => {
      const saved = await perform("register", () => registerBuildProfile(input));
      if (saved) await reload();
      return saved;
    },
    [perform, reload],
  );

  const remove = useCallback(
    async (profileId: string) => {
      const result = await perform(`remove:${profileId}`, async () => {
        await removeBuildProfile(profileId);
        return true;
      });
      if (result) await reload();
      return Boolean(result);
    },
    [perform, reload],
  );

  const start = useCallback(
    async (profileId: string) => {
      const started = await perform(`run:${profileId}`, () => startBuildRun(profileId));
      if (started) setRun(started);
      return started;
    },
    [perform],
  );

  const cancel = useCallback(async () => {
    if (!run || TERMINAL.has(run.state)) return false;
    const cancelling = await perform("cancel", () => cancelBuildRun(run.runId));
    if (cancelling) setRun(cancelling);
    return Boolean(cancelling);
  }, [perform, run]);

  const savePolicy = useCallback(
    async (next: ArtifactBudgetPolicy) => {
      const saved = await perform("policy", () => setArtifactBudgetPolicy(next));
      if (saved?.policySaved) setPolicy(saved.policy);
      await load(true);
      if (saved?.analysisStatus === "failed") {
        setError("Artifact budgets were saved, but immediate analysis failed. The saved policy remains active.");
      }
      return saved;
    },
    [load, perform],
  );

  return {
    profiles,
    policy,
    preview,
    run,
    loading,
    buildActionsDisabled: loading || !runKnown || Boolean(run && !TERMINAL.has(run.state)),
    pending,
    error,
    register,
    remove,
    start,
    cancel,
    savePolicy,
    reload,
  };
}
