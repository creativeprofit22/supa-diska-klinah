//! Optional Windows services: a compiled catalog of services the user may
//! set to Automatic, Manual, or Disabled. Start types are changed by the
//! elevated helper; services are never stopped or started.

pub mod elevated;
mod scm;

#[cfg(test)]
mod tests;

use cleanup_core::system_change::{
    CatalogId, ImpactSummary, PriorState, RestartRequirement, RiskLevel, ServiceStartState,
    ServiceStartType, SystemChange, UnsupportedReason,
};
use serde::Serialize;

use crate::system_change::{AdapterError, SystemAdapter};

pub use scm::ScmServices;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ServiceCategory {
    Telemetry,
    Gaming,
    Legacy,
    Performance,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServiceCatalogEntry {
    /// Lowercase `CatalogId`.
    pub id: &'static str,
    /// SCM key name.
    pub service_name: &'static str,
    pub label: &'static str,
    /// What turning the service off does.
    pub description: &'static str,
    pub risk: RiskLevel,
    pub recommended: ServiceStartType,
    pub category: ServiceCategory,
}

const fn entry(
    id: &'static str,
    service_name: &'static str,
    label: &'static str,
    description: &'static str,
    risk: RiskLevel,
    recommended: ServiceStartType,
    category: ServiceCategory,
) -> ServiceCatalogEntry {
    ServiceCatalogEntry {
        id,
        service_name,
        label,
        description,
        risk,
        recommended,
        category,
    }
}

use RiskLevel::{High, Low, Medium};
use ServiceCategory::{Gaming, Legacy, Other, Performance, Telemetry};
use ServiceStartType::{Disabled, Manual};

static CATALOG: &[ServiceCatalogEntry] = &[
    entry(
        "diagtrack",
        "DiagTrack",
        "Connected User Experiences and Telemetry",
        "Stops sending diagnostic and usage data to Microsoft. Windows keeps working; some feedback and tailored-experience features have less data.",
        Low,
        Disabled,
        Telemetry,
    ),
    entry(
        "dmwappushservice",
        "dmwappushservice",
        "Device Management WAP Push Message Routing",
        "Stops routing device-management push messages. Harmless on personal PCs, but work or school devices enrolled in MDM (Intune) may stop receiving policies.",
        Medium,
        Disabled,
        Telemetry,
    ),
    entry(
        "mapsbroker",
        "MapsBroker",
        "Downloaded Maps Manager",
        "Offline maps downloaded in the Maps app are no longer managed or updated automatically.",
        Low,
        Disabled,
        Other,
    ),
    entry(
        "lfsvc",
        "lfsvc",
        "Geolocation Service",
        "When disabled, apps cannot get your location and automatic time zone and Find My Device stop working. Manual keeps it available on demand.",
        Medium,
        Manual,
        Other,
    ),
    entry(
        "retaildemo",
        "RetailDemo",
        "Retail Demo Service",
        "Only used for store display mode. Disabling it has no effect on normal PCs.",
        Low,
        Disabled,
        Other,
    ),
    entry(
        "remoteregistry",
        "RemoteRegistry",
        "Remote Registry",
        "Other computers can no longer read or change this PC's registry over the network. Some remote administration tools stop working.",
        Low,
        Disabled,
        Legacy,
    ),
    entry(
        "fax",
        "Fax",
        "Fax",
        "Sending and receiving faxes through this PC stops working.",
        Low,
        Disabled,
        Legacy,
    ),
    entry(
        "xblauthmanager",
        "XblAuthManager",
        "Xbox Live Auth Manager",
        "When disabled, Xbox app, Game Pass, and Xbox-enabled games cannot sign in. Manual keeps sign-in working on demand.",
        Medium,
        Manual,
        Gaming,
    ),
    entry(
        "xblgamesave",
        "XblGameSave",
        "Xbox Live Game Save",
        "When disabled, cloud saves for Xbox-enabled games stop syncing and progress can be lost between devices. Manual keeps syncing on demand.",
        Medium,
        Manual,
        Gaming,
    ),
    entry(
        "xboxnetapisvc",
        "XboxNetApiSvc",
        "Xbox Live Networking Service",
        "When disabled, Xbox multiplayer and party chat features may fail to connect. Manual keeps them available on demand.",
        Medium,
        Manual,
        Gaming,
    ),
    entry(
        "xboxgipsvc",
        "XboxGipSvc",
        "Xbox Accessory Management Service",
        "When disabled, Xbox controllers and accessories cannot be configured or receive firmware updates. Manual keeps them working on demand.",
        Medium,
        Manual,
        Gaming,
    ),
    entry(
        "wersvc",
        "WerSvc",
        "Windows Error Reporting Service",
        "Crash and hang reports are no longer collected or sent to Microsoft, and the Reliability Monitor shows fewer problem details.",
        Low,
        Disabled,
        Telemetry,
    ),
    entry(
        "sysmain",
        "SysMain",
        "SysMain (Superfetch)",
        "Stops preloading frequently used apps into memory at startup. Can reduce disk activity on hard drives, but apps may open more slowly.",
        Medium,
        Manual,
        Performance,
    ),
    entry(
        "wsearch",
        "WSearch",
        "Windows Search",
        "Stops background indexing. Start menu, File Explorer, and Outlook searches become much slower and may miss results.",
        Medium,
        Manual,
        Performance,
    ),
    entry(
        "spooler",
        "Spooler",
        "Print Spooler",
        "Printing and printer management depend on this service. If you print, Disabled stops all printing; Manual may delay printing until the spooler starts.",
        High,
        Manual,
        Legacy,
    ),
    entry(
        "phonesvc",
        "PhoneSvc",
        "Phone Service",
        "Manages telephony state for apps such as Phone Link. When disabled, calls through a linked phone may stop working.",
        Low,
        Manual,
        Other,
    ),
    entry(
        "wisvc",
        "wisvc",
        "Windows Insider Service",
        "Windows Insider Program builds and settings stop working. Has no effect if you are not an Insider.",
        Low,
        Disabled,
        Other,
    ),
];

/// The compiled service catalog. The optimizer uses `recommended`.
pub fn catalog() -> &'static [ServiceCatalogEntry] {
    CATALOG
}

/// Resolve a catalog identifier; anything unknown fails closed.
pub(crate) fn lookup(id: &CatalogId) -> Result<&'static ServiceCatalogEntry, AdapterError> {
    CATALOG
        .iter()
        .find(|entry| entry.id == id.as_str())
        .ok_or(AdapterError::Unsupported(UnsupportedReason::NotPresent))
}

/// Current configuration and state of one installed service.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServiceStatus {
    pub start: ServiceStartState,
    pub delayed_auto_start: bool,
    pub running: bool,
}

/// Reads service configuration. A missing service is
/// `Unsupported(NotPresent)`; access denied is `Denied`.
pub trait ServiceReader: Send + Sync {
    fn query(&self, service_name: &str) -> Result<ServiceStatus, AdapterError>;
}

/// Changes only a service's start type. Never stops or starts it.
pub trait ServiceWriter: Send + Sync {
    fn set_start_type(
        &self,
        service_name: &str,
        start_type: ServiceStartType,
    ) -> Result<(), AdapterError>;
}

/// One catalog entry as listed in the UI.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceItem {
    pub id: &'static str,
    pub service_name: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub risk: RiskLevel,
    pub category: ServiceCategory,
    pub recommended: ServiceStartType,
    pub installed: bool,
    /// `None` when the service is absent or its configuration cannot be read.
    pub start: Option<ServiceStartState>,
    pub delayed_auto_start: bool,
    pub running: bool,
}

/// Every catalog entry with its current state on this machine.
pub fn list_services() -> Vec<ServiceItem> {
    inventory(&ScmServices)
}

pub fn inventory(reader: &dyn ServiceReader) -> Vec<ServiceItem> {
    CATALOG
        .iter()
        .map(|entry| {
            let status = reader.query(entry.service_name);
            // Denied/Failed mean the service exists but could not be read;
            // an absent service or unavailable SCM is not reported as installed.
            let installed = !matches!(status, Err(AdapterError::Unsupported(_)));
            let status = status.ok();
            ServiceItem {
                id: entry.id,
                service_name: entry.service_name,
                label: entry.label,
                description: entry.description,
                risk: entry.risk,
                category: entry.category,
                recommended: entry.recommended,
                installed,
                start: status.map(|status| status.start),
                delayed_auto_start: status.is_some_and(|status| status.delayed_auto_start),
                running: status.is_some_and(|status| status.running),
            }
        })
        .collect()
}

fn start_label(start: ServiceStartType) -> &'static str {
    match start {
        ServiceStartType::Automatic => "Automatic",
        ServiceStartType::Manual => "Manual",
        ServiceStartType::Disabled => "Disabled",
    }
}

/// Standard-integrity view of service changes: describes and observes.
/// Writes are helper-privileged and never reach `apply`.
pub struct ServicesAdapter {
    reader: Box<dyn ServiceReader>,
}

impl ServicesAdapter {
    pub fn new() -> Self {
        Self::with_reader(Box::new(ScmServices))
    }

    pub fn with_reader(reader: Box<dyn ServiceReader>) -> Self {
        Self { reader }
    }
}

impl Default for ServicesAdapter {
    fn default() -> Self {
        Self::new()
    }
}

fn service_target(
    change: &SystemChange,
) -> Result<(&'static ServiceCatalogEntry, ServiceStartType), AdapterError> {
    match change {
        SystemChange::SetServiceStartType {
            catalog_id,
            start_type,
        } => Ok((lookup(catalog_id)?, *start_type)),
        _ => Err(AdapterError::Failed),
    }
}

impl SystemAdapter for ServicesAdapter {
    fn describe(&self, change: &SystemChange) -> Result<ImpactSummary, AdapterError> {
        let (entry, start_type) = service_target(change)?;
        let effect = match start_type {
            ServiceStartType::Automatic => "Set start type to Automatic: the service starts with Windows from the next restart. It is not started now.".to_owned(),
            other => format!(
                "Set start type to {}: {} If the service is running, it keeps running until the next restart.",
                start_label(other),
                entry.description
            ),
        };
        Ok(ImpactSummary {
            component: entry.label.to_owned(),
            effect,
            restart: RestartRequirement::None,
            risk: match start_type {
                ServiceStartType::Automatic => RiskLevel::Low,
                _ => entry.risk,
            },
        })
    }

    fn observe(&self, change: &SystemChange) -> Result<PriorState, AdapterError> {
        let (entry, _) = service_target(change)?;
        Ok(PriorState::ServiceStart {
            start: self.reader.query(entry.service_name)?.start,
        })
    }
}
