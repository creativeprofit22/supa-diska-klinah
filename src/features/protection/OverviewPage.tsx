import { useCallback, useEffect, useState } from "react";
import { useFormat, useStrings } from "../../shared/i18n/I18nProvider";
import { getDefenderHistory, getOverview, setNetworkPolicy } from "./api";
import { EvidenceBadge, errorMessage, evidenceDetail, summaryLine } from "./labels";
import { protectionStrings } from "./strings";
import type { DefenderHistory, NetworkPolicy, ProtectionOverview } from "./types";

/** English labels; components use `useStrings(protectionStrings).rulesSource`. */
export const rulesSourceLabel = protectionStrings.en.rulesSource;

export function OverviewPage() {
  const strings = useStrings(protectionStrings);
  const t = strings.overview;
  const fmt = useFormat();
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
      setError(errorMessage(reason, t.loadFailed));
    }
  }, [t]);
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
      setError(errorMessage(reason, t.saveFailed));
    } finally {
      setSaving(false);
    }
  };

  const loadDefender = async () => {
    setDefenderLoading(true);
    try {
      setDefender(await getDefenderHistory());
    } catch (reason) {
      setError(errorMessage(reason, t.defenderFailed));
    } finally {
      setDefenderLoading(false);
    }
  };

  if (!overview) return error ? <p role="alert">{error}</p> : <p role="status">{t.loading}</p>;
  const { settings, rules } = overview;
  const network = settings.network;

  return <>
    {error && <p role="alert">{error}</p>}
    <section aria-labelledby="protection-status">
      <h2 id="protection-status">{t.statusTitle}</h2>
      <dl className="protection-facts">
        <dt>{t.rules}</dt>
        <dd>{t.rulesValue(strings.rulesSource[rules.source], String(rules.sequence), fmt.number(rules.ruleCount))}</dd>
        <dt>{t.lastScan}</dt>
        <dd>{overview.lastScan ? summaryLine(overview.lastScan, strings, fmt.number) : t.noScan}</dd>
        <dt>{t.quarantine}</dt>
        <dd>{t.quarantineItems(fmt.number(overview.quarantineCount))}</dd>
      </dl>
      {rules.recoveryNote && <p role="note">{rules.recoveryNote}</p>}
    </section>

    <section aria-labelledby="protection-network">
      <h2 id="protection-network">{t.networkTitle}</h2>
      <p>{t.networkIntro}</p>
      <fieldset aria-busy={saving}>
        <legend>{t.networkLegend}</legend>
        <label>
          <input type="checkbox" checked={network.ruleDownload}
            onChange={(event) => void update({ ...network, ruleDownload: event.target.checked }, settings.amsiEnabled)} />
          {t.ruleDownload}
        </label>
        <p className="protection-hint">{t.ruleDownloadHint}</p>
        <label>
          <input type="checkbox" checked={network.passwordBreachCheck}
            onChange={(event) => void update({ ...network, passwordBreachCheck: event.target.checked }, settings.amsiEnabled)} />
          {t.breachCheck}
        </label>
        <p className="protection-hint">{t.breachCheckHint}</p>
        <label>
          <input type="checkbox" checked={settings.amsiEnabled}
            onChange={(event) => void update(network, event.target.checked)} />
          {t.amsi}
        </label>
        <p className="protection-hint">{t.amsiHint}</p>
      </fieldset>
    </section>

    <section aria-labelledby="protection-defender">
      <h2 id="protection-defender">{t.defenderTitle}</h2>
      <p>{t.defenderIntro}</p>
      <button type="button" disabled={defenderLoading} onClick={() => void loadDefender()}>{t.readHistory}</button>
      {defenderLoading && <p role="status">{t.readingHistory}</p>}
      {defender?.unavailable && <p><EvidenceBadge evidence={defender.unavailable} /> {t.defenderUnavailable(evidenceDetail(defender.unavailable, strings))}</p>}
      {defender && !defender.unavailable && defender.detections.length === 0 && <p>{t.noDetections}</p>}
      {defender && defender.detections.length > 0 && <ul>{defender.detections.map((detection, index) => <li key={`${detection.threatId}-${index}`}>
        <EvidenceBadge evidence={detection.evidence} /> {t.threat(detection.threatId ?? t.unknownThreat)}{detection.detectedAt ? t.firstSeen(detection.detectedAt) : ""}
        {detection.resources.length > 0 && <ul>{detection.resources.map((resource) => <li key={resource}><code>{resource}</code></li>)}</ul>}
      </li>)}</ul>}
      {defender?.truncated && <p role="note">{t.truncated}</p>}
    </section>

    <section aria-labelledby="protection-heuristics">
      <h2 id="protection-heuristics">{t.heuristicsTitle}</h2>
      <ul>{overview.heuristics.map((heuristic) => <li key={heuristic.id}>
        <strong>{heuristic.title}</strong> <code>{heuristic.id}</code>
        <p className="protection-hint">{t.falsePositives(heuristic.falsePositiveNote)}</p>
      </li>)}</ul>
    </section>
  </>;
}
