//! Elevated hosts edits. Runs only inside the privileged helper; the path is
//! re-resolved from `GetSystemDirectoryW` and every op is re-validated
//! against the file's current content.

use cleanup_core::system_change::PriorState;

use super::{HostsStore, apply_ops, observe_ops, satisfied};
use crate::{security::system_changes::HelperChange, system_change::AdapterError};

pub fn observe(change: &HelperChange) -> Result<PriorState, AdapterError> {
    let HelperChange::EditHosts { line_ops } = change else {
        return Err(AdapterError::Failed);
    };
    let store = HostsStore::system()?;
    observe_ops(&store, line_ops)
}

/// Every op's line is already in its target state in the current file.
pub fn is_satisfied(change: &HelperChange, state: &PriorState) -> bool {
    let HelperChange::EditHosts { line_ops } = change else {
        return false;
    };
    HostsStore::system().is_ok_and(|store| satisfied(&store, line_ops, state))
}

pub fn apply(change: &HelperChange) -> Result<(), AdapterError> {
    let HelperChange::EditHosts { line_ops } = change else {
        return Err(AdapterError::Failed);
    };
    let store = HostsStore::system()?;
    apply_ops(&store, &store, line_ops)
}
