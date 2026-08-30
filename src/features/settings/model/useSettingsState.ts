import { useCallback, useEffect, useState } from "react";
import {
  type AutoCleanupPolicy,
  getAutoCleanupPolicy,
  setAutoCleanupPolicy,
} from "../api/autoCleanupPolicy";

type SettingsError = "load" | "save" | null;

function policiesMatch(left: AutoCleanupPolicy | null, right: AutoCleanupPolicy | null) {
  return (
    left !== null &&
    right !== null &&
    left.enabled === right.enabled &&
    left.graceDays === right.graceDays
  );
}

export function useSettingsState() {
  const [policy, setPolicy] = useState<AutoCleanupPolicy | null>(null);
  const [savedPolicy, setSavedPolicy] = useState<AutoCleanupPolicy | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<SettingsError>(null);
  const [saved, setSaved] = useState(false);

  const loadPolicy = useCallback(async () => {
    setLoading(true);
    setError(null);
    setSaved(false);
    try {
      const value = await getAutoCleanupPolicy();
      setPolicy(value);
      setSavedPolicy(value);
    } catch {
      setPolicy(null);
      setSavedPolicy(null);
      setError("load");
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadPolicy();
  }, [loadPolicy]);

  const updatePolicy = useCallback((enabled: boolean, graceDays: number) => {
    setPolicy((current) => current && { ...current, enabled, graceDays });
    setError(null);
    setSaved(false);
  }, []);

  const savePolicy = useCallback(async () => {
    if (!policy) return;
    setSaving(true);
    setError(null);
    setSaved(false);
    try {
      const value = await setAutoCleanupPolicy(policy.enabled, policy.graceDays);
      setPolicy(value);
      setSavedPolicy(value);
      setSaved(true);
    } catch {
      setError("save");
    } finally {
      setSaving(false);
    }
  }, [policy]);

  return {
    policy,
    loading,
    saving,
    error,
    saved,
    dirty: !policiesMatch(policy, savedPolicy),
    loadPolicy,
    updatePolicy,
    savePolicy,
  };
}