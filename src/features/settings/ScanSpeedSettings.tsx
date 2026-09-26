import { type ReactElement, useCallback, useEffect, useState } from "react";
import {
  getScanSettings,
  isScanProfile,
  type ScanProfile,
  setScanProfile,
} from "./api/scanSettings";

const PROFILE_OPTIONS: ReadonlyArray<{ value: ScanProfile; label: string; hint: string }> = [
  {
    value: "auto",
    label: "Automatic (recommended)",
    hint: "Checks each drive. Build-artifact discovery on solid-state drives searches more folders at once; hard drives and unknown drives keep the standard amount.",
  },
  {
    value: "ssd",
    label: "Solid-state drive",
    hint: "Build-artifact discovery always searches more folders at once. Fastest on SSD and NVMe drives.",
  },
  {
    value: "hdd",
    label: "Hard drive",
    hint: "Build-artifact discovery always searches the standard number of folders at once. Avoids extra disk seeking on spinning drives.",
  },
];

type Status = "loading" | "ready" | "saving" | "load-error";

export function ScanSpeedSettings(): ReactElement {
  const [status, setStatus] = useState<Status>("loading");
  const [profile, setProfile] = useState<ScanProfile>("auto");
  const [savedProfile, setSavedProfile] = useState<ScanProfile>("auto");
  const [saveError, setSaveError] = useState(false);
  const [saved, setSaved] = useState(false);

  const load = useCallback(async (): Promise<void> => {
    setStatus("loading");
    setSaveError(false);
    setSaved(false);
    try {
      const settings = await getScanSettings();
      setProfile(settings.profile);
      setSavedProfile(settings.profile);
      setStatus("ready");
    } catch {
      setStatus("load-error");
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const save = async (): Promise<void> => {
    setStatus("saving");
    setSaveError(false);
    setSaved(false);
    try {
      const settings = await setScanProfile(profile);
      setProfile(settings.profile);
      setSavedProfile(settings.profile);
      setSaved(true);
    } catch {
      setSaveError(true);
    } finally {
      setStatus("ready");
    }
  };

  const saving = status === "saving";
  const hint = PROFILE_OPTIONS.find((option) => option.value === profile)?.hint;

  return (
    <div className="settings-panel" aria-busy={status === "loading" || saving}>
      <h2>Scan speed</h2>
      <p>
        Currently speeds up only build-artifact discovery in project cleanup. Other scans, such as Large files and
        Disk analyzer, read one folder at a time, so this setting does not change them.
      </p>
      {status === "loading" && <p role="status">Loading scan settings…</p>}
      {status === "load-error" && (
        <div className="error-state" role="alert">
          <p>Scan settings could not be loaded.</p>
          <button type="button" className="secondary-button" onClick={() => void load()}>
            Try again
          </button>
        </div>
      )}
      {(status === "ready" || saving) && (
        <form
          onSubmit={(event) => {
            event.preventDefault();
            void save();
          }}
        >
          <label className="settings-field">
            <span>
              <strong>Drive type</strong>
              <small id="scan-profile-hint">{hint}</small>
            </span>
            <select
              value={profile}
              disabled={saving}
              aria-describedby="scan-profile-hint"
              onChange={(event) => {
                const value = event.target.value;
                if (!isScanProfile(value)) return;
                setProfile(value);
                setSaveError(false);
                setSaved(false);
              }}
            >
              {PROFILE_OPTIONS.map((option) => (
                <option key={option.value} value={option.value}>
                  {option.label}
                </option>
              ))}
            </select>
          </label>
          <p className="settings-note">Applies to the next build-artifact discovery you start.</p>
          {saveError && (
            <div className="error-state" role="alert">
              <p>Scan settings could not be saved. Your change remains unsaved.</p>
            </div>
          )}
          {saved && (
            <p className="status-message" role="status">
              Scan settings saved.
            </p>
          )}
          <div className="button-row">
            <button type="submit" disabled={saving || profile === savedProfile}>
              {saving ? "Saving…" : "Save scan settings"}
            </button>
          </div>
        </form>
      )}
    </div>
  );
}
