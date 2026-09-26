import { useId, useState } from "react";
import { useAppSettings } from "../../shared/app-settings/AppSettingsProvider";
import { useStrings } from "../../shared/i18n/I18nProvider";
import { isLanguagePreference, LANGUAGE_PREFERENCES } from "../../shared/i18n/locale";
import { settingsStrings } from "./strings";

export function LanguageSettings() {
  const t = useStrings(settingsStrings).language;
  const { settings, loaded, save } = useAppSettings();
  const [status, setStatus] = useState<"idle" | "saving" | "failed">("idle");
  const selectId = useId();

  return (
    <div className="settings-panel" aria-busy={!loaded || status === "saving"}>
      <h2>{t.heading}</h2>
      <p>{t.description}</p>
      <label className="settings-field" htmlFor={selectId}>
        <span>
          <strong>{t.label}</strong>
          <small>{t.hint}</small>
        </span>
        <select
          id={selectId}
          value={settings.language}
          disabled={!loaded || status === "saving"}
          onChange={async (event) => {
            const language = event.target.value;
            if (!isLanguagePreference(language)) return;
            setStatus("saving");
            const outcome = await save({ ...settings, language });
            setStatus(outcome === "saved" ? "idle" : "failed");
          }}
        >
          {LANGUAGE_PREFERENCES.map((option) => (
            <option key={option} value={option} lang={option === "system" ? undefined : option}>
              {t.options[option]}
            </option>
          ))}
        </select>
      </label>
      {status === "failed" && (
        <div className="error-state" role="alert">
          <p>{t.saveError}</p>
        </div>
      )}
    </div>
  );
}
