//! Windows Defender Firewall audit and typed rule/profile toggles.
//!
//! Reads go through [`FirewallPolicyReader`] (COM `INetFwPolicy2` on the real
//! system). Both mutations are helper-privileged; the elevated helper
//! re-resolves every rule name against the live rule enumeration through
//! [`elevated`].

mod windows_policy;

#[cfg(test)]
mod tests;

use cleanup_core::system_change::{
    EntryName, FirewallProfile, ImpactSummary, PriorState, RestartRequirement, RiskLevel,
    SystemChange, UnsupportedReason,
};
use serde::Serialize;

use crate::system_change::{AdapterError, SystemAdapter};

pub use windows_policy::WindowsFirewallPolicy;

/// Upper bound on enumerated rules.
pub const MAX_RULES: usize = 10_000;
/// Upper bound (in chars) on every string read from the policy.
pub const MAX_STRING_CHARS: usize = 512;
/// Upper bound on reported audit findings.
pub const MAX_FINDINGS: usize = 1_000;

pub const PROFILE_DOMAIN_BIT: i32 = 1;
pub const PROFILE_PRIVATE_BIT: i32 = 2;
pub const PROFILE_PUBLIC_BIT: i32 = 4;

pub const ALL_PROFILES: [FirewallProfile; 3] = [
    FirewallProfile::Domain,
    FirewallProfile::Private,
    FirewallProfile::Public,
];

pub fn profile_bit(profile: FirewallProfile) -> i32 {
    match profile {
        FirewallProfile::Domain => PROFILE_DOMAIN_BIT,
        FirewallProfile::Private => PROFILE_PRIVATE_BIT,
        FirewallProfile::Public => PROFILE_PUBLIC_BIT,
    }
}

fn profile_label(profile: FirewallProfile) -> &'static str {
    match profile {
        FirewallProfile::Domain => "Domain",
        FirewallProfile::Private => "Private",
        FirewallProfile::Public => "Public",
    }
}

fn profile_code(profile: FirewallProfile) -> &'static str {
    match profile {
        FirewallProfile::Domain => "domain",
        FirewallProfile::Private => "private",
        FirewallProfile::Public => "public",
    }
}

pub fn bounded(value: &str) -> String {
    value.chars().take(MAX_STRING_CHARS).collect()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FirewallAction {
    Allow,
    Block,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RuleDirection {
    Inbound,
    Outbound,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallProfileStatus {
    pub profile: FirewallProfile,
    pub enabled: bool,
    pub default_inbound_action: FirewallAction,
    pub default_outbound_action: FirewallAction,
    pub block_all_inbound_traffic: bool,
    /// Whether this profile is currently active on some network.
    pub active: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallRule {
    pub name: String,
    pub enabled: bool,
    pub direction: RuleDirection,
    pub action: FirewallAction,
    /// `NET_FW_PROFILE_TYPE2` bitmask (1 domain, 2 private, 4 public).
    pub profiles: i32,
    pub application_name: Option<String>,
    pub local_ports: String,
    pub remote_addresses: String,
    pub grouping: Option<String>,
}

/// Bounded rule enumeration.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RuleInventory {
    /// Total rules reported by the policy (may exceed `rules.len()`).
    pub total: usize,
    pub rules: Vec<FirewallRule>,
}

/// State of one rule whose name exactly matches a requested name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuleMatch {
    pub enabled: bool,
    pub action: FirewallAction,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallFinding {
    pub id: &'static str,
    pub severity: RiskLevel,
    pub title: String,
    pub detail: String,
    pub related_rule: Option<String>,
    pub related_profile: Option<FirewallProfile>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallStatus {
    pub profiles: Vec<FirewallProfileStatus>,
    pub current_profiles: Vec<FirewallProfile>,
    pub rule_count: usize,
    pub rules_truncated: bool,
    pub rules: Vec<FirewallRule>,
    pub findings: Vec<FirewallFinding>,
}

/// Read access to the firewall policy.
pub trait FirewallPolicyReader: Send + Sync {
    /// The three profiles plus the `CurrentProfileTypes` bitmask.
    fn profiles(&self) -> Result<(Vec<FirewallProfileStatus>, i32), AdapterError>;
    /// Up to [`MAX_RULES`] rules.
    fn rules(&self) -> Result<RuleInventory, AdapterError>;
    /// Every rule whose name exactly equals `name`. Fails with
    /// `AdapterError::Failed` when the enumeration bound is hit, because the
    /// result could then be incomplete.
    fn rule_matches(&self, name: &EntryName) -> Result<Vec<RuleMatch>, AdapterError>;
}

/// Write access to the firewall policy (elevated helper only).
pub trait FirewallPolicyWriter: Send + Sync {
    /// Sets `Enabled` on every rule named `name`; returns how many matched.
    fn set_rule_enabled(&self, name: &EntryName, enabled: bool) -> Result<usize, AdapterError>;
    fn set_profile_enabled(
        &self,
        profile: FirewallProfile,
        enabled: bool,
    ) -> Result<(), AdapterError>;
}

/// Read the full bounded status, including audit findings.
pub fn get_firewall_status() -> Result<FirewallStatus, AdapterError> {
    read_status(&WindowsFirewallPolicy)
}

pub fn read_status(reader: &dyn FirewallPolicyReader) -> Result<FirewallStatus, AdapterError> {
    let (profiles, current) = reader.profiles()?;
    let inventory = reader.rules()?;
    let findings = audit(&profiles, &inventory.rules);
    Ok(FirewallStatus {
        current_profiles: ALL_PROFILES
            .into_iter()
            .filter(|profile| current & profile_bit(*profile) != 0)
            .collect(),
        rule_count: inventory.total.max(inventory.rules.len()),
        rules_truncated: inventory.total > inventory.rules.len(),
        profiles,
        rules: inventory.rules,
        findings,
    })
}

fn is_any(value: &str) -> bool {
    let value = value.trim();
    value.is_empty() || value == "*" || value.eq_ignore_ascii_case("any")
}

/// True when `path` lies in a temp or Downloads folder an ordinary user can
/// write to.
pub fn is_user_writable_download_or_temp(path: &str) -> bool {
    let normalized = path.replace('/', "\\").to_ascii_lowercase();
    const MARKERS: [&str; 7] = [
        "\\appdata\\local\\temp\\",
        "\\windows\\temp\\",
        "\\downloads\\",
        "%temp%\\",
        "%tmp%\\",
        "%userprofile%\\downloads\\",
        "%localappdata%\\temp\\",
    ];
    MARKERS.iter().any(|marker| normalized.contains(marker))
}

/// Kudu-style audit of profiles and rules.
pub fn audit(profiles: &[FirewallProfileStatus], rules: &[FirewallRule]) -> Vec<FirewallFinding> {
    let mut findings = Vec::new();
    for profile in profiles {
        let label = profile_label(profile.profile);
        if !profile.enabled {
            findings.push(FirewallFinding {
                id: "profile-disabled",
                severity: RiskLevel::High,
                title: format!("{label} firewall profile is off"),
                detail: format!(
                    "Windows Defender Firewall is disabled for the {label} profile, so no rules filter traffic on {} networks.",
                    profile_code(profile.profile)
                ),
                related_rule: None,
                related_profile: Some(profile.profile),
            });
        }
        if profile.default_inbound_action == FirewallAction::Allow {
            findings.push(FirewallFinding {
                id: "default-inbound-allow",
                severity: RiskLevel::High,
                title: format!("{label} profile allows unsolicited inbound traffic"),
                detail: format!(
                    "The default inbound action for the {label} profile is Allow; inbound connections that match no rule are accepted."
                ),
                related_rule: None,
                related_profile: Some(profile.profile),
            });
        }
    }
    for rule in rules {
        if findings.len() >= MAX_FINDINGS {
            break;
        }
        if !(rule.enabled
            && rule.direction == RuleDirection::Inbound
            && rule.action == FirewallAction::Allow)
        {
            continue;
        }
        if rule.profiles & PROFILE_PUBLIC_BIT != 0
            && is_any(&rule.remote_addresses)
            && is_any(&rule.local_ports)
        {
            findings.push(FirewallFinding {
                id: "public-inbound-any",
                severity: RiskLevel::High,
                title: "Inbound rule open to any address on public networks".to_owned(),
                detail: bounded(&format!(
                    "The enabled Allow rule \"{}\" accepts inbound traffic from any remote address on any local port while on a Public network.",
                    rule.name
                )),
                related_rule: Some(rule.name.clone()),
                related_profile: Some(FirewallProfile::Public),
            });
        }
        if let Some(app) = &rule.application_name
            && is_user_writable_download_or_temp(app)
        {
            findings.push(FirewallFinding {
                id: "inbound-allow-user-writable-path",
                severity: RiskLevel::Medium,
                title: "Inbound rule allows a program in a temp or Downloads folder".to_owned(),
                detail: bounded(&format!(
                    "The enabled Allow rule \"{}\" lets {app} accept inbound connections, but that folder is writable by ordinary users, so the program can be replaced.",
                    rule.name
                )),
                related_rule: Some(rule.name.clone()),
                related_profile: None,
            });
        }
    }
    findings.truncate(MAX_FINDINGS);
    findings
}

/// The two firewall mutations, independent of whether they arrived as a
/// `SystemChange` or a `HelperChange`.
#[derive(Clone, Copy, Debug)]
enum Target<'a> {
    Rule(&'a EntryName, bool),
    Profile(FirewallProfile, bool),
}

impl<'a> Target<'a> {
    fn from_change(change: &'a SystemChange) -> Option<Self> {
        match change {
            SystemChange::SetFirewallRuleEnabled { rule_name, enabled } => {
                Some(Self::Rule(rule_name, *enabled))
            }
            SystemChange::SetFirewallProfileEnabled { profile, enabled } => {
                Some(Self::Profile(*profile, *enabled))
            }
            _ => None,
        }
    }
}

/// Observe the current state for a target.
///
/// Rule names are not unique in Windows; a change targets every rule with the
/// exact name. The contract's `PriorState::Enabled` holds a single bool, so a
/// mixed set (some enabled, some disabled) cannot be recorded or restored by
/// the inverse. Such a set fails closed as `Unsupported(NotPresent)`: there is
/// no single present state to act on.
fn observe_target(
    reader: &dyn FirewallPolicyReader,
    target: Target<'_>,
) -> Result<PriorState, AdapterError> {
    match target {
        Target::Rule(name, _) => {
            let matches = reader.rule_matches(name)?;
            let first = matches
                .first()
                .ok_or(AdapterError::Unsupported(UnsupportedReason::NotPresent))?;
            if matches.iter().all(|rule| rule.enabled == first.enabled) {
                Ok(PriorState::Enabled {
                    enabled: first.enabled,
                })
            } else {
                Err(AdapterError::Unsupported(UnsupportedReason::NotPresent))
            }
        }
        Target::Profile(profile, _) => {
            let (profiles, _) = reader.profiles()?;
            profiles
                .iter()
                .find(|status| status.profile == profile)
                .map(|status| PriorState::Enabled {
                    enabled: status.enabled,
                })
                .ok_or(AdapterError::Unsupported(UnsupportedReason::NotPresent))
        }
    }
}

fn apply_target(writer: &dyn FirewallPolicyWriter, target: Target<'_>) -> Result<(), AdapterError> {
    match target {
        Target::Rule(name, enabled) => match writer.set_rule_enabled(name, enabled)? {
            0 => Err(AdapterError::Unsupported(UnsupportedReason::NotPresent)),
            _ => Ok(()),
        },
        Target::Profile(profile, enabled) => writer.set_profile_enabled(profile, enabled),
    }
}

fn describe_target(target: Target<'_>) -> ImpactSummary {
    match target {
        Target::Rule(name, true) => ImpactSummary {
            component: format!("Firewall rule \"{}\"", name.as_str()),
            effect: "Enables every firewall rule with this exact name; the traffic those rules allow or block takes effect again.".to_owned(),
            restart: RestartRequirement::None,
            risk: RiskLevel::Low,
        },
        Target::Rule(name, false) => ImpactSummary {
            component: format!("Firewall rule \"{}\"", name.as_str()),
            effect: "Disables every firewall rule with this exact name. If a rule allows traffic, the app that relies on it may lose network access; if it blocks traffic, that traffic is no longer blocked.".to_owned(),
            restart: RestartRequirement::None,
            risk: RiskLevel::Medium,
        },
        Target::Profile(profile, true) => ImpactSummary {
            component: format!("{} firewall profile", profile_label(profile)),
            effect: format!(
                "Turns Windows Defender Firewall on for {} networks; traffic not allowed by a rule may be blocked.",
                profile_code(profile)
            ),
            restart: RestartRequirement::None,
            risk: RiskLevel::Low,
        },
        Target::Profile(profile, false) => ImpactSummary {
            component: format!("{} firewall profile", profile_label(profile)),
            effect: format!(
                "Turns Windows Defender Firewall off for {} networks; all firewall filtering stops on those networks.",
                profile_code(profile)
            ),
            restart: RestartRequirement::None,
            risk: RiskLevel::High,
        },
    }
}

/// `SystemAdapter` for `SetFirewallRuleEnabled` and
/// `SetFirewallProfileEnabled`. Both are helper-privileged, so `apply` keeps
/// the default `Failed`.
pub struct FirewallAdapter<R: FirewallPolicyReader = WindowsFirewallPolicy> {
    reader: R,
}

impl FirewallAdapter<WindowsFirewallPolicy> {
    pub fn new() -> Self {
        Self {
            reader: WindowsFirewallPolicy,
        }
    }
}

impl Default for FirewallAdapter<WindowsFirewallPolicy> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R: FirewallPolicyReader> FirewallAdapter<R> {
    pub fn with_reader(reader: R) -> Self {
        Self { reader }
    }
}

impl<R: FirewallPolicyReader> SystemAdapter for FirewallAdapter<R> {
    fn describe(&self, change: &SystemChange) -> Result<ImpactSummary, AdapterError> {
        Target::from_change(change)
            .map(describe_target)
            .ok_or(AdapterError::Failed)
    }

    fn observe(&self, change: &SystemChange) -> Result<PriorState, AdapterError> {
        let target = Target::from_change(change).ok_or(AdapterError::Failed)?;
        observe_target(&self.reader, target)
    }
}

/// Entry points for the elevated helper.
pub mod elevated {
    use cleanup_core::system_change::PriorState;

    use super::{
        FirewallPolicyReader, FirewallPolicyWriter, Target, WindowsFirewallPolicy, apply_target,
        observe_target,
    };
    use crate::{security::system_changes::HelperChange, system_change::AdapterError};

    fn target(change: &HelperChange) -> Option<Target<'_>> {
        match change {
            HelperChange::SetFirewallRuleEnabled { rule_name, enabled } => {
                Some(Target::Rule(rule_name, *enabled))
            }
            HelperChange::SetFirewallProfileEnabled { profile, enabled } => {
                Some(Target::Profile(*profile, *enabled))
            }
            _ => None,
        }
    }

    pub fn observe(change: &HelperChange) -> Result<PriorState, AdapterError> {
        observe_with(&WindowsFirewallPolicy, change)
    }

    pub fn apply(change: &HelperChange) -> Result<(), AdapterError> {
        apply_with(&WindowsFirewallPolicy, change)
    }

    pub(super) fn observe_with(
        reader: &dyn FirewallPolicyReader,
        change: &HelperChange,
    ) -> Result<PriorState, AdapterError> {
        observe_target(reader, target(change).ok_or(AdapterError::Failed)?)
    }

    pub(super) fn apply_with(
        writer: &dyn FirewallPolicyWriter,
        change: &HelperChange,
    ) -> Result<(), AdapterError> {
        apply_target(writer, target(change).ok_or(AdapterError::Failed)?)
    }
}
