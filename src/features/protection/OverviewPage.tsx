import { useCallback, useEffect, useState } from "react";
import { getDefenderHistory, getOverview, setNetworkPolicy } from "./api";
import { EvidenceBadge, errorMessage, evidenceDetail, summaryLine } from "./labels";
import type { DefenderHistory, NetworkPolicy, ProtectionOverview } from "./types";

export const rulesSourceLabel = {
  installed: "Installed pack",
  previousFallback: "Previous pack (the newest pack failed verification)",
  embeddedBaseline: "Built-in baseline pack",
} as const;

export function OverviewPage() {
  const [overview, setOverview] = useState<ProtectionOverview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [defender, setDefender] = useState<DefenderHistory | null>(null);
  const [defenderLoading, setDefenderLoading] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setOverview(await getOverview());
      setError(null);
    } catch (reason) {
      setError(errorMessage(reason, "Protection status could not be read."));
    }
  }, []);
  useEffect(() => { void refresh(); }, [refresh]);

  // The toggles stay enabled while saving: disabling them would drop keyboard
  // focus. Changes made during a save are ignored instead.
  const update = async (network: NetworkPolicy, amsiEnabled: boolean) => {
    if (saving) return;
    setSaving(true);
    try {
      const settings = await setNetworkPolicy(network, amsiEnabled);
      setOverview((current) => current && { ...current, settings });
      setError(null);
    } catch (reason) {
      setError(errorMessage(reason, "The setting could not be saved."));
    } finally {
      setSaving(false);
    }
  };

  const loadDefender = async () => {
    setDefenderLoading(true);
    try {
      setDefender(await getDefenderHistory());
    } catch (reason) {
      setError(errorMessage(reason, "Defender history could not be read."));
    } finally {
      setDefenderLoading(false);
    }
  };

  if (!overview) return error ? <p role="alert">{error}</p> : <p role="status">Loading protection status…</p>;
  const { settings, rules } = overview;
  const network = settings.network;

  return <>
    {error && <p role="alert">{error}</p>}
    <section aria-labelledby="protection-status">
      <h2 id="protection-status">Status</h2>
      <dl className="protection-facts">
        <dt>Rules</dt>
        <dd>{rulesSourceLabel[rules.source]}: sequence {rules.sequence}, {rules.ruleCount} rules</dd>
        <dt>Last scan</dt>
        <dd>{overview.lastScan ? summaryLine(overview.lastScan) : "No scan yet"}</dd>
        <dt>Quarantine</dt>
        <dd>{overview.quarantineCount} items</dd>
      </dl>
      {rules.recoveryNote && <p role="note">{rules.recoveryNote}</p>}
    </section>

    <section aria-labelledby="protection-network">
      <h2 id="protection-network">Network use</h2>
      <p>Everything below is off by default. Scanning always runs on this PC without the network.</p>
      <fieldset aria-busy={saving}>
        <legend>Optional features that send data</legend>
        <label>
          <input type="checkbox" checked={network.ruleDownload}
            onChange={(event) => void update({ ...network, ruleDownload: event.target.checked }, settings.amsiEnabled)} />
          Allow downloading signed rule packs
        </label>
        <p className="protection-hint">Contacts raw.githubusercontent.com only when you press Download. Sends no information about your files.</p>
        <label>
          <input type="checkbox" checked={network.passwordBreachCheck}
            onChange={(event) => void update({ ...network, passwordBreachCheck: event.target.checked }, settings.amsiEnabled)} />
          Allow the password breach check
        </label>
        <p className="protection-hint">Sends only the first 5 characters of the password's SHA-1 hash to api.pwnedpasswords.com. The password itself never leaves this PC.</p>
        <label>
          <input type="checkbox" checked={settings.amsiEnabled}
            onChange={(event) => void update(network, event.target.checked)} />
          Ask the installed antivirus about flagged files (AMSI)
        </label>
        <p className="protection-hint">Your antivirus may use its own cloud service for this, depending on its settings.</p>
      </fieldset>
    </section>

    <section aria-labelledby="protection-defender">
      <h2 id="protection-defender">Microsoft Defender history</h2>
      <p>Reads what Defender has already recorded. This does not start a Defender scan.</p>
      <button type="button" disabled={defenderLoading} onClick={() => void loadDefender()}>Read detection history</button>
      {defenderLoading && <p role="status">Reading Defender history…</p>}
      {defender?.unavailable && <p><EvidenceBadge evidence={defender.unavailable} /> {evidenceDetail(defender.unavailable)}. Defender may not be the active antivirus.</p>}
      {defender && !defender.unavailable && defender.detections.length === 0 && <p>Defender has no recorded detections.</p>}
      {defender && defender.detections.length > 0 && <ul>{defender.detections.map((detection, index) => <li key={`${detection.threatId}-${index}`}>
        <EvidenceBadge evidence={detection.evidence} /> Threat {detection.threatId ?? "unknown"}{detection.detectedAt ? `, first seen ${detection.detectedAt}` : ""}
        {detection.resources.length > 0 && <ul>{detection.resources.map((resource) => <li key={resource}><code>{resource}</code></li>)}</ul>}
      </li>)}</ul>}
      {defender?.truncated && <p role="note">Showing the first 500 recorded detections.</p>}
    </section>

    <section aria-labelledby="protection-heuristics">
      <h2 id="protection-heuristics">Heuristics and their limits</h2>
      <ul>{overview.heuristics.map((heuristic) => <li key={heuristic.id}>
        <strong>{heuristic.title}</strong> <code>{heuristic.id}</code>
        <p className="protection-hint">False positives: {heuristic.falsePositiveNote}</p>
      </li>)}</ul>
    </section>
  </>;
}
