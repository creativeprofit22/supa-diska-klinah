//! Windows Update status (read-only, via the Windows Update Agent COM API) and
//! a small compiled catalog of documented Windows Update Group Policy values.
//!
//! Policy writes are helper-privileged and run only in the elevated helper
//! (`elevated`). Status reads and the detection trigger run at standard
//! integrity.

pub mod elevated;
mod wua;

#[cfg(test)]
mod tests;

use std::io;

use cleanup_core::system_change::{
    ImpactSummary, PriorState, RestartRequirement, RiskLevel, SystemChange, UnsupportedReason,
};
use serde::Serialize;

use crate::{
    os_info::{self, Edition, OsFacts},
    system_change::{AdapterError, SystemAdapter},
    win_registry::{Hive, RegistryData, RegistryKey},
};

pub use wua::WuaAgent;

const ERROR_ACCESS_DENIED: i32 = 5;

pub const WINDOWS_UPDATE_POLICY_KEY: &str = r"SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate";
pub const AU_POLICY_KEY: &str = r"SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate\AU";
const REBOOT_REQUIRED_KEY: &str =
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\RebootRequired";

// ---------------------------------------------------------------------------
// Catalog
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Allowed {
    /// Discrete values, each with a plain-language label.
    Set(&'static [(u32, &'static str)]),
    /// Inclusive range.
    Range { min: u32, max: u32 },
}

impl Allowed {
    pub fn permits(self, value: u32) -> bool {
        match self {
            Self::Set(values) => values.iter().any(|(allowed, _)| *allowed == value),
            Self::Range { min, max } => (min..=max).contains(&value),
        }
    }

    fn label(self, value: u32) -> Option<&'static str> {
        match self {
            Self::Set(values) => values
                .iter()
                .find(|(allowed, _)| *allowed == value)
                .map(|(_, label)| *label),
            Self::Range { .. } => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PolicyEntry {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub key: &'static str,
    pub value_name: &'static str,
    pub allowed: Allowed,
}

pub const POLICY_CATALOG: &[PolicyEntry] = &[
    PolicyEntry {
        id: "au-options",
        label: "Automatic update behavior",
        description: "How Windows downloads and installs updates (Configure Automatic Updates).",
        key: AU_POLICY_KEY,
        value_name: "AUOptions",
        allowed: Allowed::Set(&[
            (2, "Notify before download"),
            (3, "Download automatically and notify to install"),
            (4, "Download automatically and install on a schedule"),
        ]),
    },
    PolicyEntry {
        id: "no-auto-update",
        label: "Turn off automatic updates",
        description: "When on, Windows does not check for or install updates automatically.",
        key: AU_POLICY_KEY,
        value_name: "NoAutoUpdate",
        allowed: Allowed::Set(&[(0, "Automatic updates on"), (1, "Automatic updates off")]),
    },
    PolicyEntry {
        id: "no-auto-reboot-with-users",
        label: "No automatic restart with signed-in users",
        description: "When on, Windows does not restart automatically while someone is signed in.",
        key: AU_POLICY_KEY,
        value_name: "NoAutoRebootWithLoggedOnUsers",
        allowed: Allowed::Set(&[
            (0, "Restart automatically when needed"),
            (1, "Wait for signed-in users"),
        ]),
    },
    PolicyEntry {
        id: "defer-feature-updates-days",
        label: "Defer feature updates (days)",
        description: "Number of days to delay feature updates after release.",
        key: WINDOWS_UPDATE_POLICY_KEY,
        value_name: "DeferFeatureUpdatesPeriodInDays",
        allowed: Allowed::Range { min: 0, max: 365 },
    },
];

pub fn resolve(id: &str) -> Result<&'static PolicyEntry, AdapterError> {
    POLICY_CATALOG
        .iter()
        .find(|entry| entry.id == id)
        .ok_or(AdapterError::Unsupported(UnsupportedReason::NotPresent))
}

/// Resolve an identifier and check the target value against the catalog.
/// A disallowed value is refused as not part of the catalog.
fn resolve_target(id: &str, value: Option<u32>) -> Result<&'static PolicyEntry, AdapterError> {
    let entry = resolve(id)?;
    match value {
        Some(value) if !entry.allowed.permits(value) => {
            Err(AdapterError::Unsupported(UnsupportedReason::NotPresent))
        }
        _ => Ok(entry),
    }
}

/// Edition and management gating shared by the adapter and the helper.
pub fn policy_support(os: &OsFacts) -> Result<(), UnsupportedReason> {
    if !os.is_supported() {
        return Err(UnsupportedReason::OsVersion);
    }
    if !os.edition.honors_policies() {
        return Err(UnsupportedReason::EditionUnsupported);
    }
    if os.managed {
        return Err(UnsupportedReason::ManagedDevice);
    }
    Ok(())
}

fn gate(os: &OsFacts) -> Result<(), AdapterError> {
    policy_support(os).map_err(AdapterError::Unsupported)
}

// ---------------------------------------------------------------------------
// Policy store
// ---------------------------------------------------------------------------

pub trait PolicyStore: Send + Sync {
    /// `Ok(None)` when the key or value is absent.
    fn read(&self, key: &'static str, name: &'static str) -> io::Result<Option<RegistryData>>;
    /// `None` deletes the value.
    fn write(&self, key: &'static str, name: &'static str, value: Option<u32>) -> io::Result<()>;
}

pub struct RegistryPolicyStore;

impl PolicyStore for RegistryPolicyStore {
    fn read(&self, key: &'static str, name: &'static str) -> io::Result<Option<RegistryData>> {
        match RegistryKey::open_read(Hive::LocalMachine, key)? {
            Some(key) => key.value(name),
            None => Ok(None),
        }
    }

    fn write(&self, key: &'static str, name: &'static str, value: Option<u32>) -> io::Result<()> {
        match value {
            Some(value) => {
                RegistryKey::create_write(Hive::LocalMachine, key)?.set_dword(name, value)
            }
            None => match RegistryKey::open_write(Hive::LocalMachine, key)? {
                Some(key) => key.delete_value(name),
                None => Ok(()),
            },
        }
    }
}

fn map_io(error: io::Error) -> AdapterError {
    if error.raw_os_error() == Some(ERROR_ACCESS_DENIED)
        || error.kind() == io::ErrorKind::PermissionDenied
    {
        AdapterError::Denied
    } else {
        AdapterError::Failed
    }
}

fn read_entry(store: &dyn PolicyStore, entry: &PolicyEntry) -> Result<Option<u32>, AdapterError> {
    match store.read(entry.key, entry.value_name).map_err(map_io)? {
        None => Ok(None),
        Some(RegistryData::Dword(value)) => Ok(Some(value)),
        Some(_) => Err(AdapterError::Unsupported(UnsupportedReason::NotPresent)),
    }
}

/// Gate, re-resolve, and read the current value.
pub(crate) fn observe_policy(
    os: &OsFacts,
    store: &dyn PolicyStore,
    setting_id: &str,
    value: Option<u32>,
) -> Result<PriorState, AdapterError> {
    gate(os)?;
    let entry = resolve_target(setting_id, value)?;
    Ok(PriorState::RegistryValue {
        value: read_entry(store, entry)?,
    })
}

/// Gate, re-resolve, and write (or delete) the value. Idempotent.
pub(crate) fn apply_policy(
    os: &OsFacts,
    store: &dyn PolicyStore,
    setting_id: &str,
    value: Option<u32>,
) -> Result<(), AdapterError> {
    gate(os)?;
    let entry = resolve_target(setting_id, value)?;
    if read_entry(store, entry)? == value {
        return Ok(());
    }
    store
        .write(entry.key, entry.value_name, value)
        .map_err(map_io)
}

// ---------------------------------------------------------------------------
// Adapter
// ---------------------------------------------------------------------------

pub struct UpdatesAdapter {
    os: OsFacts,
    store: Box<dyn PolicyStore>,
}

impl UpdatesAdapter {
    pub fn new() -> Self {
        Self::with(os_info::current(), Box::new(RegistryPolicyStore))
    }

    pub fn with(os: OsFacts, store: Box<dyn PolicyStore>) -> Self {
        Self { os, store }
    }
}

impl Default for UpdatesAdapter {
    fn default() -> Self {
        Self::new()
    }
}

fn policy_parts(change: &SystemChange) -> Result<(&str, Option<u32>), AdapterError> {
    match change {
        SystemChange::SetWindowsUpdatePolicy { setting_id, value } => {
            Ok((setting_id.as_str(), *value))
        }
        _ => Err(AdapterError::Failed),
    }
}

impl SystemAdapter for UpdatesAdapter {
    fn describe(&self, change: &SystemChange) -> Result<ImpactSummary, AdapterError> {
        let (id, value) = policy_parts(change)?;
        let entry = resolve_target(id, value)?;
        let effect = match value {
            None => format!(
                "Remove the {} policy value so Windows uses its default behavior",
                entry.value_name
            ),
            Some(value) => match entry.allowed.label(value) {
                Some(label) => format!("Set {} to {value}: {label}", entry.value_name),
                None => format!("Set {} to {value} days", entry.value_name),
            },
        };
        let risk = match (entry.value_name, value) {
            ("NoAutoUpdate", Some(1)) => RiskLevel::High,
            (_, None) => RiskLevel::Low,
            _ => RiskLevel::Medium,
        };
        Ok(ImpactSummary {
            component: format!("Windows Update policy: {}", entry.label),
            effect,
            restart: RestartRequirement::None,
            risk,
        })
    }

    fn observe(&self, change: &SystemChange) -> Result<PriorState, AdapterError> {
        let (id, value) = policy_parts(change)?;
        observe_policy(&self.os, self.store.as_ref(), id, value)
    }

    fn apply(&self, _change: &SystemChange) -> Result<(), AdapterError> {
        // Policy writes are helper-privileged and never applied here.
        Err(AdapterError::Failed)
    }
}

// ---------------------------------------------------------------------------
// Status inventory
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AutoUpdateInfo {
    pub service_enabled: Option<bool>,
    pub last_search_success: Option<i64>,
    pub last_install_success: Option<i64>,
}

/// Read access to the Windows Update Agent, injectable for tests.
pub trait UpdateAgent {
    fn automatic_updates(&self) -> Result<AutoUpdateInfo, AdapterError>;
    fn system_reboot_required(&self) -> Result<bool, AdapterError>;
    /// Whether the `...\Auto Update\RebootRequired` key exists.
    fn reboot_pending_key(&self) -> bool;
    /// Benign scan trigger (`IAutomaticUpdates::DetectNow`).
    fn detect_now(&self) -> Result<(), AdapterError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AllowedOption {
    pub value: u32,
    pub label: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AllowedValues {
    Options { options: Vec<AllowedOption> },
    Range { min: u32, max: u32 },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyStatus {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    /// Current DWORD, `null` when unset (or unreadable / not a DWORD).
    pub current: Option<u32>,
    pub allowed: AllowedValues,
    /// The value is configured and this device honors it.
    pub applied: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub api_available: bool,
    pub service_enabled: Option<bool>,
    pub last_search_success: Option<i64>,
    pub last_install_success: Option<i64>,
    pub reboot_required: Option<bool>,
    pub edition: Edition,
    pub managed: bool,
    pub policy_supported: bool,
    pub unsupported_reason: Option<UnsupportedReason>,
    pub policies: Vec<PolicyStatus>,
}

fn allowed_values(allowed: Allowed) -> AllowedValues {
    match allowed {
        Allowed::Set(values) => AllowedValues::Options {
            options: values
                .iter()
                .map(|(value, label)| AllowedOption {
                    value: *value,
                    label,
                })
                .collect(),
        },
        Allowed::Range { min, max } => AllowedValues::Range { min, max },
    }
}

fn is_api_missing(result: &Result<impl Sized, AdapterError>) -> bool {
    matches!(
        result,
        Err(AdapterError::Unsupported(UnsupportedReason::ApiUnavailable))
    )
}

/// Read-only status. Never calls `DetectNow`.
pub fn status_with(agent: &dyn UpdateAgent, os: &OsFacts, store: &dyn PolicyStore) -> UpdateStatus {
    let automatic = agent.automatic_updates();
    let system_reboot = agent.system_reboot_required();
    let api_available = !is_api_missing(&automatic) && !is_api_missing(&system_reboot);
    let info = automatic.unwrap_or_default();
    let reboot_required = if agent.reboot_pending_key() {
        Some(true)
    } else {
        system_reboot.ok()
    };
    let support = policy_support(os);
    let policies = POLICY_CATALOG
        .iter()
        .map(|entry| {
            let current = read_entry(store, entry).ok().flatten();
            PolicyStatus {
                id: entry.id,
                label: entry.label,
                description: entry.description,
                current,
                allowed: allowed_values(entry.allowed),
                applied: support.is_ok() && current.is_some(),
            }
        })
        .collect();
    UpdateStatus {
        api_available,
        service_enabled: info.service_enabled,
        last_search_success: info.last_search_success,
        last_install_success: info.last_install_success,
        reboot_required,
        edition: os.edition,
        managed: os.managed,
        policy_supported: support.is_ok(),
        unsupported_reason: support.err(),
        policies,
    }
}

/// Current Windows Update status for this machine (read-only).
pub fn windows_update_status() -> UpdateStatus {
    status_with(&WuaAgent, &os_info::current(), &RegistryPolicyStore)
}

pub fn trigger_detection_with(agent: &dyn UpdateAgent) -> Result<(), AdapterError> {
    agent.detect_now()
}

/// Ask Windows Update to start a scan. Allowed at standard integrity.
pub fn trigger_detection() -> Result<(), AdapterError> {
    trigger_detection_with(&WuaAgent)
}

/// OLE Automation date (days since 1899-12-30, UTC for WUA) to unix seconds.
/// Dates before the unix epoch or non-finite values mean "never".
pub fn ole_date_to_unix(days: f64) -> Option<i64> {
    const UNIX_EPOCH_OLE_DAYS: f64 = 25_569.0;
    if !days.is_finite() {
        return None;
    }
    let seconds = ((days - UNIX_EPOCH_OLE_DAYS) * 86_400.0).round();
    if !(0.0..=(i64::MAX as f64)).contains(&seconds) {
        return None;
    }
    Some(seconds as i64)
}

fn reboot_key_exists() -> bool {
    matches!(
        RegistryKey::open_read(Hive::LocalMachine, REBOOT_REQUIRED_KEY),
        Ok(Some(_))
    )
}
