import { useCallback, useEffect, useId, useRef, useState } from "react";
import { useAppSettings } from "../../shared/app-settings/AppSettingsProvider";
import { type UpdateApi, updateApi, type UpdateErrorCode, type UpdateStatus } from "../../shared/app-settings/updateApi";
import { useFormat, useStrings } from "../../shared/i18n/I18nProvider";
import { settingsStrings } from "./strings";

type Busy = "check" | "download" | "install" | "discard" | null;

/** Startup recovery re-hashes a staged installer in the background; ask again until it is done. */
const RECOVERY_POLL_MS = 1000;
const MAX_RECOVERY_POLLS = 60;

/** Opt-in self-update: every network step and the install are explicit user actions. */
export function UpdateSettings({ api = updateApi }: Readonly<{ api?: UpdateApi }>) {
  const t = useStrings(settingsStrings).updates;
  const fmt = useFormat();
  const { settings, loaded, save } = useAppSettings();
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  const [busy, setBusy] = useState<Busy>(null);
  const [error, setError] = useState<UpdateErrorCode | "invalidResponse" | "save" | null>(null);
  const toggleId = useId();
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    void (async () => {
      const result = await api.status();
      if (!mounted.current) return;
      if (result.ok) setStatus(result.value);
      else setError(result.error);
    })();
    return () => {
      mounted.current = false;
    };
  }, [api]);

  const recovering = status?.update.state === "recovering";
  useEffect(() => {
    if (!recovering) return undefined;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const poll = async (attempt: number): Promise<void> => {
      const result = await api.status();
      if (cancelled) return;
      if (!result.ok) {
        setError(result.error);
        return;
      }
      if (result.value.update.state !== "recovering") setStatus(result.value);
      else if (attempt < MAX_RECOVERY_POLLS) timer = setTimeout(() => { void poll(attempt + 1); }, RECOVERY_POLL_MS);
    };
    timer = setTimeout(() => { void poll(1); }, RECOVERY_POLL_MS);
    return () => {
      cancelled = true;
      if (timer !== undefined) clearTimeout(timer);
    };
  }, [api, recovering]);

  const run = useCallback(async (step: Exclude<Busy, null>, action: () => Promise<Awaited<ReturnType<UpdateApi["status"]>>>) => {
    setBusy(step);
    setError(null);
    const result = await action();
    if (!mounted.current) return;
    setBusy(null);
    if (result.ok) setStatus(result.value);
    else setError(result.error);
  }, []);

  const update = status?.update;
  const enabled = settings.updateCheck;
  const disabled = busy !== null || !loaded;

  return (
    <div className="settings-panel" aria-busy={busy !== null}>
      <h2>{t.heading}</h2>
      <p>{t.description}</p>
      <label className="settings-field" htmlFor={toggleId}>
        <span>
          <strong>{t.toggle}</strong>
          <small>{t.toggleHint}</small>
        </span>
        <input
          id={toggleId}
          type="checkbox"
          checked={enabled}
          disabled={disabled}
          onChange={async (event) => {
            const outcome = await save({ ...settings, updateCheck: event.target.checked });
            setError(outcome === "saved" ? null : "save");
          }}
        />
      </label>
      {status && <p>{t.currentVersion(status.currentVersion)}</p>}
      <p>{t.unsignedNote}</p>

      {status && !status.configured ? (
        <p role="status">{t.notConfigured}</p>
      ) : (
        <>
          <div role="status">
            {update?.state === "recovering" && <p>{t.recovering}</p>}
            {busy === "check" && <p>{t.checking}</p>}
            {busy === "download" && update?.state === "available" && <p>{t.downloading(update.version)}</p>}
            {busy === null && update?.state === "upToDate" && <p>{t.upToDate}</p>}
            {busy === null && update?.state === "available" && <p>{t.available(update.version, fmt.bytes(update.size))}</p>}
            {busy === null && update?.state === "verified" && <p>{t.verified(update.version)}</p>}
            {busy === null && update?.state === "launched" && <p>{t.launched(update.version)}</p>}
          </div>
          {update?.state === "interrupted" && (
            <div className="error-state" role="alert">
              <p>{t.interrupted(update.version)}</p>
            </div>
          )}
          <div className="settings-actions">
            {enabled && (update?.state === "idle" || update?.state === "upToDate" || update === undefined) && (
              <button type="button" disabled={disabled} onClick={() => void run("check", api.check)}>{t.check}</button>
            )}
            {enabled && update?.state === "available" && (
              <button type="button" disabled={disabled} onClick={() => void run("download", api.download)}>{t.download}</button>
            )}
            {(update?.state === "verified" || update?.state === "interrupted") && (
              <>
                <button type="button" disabled={disabled} onClick={() => void run("install", api.install)}>
                  {update.state === "interrupted" ? t.retry : t.install}
                </button>
                <button type="button" disabled={disabled} onClick={() => void run("discard", api.discard)}>{t.discard}</button>
              </>
            )}
          </div>
          {(update?.state === "verified" || update?.state === "interrupted") && <p><small>{t.installHint}</small></p>}
        </>
      )}
      {error && (
        <div className="error-state" role="alert">
          <p>{error === "save" ? t.saveError : t.errors[error]}</p>
        </div>
      )}
    </div>
  );
}
