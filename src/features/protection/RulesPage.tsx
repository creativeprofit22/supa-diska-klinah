import { useCallback, useEffect, useState } from "react";
import { clearAllowlist, downloadRulePack, getOverview, importRulePack, restorePreviousRulePack } from "./api";
import { errorMessage, isCancelled } from "./labels";
import { rulesSourceLabel } from "./OverviewPage";
import type { ProtectionOverview } from "./types";

export function RulesPage() {
  const [overview, setOverview] = useState<ProtectionOverview | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setOverview(await getOverview());
    } catch (reason) {
      setError(errorMessage(reason, "Rule status could not be read."));
    }
  }, []);
  useEffect(() => { void refresh(); }, [refresh]);

  const run = async (action: () => Promise<unknown>, done: string) => {
    setBusy(true);
    setError(null);
    setMessage(null);
    try {
      await action();
      setMessage(done);
    } catch (reason) {
      if (!isCancelled(reason)) setError(errorMessage(reason, "The rule pack was not changed."));
    } finally {
      setBusy(false);
      await refresh();
    }
  };

  if (!overview) return error ? <p role="alert">{error}</p> : <p role="status">Loading…</p>;
  const { rules, settings } = overview;
  return <section aria-labelledby="rules-title">
    <h2 id="rules-title">Rule packs</h2>
    <dl className="protection-facts">
      <dt>Active</dt><dd>{rulesSourceLabel[rules.source]}, sequence {rules.sequence}</dd>
      <dt>Created</dt><dd>{rules.created}</dd>
      <dt>Contents</dt><dd>{rules.ruleCount} rules. {rules.description}</dd>
      <dt>Previous pack</dt><dd>{rules.previousSequence ?? "None kept"}</dd>
    </dl>
    {rules.recoveryNote && <p role="note">{rules.recoveryNote}</p>}
    <p>Packs are accepted only if their Ed25519 signature matches the key built into this app, and only if they are newer than the active pack. A failed update leaves the active pack in place.</p>
    {!rules.externalPacksAllowed && <p role="note">This build only has a test signing key, so importing and downloading packs is turned off. The built-in pack keeps working.</p>}
    {error && <p role="alert">{error}</p>}
    {message && <p role="status">{message}</p>}
    <div className="protection-actions">
      <button type="button" disabled={busy || !rules.externalPacksAllowed} onClick={() => void run(importRulePack, "Rule pack installed.")}>Import from folder…</button>
      <button type="button" disabled={busy || !rules.externalPacksAllowed || !settings.network.ruleDownload} onClick={() => void run(downloadRulePack, "Rule pack downloaded and installed.")}>Download latest pack</button>
      <button type="button" disabled={busy || rules.previousSequence === null} onClick={() => void run(restorePreviousRulePack, "Previous rule pack restored.")}>Go back to previous pack…</button>
    </div>
    {!settings.network.ruleDownload && <p className="protection-hint">Downloading is off. Turn it on in Overview if you want it.</p>}

    <h3>Hidden heuristic findings</h3>
    <p>{settings.allowlist.length} file contents are hidden from heuristic results. Signed-rule matches are never hidden.</p>
    <button type="button" disabled={busy || settings.allowlist.length === 0} onClick={() => void run(clearAllowlist, "Hidden findings will show again in the next scan.")}>Show all again</button>

    <h3>Detection limits</h3>
    <p>The built-in pack only detects the EICAR test file. Real detection depends on packs you import. YARA rules are not supported.</p>
  </section>;
}
