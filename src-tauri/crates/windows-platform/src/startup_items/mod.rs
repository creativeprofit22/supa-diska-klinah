//! Startup manager: lists what starts at sign-in and toggles the Explorer
//! `StartupApproved` flag for Run, Run32, and Startup-folder entries.
//!
//! Enabling or disabling only writes the 12-byte `StartupApproved` REG_BINARY
//! value, exactly as Task Manager does. The Run value and the Startup-folder
//! file are never modified or deleted.
//!
//! Parity gap: deleting startup entries is intentionally NOT offered (Kudu
//! offers it). Disabling is reversible; deletion would destroy the command
//! line or shortcut with no reliable way to restore it.

mod native;
#[cfg(test)]
mod tests;

use cleanup_core::system_change::{
    ImpactSummary, PriorState, RestartRequirement, RiskLevel, StartupEntryRef, StartupLocation,
    StartupScope, SystemChange, UnsupportedReason,
};
use serde::Serialize;

pub use native::WindowsStartupStore;

use crate::{system_change::AdapterError, system_change::SystemAdapter, win_registry::Hive};

pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
pub const RUN32_KEY: &str = r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run";
pub const RUN_ONCE_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\RunOnce";
pub const APPROVED_RUN_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
pub const APPROVED_RUN32_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run32";
pub const APPROVED_FOLDER_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\StartupFolder";

pub const MAX_ITEMS: usize = 4096;
pub const MAX_NAME_CHARS: usize = 260;
pub const MAX_COMMAND_CHARS: usize = 1024;
pub const MAX_FOLDER_ENTRIES: usize = 1024;
pub const MAX_TASK_DEPTH: usize = 8;
pub const MAX_TASKS: usize = 2000;

/// A Run value or Startup-folder file: its name and display-only command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceEntry {
    pub name: String,
    pub command: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogonTask {
    pub path: String,
    pub enabled: bool,
}

/// Read access to every startup source. Paths passed to the registry
/// methods are always the compile-time constants above.
pub trait StartupReader: Send + Sync {
    /// Values of a Run-style key; empty when the key does not exist.
    fn registry_values(&self, hive: Hive, path: &str) -> Result<Vec<SourceEntry>, AdapterError>;
    /// Files directly inside the user or all-users Startup folder.
    fn folder_files(&self, scope: StartupScope) -> Result<Vec<SourceEntry>, AdapterError>;
    /// The raw `StartupApproved` REG_BINARY data; `None` when absent.
    fn approved(&self, hive: Hive, path: &str, name: &str)
    -> Result<Option<Vec<u8>>, AdapterError>;
    /// Scheduled tasks with a logon trigger.
    fn logon_tasks(&self) -> Result<Vec<LogonTask>, AdapterError>;
}

pub trait StartupWriter: Send + Sync {
    fn set_approved(
        &self,
        hive: Hive,
        path: &str,
        name: &str,
        data: &[u8],
    ) -> Result<(), AdapterError>;
}

/// `StartupApproved` decoding: an absent value (or no data) means enabled;
/// otherwise byte 0 even means enabled and odd means disabled.
pub fn decode_approved(data: Option<&[u8]>) -> bool {
    match data.and_then(|bytes| bytes.first()) {
        Some(first) => first % 2 == 0,
        None => true,
    }
}

/// `StartupApproved` encoding: enable writes `02 00 ..` (12 bytes); disable
/// writes `03 00 00 00` followed by the FILETIME (little-endian) it was
/// disabled at.
pub fn encode_approved(enabled: bool, filetime: u64) -> [u8; 12] {
    let mut data = [0_u8; 12];
    if enabled {
        data[0] = 0x02;
    } else {
        data[0] = 0x03;
        data[4..].copy_from_slice(&filetime.to_le_bytes());
    }
    data
}

fn now_filetime() -> u64 {
    const UNIX_EPOCH_AS_FILETIME: u64 = 116_444_736_000_000_000;
    let since_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    UNIX_EPOCH_AS_FILETIME + (since_unix.as_nanos() / 100) as u64
}

fn hive(scope: StartupScope) -> Hive {
    match scope {
        StartupScope::User => Hive::CurrentUser,
        StartupScope::Machine => Hive::LocalMachine,
    }
}

/// The source key (None for folders) and approval key for an entry.
fn keys(entry: &StartupEntryRef) -> Result<(Option<&'static str>, &'static str), AdapterError> {
    keys_for(entry.scope, entry.location)
}

fn keys_for(
    scope: StartupScope,
    location: StartupLocation,
) -> Result<(Option<&'static str>, &'static str), AdapterError> {
    match (scope, location) {
        (_, StartupLocation::Run) => Ok((Some(RUN_KEY), APPROVED_RUN_KEY)),
        (StartupScope::Machine, StartupLocation::Run32) => {
            Ok((Some(RUN32_KEY), APPROVED_RUN32_KEY))
        }
        (StartupScope::User, StartupLocation::Run32) => {
            Err(AdapterError::Unsupported(UnsupportedReason::NotPresent))
        }
        (_, StartupLocation::StartupFolder) => Ok((None, APPROVED_FOLDER_KEY)),
    }
}

fn same_name(left: &str, right: &str) -> bool {
    left == right || left.to_lowercase() == right.to_lowercase()
}

/// Re-resolve the entry against live enumeration; returns the name as it is
/// actually stored. Fails closed with `NotPresent`.
fn resolve<R: StartupReader + ?Sized>(
    reader: &R,
    entry: &StartupEntryRef,
) -> Result<(Hive, &'static str, String), AdapterError> {
    let (source, approved) = keys(entry)?;
    let hive = hive(entry.scope);
    let entries = match source {
        Some(path) => reader.registry_values(hive, path)?,
        None => reader.folder_files(entry.scope)?,
    };
    entries
        .into_iter()
        .map(|item| item.name)
        .find(|name| same_name(name, entry.name.as_str()))
        .map(|name| (hive, approved, name))
        .ok_or(AdapterError::Unsupported(UnsupportedReason::NotPresent))
}

pub(crate) fn observe_entry<R: StartupReader + ?Sized>(
    reader: &R,
    entry: &StartupEntryRef,
) -> Result<PriorState, AdapterError> {
    let (hive, approved, name) = resolve(reader, entry)?;
    let data = reader.approved(hive, approved, &name)?;
    Ok(PriorState::Enabled {
        enabled: decode_approved(data.as_deref()),
    })
}

/// Idempotent: no write when the entry is already in the target state.
pub(crate) fn set_entry<R: StartupReader + ?Sized, W: StartupWriter + ?Sized>(
    reader: &R,
    writer: &W,
    entry: &StartupEntryRef,
    enabled: bool,
) -> Result<(), AdapterError> {
    let (hive, approved, name) = resolve(reader, entry)?;
    let current = decode_approved(reader.approved(hive, approved, &name)?.as_deref());
    if current == enabled {
        return Ok(());
    }
    writer.set_approved(
        hive,
        approved,
        &name,
        &encode_approved(enabled, now_filetime()),
    )
}

fn location_label(location: StartupLocation) -> &'static str {
    match location {
        StartupLocation::Run => "Run key",
        StartupLocation::Run32 => "32-bit Run key",
        StartupLocation::StartupFolder => "Startup folder",
    }
}

/// Enables or disables startup entries via `StartupApproved`. `apply` only
/// writes the current user's entries; machine entries go to the elevated
/// helper (see [`elevated`]).
pub struct StartupAdapter<R = WindowsStartupStore, W = WindowsStartupStore> {
    reader: R,
    writer: W,
}

impl StartupAdapter {
    pub fn new() -> Self {
        Self {
            reader: WindowsStartupStore,
            writer: WindowsStartupStore,
        }
    }
}

impl Default for StartupAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl<R, W> StartupAdapter<R, W> {
    pub fn with(reader: R, writer: W) -> Self {
        Self { reader, writer }
    }
}

impl<R: StartupReader, W: StartupWriter> SystemAdapter for StartupAdapter<R, W> {
    fn describe(&self, change: &SystemChange) -> Result<ImpactSummary, AdapterError> {
        let SystemChange::SetStartupEntry { entry, enabled } = change else {
            return Err(AdapterError::Failed);
        };
        keys(entry)?;
        let (who, risk) = match entry.scope {
            StartupScope::User => ("your account", RiskLevel::Low),
            StartupScope::Machine => ("every user", RiskLevel::Medium),
        };
        let effect = if *enabled {
            format!("Starts automatically again when {who} signs in, from the next sign-in.")
        } else {
            format!(
                "Stops starting automatically when {who} signs in, from the next sign-in. \
                 The program and its startup entry are kept and can be re-enabled."
            )
        };
        Ok(ImpactSummary {
            component: format!(
                "Startup item \"{}\" ({})",
                entry.name,
                location_label(entry.location)
            ),
            effect,
            restart: RestartRequirement::None,
            risk,
        })
    }

    fn observe(&self, change: &SystemChange) -> Result<PriorState, AdapterError> {
        match change {
            SystemChange::SetStartupEntry { entry, .. } => observe_entry(&self.reader, entry),
            _ => Err(AdapterError::Failed),
        }
    }

    fn apply(&self, change: &SystemChange) -> Result<(), AdapterError> {
        match change {
            SystemChange::SetStartupEntry { entry, enabled }
                if entry.scope == StartupScope::User =>
            {
                set_entry(&self.reader, &self.writer, entry, *enabled)
            }
            _ => Err(AdapterError::Failed),
        }
    }
}

/// Where an inventory item comes from. Only `Run`, `Run32`, and
/// `StartupFolder` are toggleable.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StartupSource {
    Run,
    Run32,
    StartupFolder,
    RunOnce,
    LogonTask,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupItem {
    pub name: String,
    pub scope: StartupScope,
    /// Set for toggleable kinds; `None` for RunOnce and logon tasks.
    pub location: Option<StartupLocation>,
    pub source: StartupSource,
    /// Display only; never executed or written back.
    pub command: String,
    pub enabled: bool,
    pub toggleable: bool,
}

fn bounded(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

/// Everything that starts at sign-in, best effort per source.
pub fn list_startup_items() -> Vec<StartupItem> {
    list_with(&WindowsStartupStore)
}

pub fn list_with<R: StartupReader + ?Sized>(reader: &R) -> Vec<StartupItem> {
    use cleanup_core::system_change::EntryName;

    let mut items = Vec::new();
    let toggleable = [
        (StartupScope::User, StartupLocation::Run, StartupSource::Run),
        (
            StartupScope::Machine,
            StartupLocation::Run,
            StartupSource::Run,
        ),
        (
            StartupScope::Machine,
            StartupLocation::Run32,
            StartupSource::Run32,
        ),
        (
            StartupScope::User,
            StartupLocation::StartupFolder,
            StartupSource::StartupFolder,
        ),
        (
            StartupScope::Machine,
            StartupLocation::StartupFolder,
            StartupSource::StartupFolder,
        ),
    ];
    for (scope, location, source) in toggleable {
        let hive = hive(scope);
        let Ok((path, approved)) = keys_for(scope, location) else {
            continue;
        };
        let entries = match path {
            Some(path) => reader.registry_values(hive, path),
            None => reader.folder_files(scope),
        };
        for entry in entries.unwrap_or_default() {
            let approval = reader.approved(hive, approved, &entry.name);
            let valid_name = EntryName::parse(entry.name.clone()).is_ok();
            items.push(StartupItem {
                name: bounded(&entry.name, MAX_NAME_CHARS),
                scope,
                location: Some(location),
                source,
                command: bounded(&entry.command, MAX_COMMAND_CHARS),
                enabled: approval
                    .as_ref()
                    .map(|data| decode_approved(data.as_deref()))
                    .unwrap_or(true),
                toggleable: valid_name && approval.is_ok(),
            });
        }
    }
    for scope in [StartupScope::User, StartupScope::Machine] {
        for entry in reader
            .registry_values(hive(scope), RUN_ONCE_KEY)
            .unwrap_or_default()
        {
            items.push(StartupItem {
                name: bounded(&entry.name, MAX_NAME_CHARS),
                scope,
                location: None,
                source: StartupSource::RunOnce,
                command: bounded(&entry.command, MAX_COMMAND_CHARS),
                enabled: true,
                toggleable: false,
            });
        }
    }
    for task in reader.logon_tasks().unwrap_or_default() {
        items.push(StartupItem {
            name: bounded(&task.path, MAX_NAME_CHARS),
            scope: StartupScope::Machine,
            location: None,
            source: StartupSource::LogonTask,
            command: String::new(),
            enabled: task.enabled,
            toggleable: false,
        });
    }
    items.truncate(MAX_ITEMS);
    items
}

/// Elevated-helper side for machine-scope entries. Every call re-resolves
/// the entry against live enumeration inside the helper process.
pub mod elevated {
    use cleanup_core::system_change::{
        EntryName, PriorState, StartupEntryRef, StartupLocation, StartupScope,
    };

    use super::{StartupReader, StartupWriter, WindowsStartupStore, observe_entry, set_entry};
    use crate::{security::system_changes::HelperChange, system_change::AdapterError};

    fn machine_entry(location: StartupLocation, name: &EntryName) -> StartupEntryRef {
        StartupEntryRef {
            scope: StartupScope::Machine,
            location,
            name: name.clone(),
        }
    }

    pub fn observe(change: &HelperChange) -> Result<PriorState, AdapterError> {
        observe_with(&WindowsStartupStore, change)
    }

    pub fn apply(change: &HelperChange) -> Result<(), AdapterError> {
        apply_with(&WindowsStartupStore, &WindowsStartupStore, change)
    }

    pub(super) fn observe_with<R: StartupReader + ?Sized>(
        reader: &R,
        change: &HelperChange,
    ) -> Result<PriorState, AdapterError> {
        match change {
            HelperChange::SetMachineStartupEntry { location, name, .. } => {
                observe_entry(reader, &machine_entry(*location, name))
            }
            _ => Err(AdapterError::Failed),
        }
    }

    pub(super) fn apply_with<R: StartupReader + ?Sized, W: StartupWriter + ?Sized>(
        reader: &R,
        writer: &W,
        change: &HelperChange,
    ) -> Result<(), AdapterError> {
        match change {
            HelperChange::SetMachineStartupEntry {
                location,
                name,
                enabled,
            } => set_entry(reader, writer, &machine_entry(*location, name), *enabled),
            _ => Err(AdapterError::Failed),
        }
    }
}
