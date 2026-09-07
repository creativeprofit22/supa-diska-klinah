//! Backend-only storage discovery. No IPC or mutation authority is exposed here.
pub mod browser;
pub mod cleaner;
pub mod disk_analyzer;
pub mod drives;
pub mod duplicates;
#[cfg(test)]
mod duplicates_tests;
pub mod empty_folders;
#[cfg(test)]
mod empty_folders_tests;
pub mod known_folders;
pub mod large_files;
pub(crate) mod protection;
pub mod scans;
#[cfg(test)]
mod step6_tests;
pub mod uninstaller;
#[cfg(test)]
mod uninstaller_tests;
mod vendor_jobs;
pub mod vendor_uninstall;

use cleanup_core::storage::StorageError;
pub use cleanup_core::storage::{
    PageCollection, PageRequest, PartialReason, StorageLimits, StorageModule, StoragePhase,
    StorageRecord, analysis::DriveSummary,
};
use cleanup_core::{Entropy, FsError};

struct NativeEntropy;
impl Entropy for NativeEntropy {
    fn fill(&self, bytes: &mut [u8]) -> Result<(), FsError> {
        getrandom::fill(bytes)
            .map_err(|e| FsError::new(cleanup_core::FsErrorKind::Other, e.to_string()))
    }
}
fn opaque_id() -> Result<String, StorageError> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).map_err(|_| StorageError::Entropy)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
