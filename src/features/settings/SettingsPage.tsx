import { useEffect } from "react";
import { useSettingsState } from "./model/useSettingsState";

export function SettingsPage() {
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
    document.title = "Settings | Supa Diska Klinah";
  }, []);

  return (
    <section aria-labelledby="settings-heading">
      <header className="page-header">
        <p className="kicker">Preferences</p>
        <h1 id="settings-heading">Settings</h1>
        <p>Cleanup preferences are saved on this device.</p>
      </header>

      <div className="settings-panel" aria-busy={loading || saving}>
        <h2>Automatic cleanup</h2>
        <p>When enabled, eligible temporary caches enter app-managed quarantine at startup.</p>
        {loading && <p role="status">Loading cleanup settings…</p>}
        {error === "load" && (
          <div className="error-state" role="alert">
            <p>Cleanup settings could not be loaded.</p>
            <button type="button" className="secondary-button" onClick={() => void loadPolicy()}>
              Try again
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
                <strong>Clean temporary caches at startup</strong>
                <small>Off by default. Disabling stops future quarantine and purge.</small>
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
                <strong>Recovery grace period</strong>
                <small>Quarantined items remain undoable until this period ends.</small>
              </span>
              <select
                value={policy.graceDays}
                disabled={saving}
                onChange={(event) => updatePolicy(policy.enabled, Number(event.target.value))}
              >
                {[1, 3, 7, 14, 30].map((days) => (
                  <option key={days} value={days}>
                    {days} {days === 1 ? "day" : "days"}
                  </option>
                ))}
              </select>
            </label>
            <p className="settings-note">
              Due quarantine is permanently purged during a later startup maintenance pass.
            </p>
            {error === "save" && (
              <div className="error-state" role="alert">
                <p>Cleanup settings could not be saved. Your changes remain unsaved.</p>
              </div>
            )}
            {saved && (
              <p className="status-message" role="status">
                Cleanup settings saved.
              </p>
            )}
            <div className="button-row">
              <button type="submit" disabled={saving || !dirty}>
                {saving ? "Saving…" : "Save changes"}
              </button>
            </div>
          </form>
        )}
      </div>
    </section>
  );
}