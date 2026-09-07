use super::{StorageError, StorageModule, bounded_text, valid_id};
use crate::{EntryKind, FileIdentity, PathSemantics, is_local_storage_path};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Backend-owned root binding: scope cannot be transferred to another snapshot or volume.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RootAuthorization {
    pub snapshot_id: String,
    pub root_id: String,
    pub canonical_path: PathBuf,
    pub identity: FileIdentity,
}
impl RootAuthorization {
    pub fn validate(&self) -> Result<(), StorageError> {
        if !valid_id(&self.snapshot_id)
            || !valid_id(&self.root_id)
            || !is_local_storage_path(&self.canonical_path)
        {
            return Err(StorageError::InvalidEvidence);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ObservedEntry {
    pub canonical_path: PathBuf,
    pub identity: FileIdentity,
    pub kind: EntryKind,
    pub logical_bytes: u64,
    pub allocated_bytes: Option<u64>,
    pub modified_unix_nanos: u64,
}
impl ObservedEntry {
    pub fn validate_under(&self, root: &RootAuthorization) -> Result<(), StorageError> {
        let semantics = PathSemantics::CaseInsensitive;
        if !is_local_storage_path(&self.canonical_path)
            || self.kind == EntryKind::LinkLike
            || semantics.equivalent(&root.canonical_path, &self.canonical_path)
            || !semantics.contains(&root.canonical_path, &self.canonical_path)
            || root.identity.volume != self.identity.volume
        {
            return Err(StorageError::InvalidEvidence);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogEvidence {
    pub catalog_id: String,
    pub revision: String,
    pub target_id: String,
    pub rule_version: u32,
    pub minimum_age_seconds: u64,
    pub newest_descendant_unix_nanos: u64,
    pub observed_at_unix_nanos: u64,
}
impl CatalogEvidence {
    fn valid(&self) -> bool {
        bounded_text(&self.catalog_id)
            && bounded_text(&self.revision)
            && bounded_text(&self.target_id)
            && self.rule_version > 0
            && self
                .observed_at_unix_nanos
                .checked_sub(self.newest_descendant_unix_nanos)
                .is_some_and(|age| age / 1_000_000_000 >= self.minimum_age_seconds)
    }
}

/// Full digest is internal evidence, not a renderer result. No hash implementation lives here.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KeeperEvidence {
    pub group_id: String,
    pub keeper_root: RootAuthorization,
    pub keeper: ObservedEntry,
    pub full_sha256: [u8; 32],
    pub independent_copies: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "module",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum StorageEvidence {
    CatalogTarget {
        root: RootAuthorization,
        entry: ObservedEntry,
        catalog: CatalogEvidence,
    },
    UserSelectedFile {
        root: RootAuthorization,
        entry: ObservedEntry,
    },
    DuplicateMember {
        root: RootAuthorization,
        entry: ObservedEntry,
        keeper: KeeperEvidence,
    },
    EmptyFolder {
        root: RootAuthorization,
        entry: ObservedEntry,
        complete_subtree: bool,
        descendant_directories: u32,
    },
    BrowserCache {
        root: RootAuthorization,
        entry: ObservedEntry,
        catalog: CatalogEvidence,
        browser_id: String,
        profile_id: String,
        profile: ObservedEntry,
        activity_checked_at_unix_nanos: u64,
        browser_inactive: bool,
        service_worker: bool,
        service_worker_opt_in: bool,
    },
    ConfirmedApplicationLeftover {
        root: RootAuthorization,
        entry: ObservedEntry,
        program_id: String,
        registry_key: String,
        registry_view: RegistryView,
        uninstall_confirmation_id: String,
        uninstall_completed_at_unix_nanos: u64,
        exact_application_root: ObservedEntry,
    },
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RegistryView {
    CurrentUser32,
    CurrentUser64,
    LocalMachine32,
    LocalMachine64,
}

impl StorageEvidence {
    pub fn module(&self) -> StorageModule {
        match self {
            Self::CatalogTarget { .. } => StorageModule::Cleaner,
            Self::UserSelectedFile { .. } => StorageModule::LargeFiles,
            Self::DuplicateMember { .. } => StorageModule::Duplicates,
            Self::EmptyFolder { .. } => StorageModule::EmptyFolders,
            Self::BrowserCache { .. } => StorageModule::Browser,
            Self::ConfirmedApplicationLeftover { .. } => StorageModule::Uninstaller,
        }
    }
    pub fn root(&self) -> &RootAuthorization {
        match self {
            Self::CatalogTarget { root, .. }
            | Self::UserSelectedFile { root, .. }
            | Self::DuplicateMember { root, .. }
            | Self::EmptyFolder { root, .. }
            | Self::BrowserCache { root, .. }
            | Self::ConfirmedApplicationLeftover { root, .. } => root,
        }
    }
    pub fn entry(&self) -> &ObservedEntry {
        match self {
            Self::CatalogTarget { entry, .. }
            | Self::UserSelectedFile { entry, .. }
            | Self::DuplicateMember { entry, .. }
            | Self::EmptyFolder { entry, .. }
            | Self::BrowserCache { entry, .. }
            | Self::ConfirmedApplicationLeftover { entry, .. } => entry,
        }
    }
    /// Structural validation only. Current rules, activity, content and mutation guards
    /// must be re-resolved by the platform; no caller may treat this as permission.
    pub fn validate(&self) -> Result<(), StorageError> {
        let root = self.root();
        let entry = self.entry();
        root.validate()?;
        entry.validate_under(root)?;
        let valid = match self {
            Self::CatalogTarget { catalog, .. } => catalog.valid(),
            Self::UserSelectedFile { .. } => entry.kind == EntryKind::File,
            Self::DuplicateMember { keeper, .. } => {
                keeper.keeper_root.validate()?;
                keeper.keeper.validate_under(&keeper.keeper_root)?;
                entry.kind == EntryKind::File
                    && keeper.keeper.kind == EntryKind::File
                    && valid_id(&keeper.group_id)
                    && (2..=100_000).contains(&keeper.independent_copies)
                    && keeper.keeper_root.snapshot_id == root.snapshot_id
                    && keeper.keeper.identity != entry.identity
                    && keeper.keeper.logical_bytes == entry.logical_bytes
                    && !PathSemantics::CaseInsensitive
                        .equivalent(&keeper.keeper.canonical_path, &entry.canonical_path)
            }
            Self::EmptyFolder {
                complete_subtree,
                descendant_directories,
                ..
            } => {
                entry.kind == EntryKind::Directory
                    && entry.logical_bytes == 0
                    && *complete_subtree
                    && *descendant_directories <= 100_000
            }
            Self::BrowserCache {
                catalog,
                browser_id,
                profile_id,
                profile,
                browser_inactive,
                service_worker,
                service_worker_opt_in,
                ..
            } => {
                is_local_storage_path(&profile.canonical_path)
                    && profile.kind == EntryKind::Directory
                    && catalog.valid()
                    && bounded_text(browser_id)
                    && valid_id(profile_id)
                    && *browser_inactive
                    && (!service_worker || *service_worker_opt_in)
                    && profile.identity.volume == root.identity.volume
                    && PathSemantics::CaseInsensitive
                        .contains(&root.canonical_path, &profile.canonical_path)
                    && PathSemantics::CaseInsensitive
                        .contains(&profile.canonical_path, &entry.canonical_path)
                    && !PathSemantics::CaseInsensitive
                        .equivalent(&profile.canonical_path, &entry.canonical_path)
            }
            Self::ConfirmedApplicationLeftover {
                program_id,
                registry_key,
                uninstall_confirmation_id,
                exact_application_root,
                ..
            } => {
                valid_id(program_id)
                    && valid_id(uninstall_confirmation_id)
                    && bounded_text(registry_key)
                    && exact_application_root.kind == EntryKind::Directory
                    && is_local_storage_path(&exact_application_root.canonical_path)
                    && exact_application_root.identity.volume == root.identity.volume
                    && PathSemantics::CaseInsensitive
                        .contains(&root.canonical_path, &exact_application_root.canonical_path)
                    && PathSemantics::CaseInsensitive.contains(
                        &exact_application_root.canonical_path,
                        &entry.canonical_path,
                    )
            }
        };
        if valid {
            Ok(())
        } else {
            Err(StorageError::InvalidEvidence)
        }
    }
}
