//! Storage discovery contracts only. Records and persisted evidence are not mutation authority.
pub mod analysis;
pub mod duplicates;
pub mod empty_folders;
mod evidence;
pub mod large_files;
mod paging;
pub mod walk;

pub use evidence::*;
pub use paging::*;
use serde::{Deserialize, Serialize};

pub const MAX_ACTIVE_SCANS: usize = 1;
pub const MAX_WORKERS: usize = 4;
pub const MAX_VISITED_ENTRIES: usize = 1_000_000;
pub const MAX_RECORDS: usize = 100_000;
pub const MAX_DIAGNOSTICS: usize = 1_000;
pub const MAX_DEPTH: u16 = 64;
pub const DEFAULT_PAGE_SIZE: usize = 100;
pub const MAX_PAGE_SIZE: usize = 200;
pub const MAX_COMPLETED_SNAPSHOTS: usize = 2;
pub const SNAPSHOT_IDLE_SECONDS: u64 = 600;
pub const MAX_SELECTION: usize = 1_000;
pub const MAX_REQUEST_BYTES: usize = 64 * 1024;

mod input {
    pub trait Sealed {
        fn validate_input(&self) -> Result<(), super::StorageError>;
    }
}
/// Closed set of request DTOs. Future IPC adapters must cap the incoming payload
/// before deserialization and use this decoder, not deserialize arbitrary proofs.
pub trait StorageInput: input::Sealed + serde::de::DeserializeOwned {}
impl input::Sealed for StorageScanRequest {
    fn validate_input(&self) -> Result<(), StorageError> {
        self.validate()
    }
}
impl input::Sealed for StorageSelection {
    fn validate_input(&self) -> Result<(), StorageError> {
        self.validate()
    }
}
impl input::Sealed for PageRequest {
    fn validate_input(&self) -> Result<(), StorageError> {
        self.validate()
    }
}
impl StorageInput for StorageScanRequest {}
impl StorageInput for StorageSelection {}
impl StorageInput for PageRequest {}
pub fn decode_request<T: StorageInput>(bytes: &[u8]) -> Result<T, StorageError> {
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err(StorageError::LimitReached);
    }
    let request: T = serde_json::from_slice(bytes).map_err(|_| StorageError::InvalidRequest)?;
    request.validate_input()?;
    Ok(request)
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StorageModule {
    Cleaner,
    DiskAnalyzer,
    LargeFiles,
    Duplicates,
    EmptyFolders,
    Browser,
    Uninstaller,
    Drives,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StorageLimits {
    pub workers: usize,
    pub visited_entries: usize,
    pub retained_records: usize,
    pub diagnostics: usize,
    pub depth: u16,
}
impl Default for StorageLimits {
    fn default() -> Self {
        Self {
            workers: MAX_WORKERS,
            visited_entries: MAX_VISITED_ENTRIES,
            retained_records: MAX_RECORDS,
            diagnostics: MAX_DIAGNOSTICS,
            depth: MAX_DEPTH,
        }
    }
}
impl StorageLimits {
    pub fn validate(&self) -> Result<(), StorageError> {
        if !(1..=MAX_WORKERS).contains(&self.workers)
            || !(1..=MAX_VISITED_ENTRIES).contains(&self.visited_entries)
            || !(1..=MAX_RECORDS).contains(&self.retained_records)
            || !(1..=MAX_DIAGNOSTICS).contains(&self.diagnostics)
            || self.depth > MAX_DEPTH
        {
            return Err(StorageError::InvalidRequest);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PartialReason {
    Cancelled,
    EntryLimit,
    RecordLimit,
    DiagnosticLimit,
    DepthLimit,
    Unreadable,
    Changed,
    LinkLike,
    Protected,
    Excluded,
    MissingIdentity,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Completeness {
    pub reasons: Vec<PartialReason>,
}
impl Completeness {
    pub fn is_complete(&self) -> bool {
        self.reasons.is_empty()
    }
    pub(crate) fn mark(&mut self, reason: PartialReason) {
        if !self.reasons.contains(&reason) {
            self.reasons.push(reason);
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum CandidateEligibility {
    ReadOnly,
    Ineligible {
        reason: PartialReason,
    },
    /// An opaque candidate must still resolve in the same snapshot; this is not a plan.
    Eligible {
        candidate_id: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageError {
    InvalidRequest,
    InvalidEvidence,
    UnsupportedScope,
    LimitReached,
    InvalidCursor,
    SnapshotUnavailable,
    Entropy,
}
impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for StorageError {}

pub(crate) fn valid_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
pub(crate) fn bounded_text(value: &str) -> bool {
    !value.is_empty() && value.len() <= 1024 && !value.chars().any(char::is_control)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StorageScanRequest {
    pub root_id: String,
    pub limits: StorageLimits,
    pub options: StorageOptions,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(
    tag = "module",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum StorageOptions {
    Cleaner {
        catalog_id: String,
    },
    DiskAnalyzer {
        displayed_depth: u16,
    },
    LargeFiles {
        filter: large_files::FileFilter,
    },
    Duplicates {
        filter: large_files::FileFilter,
    },
    EmptyFolders,
    Browser {
        browser_id: String,
        service_worker_opt_in: bool,
    },
    Uninstaller,
    Drives,
}
impl StorageScanRequest {
    pub fn validate(&self) -> Result<(), StorageError> {
        self.limits.validate()?;
        if !valid_id(&self.root_id) {
            return Err(StorageError::InvalidRequest);
        }
        match &self.options {
            StorageOptions::Cleaner { catalog_id } if !bounded_text(catalog_id) => {
                Err(StorageError::InvalidRequest)
            }
            StorageOptions::Browser { browser_id, .. } if !bounded_text(browser_id) => {
                Err(StorageError::InvalidRequest)
            }
            StorageOptions::DiskAnalyzer { displayed_depth }
                if *displayed_depth > self.limits.depth =>
            {
                Err(StorageError::InvalidRequest)
            }
            StorageOptions::LargeFiles { filter } | StorageOptions::Duplicates { filter } => {
                filter.validate()
            }
            _ => Ok(()),
        }
    }
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StoragePhase {
    Queued,
    Walking,
    Grouping,
    PartialHash,
    FullHash,
    Finalizing,
    Complete,
    Cancelled,
    Failed,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageStatus {
    pub snapshot_id: String,
    pub module: StorageModule,
    pub phase: StoragePhase,
    pub visited_entries: usize,
    pub retained_records: usize,
    /// Bytes read by hashing across partial and full phases (not unique disk bytes).
    pub hashed_bytes: u64,
    /// Completed file hashes across phases; a file can contribute twice.
    pub completed_hashes: usize,
    pub completeness: Completeness,
}

/// IPC selections carry IDs, never paths or renderer-supplied proofs.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StorageSelection {
    pub snapshot_id: String,
    pub module: StorageModule,
    pub candidate_ids: Vec<String>,
}
impl StorageSelection {
    pub fn validate(&self) -> Result<(), StorageError> {
        let mut seen = std::collections::HashSet::new();
        if !valid_id(&self.snapshot_id)
            || self.candidate_ids.is_empty()
            || self.candidate_ids.len() > MAX_SELECTION
            || self
                .candidate_ids
                .iter()
                .any(|id| !valid_id(id) || !seen.insert(id))
        {
            return Err(StorageError::InvalidRequest);
        }
        Ok(())
    }
}
