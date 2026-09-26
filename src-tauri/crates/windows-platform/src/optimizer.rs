//! Quick optimization as a list of individually reviewable proposals.
//!
//! The optimizer owns no mutation of its own. It composes candidate
//! [`SystemChange`] values from the other modules' compiled catalogs, previews
//! each one through the shared service (so impact, privilege, reversibility and
//! prior state come from the owning adapter), and drops candidates that are
//! already satisfied or unsupported on this device. The user selects proposals
//! one by one; selected changes then go through the normal plan → native
//! confirmation → journal flow. There is deliberately no "apply all" entry
//! point.
//!
//! Not offered: Nagle/TCP tweaks (per-interface registry keys cannot be a
//! compiled catalog), debloat/app removal, and boot tracing.

use cleanup_core::system_change::{
    CatalogId, PlannedChange, PowerSchemeId, PriorState, RiskLevel, SystemChange,
};
use serde::Serialize;

use crate::{
    power::HIGH_PERFORMANCE_SCHEME,
    privacy::{self, SettingCategory, SettingHive},
    services::{self, ServiceCategory},
    system_change::{SystemChangeError, SystemChangeService},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProposalGroup {
    Services,
    Privacy,
    Performance,
    Power,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Proposal {
    pub group: ProposalGroup,
    pub label: String,
    /// Pre-selected in the UI only for low-risk, reversible proposals.
    pub suggested: bool,
    pub planned: PlannedChange,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OptimizerReport {
    pub proposals: Vec<Proposal>,
    /// Candidates skipped because they are already applied.
    pub already_applied: usize,
    /// Candidates skipped because this device does not support them.
    pub unavailable: usize,
}

/// One candidate before preview.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub group: ProposalGroup,
    pub label: &'static str,
    pub change: SystemChange,
}

fn catalog_id(id: &str) -> Option<CatalogId> {
    CatalogId::parse(id).ok()
}

/// Every candidate change, each a single typed `SystemChange`.
pub fn candidates() -> Vec<Candidate> {
    let mut out = Vec::new();
    for entry in services::catalog() {
        if matches!(
            entry.category,
            ServiceCategory::Telemetry | ServiceCategory::Gaming | ServiceCategory::Legacy
        ) && entry.risk != RiskLevel::High
            && let Some(catalog_id) = catalog_id(entry.id)
        {
            out.push(Candidate {
                group: ProposalGroup::Services,
                label: entry.label,
                change: SystemChange::SetServiceStartType {
                    catalog_id,
                    start_type: entry.recommended,
                },
            });
        }
    }
    for entry in privacy::settings_catalog() {
        let (Some(setting_id), Some(value)) = (catalog_id(entry.id), entry.recommended) else {
            continue;
        };
        let group = match entry.category {
            SettingCategory::Privacy => ProposalGroup::Privacy,
            SettingCategory::Performance => ProposalGroup::Performance,
        };
        let change = match entry.hive {
            SettingHive::User => SystemChange::SetUserSetting {
                setting_id,
                value: Some(value),
            },
            SettingHive::Machine => SystemChange::SetMachineSetting {
                setting_id,
                value: Some(value),
            },
        };
        out.push(Candidate {
            group,
            label: entry.label,
            change,
        });
    }
    for entry in privacy::task_catalog() {
        if let Some(catalog_id) = catalog_id(entry.id) {
            out.push(Candidate {
                group: ProposalGroup::Privacy,
                label: entry.label,
                change: SystemChange::SetSystemTaskEnabled {
                    catalog_id,
                    enabled: entry.recommended_enabled,
                },
            });
        }
    }
    if let Ok(scheme) = PowerSchemeId::parse(HIGH_PERFORMANCE_SCHEME) {
        out.push(Candidate {
            group: ProposalGroup::Power,
            label: "High performance power plan",
            change: SystemChange::SetActivePowerScheme { scheme },
        });
    }
    out
}

/// Preview every candidate and keep the ones that would change something.
pub fn build_report(
    candidates: Vec<Candidate>,
    preview: impl Fn(SystemChange) -> Result<PlannedChange, SystemChangeError>,
) -> OptimizerReport {
    let mut report = OptimizerReport {
        proposals: Vec::new(),
        already_applied: 0,
        unavailable: 0,
    };
    for candidate in candidates {
        let Ok(planned) = preview(candidate.change.clone()) else {
            report.unavailable += 1;
            continue;
        };
        if satisfied(&planned.change, &planned.expected_prior) {
            report.already_applied += 1;
            continue;
        }
        let suggested = planned.impact.risk == RiskLevel::Low && planned.inverse.is_some();
        report.proposals.push(Proposal {
            group: candidate.group,
            label: candidate.label.to_owned(),
            suggested,
            planned,
        });
    }
    report
}

fn satisfied(change: &SystemChange, prior: &PriorState) -> bool {
    change.is_satisfied_by(prior) == Some(true)
}

pub fn optimizer_report(service: &SystemChangeService) -> OptimizerReport {
    build_report(candidates(), |change| service.preview(change))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use cleanup_core::system_change::{ImpactSummary, Privilege, RestartRequirement};

    use super::*;

    fn fake_preview(change: SystemChange) -> Result<PlannedChange, SystemChangeError> {
        let prior = match &change {
            SystemChange::SetServiceStartType { catalog_id, .. }
                if catalog_id.as_str() == "fax" =>
            {
                return Err(SystemChangeError::Unsupported);
            }
            SystemChange::SetUserSetting { setting_id, value }
                if setting_id.as_str() == "advertising-id" =>
            {
                PriorState::RegistryValue { value: *value }
            }
            SystemChange::SetServiceStartType { .. } => PriorState::ServiceStart {
                start: cleanup_core::system_change::ServiceStartState::Automatic,
            },
            SystemChange::SetUserSetting { .. } | SystemChange::SetMachineSetting { .. } => {
                PriorState::RegistryValue { value: None }
            }
            SystemChange::SetSystemTaskEnabled { .. } => PriorState::Enabled { enabled: true },
            SystemChange::SetActivePowerScheme { .. } => PriorState::PowerScheme {
                scheme: PowerSchemeId::parse("381b4222-f694-41f0-9685-ff5bb260df2e").unwrap(),
            },
            _ => PriorState::NotApplicable,
        };
        let impact = ImpactSummary {
            component: "Test".into(),
            effect: "Test".into(),
            restart: RestartRequirement::None,
            risk: RiskLevel::Low,
        };
        Ok(PlannedChange::new(change, impact, prior).unwrap())
    }

    #[test]
    fn every_candidate_is_one_valid_typed_change() {
        let candidates = candidates();
        assert!(candidates.len() > 10);
        let mut seen = HashSet::new();
        for candidate in &candidates {
            candidate.change.validate().unwrap();
            assert!(
                seen.insert(format!("{:?}", candidate.change)),
                "duplicate {:?}",
                candidate.change
            );
        }
        for group in [
            ProposalGroup::Services,
            ProposalGroup::Privacy,
            ProposalGroup::Performance,
            ProposalGroup::Power,
        ] {
            assert!(candidates.iter().any(|candidate| candidate.group == group));
        }
    }

    #[test]
    fn report_skips_applied_and_unavailable_and_keeps_changes_separate() {
        let total = candidates().len();
        let report = build_report(candidates(), fake_preview);
        assert_eq!(report.already_applied, 1);
        assert_eq!(report.unavailable, 1);
        assert_eq!(report.proposals.len() + 2, total);
        // Each proposal carries exactly one change with its own prior and inverse.
        for proposal in &report.proposals {
            assert!(proposal.planned.inverse.is_some());
            assert_eq!(
                proposal.planned.change.privilege() == Privilege::Helper,
                proposal.planned.privilege == Privilege::Helper
            );
        }
    }

    #[test]
    fn high_risk_services_are_never_candidates() {
        let high: HashSet<_> = services::catalog()
            .iter()
            .filter(|entry| entry.risk == RiskLevel::High)
            .map(|entry| entry.id)
            .collect();
        for candidate in candidates() {
            if let SystemChange::SetServiceStartType { catalog_id, .. } = &candidate.change {
                assert!(!high.contains(catalog_id.as_str()));
            }
        }
    }

    #[test]
    fn no_irreversible_or_destructive_candidates() {
        for candidate in candidates() {
            assert!(!matches!(
                candidate.change,
                SystemChange::DeleteDriverPackage { .. }
                    | SystemChange::EditHosts { .. }
                    | SystemChange::SetHibernation { .. }
            ));
        }
    }
}
