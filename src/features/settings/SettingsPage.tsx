import { useEffect } from "react";
import { useStrings } from "../../shared/i18n/I18nProvider";
import { ArtifactBudgetSettings } from "../build-artifacts/ArtifactBudgetSettings";
import { useSettingsState } from "./model/useSettingsState";
import { LanguageSettings } from "./LanguageSettings";
import { UpdateSettings } from "./UpdateSettings";
import { ScanSpeedSettings } from "./ScanSpeedSettings";
import { settingsStrings } from "./strings";

export function SettingsPage() {
  const t = useStrings(settingsStrings);
  const {
    policy,
    loading,
    saving,
    error,
    saved,
    dirty,
    loadPolicy,
    updatePolicy,
    savePolicy,
  } = useSettingsState();

  useEffect(() => {
    document.title = t.documentTitle;
  }, [t]);

  return (
    <section aria-labelledby="settings-heading">
      <header className="page-header">
        <p className="kicker">{t.kicker}</p>
        <h1 id="settings-heading">{t.heading}</h1>
        <p>{t.intro}</p>
      </header>

      <div className="settings-panel" aria-busy={loading || saving}>
        <h2>{t.autoCleanup.heading}</h2>
        <p>{t.autoCleanup.description}</p>
        {loading && <p role="status">{t.autoCleanup.loading}</p>}
        {error === "load" && (
          <div className="error-state" role="alert">
            <p>{t.autoCleanup.loadError}</p>
            <button type="button" className="secondary-button" onClick={() => void loadPolicy()}>
              {t.tryAgain}
            </button>
          </div>
        )}
        {policy && (
          <form
            onSubmit={(event) => {
              event.preventDefault();
              void savePolicy();
            }}
          >
            <label className="toggle-row">
              <span>
                <strong>{t.autoCleanup.enabledLabel}</strong>
                <small>{t.autoCleanup.enabledHint}</small>
              </span>
              <input
                type="checkbox"
                checked={policy.enabled}
                disabled={saving}
                onChange={(event) => updatePolicy(event.target.checked, policy.graceDays)}
              />
            </label>
            <label className="settings-field">
              <span>
                <strong>{t.autoCleanup.graceLabel}</strong>
                <small>{t.autoCleanup.graceHint}</small>
              </span>
              <select
                value={policy.graceDays}
                disabled={saving}
                onChange={(event) => updatePolicy(policy.enabled, Number(event.target.value))}
              >
                {[1, 3, 7, 14, 30].map((days) => (
                  <option key={days} value={days}>
                    {t.autoCleanup.graceDays(days)}
                  </option>
                ))}
              </select>
            </label>
            <p className="settings-note">{t.autoCleanup.purgeNote}</p>
            {error === "save" && (
              <div className="error-state" role="alert">
                <p>{t.autoCleanup.saveError}</p>
              </div>
            )}
            {saved && (
              <p className="status-message" role="status">
                {t.autoCleanup.saved}
              </p>
            )}
            <div className="button-row">
              <button type="submit" disabled={saving || !dirty}>
                {saving ? t.saving : t.autoCleanup.save}
              </button>
            </div>
          </form>
        )}
      </div>
      <LanguageSettings />
      <UpdateSettings />
      <ScanSpeedSettings />
      <ArtifactBudgetSettings />
    </section>
  );
}
