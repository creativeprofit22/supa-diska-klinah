//! Helper-side driver package deletion. Runs only inside the elevated helper
//! process; the whole inventory is recomputed here, so a package that became
//! bound or current since the plan was built is refused.

use cleanup_core::system_change::PriorState;

use super::{
    DriverReader, DriverWriter, WindowsDriverReader, WindowsDriverWriter, observe_package,
};
use crate::{security::system_changes::HelperChange, system_change::AdapterError};

pub fn observe(change: &HelperChange) -> Result<PriorState, AdapterError> {
    observe_with(&WindowsDriverReader, change)
}

pub fn apply(change: &HelperChange) -> Result<(), AdapterError> {
    apply_with(&WindowsDriverReader, &WindowsDriverWriter, change)
}

pub(crate) fn observe_with(
    reader: &dyn DriverReader,
    change: &HelperChange,
) -> Result<PriorState, AdapterError> {
    match change {
        HelperChange::DeleteDriverPackage { published_name } => {
            observe_package(reader, published_name)
        }
        _ => Err(AdapterError::Failed),
    }
}

pub(crate) fn apply_with(
    reader: &dyn DriverReader,
    writer: &dyn DriverWriter,
    change: &HelperChange,
) -> Result<(), AdapterError> {
    let HelperChange::DeleteDriverPackage { published_name } = change else {
        return Err(AdapterError::Failed);
    };
    match observe_package(reader, published_name)? {
        PriorState::DriverPackage { present: false } => Ok(()),
        PriorState::DriverPackage { present: true } => writer.uninstall(published_name),
        _ => Err(AdapterError::Failed),
    }
}
