//! Operating-system facts adapters use to gate features. Injected as plain
//! data so tests can cover each supported Windows version and edition.

use serde::Serialize;

use crate::win_registry::{Hive, RegistryKey};

pub const MIN_SUPPORTED_BUILD: u32 = 19045;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Edition {
    Home,
    Pro,
    Education,
    Enterprise,
    Server,
    Other,
}

impl Edition {
    pub fn from_edition_id(value: &str) -> Self {
        let lower = value.to_ascii_lowercase();
        if lower.starts_with("core") {
            Self::Home
        } else if lower.starts_with("professional") {
            Self::Pro
        } else if lower.starts_with("education") {
            Self::Education
        } else if lower.starts_with("enterprise") || lower.starts_with("iot") {
            Self::Enterprise
        } else if lower.starts_with("server") {
            Self::Server
        } else {
            Self::Other
        }
    }

    /// Whether Group Policy values under `SOFTWARE\Policies` are honored.
    pub fn honors_policies(self) -> bool {
        !matches!(self, Self::Home)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OsFacts {
    pub build: u32,
    pub display_version: Option<String>,
    pub edition: Edition,
    /// Joined to a domain or enrolled in MDM.
    pub managed: bool,
}

impl OsFacts {
    pub fn is_windows_11(&self) -> bool {
        self.build >= 22000
    }

    pub fn is_supported(&self) -> bool {
        self.build >= MIN_SUPPORTED_BUILD
    }

    /// Fixtures for the supported matrix: Win10 22H2, Win11 23H2, Win11 24H2.
    pub fn fixture(build: u32, edition: Edition, managed: bool) -> Self {
        Self {
            build,
            display_version: None,
            edition,
            managed,
        }
    }
}

pub const SUPPORTED_BUILDS: [u32; 3] = [19045, 22631, 26100];

pub fn current() -> OsFacts {
    let key = RegistryKey::open_read(
        Hive::LocalMachine,
        r"SOFTWARE\Microsoft\Windows NT\CurrentVersion",
    )
    .ok()
    .flatten();
    let read = |name: &str| key.as_ref().and_then(|key| key.string(name).ok().flatten());
    let build = read("CurrentBuildNumber")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let edition = read("EditionID")
        .map(|value| Edition::from_edition_id(&value))
        .unwrap_or(Edition::Other);
    OsFacts {
        build,
        display_version: read("DisplayVersion"),
        edition,
        managed: is_managed(),
    }
}

fn is_managed() -> bool {
    let domain_joined = RegistryKey::open_read(
        Hive::LocalMachine,
        r"SYSTEM\CurrentControlSet\Services\Tcpip\Parameters",
    )
    .ok()
    .flatten()
    .and_then(|key| key.string("Domain").ok().flatten())
    .is_some_and(|domain| !domain.trim().is_empty());
    // MDM-delivered Windows Update policy (Intune and similar) lands here.
    let mdm_update_policy = RegistryKey::open_read(
        Hive::LocalMachine,
        r"SOFTWARE\Microsoft\PolicyManager\current\device\Update",
    )
    .ok()
    .flatten()
    .and_then(|key| key.values().ok())
    .is_some_and(|values| !values.is_empty());
    domain_joined || mdm_update_policy
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edition_ids_map_to_families() {
        assert_eq!(Edition::from_edition_id("Core"), Edition::Home);
        assert_eq!(
            Edition::from_edition_id("CoreSingleLanguage"),
            Edition::Home
        );
        assert_eq!(Edition::from_edition_id("Professional"), Edition::Pro);
        assert_eq!(Edition::from_edition_id("Enterprise"), Edition::Enterprise);
        assert_eq!(
            Edition::from_edition_id("IoTEnterprise"),
            Edition::Enterprise
        );
        assert!(!Edition::Home.honors_policies());
        assert!(Edition::Pro.honors_policies());
    }

    #[test]
    fn supported_matrix_is_recognized() {
        for build in SUPPORTED_BUILDS {
            assert!(OsFacts::fixture(build, Edition::Pro, false).is_supported());
        }
        assert!(!OsFacts::fixture(19044, Edition::Pro, false).is_supported());
        assert!(OsFacts::fixture(22631, Edition::Pro, false).is_windows_11());
        assert!(!OsFacts::fixture(19045, Edition::Pro, false).is_windows_11());
    }

    #[test]
    fn live_machine_reports_a_build() {
        assert!(current().build > 0);
    }
}
