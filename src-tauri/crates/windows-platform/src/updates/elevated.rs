//! Elevated-helper entry points for Windows Update policy changes. Every call
//! re-reads OS facts and re-resolves the setting against the compiled catalog.

use cleanup_core::system_change::PriorState;

use super::{RegistryPolicyStore, apply_policy, observe_policy};
use crate::{os_info, security::system_changes::HelperChange, system_change::AdapterError};

pub fn observe(change: &HelperChange) -> Result<PriorState, AdapterError> {
    match change {
        HelperChange::SetWindowsUpdatePolicy { setting_id, value } => observe_policy(
            &os_info::current(),
            &RegistryPolicyStore,
            setting_id.as_str(),
            *value,
        ),
        _ => Err(AdapterError::Failed),
    }
}

pub fn apply(change: &HelperChange) -> Result<(), AdapterError> {
    match change {
        HelperChange::SetWindowsUpdatePolicy { setting_id, value } => apply_policy(
            &os_info::current(),
            &RegistryPolicyStore,
            setting_id.as_str(),
            *value,
        ),
        _ => Err(AdapterError::Failed),
    }
}
