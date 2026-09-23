//! Platform-neutral contract for reviewable Windows system changes.
//!
//! Every mutation offered by the system-management modules is expressed as a
//! closed [`SystemChange`] value carrying catalog identifiers and bounded
//! values only. Adapters observe a [`PriorState`] during preview; execution
//! compares against it, and the journal records it so rollback never depends
//! on caller-supplied data.

use std::fmt;

use serde::{Deserialize, Serialize};

pub const MAX_PLAN_CHANGES: usize = 32;
pub const MAX_HOSTS_LINE_OPS: usize = 64;
pub const MAX_ENTRY_NAME_UTF16: usize = 256;
pub const MAX_RESTORE_DESCRIPTION_UTF16: usize = 128;
pub const MAX_JOURNAL_ENTRIES: usize = 500;
pub const JOURNAL_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContractError {
    InvalidCatalogId,
    InvalidEntryName,
    InvalidDriverPackage,
    InvalidScheduleId,
    InvalidDigest,
    InvalidPowerScheme,
    InvalidDescription,
    InvalidSchedule,
    TooManyLineOps,
    EmptyLineOps,
    DuplicateLine,
    UnsupportedJournalVersion,
}

impl fmt::Display for ContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidCatalogId => "catalog identifier is invalid",
            Self::InvalidEntryName => {
                "entry name is empty, too long, or contains control characters"
            }
            Self::InvalidDriverPackage => "driver package must be named oem<number>.inf",
            Self::InvalidScheduleId => "schedule identifier must be a lowercase UUID",
            Self::InvalidDigest => "digest must be 64 lowercase hexadecimal characters",
            Self::InvalidPowerScheme => "power scheme must be a lowercase GUID",
            Self::InvalidDescription => {
                "description is empty, too long, or contains control characters"
            }
            Self::InvalidSchedule => "schedule time is out of range",
            Self::TooManyLineOps => "too many hosts line operations",
            Self::EmptyLineOps => "hosts edit must contain at least one line operation",
            Self::DuplicateLine => "hosts edit repeats a line",
            Self::UnsupportedJournalVersion => "system-change journal version is unsupported",
        })
    }
}

impl std::error::Error for ContractError {}

macro_rules! validated_string {
    ($name:ident, $validate:path, $error:expr) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: impl Into<String>) -> Result<Self, ContractError> {
                let value = value.into();
                if $validate(&value) {
                    Ok(Self(value))
                } else {
                    Err($error)
                }
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = ContractError;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::parse(value)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

fn is_catalog_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0].is_ascii_lowercase()
        && bytes.iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'.')
        })
}

fn is_entry_name(value: &str) -> bool {
    !value.trim().is_empty()
        && value.encode_utf16().count() <= MAX_ENTRY_NAME_UTF16
        && !value.chars().any(char::is_control)
}

fn is_driver_package(value: &str) -> bool {
    value
        .strip_prefix("oem")
        .and_then(|rest| rest.strip_suffix(".inf"))
        .is_some_and(|digits| {
            (1..=5).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit())
        })
}

fn is_lower_hex(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_guid(value: &str) -> bool {
    let groups: Vec<_> = value.split('-').collect();
    groups.len() == 5
        && groups.iter().map(|group| group.len()).eq([8, 4, 4, 4, 12])
        && groups.iter().all(|group| is_lower_hex(group))
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && is_lower_hex(value)
}

fn is_description(value: &str) -> bool {
    !value.trim().is_empty()
        && value.encode_utf16().count() <= MAX_RESTORE_DESCRIPTION_UTF16
        && !value.chars().any(char::is_control)
}

validated_string!(CatalogId, is_catalog_id, ContractError::InvalidCatalogId);
validated_string!(EntryName, is_entry_name, ContractError::InvalidEntryName);
validated_string!(
    DriverPackageName,
    is_driver_package,
    ContractError::InvalidDriverPackage
);
validated_string!(ScheduleId, is_guid, ContractError::InvalidScheduleId);
validated_string!(Sha256Digest, is_sha256, ContractError::InvalidDigest);
validated_string!(PowerSchemeId, is_guid, ContractError::InvalidPowerScheme);
validated_string!(
    ChangeDescription,
    is_description,
    ContractError::InvalidDescription
);

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SystemModule {
    Startup,
    Services,
    Drivers,
    Firewall,
    Hosts,
    Privacy,
    Power,
    Restore,
    Updates,
    Scheduler,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StartupScope {
    User,
    Machine,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StartupLocation {
    Run,
    Run32,
    StartupFolder,
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartupEntryRef {
    pub scope: StartupScope,
    pub location: StartupLocation,
    pub name: EntryName,
}

/// Start types the application may set. Boot and system start types are
/// observable but never writable.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ServiceStartType {
    Automatic,
    Manual,
    Disabled,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ServiceStartState {
    Boot,
    System,
    Automatic,
    Manual,
    Disabled,
}

impl From<ServiceStartType> for ServiceStartState {
    fn from(value: ServiceStartType) -> Self {
        match value {
            ServiceStartType::Automatic => Self::Automatic,
            ServiceStartType::Manual => Self::Manual,
            ServiceStartType::Disabled => Self::Disabled,
        }
    }
}

impl ServiceStartState {
    pub fn writable(self) -> Option<ServiceStartType> {
        match self {
            Self::Automatic => Some(ServiceStartType::Automatic),
            Self::Manual => Some(ServiceStartType::Manual),
            Self::Disabled => Some(ServiceStartType::Disabled),
            Self::Boot | Self::System => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FirewallProfile {
    Domain,
    Private,
    Public,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HostsLineAction {
    /// Comment out an active mapping.
    Disable,
    /// Uncomment a mapping this application previously disabled.
    Restore,
}

impl HostsLineAction {
    pub fn inverse(self) -> Self {
        match self {
            Self::Disable => Self::Restore,
            Self::Restore => Self::Disable,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostsLineOp {
    pub line: u32,
    pub action: HostsLineAction,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Weekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ScheduleCadence {
    Daily { hour: u8, minute: u8 },
    Weekly { day: Weekday, hour: u8, minute: u8 },
}

impl ScheduleCadence {
    pub fn validate(&self) -> Result<(), ContractError> {
        let (hour, minute) = match *self {
            Self::Daily { hour, minute } | Self::Weekly { hour, minute, .. } => (hour, minute),
        };
        if hour < 24 && minute < 60 {
            Ok(())
        } else {
            Err(ContractError::InvalidSchedule)
        }
    }
}

/// A single reviewable mutation. Values are identifiers into compiled
/// catalogs or bounded scalars; there is no variant carrying a path, command
/// line, registry key, or executable.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum SystemChange {
    SetStartupEntry {
        entry: StartupEntryRef,
        enabled: bool,
    },
    SetServiceStartType {
        catalog_id: CatalogId,
        start_type: ServiceStartType,
    },
    SetUserSetting {
        setting_id: CatalogId,
        value: Option<u32>,
    },
    SetMachineSetting {
        setting_id: CatalogId,
        value: Option<u32>,
    },
    SetSystemTaskEnabled {
        catalog_id: CatalogId,
        enabled: bool,
    },
    SetWindowsUpdatePolicy {
        setting_id: CatalogId,
        value: Option<u32>,
    },
    SetFirewallRuleEnabled {
        rule_name: EntryName,
        enabled: bool,
    },
    SetFirewallProfileEnabled {
        profile: FirewallProfile,
        enabled: bool,
    },
    SetHibernation {
        enabled: bool,
    },
    SetActivePowerScheme {
        scheme: PowerSchemeId,
    },
    DeleteDriverPackage {
        published_name: DriverPackageName,
    },
    EditHosts {
        line_ops: Vec<HostsLineOp>,
    },
    CreateRestorePoint {
        description: ChangeDescription,
    },
    UpsertScanSchedule {
        schedule_id: ScheduleId,
        cadence: ScheduleCadence,
    },
    RemoveScanSchedule {
        schedule_id: ScheduleId,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Privilege {
    Standard,
    Helper,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Reversibility {
    Reversible,
    ReversibleWithBackup,
    Irreversible { reason: String },
}

impl Reversibility {
    pub fn is_reversible(&self) -> bool {
        !matches!(self, Self::Irreversible { .. })
    }
}

impl SystemChange {
    pub fn module(&self) -> SystemModule {
        match self {
            Self::SetStartupEntry { .. } => SystemModule::Startup,
            Self::SetServiceStartType { .. } => SystemModule::Services,
            Self::SetUserSetting { .. }
            | Self::SetMachineSetting { .. }
            | Self::SetSystemTaskEnabled { .. } => SystemModule::Privacy,
            Self::SetWindowsUpdatePolicy { .. } => SystemModule::Updates,
            Self::SetFirewallRuleEnabled { .. } | Self::SetFirewallProfileEnabled { .. } => {
                SystemModule::Firewall
            }
            Self::SetHibernation { .. } | Self::SetActivePowerScheme { .. } => SystemModule::Power,
            Self::DeleteDriverPackage { .. } => SystemModule::Drivers,
            Self::EditHosts { .. } => SystemModule::Hosts,
            Self::CreateRestorePoint { .. } => SystemModule::Restore,
            Self::UpsertScanSchedule { .. } | Self::RemoveScanSchedule { .. } => {
                SystemModule::Scheduler
            }
        }
    }

    pub fn privilege(&self) -> Privilege {
        match self {
            Self::SetStartupEntry { entry, .. } if entry.scope == StartupScope::User => {
                Privilege::Standard
            }
            Self::SetUserSetting { .. }
            | Self::SetActivePowerScheme { .. }
            | Self::UpsertScanSchedule { .. }
            | Self::RemoveScanSchedule { .. } => Privilege::Standard,
            _ => Privilege::Helper,
        }
    }

    pub fn reversibility(&self) -> Reversibility {
        match self {
            Self::DeleteDriverPackage { .. } => Reversibility::Irreversible {
                reason: "Windows removes the driver package from the driver store; reinstalling requires the original installer.".into(),
            },
            Self::CreateRestorePoint { .. } => Reversibility::Irreversible {
                reason: "Creates a new restore point; Windows manages its retention.".into(),
            },
            Self::EditHosts { .. } => Reversibility::ReversibleWithBackup,
            _ => Reversibility::Reversible,
        }
    }

    /// Structural validation beyond what the field types enforce.
    pub fn validate(&self) -> Result<(), ContractError> {
        match self {
            Self::EditHosts { line_ops } => {
                if line_ops.is_empty() {
                    return Err(ContractError::EmptyLineOps);
                }
                if line_ops.len() > MAX_HOSTS_LINE_OPS {
                    return Err(ContractError::TooManyLineOps);
                }
                let mut lines: Vec<_> = line_ops.iter().map(|op| op.line).collect();
                lines.sort_unstable();
                if lines.windows(2).any(|pair| pair[0] == pair[1]) {
                    return Err(ContractError::DuplicateLine);
                }
                Ok(())
            }
            Self::UpsertScanSchedule { cadence, .. } => cadence.validate(),
            _ => Ok(()),
        }
    }

    /// Whether the observed state already equals this change's target.
    /// `None` means the adapter must decide (hosts edits, driver deletion,
    /// restore points).
    pub fn is_satisfied_by(&self, state: &PriorState) -> Option<bool> {
        match (self, state) {
            (
                Self::SetStartupEntry { enabled, .. }
                | Self::SetSystemTaskEnabled { enabled, .. }
                | Self::SetFirewallRuleEnabled { enabled, .. }
                | Self::SetFirewallProfileEnabled { enabled, .. }
                | Self::SetHibernation { enabled },
                PriorState::Enabled { enabled: current },
            ) => Some(enabled == current),
            (Self::SetServiceStartType { start_type, .. }, PriorState::ServiceStart { start }) => {
                Some(ServiceStartState::from(*start_type) == *start)
            }
            (
                Self::SetUserSetting { value, .. }
                | Self::SetMachineSetting { value, .. }
                | Self::SetWindowsUpdatePolicy { value, .. },
                PriorState::RegistryValue { value: current },
            ) => Some(value == current),
            (
                Self::SetActivePowerScheme { scheme },
                PriorState::PowerScheme { scheme: current },
            ) => Some(scheme == current),
            (Self::DeleteDriverPackage { .. }, PriorState::DriverPackage { present }) => {
                Some(!present)
            }
            (
                Self::UpsertScanSchedule { cadence, .. },
                PriorState::Schedule { cadence: current },
            ) => Some(current.as_ref() == Some(cadence)),
            (Self::RemoveScanSchedule { .. }, PriorState::Schedule { cadence }) => {
                Some(cadence.is_none())
            }
            _ => None,
        }
    }

    /// Build the change that restores `prior`. Irreversible changes and
    /// mismatched state shapes return an error instead of a guess.
    pub fn inverse(&self, prior: &PriorState) -> Result<Option<SystemChange>, InverseError> {
        if let Reversibility::Irreversible { .. } = self.reversibility() {
            return Err(InverseError::Irreversible);
        }
        let inverse = match (self, prior) {
            (Self::SetStartupEntry { entry, .. }, PriorState::Enabled { enabled }) => {
                Self::SetStartupEntry {
                    entry: entry.clone(),
                    enabled: *enabled,
                }
            }
            (Self::SetServiceStartType { catalog_id, .. }, PriorState::ServiceStart { start }) => {
                Self::SetServiceStartType {
                    catalog_id: catalog_id.clone(),
                    start_type: start.writable().ok_or(InverseError::PriorNotWritable)?,
                }
            }
            (Self::SetUserSetting { setting_id, .. }, PriorState::RegistryValue { value }) => {
                Self::SetUserSetting {
                    setting_id: setting_id.clone(),
                    value: *value,
                }
            }
            (Self::SetMachineSetting { setting_id, .. }, PriorState::RegistryValue { value }) => {
                Self::SetMachineSetting {
                    setting_id: setting_id.clone(),
                    value: *value,
                }
            }
            (
                Self::SetWindowsUpdatePolicy { setting_id, .. },
                PriorState::RegistryValue { value },
            ) => Self::SetWindowsUpdatePolicy {
                setting_id: setting_id.clone(),
                value: *value,
            },
            (Self::SetSystemTaskEnabled { catalog_id, .. }, PriorState::Enabled { enabled }) => {
                Self::SetSystemTaskEnabled {
                    catalog_id: catalog_id.clone(),
                    enabled: *enabled,
                }
            }
            (Self::SetFirewallRuleEnabled { rule_name, .. }, PriorState::Enabled { enabled }) => {
                Self::SetFirewallRuleEnabled {
                    rule_name: rule_name.clone(),
                    enabled: *enabled,
                }
            }
            (Self::SetFirewallProfileEnabled { profile, .. }, PriorState::Enabled { enabled }) => {
                Self::SetFirewallProfileEnabled {
                    profile: *profile,
                    enabled: *enabled,
                }
            }
            (Self::SetHibernation { .. }, PriorState::Enabled { enabled }) => {
                Self::SetHibernation { enabled: *enabled }
            }
            (Self::SetActivePowerScheme { .. }, PriorState::PowerScheme { scheme }) => {
                Self::SetActivePowerScheme {
                    scheme: scheme.clone(),
                }
            }
            (Self::EditHosts { line_ops }, PriorState::Hosts { .. }) => Self::EditHosts {
                line_ops: line_ops
                    .iter()
                    .map(|op| HostsLineOp {
                        line: op.line,
                        action: op.action.inverse(),
                    })
                    .collect(),
            },
            (Self::UpsertScanSchedule { schedule_id, .. }, PriorState::Schedule { cadence }) => {
                match cadence {
                    Some(cadence) => Self::UpsertScanSchedule {
                        schedule_id: schedule_id.clone(),
                        cadence: *cadence,
                    },
                    None => Self::RemoveScanSchedule {
                        schedule_id: schedule_id.clone(),
                    },
                }
            }
            (Self::RemoveScanSchedule { schedule_id }, PriorState::Schedule { cadence }) => {
                match cadence {
                    Some(cadence) => Self::UpsertScanSchedule {
                        schedule_id: schedule_id.clone(),
                        cadence: *cadence,
                    },
                    None => return Ok(None),
                }
            }
            _ => return Err(InverseError::PriorMismatch),
        };
        Ok(Some(inverse))
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum InverseError {
    Irreversible,
    PriorMismatch,
    PriorNotWritable,
}

impl fmt::Display for InverseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Irreversible => "this change cannot be rolled back",
            Self::PriorMismatch => "recorded prior state does not match the change",
            Self::PriorNotWritable => "recorded prior state cannot be restored by this application",
        })
    }
}

impl std::error::Error for InverseError {}

/// State observed before a change. Serialized into plans, helper responses,
/// and the journal.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum PriorState {
    Enabled { enabled: bool },
    ServiceStart { start: ServiceStartState },
    RegistryValue { value: Option<u32> },
    PowerScheme { scheme: PowerSchemeId },
    DriverPackage { present: bool },
    Hosts { sha256: Sha256Digest },
    Schedule { cadence: Option<ScheduleCadence> },
    NotApplicable,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RestartRequirement {
    None,
    SignOut,
    Reboot,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImpactSummary {
    pub component: String,
    pub effect: String,
    pub restart: RestartRequirement,
    pub risk: RiskLevel,
}

/// A change as presented for review: the mutation, what it affects, whether
/// it can be undone, and the state it expects to find.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlannedChange {
    pub change: SystemChange,
    pub module: SystemModule,
    pub privilege: Privilege,
    pub reversibility: Reversibility,
    pub impact: ImpactSummary,
    pub expected_prior: PriorState,
    pub inverse: Option<SystemChange>,
}

impl PlannedChange {
    pub fn new(
        change: SystemChange,
        impact: ImpactSummary,
        expected_prior: PriorState,
    ) -> Result<Self, ContractError> {
        change.validate()?;
        let inverse = change.inverse(&expected_prior).ok().flatten();
        Ok(Self {
            module: change.module(),
            privilege: change.privilege(),
            reversibility: change.reversibility(),
            change,
            impact,
            expected_prior,
            inverse,
        })
    }

    /// One plain line for native confirmation dialogs.
    pub fn summary_line(&self) -> String {
        let reversibility = match &self.reversibility {
            Reversibility::Reversible => "can be undone".to_owned(),
            Reversibility::ReversibleWithBackup => "can be undone from a backup".to_owned(),
            Reversibility::Irreversible { reason } => format!("CANNOT be undone: {reason}"),
        };
        let restart = match self.impact.restart {
            RestartRequirement::None => "",
            RestartRequirement::SignOut => "; sign-out required",
            RestartRequirement::Reboot => "; restart required",
        };
        let admin = if self.privilege == Privilege::Helper {
            "; needs administrator"
        } else {
            ""
        };
        format!(
            "{}: {} ({reversibility}{restart}{admin})",
            self.impact.component, self.impact.effect
        )
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UnsupportedReason {
    ApiUnavailable,
    OsVersion,
    EditionUnsupported,
    ManagedDevice,
    NotPresent,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FailureCode {
    SystemError,
    HelperUnavailable,
    Timeout,
    InvalidRequest,
    Interrupted,
    NotAttempted,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase", deny_unknown_fields)]
pub enum ChangeOutcome {
    Applied,
    AlreadyApplied,
    StateChanged,
    Unsupported { reason: UnsupportedReason },
    Denied,
    Failed { code: FailureCode },
}

impl ChangeOutcome {
    pub fn modified_system(self) -> bool {
        matches!(self, Self::Applied)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangeResult {
    pub change: SystemChange,
    pub outcome: ChangeOutcome,
    pub journal_entry_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionReport {
    pub plan_id: String,
    pub results: Vec<ChangeResult>,
}

impl ExecutionReport {
    pub fn all_succeeded(&self) -> bool {
        self.results.iter().all(|result| {
            matches!(
                result.outcome,
                ChangeOutcome::Applied | ChangeOutcome::AlreadyApplied
            )
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JournalEntry {
    pub id: String,
    pub plan_id: String,
    pub recorded_at: u64,
    pub change: SystemChange,
    pub prior: PriorState,
    pub reversibility: Reversibility,
    pub inverse: Option<SystemChange>,
    /// `None` means an intent record without an outcome: the process stopped
    /// between recording intent and finishing the change.
    pub outcome: Option<ChangeOutcome>,
    pub rolled_back_by: Option<String>,
}

impl JournalEntry {
    pub fn is_interrupted(&self) -> bool {
        self.outcome.is_none()
    }

    pub fn rollback_status(&self) -> RollbackStatus {
        if self.rolled_back_by.is_some() {
            RollbackStatus::AlreadyRolledBack
        } else if !self.reversibility.is_reversible() {
            RollbackStatus::Irreversible
        } else if self.outcome.is_none() {
            RollbackStatus::Interrupted
        } else if !matches!(self.outcome, Some(ChangeOutcome::Applied)) || self.inverse.is_none() {
            RollbackStatus::NothingApplied
        } else {
            RollbackStatus::Available
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RollbackStatus {
    Available,
    AlreadyRolledBack,
    Irreversible,
    Interrupted,
    NothingApplied,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Journal {
    pub version: u32,
    pub entries: Vec<JournalEntry>,
}

impl Journal {
    pub fn empty() -> Self {
        Self {
            version: JOURNAL_VERSION,
            entries: Vec::new(),
        }
    }

    pub fn from_json(bytes: &[u8]) -> Result<Self, JournalParseError> {
        let journal: Self =
            serde_json::from_slice(bytes).map_err(|_| JournalParseError::Malformed)?;
        if journal.version != JOURNAL_VERSION {
            return Err(JournalParseError::UnsupportedVersion);
        }
        Ok(journal)
    }

    pub fn record_intent(&mut self, entry: JournalEntry) {
        self.entries.push(entry);
        self.trim();
    }

    pub fn record_outcome(
        &mut self,
        id: &str,
        outcome: ChangeOutcome,
        prior: Option<PriorState>,
    ) -> bool {
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) else {
            return false;
        };
        if let Some(prior) = prior {
            entry.inverse = entry.change.inverse(&prior).ok().flatten();
            entry.prior = prior;
        }
        entry.outcome = Some(outcome);
        true
    }

    pub fn mark_rolled_back(&mut self, id: &str, by: &str) -> bool {
        match self.entries.iter_mut().find(|entry| entry.id == id) {
            Some(entry) => {
                entry.rolled_back_by = Some(by.to_owned());
                true
            }
            None => false,
        }
    }

    pub fn find(&self, id: &str) -> Option<&JournalEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    pub fn interrupted(&self) -> impl Iterator<Item = &JournalEntry> {
        self.entries.iter().filter(|entry| entry.is_interrupted())
    }

    /// Keep the newest entries, but never drop interrupted intent records:
    /// those are the evidence recovery needs.
    fn trim(&mut self) {
        while self.entries.len() > MAX_JOURNAL_ENTRIES {
            match self
                .entries
                .iter()
                .position(|entry| !entry.is_interrupted())
            {
                Some(index) => {
                    self.entries.remove(index);
                }
                None => break,
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JournalParseError {
    Malformed,
    UnsupportedVersion,
}

impl fmt::Display for JournalParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Malformed => "system-change journal is malformed",
            Self::UnsupportedVersion => "system-change journal version is unsupported",
        })
    }
}

impl std::error::Error for JournalParseError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: &str) -> CatalogId {
        CatalogId::parse(value).unwrap()
    }

    fn impact() -> ImpactSummary {
        ImpactSummary {
            component: "Diagnostics Tracking".into(),
            effect: "Stops telemetry upload".into(),
            restart: RestartRequirement::None,
            risk: RiskLevel::Low,
        }
    }

    fn all_changes() -> Vec<SystemChange> {
        let schedule = ScheduleId::parse("0f8fad5b-d9cb-469f-a165-70867728950e").unwrap();
        vec![
            SystemChange::SetStartupEntry {
                entry: StartupEntryRef {
                    scope: StartupScope::User,
                    location: StartupLocation::Run,
                    name: EntryName::parse("OneDrive").unwrap(),
                },
                enabled: false,
            },
            SystemChange::SetServiceStartType {
                catalog_id: id("diagtrack"),
                start_type: ServiceStartType::Disabled,
            },
            SystemChange::SetUserSetting {
                setting_id: id("advertising-id"),
                value: Some(0),
            },
            SystemChange::SetMachineSetting {
                setting_id: id("telemetry-level"),
                value: None,
            },
            SystemChange::SetSystemTaskEnabled {
                catalog_id: id("ceip-consolidator"),
                enabled: false,
            },
            SystemChange::SetWindowsUpdatePolicy {
                setting_id: id("au-options"),
                value: Some(2),
            },
            SystemChange::SetFirewallRuleEnabled {
                rule_name: EntryName::parse("Remote Desktop").unwrap(),
                enabled: false,
            },
            SystemChange::SetFirewallProfileEnabled {
                profile: FirewallProfile::Public,
                enabled: true,
            },
            SystemChange::SetHibernation { enabled: false },
            SystemChange::SetActivePowerScheme {
                scheme: PowerSchemeId::parse("381b4222-f694-41f0-9685-ff5bb260df2e").unwrap(),
            },
            SystemChange::DeleteDriverPackage {
                published_name: DriverPackageName::parse("oem42.inf").unwrap(),
            },
            SystemChange::EditHosts {
                line_ops: vec![HostsLineOp {
                    line: 3,
                    action: HostsLineAction::Disable,
                }],
            },
            SystemChange::CreateRestorePoint {
                description: ChangeDescription::parse("Before tuning").unwrap(),
            },
            SystemChange::UpsertScanSchedule {
                schedule_id: schedule.clone(),
                cadence: ScheduleCadence::Weekly {
                    day: Weekday::Sunday,
                    hour: 3,
                    minute: 30,
                },
            },
            SystemChange::RemoveScanSchedule {
                schedule_id: schedule,
            },
        ]
    }

    #[test]
    fn every_change_round_trips_through_json() {
        for change in all_changes() {
            let json = serde_json::to_string(&change).unwrap();
            assert_eq!(
                serde_json::from_str::<SystemChange>(&json).unwrap(),
                change,
                "{json}"
            );
        }
    }

    #[test]
    fn json_uses_camel_case_tags_and_fields() {
        let json = serde_json::to_value(SystemChange::SetServiceStartType {
            catalog_id: id("diagtrack"),
            start_type: ServiceStartType::Disabled,
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({"kind":"setServiceStartType","catalogId":"diagtrack","startType":"disabled"})
        );
    }

    #[test]
    fn deserialization_rejects_unknown_kinds_fields_and_invalid_values() {
        for json in [
            r#"{"kind":"runCommand","command":"cmd /c del"}"#,
            r#"{"kind":"setHibernation","enabled":true,"path":"C:\\"}"#,
            r#"{"kind":"setServiceStartType","catalogId":"../HKLM","startType":"disabled"}"#,
            r#"{"kind":"setServiceStartType","catalogId":"diagtrack","startType":"boot"}"#,
            r#"{"kind":"deleteDriverPackage","publishedName":"C:\\Windows\\inf\\oem1.inf"}"#,
            r#"{"kind":"deleteDriverPackage","publishedName":"oem123456.inf"}"#,
            r#"{"kind":"setFirewallRuleEnabled","ruleName":"a\u0000b","enabled":true}"#,
            r#"{"kind":"removeScanSchedule","scheduleId":"not-a-uuid"}"#,
            r#"{"kind":"setActivePowerScheme","scheme":"381B4222-F694-41F0-9685-FF5BB260DF2E"}"#,
        ] {
            assert!(
                serde_json::from_str::<SystemChange>(json).is_err(),
                "{json}"
            );
        }
    }

    #[test]
    fn bounded_strings_enforce_limits() {
        assert!(EntryName::parse("a".repeat(256)).is_ok());
        assert!(EntryName::parse("a".repeat(257)).is_err());
        assert!(EntryName::parse("   ").is_err());
        assert!(CatalogId::parse("a".repeat(64)).is_ok());
        assert!(CatalogId::parse("a".repeat(65)).is_err());
        assert!(CatalogId::parse("Upper").is_err());
        assert!(ChangeDescription::parse("a".repeat(129)).is_err());
        assert!(Sha256Digest::parse("0".repeat(64)).is_ok());
        assert!(Sha256Digest::parse("G".repeat(64)).is_err());
    }

    #[test]
    fn structural_validation_rejects_bad_hosts_edits_and_schedules() {
        let op = |line| HostsLineOp {
            line,
            action: HostsLineAction::Disable,
        };
        assert_eq!(
            SystemChange::EditHosts { line_ops: vec![] }.validate(),
            Err(ContractError::EmptyLineOps)
        );
        assert_eq!(
            SystemChange::EditHosts {
                line_ops: vec![op(1), op(1)]
            }
            .validate(),
            Err(ContractError::DuplicateLine)
        );
        assert_eq!(
            SystemChange::EditHosts {
                line_ops: (0..65).map(op).collect()
            }
            .validate(),
            Err(ContractError::TooManyLineOps)
        );
        let schedule = SystemChange::UpsertScanSchedule {
            schedule_id: ScheduleId::parse("0f8fad5b-d9cb-469f-a165-70867728950e").unwrap(),
            cadence: ScheduleCadence::Daily {
                hour: 24,
                minute: 0,
            },
        };
        assert_eq!(schedule.validate(), Err(ContractError::InvalidSchedule));
    }

    #[test]
    fn privilege_routes_only_machine_state_to_the_helper() {
        let standard: Vec<_> = all_changes()
            .into_iter()
            .filter(|c| c.privilege() == Privilege::Standard)
            .collect();
        assert!(standard.iter().all(|change| matches!(
            change,
            SystemChange::SetStartupEntry {
                entry: StartupEntryRef {
                    scope: StartupScope::User,
                    ..
                },
                ..
            } | SystemChange::SetUserSetting { .. }
                | SystemChange::SetActivePowerScheme { .. }
                | SystemChange::UpsertScanSchedule { .. }
                | SystemChange::RemoveScanSchedule { .. }
        )));
        let machine_startup = SystemChange::SetStartupEntry {
            entry: StartupEntryRef {
                scope: StartupScope::Machine,
                location: StartupLocation::Run,
                name: EntryName::parse("Vendor").unwrap(),
            },
            enabled: false,
        };
        assert_eq!(machine_startup.privilege(), Privilege::Helper);
    }

    #[test]
    fn every_change_declares_reversibility_and_irreversible_ones_explain_why() {
        for change in all_changes() {
            if let Reversibility::Irreversible { reason } = change.reversibility() {
                assert!(!reason.is_empty());
                assert_eq!(
                    change.inverse(&PriorState::NotApplicable),
                    Err(InverseError::Irreversible)
                );
            }
        }
    }

    #[test]
    fn inverse_restores_recorded_prior_state() {
        let change = SystemChange::SetServiceStartType {
            catalog_id: id("diagtrack"),
            start_type: ServiceStartType::Disabled,
        };
        assert_eq!(
            change
                .inverse(&PriorState::ServiceStart {
                    start: ServiceStartState::Automatic
                })
                .unwrap(),
            Some(SystemChange::SetServiceStartType {
                catalog_id: id("diagtrack"),
                start_type: ServiceStartType::Automatic
            })
        );
        assert_eq!(
            change.inverse(&PriorState::ServiceStart {
                start: ServiceStartState::Boot
            }),
            Err(InverseError::PriorNotWritable)
        );
        assert_eq!(
            change.inverse(&PriorState::Enabled { enabled: true }),
            Err(InverseError::PriorMismatch)
        );

        let setting = SystemChange::SetMachineSetting {
            setting_id: id("telemetry-level"),
            value: Some(0),
        };
        assert_eq!(
            setting
                .inverse(&PriorState::RegistryValue { value: None })
                .unwrap(),
            Some(SystemChange::SetMachineSetting {
                setting_id: id("telemetry-level"),
                value: None
            })
        );

        let hosts = SystemChange::EditHosts {
            line_ops: vec![HostsLineOp {
                line: 2,
                action: HostsLineAction::Disable,
            }],
        };
        let prior = PriorState::Hosts {
            sha256: Sha256Digest::parse("a".repeat(64)).unwrap(),
        };
        assert_eq!(
            hosts.inverse(&prior).unwrap(),
            Some(SystemChange::EditHosts {
                line_ops: vec![HostsLineOp {
                    line: 2,
                    action: HostsLineAction::Restore
                }]
            })
        );
    }

    #[test]
    fn schedule_inverse_removes_new_schedules_and_recreates_removed_ones() {
        let schedule_id = ScheduleId::parse("0f8fad5b-d9cb-469f-a165-70867728950e").unwrap();
        let cadence = ScheduleCadence::Daily { hour: 2, minute: 0 };
        let upsert = SystemChange::UpsertScanSchedule {
            schedule_id: schedule_id.clone(),
            cadence,
        };
        assert_eq!(
            upsert
                .inverse(&PriorState::Schedule { cadence: None })
                .unwrap(),
            Some(SystemChange::RemoveScanSchedule {
                schedule_id: schedule_id.clone()
            })
        );
        let remove = SystemChange::RemoveScanSchedule {
            schedule_id: schedule_id.clone(),
        };
        assert_eq!(
            remove
                .inverse(&PriorState::Schedule {
                    cadence: Some(cadence)
                })
                .unwrap(),
            Some(upsert.clone())
        );
        assert_eq!(
            remove
                .inverse(&PriorState::Schedule { cadence: None })
                .unwrap(),
            None
        );
    }

    #[test]
    fn satisfaction_detects_idempotent_reapply() {
        let change = SystemChange::SetHibernation { enabled: false };
        assert_eq!(
            change.is_satisfied_by(&PriorState::Enabled { enabled: false }),
            Some(true)
        );
        assert_eq!(
            change.is_satisfied_by(&PriorState::Enabled { enabled: true }),
            Some(false)
        );
        let hosts = SystemChange::EditHosts {
            line_ops: vec![HostsLineOp {
                line: 0,
                action: HostsLineAction::Disable,
            }],
        };
        assert_eq!(hosts.is_satisfied_by(&PriorState::NotApplicable), None);
    }

    #[test]
    fn planned_change_summary_declares_reversibility_restart_and_privilege() {
        let planned = PlannedChange::new(
            SystemChange::DeleteDriverPackage {
                published_name: DriverPackageName::parse("oem7.inf").unwrap(),
            },
            ImpactSummary {
                restart: RestartRequirement::Reboot,
                risk: RiskLevel::High,
                ..impact()
            },
            PriorState::DriverPackage { present: true },
        )
        .unwrap();
        let line = planned.summary_line();
        assert!(line.contains("CANNOT be undone"));
        assert!(line.contains("restart required"));
        assert!(line.contains("needs administrator"));
        assert!(planned.inverse.is_none());
    }

    fn entry(id: &str, outcome: Option<ChangeOutcome>) -> JournalEntry {
        let change = SystemChange::SetHibernation { enabled: false };
        let prior = PriorState::Enabled { enabled: true };
        JournalEntry {
            id: id.into(),
            plan_id: "plan".into(),
            recorded_at: 1,
            inverse: change.inverse(&prior).unwrap(),
            reversibility: change.reversibility(),
            change,
            prior,
            outcome,
            rolled_back_by: None,
        }
    }

    #[test]
    fn journal_records_intent_then_outcome_and_exposes_interrupted_entries() {
        let mut journal = Journal::empty();
        journal.record_intent(entry("a", None));
        journal.record_intent(entry("b", None));
        assert!(journal.record_outcome("a", ChangeOutcome::Applied, None));
        assert!(!journal.record_outcome("missing", ChangeOutcome::Applied, None));
        let interrupted: Vec<_> = journal
            .interrupted()
            .map(|entry| entry.id.as_str())
            .collect();
        assert_eq!(interrupted, ["b"]);
        assert_eq!(
            journal.find("a").unwrap().rollback_status(),
            RollbackStatus::Available
        );
        assert_eq!(
            journal.find("b").unwrap().rollback_status(),
            RollbackStatus::Interrupted
        );
        assert!(journal.mark_rolled_back("a", "c"));
        assert_eq!(
            journal.find("a").unwrap().rollback_status(),
            RollbackStatus::AlreadyRolledBack
        );
    }

    #[test]
    fn journal_outcome_prior_from_helper_replaces_preview_prior() {
        let mut journal = Journal::empty();
        journal.record_intent(entry("a", None));
        journal.record_outcome(
            "a",
            ChangeOutcome::Applied,
            Some(PriorState::Enabled { enabled: false }),
        );
        assert_eq!(
            journal.find("a").unwrap().inverse,
            Some(SystemChange::SetHibernation { enabled: false })
        );
    }

    #[test]
    fn journal_only_rolls_back_applied_entries() {
        for outcome in [
            ChangeOutcome::AlreadyApplied,
            ChangeOutcome::StateChanged,
            ChangeOutcome::Denied,
            ChangeOutcome::Failed {
                code: FailureCode::SystemError,
            },
            ChangeOutcome::Unsupported {
                reason: UnsupportedReason::ApiUnavailable,
            },
        ] {
            assert_eq!(
                entry("x", Some(outcome)).rollback_status(),
                RollbackStatus::NothingApplied
            );
        }
    }

    #[test]
    fn journal_round_trips_and_rejects_other_versions() {
        let mut journal = Journal::empty();
        journal.record_intent(entry("a", Some(ChangeOutcome::Applied)));
        let bytes = serde_json::to_vec(&journal).unwrap();
        assert_eq!(Journal::from_json(&bytes).unwrap(), journal);
        journal.version = 2;
        let bytes = serde_json::to_vec(&journal).unwrap();
        assert_eq!(
            Journal::from_json(&bytes),
            Err(JournalParseError::UnsupportedVersion)
        );
        assert_eq!(Journal::from_json(b"{"), Err(JournalParseError::Malformed));
    }

    #[test]
    fn journal_trim_keeps_interrupted_records() {
        let mut journal = Journal::empty();
        journal.record_intent(entry("interrupted", None));
        for index in 0..MAX_JOURNAL_ENTRIES + 10 {
            journal.record_intent(entry(&index.to_string(), Some(ChangeOutcome::Applied)));
        }
        assert_eq!(journal.entries.len(), MAX_JOURNAL_ENTRIES);
        assert!(journal.find("interrupted").is_some());
    }

    #[test]
    fn execution_report_flags_partial_failure() {
        let change = SystemChange::SetHibernation { enabled: false };
        let report = ExecutionReport {
            plan_id: "p".into(),
            results: vec![
                ChangeResult {
                    change: change.clone(),
                    outcome: ChangeOutcome::Applied,
                    journal_entry_id: None,
                },
                ChangeResult {
                    change,
                    outcome: ChangeOutcome::Failed {
                        code: FailureCode::SystemError,
                    },
                    journal_entry_id: None,
                },
            ],
        };
        assert!(!report.all_succeeded());
    }
}
