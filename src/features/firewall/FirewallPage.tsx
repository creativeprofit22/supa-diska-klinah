import { useCallback, useEffect, useMemo, useState } from "react";
import { SystemChangeJournal } from "../../shared/system-change/SystemChangeJournal";
import { SystemChangeReview } from "../../shared/system-change/SystemChangeReview";
import { riskLabel } from "../../shared/system-change/labels";
import type { SystemChange } from "../../shared/system-change/types";
import { getFirewallStatus } from "./api";
import type { FirewallAction, FirewallProfile, FirewallRule, FirewallStatus } from "./types";

export const MAX_VISIBLE_RULES = 200;

const profileLabel: Record<FirewallProfile, string> = { domain: "Domain", private: "Private", public: "Public" };
const actionLabel: Record<FirewallAction, string> = { allow: "Allow", block: "Block", unknown: "Unknown" };

export function describe(change: SystemChange): string {
  switch (change.kind) {
    case "setFirewallRuleEnabled": return `${change.enabled ? "Turn on" : "Turn off"} firewall rule: ${change.ruleName}`;
    case "setFirewallProfileEnabled": return `${change.enabled ? "Turn on" : "Turn off"} ${profileLabel[change.profile]} firewall profile`;
    default: return change.kind;
  }
}

const ruleKey = (name: string) => `rule:${name}`;
const profileKey = (profile: FirewallProfile) => `profile:${profile}`;

function loadError(reason: unknown): string {
  const message = (reason as { message?: unknown } | null)?.message;
  return typeof message === "string" && message.length <= 300 ? message : "The firewall status could not be read.";
}

function profileNames(mask: number): string {
  const names = (["domain", "private", "public"] as const).filter((_, bit) => (mask & (1 << bit)) !== 0).map((profile) => profileLabel[profile]);
  return names.length === 3 || mask === 0x7fffffff ? "All" : names.join(", ") || "None";
}

function matches(rule: FirewallRule, filter: string): boolean {
  if (!filter) return true;
  return [rule.name, rule.applicationName ?? "", rule.grouping ?? "", rule.localPorts, rule.remoteAddresses]
    .some((value) => value.toLowerCase().includes(filter));
}

/** Windows Firewall profiles, audit findings and rules, with explicit per-item changes. */
export function FirewallPage() {
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
      setError(loadError(reason));
    } finally {
      setLoading(false);
    }
  }, []);
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
        <p className="eyebrow">System</p>
        <h1 id="firewall-title">Firewall</h1>
        <p>Shows Windows Firewall profiles, notable rules and audit findings; nothing changes until you review and confirm in Windows.</p>
      </div>
      <button type="button" disabled={loading} onClick={() => void refresh()}>Refresh</button>
    </header>
    {loading && <p role="status">Reading firewall status…</p>}
    {error && <p role="alert">{error}</p>}
    {status && <>
      <section aria-labelledby="firewall-profiles">
        <h2 id="firewall-profiles">Profiles</h2>
        <ul>{status.profiles.map((profile) => {
          const key = profileKey(profile.profile);
          const name = profileLabel[profile.profile];
          const target = !profile.enabled;
          return <li key={profile.profile}>
            <strong>{name}</strong>: {profile.enabled ? "On" : "Off"}{profile.active ? " (active)" : ""} · Inbound default: {actionLabel[profile.defaultInboundAction]} · Outbound default: {actionLabel[profile.defaultOutboundAction]}{profile.blockAllInboundTraffic ? " · Blocks all inbound" : ""}
            <label>
              <input type="checkbox" checked={selected.has(key)} onChange={() => toggle(key, { kind: "setFirewallProfileEnabled", profile: profile.profile, enabled: target })} />
              {`Change ${name} to ${target ? "on" : "off"}`}
            </label>
          </li>;
        })}</ul>
        {profilesTurningOff.length > 0 && <p className="firewall-warning"><strong>High risk:</strong> turning off the {profilesTurningOff.map((profile) => profileLabel[profile]).join(", ")} firewall profile leaves this PC open to unsolicited network traffic on those networks.</p>}
      </section>

      <section aria-labelledby="firewall-findings">
        <h2 id="firewall-findings">Findings</h2>
        {status.findings.length === 0 && <p>No findings.</p>}
        <ul>{status.findings.map((finding, index) => {
          const rule = finding.relatedRule;
          const ruleOn = rule !== null ? ruleStates.get(rule) : undefined;
          const reason = rule === null ? null : ruleOn === undefined ? "Rule not found in the listed rules" : !ruleOn ? "Rule is already off" : null;
          return <li key={`${finding.id}-${index}`}>
            <p><span>{riskLabel[finding.severity]}</span> · <strong>{finding.title}</strong></p>
            <p>{finding.detail}</p>
            {rule !== null && <>
              <label>
                <input type="checkbox" disabled={reason !== null} checked={reason === null && selected.has(ruleKey(rule))}
                  onChange={() => reason === null && toggle(ruleKey(rule), { kind: "setFirewallRuleEnabled", ruleName: rule, enabled: false })} />
                {`Turn off rule ${rule}`}
              </label>
              {reason && <span> ({reason})</span>}
            </>}
          </li>;
        })}</ul>
      </section>

      <section aria-labelledby="firewall-rules">
        <h2 id="firewall-rules">Rules</h2>
        <p>{status.ruleCount} rules{status.rulesTruncated ? `; only the first ${status.rules.length} were read` : ""}. Showing {visible.length} of {matching.length} matching (at most {MAX_VISIBLE_RULES}).</p>
        <label>Filter rules <input type="search" value={filter} onChange={(event) => setFilter(event.target.value)} /></label>
        <table>
          <thead><tr><th scope="col">Change</th><th scope="col">Name</th><th scope="col">State</th><th scope="col">Direction</th><th scope="col">Action</th><th scope="col">Profiles</th><th scope="col">Program</th><th scope="col">Local ports</th><th scope="col">Remote addresses</th></tr></thead>
          <tbody>{visible.map((rule, index) => {
            const key = ruleKey(rule.name);
            const target = !(ruleStates.get(rule.name) ?? rule.enabled);
            return <tr key={`${rule.name}-${index}`}>
              <td><label>
                <input type="checkbox" checked={selected.has(key)} onChange={() => toggle(key, { kind: "setFirewallRuleEnabled", ruleName: rule.name, enabled: target })} />
                <span>{`Turn ${target ? "on" : "off"} rule ${rule.name}`}</span>
              </label></td>
              <td>{rule.name}</td>
              <td>{rule.enabled ? "On" : "Off"}</td>
              <td>{rule.direction}</td>
              <td>{actionLabel[rule.action]}</td>
              <td>{profileNames(rule.profiles)}</td>
              <td>{rule.applicationName ?? "Any"}</td>
              <td>{rule.localPorts || "Any"}</td>
              <td>{rule.remoteAddresses || "Any"}</td>
            </tr>;
          })}</tbody>
        </table>
      </section>
    </>}
    <SystemChangeReview changes={changes} describe={describe} onFinished={onFinished} />
    <SystemChangeJournal module="firewall" describe={describe} refreshKey={journalKey} onFinished={onFinished} />
  </section>;
}
