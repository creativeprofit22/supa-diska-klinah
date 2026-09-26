import { useCallback, useEffect, useMemo, useState } from "react";
import { SystemChangeJournal } from "../../shared/system-change/SystemChangeJournal";
import { SystemChangeReview } from "../../shared/system-change/SystemChangeReview";
import { useFormat, useStrings } from "../../shared/i18n/I18nProvider";
import { useSystemChangeLabels } from "../../shared/system-change/labels";
import type { SystemChange } from "../../shared/system-change/types";
import { getFirewallStatus } from "./api";
import { firewallStrings, type FirewallStrings } from "./strings";
import type { FirewallProfile, FirewallRule, FirewallStatus } from "./types";

export const MAX_VISIBLE_RULES = 200;

export function describe(change: SystemChange, t: FirewallStrings = firewallStrings.en): string {
  switch (change.kind) {
    case "setFirewallRuleEnabled": return t.describeRule(change.enabled, change.ruleName);
    case "setFirewallProfileEnabled": return t.describeProfile(change.enabled, t.profile[change.profile]);
    default: return change.kind;
  }
}

const ruleKey = (name: string) => `rule:${name}`;
const profileKey = (profile: FirewallProfile) => `profile:${profile}`;

function loadError(reason: unknown, t: FirewallStrings): string {
  const message = (reason as { message?: unknown } | null)?.message;
  return typeof message === "string" && message.length <= 300 ? message : t.loadError;
}

function profileNames(mask: number, t: FirewallStrings): string {
  const names = (["domain", "private", "public"] as const).filter((_, bit) => (mask & (1 << bit)) !== 0).map((profile) => t.profile[profile]);
  return names.length === 3 || mask === 0x7fffffff ? t.all : names.join(", ") || t.none;
}

function matches(rule: FirewallRule, filter: string): boolean {
  if (!filter) return true;
  return [rule.name, rule.applicationName ?? "", rule.grouping ?? "", rule.localPorts, rule.remoteAddresses]
    .some((value) => value.toLowerCase().includes(filter));
}

/** Windows Firewall profiles, audit findings and rules, with explicit per-item changes. */
export function FirewallPage() {
  const sc = useSystemChangeLabels();
  const t = useStrings(firewallStrings);
  const fmt = useFormat();
  const describeChange = useCallback((change: SystemChange) => describe(change, t), [t]);
  const [status, setStatus] = useState<FirewallStatus | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  // Keyed by rule name / profile, so the same rule is never changed twice.
  const [selected, setSelected] = useState<Map<string, SystemChange>>(() => new Map());
  const [journalKey, setJournalKey] = useState(0);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      setStatus(await getFirewallStatus());
      setSelected(new Map());
      setError(null);
    } catch (reason) {
      setError(loadError(reason, t));
    } finally {
      setLoading(false);
    }
  }, [t]);
  useEffect(() => { void refresh(); }, [refresh]);

  const toggle = (key: string, change: SystemChange) => setSelected((current) => {
    const next = new Map(current);
    if (next.has(key)) next.delete(key); else next.set(key, change);
    return next;
  });
  const changes = useMemo(() => [...selected.values()], [selected]);
  const onFinished = useCallback(() => { setJournalKey((n) => n + 1); void refresh(); }, [refresh]);

  const ruleStates = useMemo(() => {
    const states = new Map<string, boolean>();
    for (const rule of status?.rules ?? []) states.set(rule.name, (states.get(rule.name) ?? false) || rule.enabled);
    return states;
  }, [status]);
  const needle = filter.trim().toLowerCase();
  const matching = useMemo(() => (status?.rules ?? []).filter((rule) => matches(rule, needle)), [status, needle]);
  const visible = matching.slice(0, MAX_VISIBLE_RULES);
  const profilesTurningOff = changes.flatMap((change) => change.kind === "setFirewallProfileEnabled" && !change.enabled ? [change.profile] : []);

  return <section aria-labelledby="firewall-title">
    <header className="page-header">
      <div>
        <p className="eyebrow">{t.eyebrow}</p>
        <h1 id="firewall-title">{t.title}</h1>
        <p>{t.intro}</p>
      </div>
      <button type="button" disabled={loading} onClick={() => void refresh()}>{t.refresh}</button>
    </header>
    {loading && <p role="status">{t.loading}</p>}
    {error && <p role="alert">{error}</p>}
    {status && <>
      <section aria-labelledby="firewall-profiles">
        <h2 id="firewall-profiles">{t.profilesTitle}</h2>
        <ul>{status.profiles.map((profile) => {
          const key = profileKey(profile.profile);
          const name = t.profile[profile.profile];
          const target = !profile.enabled;
          return <li key={profile.profile}>
            <strong>{name}</strong>: {profile.enabled ? t.on : t.off}{profile.active ? t.active : ""}{t.inboundDefault(t.action[profile.defaultInboundAction])}{t.outboundDefault(t.action[profile.defaultOutboundAction])}{profile.blockAllInboundTraffic ? t.blocksAllInbound : ""}
            <label>
              <input type="checkbox" checked={selected.has(key)} onChange={() => toggle(key, { kind: "setFirewallProfileEnabled", profile: profile.profile, enabled: target })} />
              {t.changeProfile(name, target)}
            </label>
          </li>;
        })}</ul>
        {profilesTurningOff.length > 0 && <p className="firewall-warning"><strong>{t.highRisk}</strong>{t.profileOffWarning(profilesTurningOff.map((profile) => t.profile[profile]).join(", "))}</p>}
      </section>

      <section aria-labelledby="firewall-findings">
        <h2 id="firewall-findings">{t.findingsTitle}</h2>
        {status.findings.length === 0 && <p>{t.noFindings}</p>}
        <ul>{status.findings.map((finding, index) => {
          const rule = finding.relatedRule;
          const ruleOn = rule !== null ? ruleStates.get(rule) : undefined;
          const reason = rule === null ? null : ruleOn === undefined ? t.ruleNotFound : !ruleOn ? t.ruleAlreadyOff : null;
          return <li key={`${finding.id}-${index}`}>
            <p><span>{sc.risk[finding.severity]}</span> · <strong>{finding.title}</strong></p>
            <p>{finding.detail}</p>
            {rule !== null && <>
              <label>
                <input type="checkbox" disabled={reason !== null} checked={reason === null && selected.has(ruleKey(rule))}
                  onChange={() => reason === null && toggle(ruleKey(rule), { kind: "setFirewallRuleEnabled", ruleName: rule, enabled: false })} />
                {t.turnOffRule(rule)}
              </label>
              {reason && <span> ({reason})</span>}
            </>}
          </li>;
        })}</ul>
      </section>

      <section aria-labelledby="firewall-rules">
        <h2 id="firewall-rules">{t.rulesTitle}</h2>
        <p>{t.rulesSummary(fmt.number(status.ruleCount), status.rulesTruncated ? fmt.number(status.rules.length) : null, fmt.number(visible.length), fmt.number(matching.length), fmt.number(MAX_VISIBLE_RULES))}</p>
        <label>{t.filterRules} <input type="search" value={filter} onChange={(event) => setFilter(event.target.value)} /></label>
        <table>
          <thead><tr><th scope="col">{t.columns.change}</th><th scope="col">{t.columns.name}</th><th scope="col">{t.columns.state}</th><th scope="col">{t.columns.direction}</th><th scope="col">{t.columns.action}</th><th scope="col">{t.columns.profiles}</th><th scope="col">{t.columns.program}</th><th scope="col">{t.columns.localPorts}</th><th scope="col">{t.columns.remoteAddresses}</th></tr></thead>
          <tbody>{visible.map((rule, index) => {
            const key = ruleKey(rule.name);
            const target = !(ruleStates.get(rule.name) ?? rule.enabled);
            return <tr key={`${rule.name}-${index}`}>
              <td><label>
                <input type="checkbox" checked={selected.has(key)} onChange={() => toggle(key, { kind: "setFirewallRuleEnabled", ruleName: rule.name, enabled: target })} />
                <span>{t.toggleRule(target, rule.name)}</span>
              </label></td>
              <td>{rule.name}</td>
              <td>{rule.enabled ? t.on : t.off}</td>
              <td>{t.direction[rule.direction]}</td>
              <td>{t.action[rule.action]}</td>
              <td>{profileNames(rule.profiles, t)}</td>
              <td>{rule.applicationName ?? t.any}</td>
              <td>{rule.localPorts || t.any}</td>
              <td>{rule.remoteAddresses || t.any}</td>
            </tr>;
          })}</tbody>
        </table>
      </section>
    </>}
    <SystemChangeReview changes={changes} describe={describeChange} onFinished={onFinished} />
    <SystemChangeJournal module="firewall" describe={describeChange} refreshKey={journalKey} onFinished={onFinished} />
  </section>;
}
