//! Helper-side dispatcher: routes each closed `HelperChange` to its module's
//! elevated implementation. Runs only inside the elevated helper process;
//! every module re-resolves identifiers against its compiled catalog or the
//! live system before reading or writing.

use cleanup_core::system_change::{PriorState, UnsupportedReason};

use super::AdapterError;
use crate::{
    drivers, firewall, hosts, os_info, power, privacy,
    security::{
        RestorePointDescription,
        restore_point::{RestorePointBackend, WindowsRestorePointBackend},
        system_changes::{HelperChange, SystemChangeBackend},
    },
    services, startup_items, updates,
};

pub struct WindowsSystemChangeBackend;

impl SystemChangeBackend for WindowsSystemChangeBackend {
    fn observe(&self, change: &HelperChange) -> Result<PriorState, AdapterError> {
        // Re-check the build floor in the elevated process; apply only runs
        // after a successful observe.
        if !os_info::current().is_supported() {
            return Err(AdapterError::Unsupported(UnsupportedReason::OsVersion));
        }
        match change {
            HelperChange::SetServiceStartType { .. } => services::elevated::observe(change),
            HelperChange::SetMachinePolicyValue { .. }
            | HelperChange::SetSystemTaskEnabled { .. } => privacy::elevated::observe(change),
            HelperChange::SetFirewallRuleEnabled { .. }
            | HelperChange::SetFirewallProfileEnabled { .. } => firewall::elevated::observe(change),
            HelperChange::SetHibernation { .. } => power::elevated::observe(change),
            HelperChange::DeleteDriverPackage { .. } => drivers::elevated::observe(change),
            HelperChange::EditHosts { .. } => hosts::elevated::observe(change),
            HelperChange::SetMachineStartupEntry { .. } => startup_items::elevated::observe(change),
            HelperChange::SetWindowsUpdatePolicy { .. } => updates::elevated::observe(change),
            HelperChange::CreateRestorePoint { .. } => Ok(PriorState::NotApplicable),
        }
    }

    fn apply(&self, change: &HelperChange) -> Result<(), AdapterError> {
        match change {
            HelperChange::SetServiceStartType { .. } => services::elevated::apply(change),
            HelperChange::SetMachinePolicyValue { .. }
            | HelperChange::SetSystemTaskEnabled { .. } => privacy::elevated::apply(change),
            HelperChange::SetFirewallRuleEnabled { .. }
            | HelperChange::SetFirewallProfileEnabled { .. } => firewall::elevated::apply(change),
            HelperChange::SetHibernation { .. } => power::elevated::apply(change),
            HelperChange::DeleteDriverPackage { .. } => drivers::elevated::apply(change),
            HelperChange::EditHosts { .. } => hosts::elevated::apply(change),
            HelperChange::SetMachineStartupEntry { .. } => startup_items::elevated::apply(change),
            HelperChange::SetWindowsUpdatePolicy { .. } => updates::elevated::apply(change),
            HelperChange::CreateRestorePoint { description } => {
                let description = RestorePointDescription::parse(description.as_str().to_owned())
                    .map_err(|_| AdapterError::Failed)?;
                WindowsRestorePointBackend
                    .create(&description)
                    .map(|_| ())
                    .map_err(|_| AdapterError::Failed)
            }
        }
    }

    fn is_satisfied(&self, change: &HelperChange, state: &PriorState) -> bool {
        match change {
            HelperChange::EditHosts { .. } => hosts::elevated::is_satisfied(change, state),
            _ => false,
        }
    }
}
