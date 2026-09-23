use std::sync::Mutex;

use cleanup_core::system_change::{
    EntryName, FirewallProfile, PriorState, RiskLevel, SystemChange, UnsupportedReason,
};
use windows::{
    Win32::Foundation::{E_ACCESSDENIED, E_FAIL, REGDB_E_CLASSNOTREG},
    core::HRESULT,
};

use super::{
    FirewallAction, FirewallAdapter, FirewallPolicyReader, FirewallPolicyWriter,
    FirewallProfileStatus, FirewallRule, MAX_STRING_CHARS, PROFILE_DOMAIN_BIT, PROFILE_PUBLIC_BIT,
    RuleDirection, RuleInventory, RuleMatch, audit, bounded, elevated, read_status,
    windows_policy::map_hresult,
};
use crate::{
    security::system_changes::HelperChange,
    system_change::{AdapterError, SystemAdapter},
};

const API_UNAVAILABLE: AdapterError = AdapterError::Unsupported(UnsupportedReason::ApiUnavailable);
const NOT_PRESENT: AdapterError = AdapterError::Unsupported(UnsupportedReason::NotPresent);

fn profile(profile: FirewallProfile, enabled: bool) -> FirewallProfileStatus {
    FirewallProfileStatus {
        profile,
        enabled,
        default_inbound_action: FirewallAction::Block,
        default_outbound_action: FirewallAction::Allow,
        block_all_inbound_traffic: false,
        active: profile == FirewallProfile::Private,
    }
}

fn rule(name: &str, enabled: bool) -> FirewallRule {
    FirewallRule {
        name: name.to_owned(),
        enabled,
        direction: RuleDirection::Inbound,
        action: FirewallAction::Allow,
        profiles: PROFILE_DOMAIN_BIT,
        application_name: None,
        local_ports: "443".to_owned(),
        remote_addresses: "LocalSubnet".to_owned(),
        grouping: None,
    }
}

fn name(value: &str) -> EntryName {
    EntryName::parse(value).expect("valid rule name")
}

#[derive(Default)]
struct FakePolicy {
    profiles: Mutex<Vec<FirewallProfileStatus>>,
    rules: Mutex<Vec<FirewallRule>>,
    error: Option<AdapterError>,
}

impl FakePolicy {
    fn new(rules: Vec<FirewallRule>) -> Self {
        Self {
            profiles: Mutex::new(vec![
                profile(FirewallProfile::Domain, true),
                profile(FirewallProfile::Private, true),
                profile(FirewallProfile::Public, true),
            ]),
            rules: Mutex::new(rules),
            error: None,
        }
    }

    fn failing(error: AdapterError) -> Self {
        Self {
            error: Some(error),
            ..Self::new(Vec::new())
        }
    }

    fn check(&self) -> Result<(), AdapterError> {
        self.error.map_or(Ok(()), Err)
    }
}

impl FirewallPolicyReader for FakePolicy {
    fn profiles(&self) -> Result<(Vec<FirewallProfileStatus>, i32), AdapterError> {
        self.check()?;
        Ok((self.profiles.lock().expect("lock").clone(), 2))
    }

    fn rules(&self) -> Result<RuleInventory, AdapterError> {
        self.check()?;
        let rules = self.rules.lock().expect("lock").clone();
        Ok(RuleInventory {
            total: rules.len(),
            rules,
        })
    }

    fn rule_matches(&self, name: &EntryName) -> Result<Vec<RuleMatch>, AdapterError> {
        self.check()?;
        Ok(self
            .rules
            .lock()
            .expect("lock")
            .iter()
            .filter(|rule| rule.name == name.as_str())
            .map(|rule| RuleMatch {
                enabled: rule.enabled,
                action: rule.action,
            })
            .collect())
    }
}

impl FirewallPolicyWriter for FakePolicy {
    fn set_rule_enabled(&self, name: &EntryName, enabled: bool) -> Result<usize, AdapterError> {
        self.check()?;
        let mut count = 0;
        for rule in self.rules.lock().expect("lock").iter_mut() {
            if rule.name == name.as_str() {
                rule.enabled = enabled;
                count += 1;
            }
        }
        Ok(count)
    }

    fn set_profile_enabled(
        &self,
        profile: FirewallProfile,
        enabled: bool,
    ) -> Result<(), AdapterError> {
        self.check()?;
        for status in self.profiles.lock().expect("lock").iter_mut() {
            if status.profile == profile {
                status.enabled = enabled;
            }
        }
        Ok(())
    }
}

fn rule_change(rule_name: &str, enabled: bool) -> SystemChange {
    SystemChange::SetFirewallRuleEnabled {
        rule_name: name(rule_name),
        enabled,
    }
}

#[test]
fn audit_flags_disabled_profile_and_default_inbound_allow() {
    let mut public = profile(FirewallProfile::Public, false);
    public.default_inbound_action = FirewallAction::Allow;
    let findings = audit(&[profile(FirewallProfile::Domain, true), public], &[]);
    let ids: Vec<_> = findings.iter().map(|finding| finding.id).collect();
    assert_eq!(ids, ["profile-disabled", "default-inbound-allow"]);
    assert!(findings.iter().all(|finding| {
        finding.severity == RiskLevel::High
            && finding.related_profile == Some(FirewallProfile::Public)
    }));
}

#[test]
fn audit_flags_public_inbound_rule_open_to_everything() {
    let mut open = rule("Open", true);
    open.profiles = PROFILE_PUBLIC_BIT;
    open.remote_addresses = "*".to_owned();
    open.local_ports = "*".to_owned();
    let mut all_profiles = open.clone();
    all_profiles.name = "All".to_owned();
    all_profiles.profiles = 0x7fff_ffff;
    all_profiles.local_ports = String::new();

    let mut disabled = open.clone();
    disabled.enabled = false;
    let mut outbound = open.clone();
    outbound.direction = RuleDirection::Outbound;
    let mut block = open.clone();
    block.action = FirewallAction::Block;
    let mut private_only = open.clone();
    private_only.profiles = 2;
    let mut scoped = open.clone();
    scoped.remote_addresses = "LocalSubnet".to_owned();
    let mut one_port = open.clone();
    one_port.local_ports = "3389".to_owned();

    let findings = audit(
        &[],
        &[
            open,
            all_profiles,
            disabled,
            outbound,
            block,
            private_only,
            scoped,
            one_port,
        ],
    );
    let related: Vec<_> = findings
        .iter()
        .map(|finding| (finding.id, finding.related_rule.as_deref()))
        .collect();
    assert_eq!(
        related,
        [
            ("public-inbound-any", Some("Open")),
            ("public-inbound-any", Some("All"))
        ]
    );
}

#[test]
fn audit_flags_allow_rules_for_programs_in_temp_or_downloads() {
    let paths = [
        (r"C:\Users\ann\AppData\Local\Temp\x\setup.exe", true),
        (r"C:\Users\ann\Downloads\tool.exe", true),
        (r"%USERPROFILE%\Downloads\tool.exe", true),
        (r"%TEMP%\a.exe", true),
        (r"C:\Windows\Temp\a.exe", true),
        (r"C:\Program Files\App\app.exe", false),
        (r"C:\Windows\System32\svchost.exe", false),
    ];
    for (path, expected) in paths {
        let mut entry = rule("App", true);
        entry.application_name = Some(path.to_owned());
        let findings = audit(&[], &[entry.clone()]);
        assert_eq!(
            findings
                .iter()
                .any(|finding| finding.id == "inbound-allow-user-writable-path"),
            expected,
            "{path}"
        );
        entry.enabled = false;
        assert!(audit(&[], &[entry]).is_empty(), "{path}");
    }
}

#[test]
fn status_reports_current_profiles_rules_and_findings() {
    let mut risky = rule("Risky", true);
    risky.application_name = Some(r"C:\Users\a\Downloads\x.exe".to_owned());
    let policy = FakePolicy::new(vec![rule("Ok", true), risky]);
    let status = read_status(&policy).expect("status");
    assert_eq!(status.current_profiles, [FirewallProfile::Private]);
    assert_eq!(status.rule_count, 2);
    assert!(!status.rules_truncated);
    assert_eq!(status.findings.len(), 1);
    let json = serde_json::to_value(&status).expect("json");
    assert!(json.get("currentProfiles").is_some());
    assert!(json["rules"][0].get("remoteAddresses").is_some());
    assert!(json["findings"][0].get("relatedRule").is_some());
}

#[test]
fn strings_are_bounded() {
    let long = "x".repeat(MAX_STRING_CHARS * 2);
    assert_eq!(bounded(&long).chars().count(), MAX_STRING_CHARS);
    let mut entry = rule(&"n".repeat(MAX_STRING_CHARS), true);
    entry.application_name = Some(format!(r"C:\Users\a\Downloads\{long}"));
    for finding in audit(&[], &[entry]) {
        assert!(finding.detail.chars().count() <= MAX_STRING_CHARS);
    }
}

#[test]
fn observe_same_name_rules_that_agree() {
    let adapter = FirewallAdapter::with_reader(FakePolicy::new(vec![
        rule("Shared", false),
        rule("Shared", false),
        rule("Other", true),
    ]));
    assert_eq!(
        adapter.observe(&rule_change("Shared", true)),
        Ok(PriorState::Enabled { enabled: false })
    );
}

#[test]
fn observe_same_name_rules_that_disagree_fails_closed() {
    let adapter = FirewallAdapter::with_reader(FakePolicy::new(vec![
        rule("Shared", false),
        rule("Shared", true),
    ]));
    assert_eq!(
        adapter.observe(&rule_change("Shared", true)),
        Err(NOT_PRESENT)
    );
}

#[test]
fn observe_absent_rule_fails_closed() {
    let adapter = FirewallAdapter::with_reader(FakePolicy::new(vec![rule("Other", true)]));
    assert_eq!(
        adapter.observe(&rule_change("Missing", true)),
        Err(NOT_PRESENT)
    );
    let policy = FakePolicy::new(vec![rule("Other", true)]);
    let change = HelperChange::SetFirewallRuleEnabled {
        rule_name: name("Missing"),
        enabled: false,
    };
    assert_eq!(elevated::apply_with(&policy, &change), Err(NOT_PRESENT));
}

#[test]
fn observe_profile_state() {
    let policy = FakePolicy::new(Vec::new());
    policy.profiles.lock().expect("lock")[2].enabled = false;
    let adapter = FirewallAdapter::with_reader(policy);
    let change = SystemChange::SetFirewallProfileEnabled {
        profile: FirewallProfile::Public,
        enabled: true,
    };
    assert_eq!(
        adapter.observe(&change),
        Ok(PriorState::Enabled { enabled: false })
    );
}

#[test]
fn unavailable_api_and_access_denied_propagate() {
    for error in [API_UNAVAILABLE, AdapterError::Denied] {
        let policy = FakePolicy::failing(error);
        assert_eq!(read_status(&policy), Err(error));
        let change = HelperChange::SetFirewallProfileEnabled {
            profile: FirewallProfile::Domain,
            enabled: false,
        };
        assert_eq!(elevated::observe_with(&policy, &change), Err(error));
        assert_eq!(elevated::apply_with(&policy, &change), Err(error));
        let adapter = FirewallAdapter::with_reader(policy);
        assert_eq!(adapter.observe(&rule_change("Any", true)), Err(error));
    }
}

#[test]
fn hresults_map_to_adapter_errors() {
    assert_eq!(map_hresult(E_ACCESSDENIED), AdapterError::Denied);
    assert_eq!(map_hresult(REGDB_E_CLASSNOTREG), API_UNAVAILABLE);
    assert_eq!(
        map_hresult(HRESULT(0x8007_06D9_u32 as i32)),
        API_UNAVAILABLE
    );
    assert_eq!(
        map_hresult(HRESULT(0x8007_06BA_u32 as i32)),
        API_UNAVAILABLE
    );
    assert_eq!(map_hresult(E_FAIL), AdapterError::Failed);
}

#[test]
fn apply_changes_every_same_name_rule_and_is_idempotent() {
    let policy = FakePolicy::new(vec![
        rule("Shared", true),
        rule("Shared", true),
        rule("Other", true),
    ]);
    let helper = HelperChange::SetFirewallRuleEnabled {
        rule_name: name("Shared"),
        enabled: false,
    };
    let change = rule_change("Shared", false);

    let before = elevated::observe_with(&policy, &helper).expect("observe");
    assert_eq!(change.is_satisfied_by(&before), Some(false));
    elevated::apply_with(&policy, &helper).expect("apply");
    let after = elevated::observe_with(&policy, &helper).expect("observe");
    assert_eq!(after, PriorState::Enabled { enabled: false });
    assert_eq!(change.is_satisfied_by(&after), Some(true));
    // Applying again is harmless and leaves the state unchanged.
    elevated::apply_with(&policy, &helper).expect("reapply");
    assert_eq!(elevated::observe_with(&policy, &helper), Ok(after));
    assert!(
        policy
            .rules
            .lock()
            .expect("lock")
            .iter()
            .any(|rule| rule.name == "Other" && rule.enabled)
    );
}

#[test]
fn inverse_restores_prior_rule_and_profile_state() {
    let policy = FakePolicy::new(vec![rule("Shared", true), rule("Shared", true)]);
    let change = rule_change("Shared", false);
    let helper = HelperChange::SetFirewallRuleEnabled {
        rule_name: name("Shared"),
        enabled: false,
    };
    let prior = elevated::observe_with(&policy, &helper).expect("observe");
    elevated::apply_with(&policy, &helper).expect("apply");
    let inverse = change
        .inverse(&prior)
        .expect("inverse")
        .expect("reversible");
    assert_eq!(inverse, rule_change("Shared", true));
    let inverse_helper = HelperChange::SetFirewallRuleEnabled {
        rule_name: name("Shared"),
        enabled: true,
    };
    elevated::apply_with(&policy, &inverse_helper).expect("undo");
    assert_eq!(elevated::observe_with(&policy, &helper), Ok(prior));

    let profile_change = SystemChange::SetFirewallProfileEnabled {
        profile: FirewallProfile::Public,
        enabled: false,
    };
    assert_eq!(
        profile_change.inverse(&PriorState::Enabled { enabled: true }),
        Ok(Some(SystemChange::SetFirewallProfileEnabled {
            profile: FirewallProfile::Public,
            enabled: true,
        }))
    );
}

#[test]
fn describe_rates_risk_truthfully() {
    let adapter = FirewallAdapter::with_reader(FakePolicy::new(Vec::new()));
    let risk = |change: SystemChange| adapter.describe(&change).expect("describe").risk;
    assert_eq!(risk(rule_change("A", true)), RiskLevel::Low);
    assert_eq!(risk(rule_change("A", false)), RiskLevel::Medium);
    assert_eq!(
        risk(SystemChange::SetFirewallProfileEnabled {
            profile: FirewallProfile::Private,
            enabled: false,
        }),
        RiskLevel::High
    );
    assert_eq!(
        risk(SystemChange::SetFirewallProfileEnabled {
            profile: FirewallProfile::Private,
            enabled: true,
        }),
        RiskLevel::Low
    );
    assert_eq!(
        adapter.describe(&SystemChange::SetHibernation { enabled: true }),
        Err(AdapterError::Failed)
    );
}

#[test]
fn elevated_rejects_other_variants() {
    let policy = FakePolicy::new(Vec::new());
    let other = HelperChange::SetHibernation { enabled: false };
    assert_eq!(
        elevated::observe_with(&policy, &other),
        Err(AdapterError::Failed)
    );
    assert_eq!(
        elevated::apply_with(&policy, &other),
        Err(AdapterError::Failed)
    );
}

#[test]
fn live_status_is_read_only_and_tolerates_stopped_service() {
    match super::get_firewall_status() {
        Ok(status) => {
            assert_eq!(status.profiles.len(), 3);
            assert!(status.rules.len() <= super::MAX_RULES);
            assert!(status.rules.iter().all(|rule| {
                rule.name.chars().count() <= MAX_STRING_CHARS
                    && rule.local_ports.chars().count() <= MAX_STRING_CHARS
                    && rule.remote_addresses.chars().count() <= MAX_STRING_CHARS
            }));
        }
        Err(error) => assert_eq!(error, API_UNAVAILABLE),
    }
}
