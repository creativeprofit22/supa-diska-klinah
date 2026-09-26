import { type ReactElement, useCallback, useEffect, useState } from "react";
import { useStrings } from "../../shared/i18n/I18nProvider";
import {
  getScanSettings,
  isScanProfile,
  SCAN_PROFILES,
  type ScanProfile,
  setScanProfile,
} from "./api/scanSettings";
import { settingsStrings } from "./strings";

type Status = "loading" | "ready" | "saving" | "load-error";

export function ScanSpeedSettings(): ReactElement {
  const common = useStrings(settingsStrings);
  const t = common.scanSpeed;
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
  const hint = t.hints[profile];

  return (
    <div className="settings-panel" aria-busy={status === "loading" || saving}>
      <h2>{t.heading}</h2>
      <p>{t.description}</p>
      {status === "loading" && <p role="status">{t.loading}</p>}
      {status === "load-error" && (
        <div className="error-state" role="alert">
          <p>{t.loadError}</p>
          <button type="button" className="secondary-button" onClick={() => void load()}>
            {common.tryAgain}
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
              <strong>{t.driveType}</strong>
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
              {SCAN_PROFILES.map((option) => (
                <option key={option} value={option}>
                  {t.profiles[option]}
                </option>
              ))}
            </select>
          </label>
          <p className="settings-note">{t.appliesNote}</p>
          {saveError && (
            <div className="error-state" role="alert">
              <p>{t.saveError}</p>
            </div>
          )}
          {saved && (
            <p className="status-message" role="status">
              {t.saved}
            </p>
          )}
          <div className="button-row">
            <button type="submit" disabled={saving || profile === savedProfile}>
              {saving ? common.saving : t.save}
            </button>
          </div>
        </form>
      )}
    </div>
  );
}
