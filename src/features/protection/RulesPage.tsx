import { useCallback, useEffect, useState } from "react";
import { useFormat, useStrings } from "../../shared/i18n/I18nProvider";
import { clearAllowlist, downloadRulePack, getOverview, importRulePack, restorePreviousRulePack } from "./api";
import { errorMessage, isCancelled } from "./labels";
import { protectionStrings } from "./strings";
import type { ProtectionOverview } from "./types";

export function RulesPage() {
  const strings = useStrings(protectionStrings);
  const t = strings.rules;
  const fmt = useFormat();
  const [overview, setOverview] = useState<ProtectionOverview | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setOverview(await getOverview());
    } catch (reason) {
      setError(errorMessage(reason, t.loadFailed));
    }
  }, [t]);
  useEffect(() => { void refresh(); }, [refresh]);

  const run = async (action: () => Promise<unknown>, done: string) => {
    setBusy(true);
    setError(null);
    setMessage(null);
    try {
      await action();
      setMessage(done);
    } catch (reason) {
      if (!isCancelled(reason)) setError(errorMessage(reason, t.unchanged));
    } finally {
      setBusy(false);
      await refresh();
    }
  };

  if (!overview) return error ? <p role="alert">{error}</p> : <p role="status">{strings.loading}</p>;
  const { rules, settings } = overview;
  return <section aria-labelledby="rules-title">
    <h2 id="rules-title">{t.title}</h2>
    <dl className="protection-facts">
      <dt>{t.active}</dt><dd>{t.activeValue(strings.rulesSource[rules.source], String(rules.sequence))}</dd>
      <dt>{t.created}</dt><dd>{rules.created}</dd>
      <dt>{t.contents}</dt><dd>{t.contentsValue(fmt.number(rules.ruleCount), rules.description)}</dd>
      <dt>{t.previous}</dt><dd>{rules.previousSequence ?? t.noneKept}</dd>
    </dl>
    {rules.recoveryNote && <p role="note">{rules.recoveryNote}</p>}
    <p>{t.signatureNote}</p>
    {!rules.externalPacksAllowed && <p role="note">{t.testKeyNote}</p>}
    {error && <p role="alert">{error}</p>}
    {message && <p role="status">{message}</p>}
    <div className="protection-actions">
      <button type="button" disabled={busy || !rules.externalPacksAllowed} onClick={() => void run(importRulePack, t.installed)}>{t.importFromFolder}</button>
      <button type="button" disabled={busy || !rules.externalPacksAllowed || !settings.network.ruleDownload} onClick={() => void run(downloadRulePack, t.downloaded)}>{t.download}</button>
      <button type="button" disabled={busy || rules.previousSequence === null} onClick={() => void run(restorePreviousRulePack, t.restoredPrevious)}>{t.goBack}</button>
    </div>
    {!settings.network.ruleDownload && <p className="protection-hint">{t.downloadOff}</p>}

    <h3>{t.hiddenTitle}</h3>
    <p>{t.hiddenCount(fmt.number(settings.allowlist.length))}</p>
    <button type="button" disabled={busy || settings.allowlist.length === 0} onClick={() => void run(clearAllowlist, t.shownAgain)}>{t.showAll}</button>

    <h3>{t.limitsTitle}</h3>
    <p>{t.limits}</p>
  </section>;
}
