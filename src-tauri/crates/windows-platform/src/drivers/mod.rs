//! Third-party driver store cleanup.
//!
//! Lists the published third-party driver packages (`oem<N>.inf`) and lets the
//! user remove *superseded* ones: packages that no present device is bound to
//! and for which a newer package with the same original INF name and provider
//! exists in the store. Everything goes through SetupAPI; no process is ever
//! launched (no `pnputil`).
//!
//! Parity gap vs Kudu: installing driver *updates* is intentionally
//! unsupported. This module only removes stale packages from the local driver
//! store; it never downloads, stages, or installs drivers.

pub mod elevated;
mod windows;

#[cfg(test)]
mod tests;

use std::{cmp::Ordering, collections::HashSet};

use cleanup_core::system_change::{
    DriverPackageName, ImpactSummary, PriorState, RestartRequirement, RiskLevel, SystemChange,
    UnsupportedReason,
};
use serde::Serialize;

use crate::system_change::{AdapterError, SystemAdapter};

pub use windows::{WindowsDriverReader, WindowsDriverWriter};

/// Maximum number of `oem*.inf` packages inspected.
pub const MAX_PACKAGES: usize = 2000;
/// Maximum number of present devices inspected for driver bindings.
pub const MAX_DEVICES: usize = 10_000;
/// Maximum length (in chars) of any string surfaced from an INF.
pub const MAX_FIELD_CHARS: usize = 128;

/// One published package as read from the system, before classification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawDriverPackage {
    pub published_name: DriverPackageName,
    /// Original INF file name from the driver store (lowercase), when known.
    pub original_name: Option<String>,
    pub provider: Option<String>,
    pub class: Option<String>,
    /// `DriverVer` date field as written in the INF (`mm/dd/yyyy`).
    pub driver_date: Option<String>,
    /// `DriverVer` version field as written in the INF (`a.b.c.d`).
    pub driver_version: Option<String>,
}

/// Published names of packages bound to present devices (lowercase).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BoundPackages {
    pub names: HashSet<String>,
    /// False when the device enumeration hit its bound; nothing is then
    /// considered stale, because an unseen device could use any package.
    pub complete: bool,
}

/// Read-only access to the driver store and device bindings.
pub trait DriverReader: Send + Sync {
    fn packages(&self) -> Result<Vec<RawDriverPackage>, AdapterError>;
    fn bound_packages(&self) -> Result<BoundPackages, AdapterError>;
}

/// The single write: remove a package from the driver store (never forced).
pub trait DriverWriter: Send + Sync {
    fn uninstall(&self, name: &DriverPackageName) -> Result<(), AdapterError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DriverPackageStatus {
    /// Bound to at least one present device.
    InUse,
    /// Not bound, but no newer package from the same provider replaces it.
    Current,
    /// Not bound and replaced by a newer package with the same original INF
    /// name and provider. Only these are deletable.
    Superseded,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DriverPackage {
    pub published_name: String,
    pub original_name: Option<String>,
    pub provider: Option<String>,
    pub class: Option<String>,
    /// ISO `yyyy-mm-dd` when the INF date parses, otherwise the raw text.
    pub driver_date: Option<String>,
    pub driver_version: Option<String>,
    pub status: DriverPackageStatus,
    pub deletable: bool,
}

/// Sanitize and bound a string read from an INF.
pub(crate) fn bounded(value: &str) -> Option<String> {
    let cleaned: String = value
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_FIELD_CHARS)
        .collect();
    let cleaned = cleaned.trim();
    (!cleaned.is_empty()).then(|| cleaned.to_owned())
}

/// `mm/dd/yyyy` → `(yyyy, mm, dd)`.
pub(crate) fn parse_date(value: &str) -> Option<(u16, u8, u8)> {
    let mut parts = value.trim().split('/');
    let month: u8 = parts.next()?.trim().parse().ok()?;
    let day: u8 = parts.next()?.trim().parse().ok()?;
    let year: u16 = parts.next()?.trim().parse().ok()?;
    if parts.next().is_some()
        || !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || !(1900..=9999).contains(&year)
    {
        return None;
    }
    Some((year, month, day))
}

/// `a[.b[.c[.d]]]` → four numeric components.
pub(crate) fn parse_version(value: &str) -> Option<[u32; 4]> {
    let mut version = [0_u32; 4];
    for (count, part) in value.trim().split('.').enumerate() {
        if count == 4 {
            return None;
        }
        version[count] = part.trim().parse().ok()?;
    }
    Some(version)
}

fn same_key(left: &Option<String>, right: &Option<String>) -> bool {
    matches!((left, right), (Some(left), Some(right)) if left.eq_ignore_ascii_case(right))
}

/// Strictly newer by DriverVer date, then version. Anything unparsable is
/// never newer (and never older).
fn is_newer(candidate: &RawDriverPackage, package: &RawDriverPackage) -> bool {
    let (Some(candidate_date), Some(package_date)) = (
        candidate.driver_date.as_deref().and_then(parse_date),
        package.driver_date.as_deref().and_then(parse_date),
    ) else {
        return false;
    };
    match candidate_date.cmp(&package_date) {
        Ordering::Greater => true,
        Ordering::Less => false,
        Ordering::Equal => matches!(
            (
                candidate.driver_version.as_deref().and_then(parse_version),
                package.driver_version.as_deref().and_then(parse_version),
            ),
            (Some(candidate), Some(package)) if candidate > package
        ),
    }
}

fn status_of(
    package: &RawDriverPackage,
    all: &[RawDriverPackage],
    bound: &BoundPackages,
) -> DriverPackageStatus {
    if bound
        .names
        .contains(&package.published_name.as_str().to_ascii_lowercase())
    {
        return DriverPackageStatus::InUse;
    }
    let superseded = bound.complete
        && all.iter().any(|other| {
            other.published_name != package.published_name
                && same_key(&other.original_name, &package.original_name)
                && same_key(&other.provider, &package.provider)
                && is_newer(other, package)
        });
    if superseded {
        DriverPackageStatus::Superseded
    } else {
        DriverPackageStatus::Current
    }
}

/// Classify every package.
pub fn classify(packages: &[RawDriverPackage], bound: &BoundPackages) -> Vec<DriverPackage> {
    packages
        .iter()
        .map(|package| {
            let status = status_of(package, packages, bound);
            DriverPackage {
                published_name: package.published_name.to_string(),
                original_name: package.original_name.clone(),
                provider: package.provider.clone(),
                class: package.class.clone(),
                driver_date: package.driver_date.as_deref().map(|raw| {
                    parse_date(raw).map_or_else(
                        || raw.to_owned(),
                        |(year, month, day)| format!("{year:04}-{month:02}-{day:02}"),
                    )
                }),
                driver_version: package.driver_version.clone(),
                status,
                deletable: status == DriverPackageStatus::Superseded,
            }
        })
        .collect()
}

/// Read and classify the whole inventory through `reader`.
pub fn inventory(reader: &dyn DriverReader) -> Result<Vec<DriverPackage>, AdapterError> {
    let packages = reader.packages()?;
    let bound = reader.bound_packages()?;
    Ok(classify(&packages, &bound))
}

/// Live read-only inventory of third-party driver packages.
pub fn list_driver_packages() -> Result<Vec<DriverPackage>, AdapterError> {
    inventory(&WindowsDriverReader)
}

/// Current state of one package: absent → `present: false`; stale →
/// `present: true`; anything else (in use, current) fails closed.
pub(crate) fn observe_package(
    reader: &dyn DriverReader,
    name: &DriverPackageName,
) -> Result<PriorState, AdapterError> {
    let packages = reader.packages()?;
    if !packages
        .iter()
        .any(|package| &package.published_name == name)
    {
        // A truncated enumeration cannot prove absence.
        if packages.len() >= MAX_PACKAGES {
            return Err(AdapterError::Unsupported(UnsupportedReason::NotPresent));
        }
        return Ok(PriorState::DriverPackage { present: false });
    }
    let bound = reader.bound_packages()?;
    let item = classify(&packages, &bound)
        .into_iter()
        .find(|item| item.published_name == name.as_str());
    match item {
        Some(item) if item.deletable => Ok(PriorState::DriverPackage { present: true }),
        _ => Err(AdapterError::Unsupported(UnsupportedReason::NotPresent)),
    }
}

pub struct DriverAdapter {
    reader: Box<dyn DriverReader>,
}

impl DriverAdapter {
    pub fn new(reader: Box<dyn DriverReader>) -> Self {
        Self { reader }
    }

    pub fn windows() -> Self {
        Self::new(Box::new(WindowsDriverReader))
    }
}

impl SystemAdapter for DriverAdapter {
    fn describe(&self, change: &SystemChange) -> Result<ImpactSummary, AdapterError> {
        let SystemChange::DeleteDriverPackage { published_name } = change else {
            return Err(AdapterError::Failed);
        };
        let item = inventory(self.reader.as_ref())?
            .into_iter()
            .find(|item| item.published_name == published_name.as_str())
            .ok_or(AdapterError::Unsupported(UnsupportedReason::NotPresent))?;
        let unknown = || "unknown".to_owned();
        Ok(ImpactSummary {
            component: format!("Driver store package {published_name}"),
            effect: format!(
                "Permanently removes the unused driver package {} (provider {}, class {}, version {}) \
                 from the Windows driver store because a newer package from the same provider \
                 replaces it. This is irreversible; a restore point is recommended and is offered \
                 first in the same plan.",
                item.original_name
                    .as_deref()
                    .unwrap_or(published_name.as_str()),
                item.provider.unwrap_or_else(unknown),
                item.class.unwrap_or_else(unknown),
                item.driver_version.unwrap_or_else(unknown),
            ),
            restart: RestartRequirement::None,
            risk: RiskLevel::High,
        })
    }

    fn observe(&self, change: &SystemChange) -> Result<PriorState, AdapterError> {
        match change {
            SystemChange::DeleteDriverPackage { published_name } => {
                observe_package(self.reader.as_ref(), published_name)
            }
            _ => Err(AdapterError::Failed),
        }
    }
}
