//! System Restore: read-only restore-point listing (WMI `ROOT\DEFAULT:SystemRestore`),
//! protection status (registry), and the adapter that previews
//! `SystemChange::CreateRestorePoint`. Creation itself runs in the elevated
//! helper through `security::restore_point`.

mod wmi;

#[cfg(test)]
mod tests;

use std::io;

use cleanup_core::system_change::{
    ImpactSummary, PriorState, RestartRequirement, RiskLevel, SystemChange, UnsupportedReason,
};
use serde::Serialize;
use windows::Win32::{
    Foundation::{E_ACCESSDENIED, REGDB_E_CLASSNOTREG},
    System::Wmi::{
        WBEM_E_ACCESS_DENIED, WBEM_E_INVALID_CLASS, WBEM_E_INVALID_NAMESPACE, WBEM_E_NOT_FOUND,
    },
};

use crate::{
    system_change::{AdapterError, SystemAdapter},
    win_registry::{Hive, RegistryKey},
};

pub use wmi::WmiRestorePointSource;

pub const MAX_RESTORE_POINTS: usize = 256;
pub const MAX_DESCRIPTION_CHARS: usize = 256;
pub const DEFAULT_CREATION_FREQUENCY_MINUTES: u32 = 1440;

const POLICY_KEY: &str = r"SOFTWARE\Policies\Microsoft\Windows NT\SystemRestore";
const SETTINGS_KEY: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\SystemRestore";

// ---------------------------------------------------------------------------
// Restore-point listing
// ---------------------------------------------------------------------------

/// A raw HRESULT from the WMI client.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmiFailure(pub i32);

/// One `SystemRestore` instance as WMI returned it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RawRestorePoint {
    pub sequence_number: Option<u32>,
    pub description: Option<String>,
    pub creation_time: Option<String>,
    pub restore_point_type: Option<u32>,
}

pub trait RestorePointSource {
    /// At most [`MAX_RESTORE_POINTS`] instances.
    fn query(&self) -> Result<Vec<RawRestorePoint>, WmiFailure>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RestorePointKind {
    ApplicationInstall,
    ApplicationUninstall,
    DeviceDriverInstall,
    ModifySettings,
    CancelledOperation,
    Other,
}

impl RestorePointKind {
    pub fn from_type(value: u32) -> Self {
        match value {
            0 => Self::ApplicationInstall,
            1 => Self::ApplicationUninstall,
            10 => Self::DeviceDriverInstall,
            12 => Self::ModifySettings,
            13 => Self::CancelledOperation,
            _ => Self::Other,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestorePoint {
    pub sequence_number: u32,
    pub description: String,
    /// Unix seconds (UTC); `None` when WMI returned no parseable time.
    pub created_at: Option<i64>,
    pub restore_point_type: Option<u32>,
    pub kind: RestorePointKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RestorePointListStatus {
    Available,
    RequiresAdministrator,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestorePointList {
    pub status: RestorePointListStatus,
    pub points: Vec<RestorePoint>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RestoreError {
    /// WMI failed for a reason other than access or availability.
    Wmi(i32),
    Registry,
}

pub fn list_restore_points() -> Result<RestorePointList, RestoreError> {
    list_restore_points_with(&WmiRestorePointSource)
}

pub fn list_restore_points_with(
    source: &impl RestorePointSource,
) -> Result<RestorePointList, RestoreError> {
    let raw = match source.query() {
        Ok(raw) => raw,
        Err(WmiFailure(code)) if is_access_denied(code) => {
            return Ok(RestorePointList {
                status: RestorePointListStatus::RequiresAdministrator,
                points: Vec::new(),
            });
        }
        Err(WmiFailure(code)) if is_unavailable(code) => {
            return Ok(RestorePointList {
                status: RestorePointListStatus::Unavailable,
                points: Vec::new(),
            });
        }
        Err(WmiFailure(code)) => return Err(RestoreError::Wmi(code)),
    };
    let mut points: Vec<_> = raw
        .into_iter()
        .take(MAX_RESTORE_POINTS)
        .filter_map(restore_point)
        .collect();
    points.sort_by(|left, right| right.sequence_number.cmp(&left.sequence_number));
    Ok(RestorePointList {
        status: RestorePointListStatus::Available,
        points,
    })
}

fn restore_point(raw: RawRestorePoint) -> Option<RestorePoint> {
    let sequence_number = raw.sequence_number?;
    Some(RestorePoint {
        sequence_number,
        description: raw
            .description
            .unwrap_or_default()
            .chars()
            .take(MAX_DESCRIPTION_CHARS)
            .collect(),
        created_at: raw.creation_time.as_deref().and_then(parse_cim_datetime),
        restore_point_type: raw.restore_point_type,
        kind: raw
            .restore_point_type
            .map_or(RestorePointKind::Other, RestorePointKind::from_type),
    })
}

fn is_access_denied(code: i32) -> bool {
    code == WBEM_E_ACCESS_DENIED.0 || code == E_ACCESSDENIED.0
}

fn is_unavailable(code: i32) -> bool {
    [
        WBEM_E_INVALID_NAMESPACE.0,
        WBEM_E_INVALID_CLASS.0,
        WBEM_E_NOT_FOUND.0,
        REGDB_E_CLASSNOTREG.0,
    ]
    .contains(&code)
}

/// Parse a CIM datetime `yyyymmddHHMMSS.mmmmmmsUUU` (local time plus a signed
/// UTC offset in minutes) into Unix seconds.
pub fn parse_cim_datetime(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    if bytes.len() != 25 || bytes[14] != b'.' {
        return None;
    }
    let digits = |range: std::ops::Range<usize>| -> Option<i64> {
        let slice = bytes.get(range)?;
        if slice.is_empty() || !slice.iter().all(u8::is_ascii_digit) {
            return None;
        }
        Some(
            slice
                .iter()
                .fold(0_i64, |total, digit| total * 10 + i64::from(digit - b'0')),
        )
    };
    let year = digits(0..4)?;
    let month = digits(4..6)?;
    let day = digits(6..8)?;
    let hour = digits(8..10)?;
    let minute = digits(10..12)?;
    let second = digits(12..14)?;
    digits(15..21)?;
    let sign = match bytes[21] {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let offset_minutes = digits(22..25)?;
    if !(1..=12).contains(&month)
        || day < 1
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 59
        || offset_minutes > 14 * 60
    {
        return None;
    }
    let local = days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second;
    Some(local - sign * offset_minutes * 60)
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let shifted_month = (month + 9) % 12;
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

// ---------------------------------------------------------------------------
// Protection status
// ---------------------------------------------------------------------------

/// Raw registry values that decide System Restore availability.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProtectionValues {
    pub disable_sr: Option<u32>,
    pub disable_config: Option<u32>,
    pub rp_session_interval: Option<u32>,
    pub creation_frequency_minutes: Option<u32>,
}

pub trait ProtectionReader: Send + Sync {
    fn read(&self) -> io::Result<ProtectionValues>;
}

pub struct RegistryProtectionReader;

impl ProtectionReader for RegistryProtectionReader {
    fn read(&self) -> io::Result<ProtectionValues> {
        let policy = RegistryKey::open_read(Hive::LocalMachine, POLICY_KEY)?;
        let settings = RegistryKey::open_read(Hive::LocalMachine, SETTINGS_KEY)?;
        let dword = |key: &Option<RegistryKey>, name: &str| -> io::Result<Option<u32>> {
            key.as_ref().map_or(Ok(None), |key| key.dword(name))
        };
        Ok(ProtectionValues {
            disable_sr: dword(&policy, "DisableSR")?,
            disable_config: dword(&policy, "DisableConfig")?,
            rp_session_interval: dword(&settings, "RPSessionInterval")?,
            creation_frequency_minutes: dword(&settings, "SystemRestorePointCreationFrequency")?,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreProtection {
    /// `DisableSR` or `DisableConfig` policy is set (managed device).
    pub policy_disabled: bool,
    /// From `RPSessionInterval`: 0 means system-drive protection is off;
    /// `None` when Windows has not recorded the value.
    pub protection_enabled: Option<bool>,
    /// Windows silently skips a new point if one was created within this window.
    pub creation_frequency_minutes: u32,
}

impl RestoreProtection {
    pub fn from_values(values: ProtectionValues) -> Self {
        Self {
            policy_disabled: values.disable_sr.is_some_and(|value| value != 0)
                || values.disable_config.is_some_and(|value| value != 0),
            protection_enabled: values.rp_session_interval.map(|value| value != 0),
            creation_frequency_minutes: values
                .creation_frequency_minutes
                .unwrap_or(DEFAULT_CREATION_FREQUENCY_MINUTES),
        }
    }
}

pub fn get_restore_protection() -> Result<RestoreProtection, RestoreError> {
    read_protection(&RegistryProtectionReader).map_err(|_| RestoreError::Registry)
}

fn read_protection(reader: &impl ProtectionReader) -> io::Result<RestoreProtection> {
    reader.read().map(RestoreProtection::from_values)
}

// ---------------------------------------------------------------------------
// Adapter
// ---------------------------------------------------------------------------

pub struct RestoreAdapter<P: ProtectionReader = RegistryProtectionReader> {
    reader: P,
}

impl RestoreAdapter {
    pub fn new() -> Self {
        Self {
            reader: RegistryProtectionReader,
        }
    }
}

impl Default for RestoreAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl<P: ProtectionReader> RestoreAdapter<P> {
    pub fn with_reader(reader: P) -> Self {
        Self { reader }
    }

    fn protection(&self) -> Result<RestoreProtection, AdapterError> {
        read_protection(&self.reader).map_err(|error| {
            if error.kind() == io::ErrorKind::PermissionDenied {
                AdapterError::Denied
            } else {
                AdapterError::Failed
            }
        })
    }
}

impl<P: ProtectionReader> SystemAdapter for RestoreAdapter<P> {
    fn describe(&self, change: &SystemChange) -> Result<ImpactSummary, AdapterError> {
        let SystemChange::CreateRestorePoint { description } = change else {
            return Err(AdapterError::Failed);
        };
        let window = match self.protection() {
            Ok(protection) => format!(
                "if a restore point was already made in the last {} minutes",
                protection.creation_frequency_minutes
            ),
            Err(_) => "if a restore point was made recently".to_owned(),
        };
        Ok(ImpactSummary {
            component: "System Restore".to_owned(),
            effect: format!(
                "Creates a System Restore point named \"{}\". Needs administrator rights. \
                 Windows may silently skip it {window}.",
                description.as_str()
            ),
            restart: RestartRequirement::None,
            risk: RiskLevel::Low,
        })
    }

    fn observe(&self, change: &SystemChange) -> Result<PriorState, AdapterError> {
        if !matches!(change, SystemChange::CreateRestorePoint { .. }) {
            return Err(AdapterError::Failed);
        }
        let protection = self.protection()?;
        if protection.policy_disabled {
            Err(AdapterError::Unsupported(UnsupportedReason::ManagedDevice))
        } else if protection.protection_enabled == Some(false) {
            Err(AdapterError::Unsupported(UnsupportedReason::ApiUnavailable))
        } else {
            Ok(PriorState::NotApplicable)
        }
    }

    fn is_satisfied(&self, _change: &SystemChange, _state: &PriorState) -> bool {
        false
    }
}
