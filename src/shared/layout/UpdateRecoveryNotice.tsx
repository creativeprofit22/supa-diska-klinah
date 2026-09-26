import { useEffect, useState, type ReactElement } from "react";
import { Link } from "react-router-dom";
import { updateApi, type UpdateApi, type UpdateRecovery } from "../app-settings/updateApi";
import { useStrings } from "../i18n/I18nProvider";
import { layoutStrings } from "./strings";

/** Startup recovery runs in the background; ask again briefly until it has. */
const POLL_MS = 500;
const MAX_POLLS = 20;

type Props = Readonly<{ api?: Pick<UpdateApi, "status" | "acknowledgeRecovery"> }>;

/**
 * One-time, app-wide notice about what startup recovery found: a finished
 * update, an interrupted one (retry/discard live in Settings) or a staged
 * installer that failed re-verification and was deleted.
 */
export function UpdateRecoveryNotice({ api = updateApi }: Props): ReactElement | null {
  const t = useStrings(layoutStrings).updateNotice;
  const [recovery, setRecovery] = useState<UpdateRecovery | null>(null);

  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const poll = async (attempt: number): Promise<void> => {
      const result = await api.status();
      if (cancelled || !result.ok) return;
      if (result.value.recoveryPending && attempt < MAX_POLLS) {
        timer = setTimeout(() => { void poll(attempt + 1); }, POLL_MS);
        return;
      }
      setRecovery(result.value.recovery ?? null);
    };
    void poll(1);
    return () => {
      cancelled = true;
      if (timer !== undefined) clearTimeout(timer);
    };
  }, [api]);

  if (!recovery) return null;

  const dismiss = async (): Promise<void> => {
    setRecovery(null);
    await api.acknowledgeRecovery();
  };
  const interrupted = recovery.outcome === "interrupted";
  const message =
    recovery.outcome === "updated" ? t.updated(recovery.version)
    : recovery.outcome === "interrupted" ? t.interrupted(recovery.version)
    : t.discarded;

  return (
    <div className={interrupted ? "update-notice update-notice-alert" : "update-notice"} role={interrupted ? "alert" : "status"}>
      <p>{message}</p>
      <div className="update-notice-actions">
        {recovery.outcome !== "updated" && <Link to="/settings">{t.openSettings}</Link>}
        <button type="button" onClick={() => { void dismiss(); }}>{t.dismiss}</button>
      </div>
    </div>
  );
}
