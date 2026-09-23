//! Elevated-helper entry points for service start-type changes. Every
//! identifier is re-resolved against the compiled catalog inside the helper.

use cleanup_core::system_change::{PriorState, ServiceStartType};

use super::{ScmServices, ServiceReader, ServiceWriter, lookup};
use crate::{security::system_changes::HelperChange, system_change::AdapterError};

pub fn observe(change: &HelperChange) -> Result<PriorState, AdapterError> {
    observe_with(&ScmServices, change)
}

pub fn apply(change: &HelperChange) -> Result<(), AdapterError> {
    apply_with(&ScmServices, &ScmServices, change)
}

fn target(change: &HelperChange) -> Result<(&'static str, ServiceStartType), AdapterError> {
    let HelperChange::SetServiceStartType {
        catalog_id,
        start_type,
    } = change
    else {
        return Err(AdapterError::Failed);
    };
    Ok((lookup(catalog_id)?.service_name, *start_type))
}

pub(crate) fn observe_with(
    reader: &dyn ServiceReader,
    change: &HelperChange,
) -> Result<PriorState, AdapterError> {
    let (service_name, _) = target(change)?;
    Ok(PriorState::ServiceStart {
        start: reader.query(service_name)?.start,
    })
}

pub(crate) fn apply_with(
    reader: &dyn ServiceReader,
    writer: &dyn ServiceWriter,
    change: &HelperChange,
) -> Result<(), AdapterError> {
    let (service_name, start_type) = target(change)?;
    // Boot and system start types are observable but never writable.
    if reader.query(service_name)?.start.writable().is_none() {
        return Err(AdapterError::Failed);
    }
    writer.set_start_type(service_name, start_type)
}
