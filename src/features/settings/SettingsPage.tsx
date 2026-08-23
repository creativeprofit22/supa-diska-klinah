import { useEffect } from "react";
import { useSettingsState } from "./model/useSettingsState";

export function SettingsPage() {
  const { showAdvanced, setShowAdvanced } = useSettingsState();

  useEffect(() => {
    document.title = "Settings | Supa Diska Klinah";
  }, []);

  return (
    <section aria-labelledby="settings-heading">
      <header className="page-header">
        <p className="kicker">Preferences</p>
        <h1 id="settings-heading">Settings</h1>
        <p>Foundation-only preferences stay local to this screen and are not saved.</p>
      </header>

      <div className="settings-panel">
        <h2>Interface preview</h2>
        <label className="toggle-row">
          <span>
            <strong>Show advanced cleanup options</strong>
            <small>Preview only. Cleanup controls are not implemented.</small>
          </span>
          <input
            type="checkbox"
            checked={showAdvanced}
            onChange={(event) => setShowAdvanced(event.target.checked)}
          />
        </label>
      </div>
    </section>
  );
}
