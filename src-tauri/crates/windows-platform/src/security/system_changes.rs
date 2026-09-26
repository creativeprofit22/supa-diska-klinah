//! Closed set of machine-wide changes the elevated helper may apply
//! (ADR 0002). Every identifier is resolved inside the helper against a
//! compiled catalog or the current system enumeration; no variant carries a
//! path, command line, registry key, or executable.

use cleanup_core::system_change::{
    CatalogId, ChangeDescription, ChangeOutcome, DriverPackageName, EntryName, FailureCode,
    FirewallProfile, HostsLineOp, MAX_PLAN_CHANGES, PriorState, Reversibility, ServiceStartType,
    StartupEntryRef, StartupLocation, StartupScope, SystemChange,
};
use serde::{Deserialize, Serialize};

use crate::system_change::AdapterError;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum HelperChange {
    SetServiceStartType {
        catalog_id: CatalogId,
        start_type: ServiceStartType,
    },
    SetMachinePolicyValue {
        setting_id: CatalogId,
        value: Option<u32>,
    },
    SetFirewallRuleEnabled {
        rule_name: EntryName,
        enabled: bool,
    },
    SetFirewallProfileEnabled {
        profile: FirewallProfile,
        enabled: bool,
    },
    SetHibernation {
        enabled: bool,
    },
    DeleteDriverPackage {
        published_name: DriverPackageName,
    },
    EditHosts {
        line_ops: Vec<HostsLineOp>,
    },
    SetMachineStartupEntry {
        location: StartupLocation,
        name: EntryName,
        enabled: bool,
    },
    SetSystemTaskEnabled {
        catalog_id: CatalogId,
        enabled: bool,
    },
    SetWindowsUpdatePolicy {
        setting_id: CatalogId,
        value: Option<u32>,
    },
    CreateRestorePoint {
        description: ChangeDescription,
    },
}

impl HelperChange {
    /// Map a helper-privileged system change. Standard-integrity changes
    /// return `None` and must never be sent to the helper.
    pub fn from_system_change(change: &SystemChange) -> Option<Self> {
        Some(match change.clone() {
            SystemChange::SetServiceStartType {
                catalog_id,
                start_type,
            } => Self::SetServiceStartType {
                catalog_id,
                start_type,
            },
            SystemChange::SetMachineSetting { setting_id, value } => {
                Self::SetMachinePolicyValue { setting_id, value }
            }
            SystemChange::SetFirewallRuleEnabled { rule_name, enabled } => {
                Self::SetFirewallRuleEnabled { rule_name, enabled }
            }
            SystemChange::SetFirewallProfileEnabled { profile, enabled } => {
                Self::SetFirewallProfileEnabled { profile, enabled }
            }
            SystemChange::SetHibernation { enabled } => Self::SetHibernation { enabled },
            SystemChange::DeleteDriverPackage { published_name } => {
                Self::DeleteDriverPackage { published_name }
            }
            SystemChange::EditHosts { line_ops } => Self::EditHosts { line_ops },
            SystemChange::SetStartupEntry { entry, enabled }
                if entry.scope == StartupScope::Machine =>
            {
                Self::SetMachineStartupEntry {
                    location: entry.location,
                    name: entry.name,
                    enabled,
                }
            }
            SystemChange::SetSystemTaskEnabled {
                catalog_id,
                enabled,
            } => Self::SetSystemTaskEnabled {
                catalog_id,
                enabled,
            },
            SystemChange::SetWindowsUpdatePolicy { setting_id, value } => {
                Self::SetWindowsUpdatePolicy { setting_id, value }
            }
            SystemChange::CreateRestorePoint { description } => {
                Self::CreateRestorePoint { description }
            }
            SystemChange::SetStartupEntry { .. }
            | SystemChange::SetUserSetting { .. }
            | SystemChange::SetActivePowerScheme { .. }
            | SystemChange::UpsertScanSchedule { .. }
            | SystemChange::RemoveScanSchedule { .. } => return None,
        })
    }

    /// The equivalent shared contract value, used for satisfaction checks.
    pub fn to_system_change(&self) -> SystemChange {
        match self.clone() {
            Self::SetServiceStartType {
                catalog_id,
                start_type,
            } => SystemChange::SetServiceStartType {
                catalog_id,
                start_type,
            },
            Self::SetMachinePolicyValue { setting_id, value } => {
                SystemChange::SetMachineSetting { setting_id, value }
            }
            Self::SetFirewallRuleEnabled { rule_name, enabled } => {
                SystemChange::SetFirewallRuleEnabled { rule_name, enabled }
            }
            Self::SetFirewallProfileEnabled { profile, enabled } => {
                SystemChange::SetFirewallProfileEnabled { profile, enabled }
            }
            Self::SetHibernation { enabled } => SystemChange::SetHibernation { enabled },
            Self::DeleteDriverPackage { published_name } => {
                SystemChange::DeleteDriverPackage { published_name }
            }
            Self::EditHosts { line_ops } => SystemChange::EditHosts { line_ops },
            Self::SetMachineStartupEntry {
                location,
                name,
                enabled,
            } => SystemChange::SetStartupEntry {
                entry: StartupEntryRef {
                    scope: StartupScope::Machine,
                    location,
                    name,
                },
                enabled,
            },
            Self::SetSystemTaskEnabled {
                catalog_id,
                enabled,
            } => SystemChange::SetSystemTaskEnabled {
                catalog_id,
                enabled,
            },
            Self::SetWindowsUpdatePolicy { setting_id, value } => {
                SystemChange::SetWindowsUpdatePolicy { setting_id, value }
            }
            Self::CreateRestorePoint { description } => {
                SystemChange::CreateRestorePoint { description }
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HelperChangeItem {
    pub change: HelperChange,
    pub expected_prior: PriorState,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HelperChangeResult {
    pub prior: PriorState,
    pub outcome: ChangeOutcome,
}

/// Validate the batch shape before any system access.
pub fn validate_batch(items: &[HelperChangeItem]) -> bool {
    (1..=MAX_PLAN_CHANGES).contains(&items.len())
        && items
            .iter()
            .all(|item| item.change.to_system_change().validate().is_ok())
}

/// Bytes reserved in a request frame for the envelope around the batch.
const ENVELOPE_RESERVE_BYTES: usize = 512;

/// Whether the batch serializes into one protocol frame. Plans are rejected
/// up front when it does not, so an oversize frame never reaches UAC.
pub fn batch_fits_frame(items: &[HelperChangeItem]) -> bool {
    serde_json::to_vec(items)
        .is_ok_and(|bytes| bytes.len() + ENVELOPE_RESERVE_BYTES <= super::protocol::MAX_FRAME_BYTES)
}

/// Elevated reads and writes. Implementations resolve identifiers against
/// compiled catalogs or live enumeration and fail closed.
pub trait SystemChangeBackend {
    fn observe(&self, change: &HelperChange) -> Result<PriorState, AdapterError>;
    fn apply(&self, change: &HelperChange) -> Result<(), AdapterError>;
    /// Satisfaction for changes the shared contract cannot decide (hosts,
    /// restore points).
    fn is_satisfied(&self, _change: &HelperChange, _state: &PriorState) -> bool {
        false
    }
}

/// Apply every item in order. Each item re-reads its prior state; a mismatch
/// with the preview is reported as `stateChanged` without writing. A failure
/// never stops later items, with one exception: a restore point in the batch
/// guards the irreversible changes after it. If any earlier
/// `CreateRestorePoint` ended in anything other than `Applied`, every later
/// irreversible change is reported as `Failed { NotAttempted }` without
/// writing, so nothing permanent happens without the recovery point the user
/// asked for. Reversible changes still run.
pub fn apply_batch(
    items: &[HelperChangeItem],
    backend: &impl SystemChangeBackend,
) -> Vec<HelperChangeResult> {
    let mut restore_point_missing = false;
    items
        .iter()
        .map(|item| {
            let result = apply_item(item, backend, restore_point_missing);
            if matches!(item.change, HelperChange::CreateRestorePoint { .. })
                && result.outcome != ChangeOutcome::Applied
            {
                restore_point_missing = true;
            }
            result
        })
        .collect()
}

fn apply_item(
    item: &HelperChangeItem,
    backend: &impl SystemChangeBackend,
    restore_point_missing: bool,
) -> HelperChangeResult {
    if restore_point_missing
        && matches!(
            item.change.to_system_change().reversibility(),
            Reversibility::Irreversible { .. }
        )
    {
        return HelperChangeResult {
            prior: backend
                .observe(&item.change)
                .unwrap_or_else(|_| item.expected_prior.clone()),
            outcome: ChangeOutcome::Failed {
                code: FailureCode::NotAttempted,
            },
        };
    }
    let current = match backend.observe(&item.change) {
        Ok(current) => current,
        Err(error) => {
            return HelperChangeResult {
                prior: item.expected_prior.clone(),
                outcome: adapter_outcome(error),
            };
        }
    };
    let satisfied = item
        .change
        .to_system_change()
        .is_satisfied_by(&current)
        .unwrap_or_else(|| backend.is_satisfied(&item.change, &current));
    let outcome = if satisfied {
        ChangeOutcome::AlreadyApplied
    } else if current != item.expected_prior {
        ChangeOutcome::StateChanged
    } else {
        match backend.apply(&item.change) {
            Ok(()) => ChangeOutcome::Applied,
            Err(error) => adapter_outcome(error),
        }
    };
    HelperChangeResult {
        prior: current,
        outcome,
    }
}

fn adapter_outcome(error: AdapterError) -> ChangeOutcome {
    match error {
        AdapterError::Unsupported(reason) => ChangeOutcome::Unsupported { reason },
        AdapterError::Denied => ChangeOutcome::Denied,
        AdapterError::Failed => ChangeOutcome::Failed {
            code: FailureCode::SystemError,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cleanup_core::system_change::{HostsLineAction, ServiceStartState, UnsupportedReason};
    use std::{cell::RefCell, collections::HashMap};

    fn service(id: &str, start_type: ServiceStartType) -> HelperChange {
        HelperChange::SetServiceStartType {
            catalog_id: CatalogId::parse(id).unwrap(),
            start_type,
        }
    }

    #[derive(Default)]
    struct Fake {
        state: RefCell<HashMap<String, PriorState>>,
        fail: Vec<String>,
        writes: RefCell<u32>,
    }

    fn key(change: &HelperChange) -> String {
        format!("{change:?}")
            .split([' ', '{'])
            .next()
            .unwrap()
            .to_owned()
            + &match change {
                HelperChange::SetServiceStartType { catalog_id, .. } => catalog_id.to_string(),
                _ => String::new(),
            }
    }

    impl SystemChangeBackend for Fake {
        fn observe(&self, change: &HelperChange) -> Result<PriorState, AdapterError> {
            self.state
                .borrow()
                .get(&key(change))
                .cloned()
                .ok_or(AdapterError::Unsupported(UnsupportedReason::NotPresent))
        }

        fn apply(&self, change: &HelperChange) -> Result<(), AdapterError> {
            *self.writes.borrow_mut() += 1;
            if self.fail.contains(&key(change)) {
                return Err(AdapterError::Failed);
            }
            if let HelperChange::SetServiceStartType { start_type, .. } = change {
                self.state.borrow_mut().insert(
                    key(change),
                    PriorState::ServiceStart {
                        start: (*start_type).into(),
                    },
                );
            }
            Ok(())
        }
    }

    fn manual() -> PriorState {
        PriorState::ServiceStart {
            start: ServiceStartState::Manual,
        }
    }

    #[test]
    fn batch_reports_every_item_and_continues_after_failure() {
        let mut fake = Fake::default();
        for id in ["a", "b", "c", "d"] {
            fake.state
                .borrow_mut()
                .insert(key(&service(id, ServiceStartType::Manual)), manual());
        }
        fake.state.borrow_mut().insert(
            key(&service("d", ServiceStartType::Manual)),
            PriorState::ServiceStart {
                start: ServiceStartState::Disabled,
            },
        );
        fake.fail.push(key(&service("b", ServiceStartType::Manual)));
        let items = vec![
            HelperChangeItem {
                change: service("a", ServiceStartType::Disabled),
                expected_prior: manual(),
            },
            HelperChangeItem {
                change: service("b", ServiceStartType::Disabled),
                expected_prior: manual(),
            },
            HelperChangeItem {
                change: service("c", ServiceStartType::Manual),
                expected_prior: manual(),
            },
            HelperChangeItem {
                change: service("d", ServiceStartType::Automatic),
                expected_prior: manual(),
            },
            HelperChangeItem {
                change: service("zz", ServiceStartType::Disabled),
                expected_prior: manual(),
            },
        ];
        let results = apply_batch(&items, &fake);
        let outcomes: Vec<_> = results.iter().map(|result| result.outcome).collect();
        assert_eq!(
            outcomes,
            [
                ChangeOutcome::Applied,
                ChangeOutcome::Failed {
                    code: FailureCode::SystemError
                },
                ChangeOutcome::AlreadyApplied,
                ChangeOutcome::StateChanged,
                ChangeOutcome::Unsupported {
                    reason: UnsupportedReason::NotPresent
                },
            ]
        );
        assert_eq!(*fake.writes.borrow(), 2);
        assert_eq!(results[0].prior, manual());
    }

    fn restore_point_plan() -> (Fake, Vec<HelperChangeItem>) {
        let restore = HelperChange::CreateRestorePoint {
            description: ChangeDescription::parse("Before removing drivers").unwrap(),
        };
        let delete = HelperChange::DeleteDriverPackage {
            published_name: DriverPackageName::parse("oem12.inf").unwrap(),
        };
        let hibernate = HelperChange::SetHibernation { enabled: false };
        let fake = Fake::default();
        for (change, state) in [
            (&restore, PriorState::NotApplicable),
            (&delete, PriorState::DriverPackage { present: true }),
            (&hibernate, PriorState::Enabled { enabled: true }),
        ] {
            fake.state.borrow_mut().insert(key(change), state);
        }
        let items = vec![
            HelperChangeItem {
                change: restore,
                expected_prior: PriorState::NotApplicable,
            },
            HelperChangeItem {
                change: delete,
                expected_prior: PriorState::DriverPackage { present: true },
            },
            HelperChangeItem {
                change: hibernate,
                expected_prior: PriorState::Enabled { enabled: true },
            },
        ];
        (fake, items)
    }

    #[test]
    fn failed_restore_point_blocks_later_irreversible_changes_only() {
        let (mut fake, items) = restore_point_plan();
        fake.fail.push(key(&items[0].change));
        let results = apply_batch(&items, &fake);
        let outcomes: Vec<_> = results.iter().map(|result| result.outcome).collect();
        assert_eq!(
            outcomes,
            [
                ChangeOutcome::Failed {
                    code: FailureCode::SystemError
                },
                ChangeOutcome::Failed {
                    code: FailureCode::NotAttempted
                },
                ChangeOutcome::Applied,
            ]
        );
        // Restore point and hibernation were attempted; the delete never was.
        assert_eq!(*fake.writes.borrow(), 2);
        assert_eq!(
            results[1].prior,
            PriorState::DriverPackage { present: true }
        );
    }

    #[test]
    fn applied_restore_point_lets_irreversible_changes_run() {
        let (fake, items) = restore_point_plan();
        let outcomes: Vec<_> = apply_batch(&items, &fake)
            .iter()
            .map(|result| result.outcome)
            .collect();
        assert_eq!(outcomes, [ChangeOutcome::Applied; 3]);
        assert_eq!(*fake.writes.borrow(), 3);
    }

    #[test]
    fn standard_changes_never_map_to_helper_changes() {
        let user = SystemChange::SetStartupEntry {
            entry: StartupEntryRef {
                scope: StartupScope::User,
                location: StartupLocation::Run,
                name: EntryName::parse("App").unwrap(),
            },
            enabled: false,
        };
        assert!(HelperChange::from_system_change(&user).is_none());
        assert!(
            HelperChange::from_system_change(&SystemChange::SetUserSetting {
                setting_id: CatalogId::parse("x").unwrap(),
                value: None
            })
            .is_none()
        );
        let machine = SystemChange::SetStartupEntry {
            entry: StartupEntryRef {
                scope: StartupScope::Machine,
                location: StartupLocation::Run32,
                name: EntryName::parse("App").unwrap(),
            },
            enabled: true,
        };
        let helper = HelperChange::from_system_change(&machine).unwrap();
        assert_eq!(helper.to_system_change(), machine);
    }

    #[test]
    fn batch_validation_bounds_count_and_contents() {
        let item = HelperChangeItem {
            change: HelperChange::SetHibernation { enabled: false },
            expected_prior: manual(),
        };
        assert!(validate_batch(std::slice::from_ref(&item)));
        assert!(!validate_batch(&[]));
        assert!(!validate_batch(&vec![item; MAX_PLAN_CHANGES + 1]));
        let hosts = HelperChangeItem {
            change: HelperChange::EditHosts { line_ops: vec![] },
            expected_prior: PriorState::NotApplicable,
        };
        assert!(!validate_batch(&[hosts]));
        let duplicate_lines = HelperChangeItem {
            change: HelperChange::EditHosts {
                line_ops: vec![
                    HostsLineOp {
                        line: 1,
                        action: HostsLineAction::Disable,
                    },
                    HostsLineOp {
                        line: 1,
                        action: HostsLineAction::Restore,
                    },
                ],
            },
            expected_prior: PriorState::NotApplicable,
        };
        assert!(!validate_batch(&[duplicate_lines]));
    }

    #[test]
    fn oversize_batches_are_detected_before_launch() {
        let op = |line| HostsLineOp {
            line,
            action: HostsLineAction::Disable,
        };
        let item = HelperChangeItem {
            change: HelperChange::EditHosts {
                line_ops: (1..=64).map(op).collect(),
            },
            expected_prior: PriorState::NotApplicable,
        };
        assert!(batch_fits_frame(std::slice::from_ref(&item)));
        assert!(!batch_fits_frame(&vec![item; MAX_PLAN_CHANGES]));
    }

    #[test]
    fn parser_rejects_paths_commands_and_unknown_kinds() {
        for json in [
            r#"{"kind":"runCommand","command":"whoami /all"}"#,
            r#"{"kind":"setServiceStartType","catalogId":"C:\\evil","startType":"disabled"}"#,
            r#"{"kind":"setServiceStartType","catalogId":"diagtrack","startType":"boot"}"#,
            r#"{"kind":"deleteDriverPackage","publishedName":"..\\..\\x.inf"}"#,
            r#"{"kind":"setHibernation","enabled":true,"path":"C:\\"}"#,
            r#"{"kind":"setMachinePolicyValue","settingId":"x","value":1,"key":"HKLM\\SAM"}"#,
        ] {
            assert!(
                serde_json::from_str::<HelperChange>(json).is_err(),
                "{json}"
            );
        }
    }
}
