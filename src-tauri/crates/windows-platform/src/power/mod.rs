//! Hibernation and power plans.
//!
//! `SetHibernation` is helper-privileged (see [`elevated`]); switching the
//! active power scheme is a standard-integrity `PowerSetActiveScheme` call.

pub mod elevated;
mod windows;

use cleanup_core::system_change::{
    ImpactSummary, PowerSchemeId, PriorState, RestartRequirement, RiskLevel, SystemChange,
    UnsupportedReason,
};
use serde::Serialize;

use crate::system_change::{AdapterError, SystemAdapter};
pub use windows::{WindowsPowerReader, WindowsPowerWriter};

pub const HIGH_PERFORMANCE_SCHEME: &str = "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c";
pub const BALANCED_SCHEME: &str = "381b4222-f694-41f0-9685-ff5bb260df2e";
pub const ULTIMATE_PERFORMANCE_SCHEME: &str = "e9a42b02-d5df-448d-aa00-03f14749eb61";

pub const MAX_SCHEMES: usize = 64;
pub const MAX_SCHEME_NAME_CHARS: usize = 128;

/// Raw hibernation facts from `GetPwrCapabilities`, the `HibernateEnabled`
/// registry value, and the size of `%SystemDrive%\hiberfil.sys`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HibernationFacts {
    pub system_s4: bool,
    pub hiberfile_present: bool,
    pub hibernate_enabled: Option<u32>,
    pub hiberfile_bytes: Option<u64>,
}

impl HibernationFacts {
    /// S4 is reported by the firmware or a hiberfile exists. Windows also
    /// clears `SystemS4` after `powercfg /hibernate off`, so an explicit
    /// `HibernateEnabled = 0` is treated as supported-but-disabled; otherwise
    /// a machine that turned hibernation off could never turn it back on.
    pub fn supported(&self) -> bool {
        self.system_s4 || self.hiberfile_present || self.hibernate_enabled == Some(0)
    }

    pub fn enabled(&self) -> bool {
        self.supported()
            && match self.hibernate_enabled {
                Some(value) => value != 0,
                None => self.system_s4 || self.hiberfile_present,
            }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PowerSchemeEntry {
    pub id: PowerSchemeId,
    pub name: String,
}

pub trait PowerReader: Send + Sync {
    fn hibernation(&self) -> Result<HibernationFacts, AdapterError>;
    /// Enumerated schemes, at most [`MAX_SCHEMES`].
    fn schemes(&self) -> Result<Vec<PowerSchemeEntry>, AdapterError>;
    fn active_scheme(&self) -> Result<PowerSchemeId, AdapterError>;
}

pub trait PowerWriter: Send + Sync {
    fn set_active_scheme(&self, scheme: &PowerSchemeId) -> Result<(), AdapterError>;
}

pub(crate) fn parse_scheme(value: &str) -> Option<PowerSchemeId> {
    PowerSchemeId::parse(value.to_ascii_lowercase()).ok()
}

/// `Ok(enabled)` or `Unsupported(ApiUnavailable)` when S4 is unavailable.
pub(crate) fn hibernation_state(facts: &HibernationFacts) -> Result<bool, AdapterError> {
    if facts.supported() {
        Ok(facts.enabled())
    } else {
        Err(AdapterError::Unsupported(UnsupportedReason::ApiUnavailable))
    }
}

fn find_scheme<R: PowerReader>(
    reader: &R,
    scheme: &PowerSchemeId,
) -> Result<PowerSchemeEntry, AdapterError> {
    reader
        .schemes()?
        .into_iter()
        .take(MAX_SCHEMES)
        .find(|entry| &entry.id == scheme)
        .ok_or(AdapterError::Unsupported(UnsupportedReason::NotPresent))
}

fn format_bytes(bytes: u64) -> String {
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    let value = bytes as f64;
    if value >= GIB {
        format!("{:.1} GB", value / GIB)
    } else {
        format!("{:.0} MB", value / MIB)
    }
}

pub struct PowerAdapter<R = WindowsPowerReader, W = WindowsPowerWriter> {
    reader: R,
    writer: W,
}

impl PowerAdapter {
    pub fn new() -> Self {
        Self::with(WindowsPowerReader, WindowsPowerWriter)
    }
}

impl Default for PowerAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl<R: PowerReader, W: PowerWriter> PowerAdapter<R, W> {
    pub fn with(reader: R, writer: W) -> Self {
        Self { reader, writer }
    }
}

impl<R: PowerReader, W: PowerWriter> SystemAdapter for PowerAdapter<R, W> {
    fn describe(&self, change: &SystemChange) -> Result<ImpactSummary, AdapterError> {
        match change {
            SystemChange::SetHibernation { enabled } => {
                let facts = self.reader.hibernation()?;
                hibernation_state(&facts)?;
                let effect = if *enabled {
                    "Turns on hibernation and Fast Startup. Windows creates a hibernation file \
                     (hiberfil.sys) that uses disk space roughly proportional to installed memory."
                        .to_owned()
                } else {
                    let freed = match facts.hiberfile_bytes {
                        Some(bytes) if bytes > 0 => format!(
                            "frees about {} of disk space by deleting the hibernation file",
                            format_bytes(bytes)
                        ),
                        _ => "frees the disk space used by the hibernation file (hiberfil.sys)"
                            .to_owned(),
                    };
                    format!(
                        "Turns off hibernation, which also disables Fast Startup, and {freed}. \
                         No restart is needed."
                    )
                };
                Ok(ImpactSummary {
                    component: "Hibernation".to_owned(),
                    effect,
                    restart: RestartRequirement::None,
                    risk: RiskLevel::Low,
                })
            }
            SystemChange::SetActivePowerScheme { scheme } => {
                let entry = find_scheme(&self.reader, scheme)?;
                Ok(ImpactSummary {
                    component: "Power plan".to_owned(),
                    effect: format!(
                        "Switches the active power plan to \"{}\". This can change battery life, \
                         heat and performance.",
                        entry.name
                    ),
                    restart: RestartRequirement::None,
                    risk: RiskLevel::Low,
                })
            }
            _ => Err(AdapterError::Failed),
        }
    }

    fn observe(&self, change: &SystemChange) -> Result<PriorState, AdapterError> {
        match change {
            SystemChange::SetHibernation { .. } => hibernation_state(&self.reader.hibernation()?)
                .map(|enabled| PriorState::Enabled { enabled }),
            SystemChange::SetActivePowerScheme { scheme } => {
                find_scheme(&self.reader, scheme)?;
                Ok(PriorState::PowerScheme {
                    scheme: self.reader.active_scheme()?,
                })
            }
            _ => Err(AdapterError::Failed),
        }
    }

    fn apply(&self, change: &SystemChange) -> Result<(), AdapterError> {
        match change {
            SystemChange::SetActivePowerScheme { scheme } => {
                let entry = find_scheme(&self.reader, scheme)?;
                self.writer.set_active_scheme(&entry.id)
            }
            _ => Err(AdapterError::Failed),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HibernationStatus {
    pub supported: bool,
    pub enabled: bool,
    pub hiberfile_bytes: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PowerSchemeInfo {
    pub id: PowerSchemeId,
    pub name: String,
    pub active: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PowerStatus {
    pub hibernation: HibernationStatus,
    pub schemes: Vec<PowerSchemeInfo>,
}

pub fn power_status() -> Result<PowerStatus, AdapterError> {
    power_status_with(&WindowsPowerReader)
}

pub fn power_status_with<R: PowerReader>(reader: &R) -> Result<PowerStatus, AdapterError> {
    let facts = reader.hibernation()?;
    let supported = facts.supported();
    let active = reader.active_scheme().ok();
    let schemes = reader
        .schemes()?
        .into_iter()
        .take(MAX_SCHEMES)
        .map(|entry| PowerSchemeInfo {
            active: active.as_ref() == Some(&entry.id),
            name: entry.name.chars().take(MAX_SCHEME_NAME_CHARS).collect(),
            id: entry.id,
        })
        .collect();
    Ok(PowerStatus {
        hibernation: HibernationStatus {
            supported,
            enabled: supported && facts.enabled(),
            hiberfile_bytes: facts.hiberfile_bytes,
        },
        schemes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct FakeReader {
        facts: Result<HibernationFacts, AdapterError>,
        schemes: Result<Vec<PowerSchemeEntry>, AdapterError>,
        active: Mutex<Result<PowerSchemeId, AdapterError>>,
    }

    struct FakeWriter<'a> {
        reader: &'a FakeReader,
        result: Result<(), AdapterError>,
        calls: Mutex<Vec<PowerSchemeId>>,
    }

    impl PowerReader for FakeReader {
        fn hibernation(&self) -> Result<HibernationFacts, AdapterError> {
            self.facts
        }
        fn schemes(&self) -> Result<Vec<PowerSchemeEntry>, AdapterError> {
            self.schemes.clone()
        }
        fn active_scheme(&self) -> Result<PowerSchemeId, AdapterError> {
            self.active.lock().unwrap().clone()
        }
    }

    impl PowerReader for &FakeReader {
        fn hibernation(&self) -> Result<HibernationFacts, AdapterError> {
            (*self).hibernation()
        }
        fn schemes(&self) -> Result<Vec<PowerSchemeEntry>, AdapterError> {
            (*self).schemes()
        }
        fn active_scheme(&self) -> Result<PowerSchemeId, AdapterError> {
            (*self).active_scheme()
        }
    }

    impl PowerWriter for &FakeWriter<'_> {
        fn set_active_scheme(&self, scheme: &PowerSchemeId) -> Result<(), AdapterError> {
            self.calls.lock().unwrap().push(scheme.clone());
            self.result?;
            *self.reader.active.lock().unwrap() = Ok(scheme.clone());
            Ok(())
        }
    }

    fn id(value: &str) -> PowerSchemeId {
        PowerSchemeId::parse(value).unwrap()
    }

    fn enabled_facts() -> HibernationFacts {
        HibernationFacts {
            system_s4: true,
            hiberfile_present: true,
            hibernate_enabled: Some(1),
            hiberfile_bytes: Some(6 * 1024 * 1024 * 1024),
        }
    }

    fn reader(facts: HibernationFacts) -> FakeReader {
        FakeReader {
            facts: Ok(facts),
            schemes: Ok(vec![
                PowerSchemeEntry {
                    id: id(BALANCED_SCHEME),
                    name: "Balanced".into(),
                },
                PowerSchemeEntry {
                    id: id(HIGH_PERFORMANCE_SCHEME),
                    name: "High performance".into(),
                },
            ]),
            active: Mutex::new(Ok(id(BALANCED_SCHEME))),
        }
    }

    fn writer(reader: &FakeReader, result: Result<(), AdapterError>) -> FakeWriter<'_> {
        FakeWriter {
            reader,
            result,
            calls: Mutex::new(Vec::new()),
        }
    }

    #[test]
    fn scheme_constants_are_valid_ids() {
        for value in [
            HIGH_PERFORMANCE_SCHEME,
            BALANCED_SCHEME,
            ULTIMATE_PERFORMANCE_SCHEME,
        ] {
            assert!(PowerSchemeId::parse(value).is_ok());
            let guid = windows::parse_guid(value).unwrap();
            assert_eq!(windows::format_guid(&guid), value);
        }
        assert_eq!(
            parse_scheme("381B4222-F694-41F0-9685-FF5BB260DF2E"),
            Some(id(BALANCED_SCHEME))
        );
        assert_eq!(parse_scheme("not-a-guid"), None);
    }

    #[test]
    fn unsupported_s4_is_api_unavailable_and_reported_in_inventory() {
        let facts = HibernationFacts {
            system_s4: false,
            hiberfile_present: false,
            hibernate_enabled: None,
            hiberfile_bytes: None,
        };
        let fake = reader(facts);
        let w = writer(&fake, Ok(()));
        let adapter = PowerAdapter::with(&fake, &w);
        let change = SystemChange::SetHibernation { enabled: false };
        let unavailable = AdapterError::Unsupported(UnsupportedReason::ApiUnavailable);
        assert_eq!(adapter.observe(&change).unwrap_err(), unavailable);
        assert_eq!(adapter.describe(&change).unwrap_err(), unavailable);
        let status = power_status_with(&&fake).unwrap();
        assert!(!status.hibernation.supported);
        assert!(!status.hibernation.enabled);
    }

    #[test]
    fn disabled_hibernation_stays_supported_for_re_enable() {
        let facts = HibernationFacts {
            system_s4: false,
            hiberfile_present: false,
            hibernate_enabled: Some(0),
            hiberfile_bytes: None,
        };
        let fake = reader(facts);
        let w = writer(&fake, Ok(()));
        let adapter = PowerAdapter::with(&fake, &w);
        assert_eq!(
            adapter.observe(&SystemChange::SetHibernation { enabled: true }),
            Ok(PriorState::Enabled { enabled: false })
        );
    }

    #[test]
    fn hibernation_idempotency_inverse_and_describe() {
        let fake = reader(enabled_facts());
        let w = writer(&fake, Ok(()));
        let adapter = PowerAdapter::with(&fake, &w);
        let off = SystemChange::SetHibernation { enabled: false };
        let prior = adapter.observe(&off).unwrap();
        assert_eq!(prior, PriorState::Enabled { enabled: true });
        assert_eq!(off.is_satisfied_by(&prior), Some(false));
        let on = SystemChange::SetHibernation { enabled: true };
        assert_eq!(on.is_satisfied_by(&prior), Some(true));
        assert_eq!(off.inverse(&prior), Ok(Some(on.clone())));

        let impact = adapter.describe(&off).unwrap();
        assert!(impact.effect.contains("Fast Startup"));
        assert!(impact.effect.contains("6.0 GB"));
        assert_eq!(impact.restart, RestartRequirement::None);
        assert!(
            adapter
                .describe(&on)
                .unwrap()
                .effect
                .contains("Fast Startup")
        );

        // Helper-privileged: never applied in-process.
        assert_eq!(adapter.apply(&off), Err(AdapterError::Failed));
    }

    #[test]
    fn scheme_idempotency_inverse_apply_and_describe() {
        let fake = reader(enabled_facts());
        let w = writer(&fake, Ok(()));
        let adapter = PowerAdapter::with(&fake, &w);
        let high = SystemChange::SetActivePowerScheme {
            scheme: id(HIGH_PERFORMANCE_SCHEME),
        };
        let prior = adapter.observe(&high).unwrap();
        assert_eq!(
            prior,
            PriorState::PowerScheme {
                scheme: id(BALANCED_SCHEME)
            }
        );
        assert_eq!(high.is_satisfied_by(&prior), Some(false));
        let back = SystemChange::SetActivePowerScheme {
            scheme: id(BALANCED_SCHEME),
        };
        assert_eq!(high.inverse(&prior), Ok(Some(back)));

        let impact = adapter.describe(&high).unwrap();
        assert!(impact.effect.contains("High performance"));
        assert!(impact.effect.contains("battery life"));
        assert_eq!(impact.risk, RiskLevel::Low);

        adapter.apply(&high).unwrap();
        assert_eq!(*w.calls.lock().unwrap(), vec![id(HIGH_PERFORMANCE_SCHEME)]);
        let after = adapter.observe(&high).unwrap();
        assert_eq!(high.is_satisfied_by(&after), Some(true));
        let status = power_status_with(&&fake).unwrap();
        assert!(
            status
                .schemes
                .iter()
                .any(|s| s.active && s.id.as_str() == HIGH_PERFORMANCE_SCHEME)
        );
    }

    #[test]
    fn unknown_scheme_fails_closed_without_writing() {
        let fake = reader(enabled_facts());
        let w = writer(&fake, Ok(()));
        let adapter = PowerAdapter::with(&fake, &w);
        let ultimate = SystemChange::SetActivePowerScheme {
            scheme: id(ULTIMATE_PERFORMANCE_SCHEME),
        };
        let not_present = AdapterError::Unsupported(UnsupportedReason::NotPresent);
        assert_eq!(adapter.observe(&ultimate).unwrap_err(), not_present);
        assert_eq!(adapter.describe(&ultimate).unwrap_err(), not_present);
        assert_eq!(adapter.apply(&ultimate).unwrap_err(), not_present);
        assert!(w.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn access_denied_propagates() {
        let mut fake = reader(enabled_facts());
        fake.facts = Err(AdapterError::Denied);
        let w = writer(&fake, Err(AdapterError::Denied));
        let adapter = PowerAdapter::with(&fake, &w);
        assert_eq!(
            adapter.observe(&SystemChange::SetHibernation { enabled: false }),
            Err(AdapterError::Denied)
        );
        let high = SystemChange::SetActivePowerScheme {
            scheme: id(HIGH_PERFORMANCE_SCHEME),
        };
        assert_eq!(adapter.apply(&high), Err(AdapterError::Denied));
        assert_eq!(power_status_with(&&fake), Err(AdapterError::Denied));
        assert_eq!(windows::win32_error(5), AdapterError::Denied);
    }

    #[test]
    fn unrelated_variants_are_rejected() {
        let fake = reader(enabled_facts());
        let w = writer(&fake, Ok(()));
        let adapter = PowerAdapter::with(&fake, &w);
        let other = SystemChange::SetFirewallProfileEnabled {
            profile: cleanup_core::system_change::FirewallProfile::Public,
            enabled: true,
        };
        assert_eq!(adapter.observe(&other), Err(AdapterError::Failed));
        assert_eq!(adapter.apply(&other), Err(AdapterError::Failed));
    }

    #[test]
    fn inventory_serializes_camel_case() {
        let fake = reader(enabled_facts());
        let json = serde_json::to_value(power_status_with(&&fake).unwrap()).unwrap();
        assert_eq!(
            json["hibernation"]["hiberfileBytes"],
            6_u64 * 1024 * 1024 * 1024
        );
        assert_eq!(json["schemes"][0]["id"], BALANCED_SCHEME);
        assert_eq!(json["schemes"][0]["active"], true);
    }

    #[test]
    fn live_status_is_read_only() {
        let status = power_status().unwrap();
        assert!(status.schemes.len() <= MAX_SCHEMES);
        assert!(!status.schemes.is_empty());
        assert!(status.schemes.iter().filter(|s| s.active).count() <= 1);
        if !status.hibernation.supported {
            assert!(!status.hibernation.enabled);
        }
        let adapter = PowerAdapter::new();
        let active = status.schemes.iter().find(|s| s.active).unwrap();
        let change = SystemChange::SetActivePowerScheme {
            scheme: active.id.clone(),
        };
        let prior = adapter.observe(&change).unwrap();
        assert_eq!(change.is_satisfied_by(&prior), Some(true));
    }
}
