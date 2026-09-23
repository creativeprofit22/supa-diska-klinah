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
pub mod root_picker;
pub mod scans;
#[cfg(test)]
mod step6_tests;
pub mod uninstaller;
#[cfg(test)]
mod uninstaller_tests;
mod vendor_jobs;
pub mod vendor_uninstall;

pub use cleanup_core::storage::{
    CandidateEligibility, MAX_REQUEST_BYTES, MAX_SELECTION, PageCollection, PageRequest,
    PartialReason, StorageError, StorageLimits, StorageModule, StoragePage, StoragePhase,
    StorageRecord, StorageSelection, StorageStatus,
    analysis::{DirectorySummary, DriveSummary, ExtensionSummary},
    duplicates::{DuplicateGroup, DuplicateMember},
    empty_folders::EmptyFolderRecord,
    large_files::{FileFilter, FileRecord},
};

pub use cleanup_core::{Lifecycle, Risk};

/// Raw IPC bytes are bounded before serde can allocate request fields. Proof types
/// are never accepted by the app adapter; only its closed request DTOs use this.
pub fn decode_command<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, StorageError> {
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err(StorageError::LimitReached);
    }
    serde_json::from_slice(bytes).map_err(|_| StorageError::InvalidRequest)
}
/// Resolve the same native protection policy used by cleanup at execution time.
pub fn current_protection() -> Result<cleanup_core::ProtectionPolicy, StorageError> {
    crate::cleanup::current_protection().map_err(|_| StorageError::UnsupportedScope)
}
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
