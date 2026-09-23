//! Privacy and performance tweaks: a static catalog of registry values and
//! scheduled tasks, a store abstraction so tests can run without touching the
//! machine, and a `SystemAdapter` for the per-user settings.

use std::io;

use cleanup_core::system_change::{
    ImpactSummary, PriorState, RestartRequirement, RiskLevel, SystemChange, UnsupportedReason,
};
use serde::Serialize;

use crate::os_info::{self, Edition, OsFacts};
use crate::system_change::{AdapterError, SystemAdapter};
use crate::win_registry::{Hive, RegistryData, RegistryKey, is_not_found};

#[cfg(test)]
mod tests;

pub trait PrivacyStore: Send + Sync {
    fn read_value(&self, hive: Hive, key: &str, name: &str) -> Result<Option<u32>, AdapterError>;
    fn write_value(
        &self,
        hive: Hive,
        key: &str,
        name: &str,
        value: Option<u32>,
    ) -> Result<(), AdapterError>;
    fn task_enabled(&self, path: &str) -> Result<Option<bool>, AdapterError>;
    fn set_task_enabled(&self, path: &str, enabled: bool) -> Result<(), AdapterError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SettingHive {
    User,
    Machine,
}

impl SettingHive {
    pub fn hive(self) -> Hive {
        match self {
            Self::User => Hive::CurrentUser,
            Self::Machine => Hive::LocalMachine,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SettingCategory {
    Privacy,
    Performance,
}

#[derive(Clone, Copy, Debug)]
pub struct SettingEntry {
    pub id: &'static str,
    pub hive: SettingHive,
    pub key: &'static str,
    pub value_name: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub category: SettingCategory,
    pub recommended: Option<u32>,
    pub allowed: &'static [u32],
    pub requires_policy_support: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct TaskEntry {
    pub id: &'static str,
    pub path: &'static str,
    pub label: &'static str,
    pub recommended_enabled: bool,
}

pub const RELATED_SERVICES: &[&str] = &["diagtrack", "dmwappushservice"];

const BIN: &[u32] = &[0, 1];
const LEVELS: &[u32] = &[0, 1, 2, 3];

#[allow(clippy::too_many_arguments)]
const fn user(
    id: &'static str,
    key: &'static str,
    value_name: &'static str,
    label: &'static str,
    description: &'static str,
    category: SettingCategory,
    recommended: u32,
    allowed: &'static [u32],
) -> SettingEntry {
    SettingEntry {
        id,
        hive: SettingHive::User,
        key,
        value_name,
        label,
        description,
        category,
        recommended: Some(recommended),
        allowed,
        requires_policy_support: false,
    }
}

#[allow(clippy::too_many_arguments)]
const fn machine(
    id: &'static str,
    key: &'static str,
    value_name: &'static str,
    label: &'static str,
    description: &'static str,
    recommended: u32,
    allowed: &'static [u32],
    requires_policy_support: bool,
) -> SettingEntry {
    SettingEntry {
        id,
        hive: SettingHive::Machine,
        key,
        value_name,
        label,
        description,
        category: SettingCategory::Privacy,
        recommended: Some(recommended),
        allowed,
        requires_policy_support,
    }
}

use SettingCategory::{Performance, Privacy};

static SETTINGS: [SettingEntry; 22] = [
    user(
        "advertising-id",
        r"Software\Microsoft\Windows\CurrentVersion\AdvertisingInfo",
        "Enabled",
        "Advertising ID",
        "Let apps use your advertising ID for personalized ads.",
        Privacy,
        0,
        BIN,
    ),
    user(
        "tailored-experiences",
        r"Software\Microsoft\Windows\CurrentVersion\Privacy",
        "TailoredExperiencesWithDiagnosticDataEnabled",
        "Tailored experiences",
        "Use diagnostic data to offer personalized tips and ads.",
        Privacy,
        0,
        BIN,
    ),
    user(
        "suggested-content-settings",
        r"Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager",
        "SubscribedContent-338393Enabled",
        "Suggested content in Settings",
        "Show suggested content in the Settings app.",
        Privacy,
        0,
        BIN,
    ),
    user(
        "start-suggestions",
        r"Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager",
        "SystemPaneSuggestionsEnabled",
        "Start suggestions",
        "Show app suggestions in Start.",
        Privacy,
        0,
        BIN,
    ),
    user(
        "silent-app-installs",
        r"Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager",
        "SilentInstalledAppsEnabled",
        "Silent app installs",
        "Allow Windows to install suggested apps automatically.",
        Privacy,
        0,
        BIN,
    ),
    user(
        "app-launch-tracking",
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced",
        "Start_TrackProgs",
        "App launch tracking",
        "Track app launches to improve Start and search results.",
        Privacy,
        0,
        BIN,
    ),
    user(
        "feedback-frequency",
        r"Software\Microsoft\Siuf\Rules",
        "NumberOfSIUFInPeriod",
        "Feedback frequency",
        "How often Windows asks for feedback.",
        Privacy,
        0,
        BIN,
    ),
    user(
        "online-speech",
        r"Software\Microsoft\Speech_OneCore\Settings\OnlineSpeechPrivacy",
        "HasAccepted",
        "Online speech recognition",
        "Send voice data to Microsoft for online speech recognition.",
        Privacy,
        0,
        BIN,
    ),
    user(
        "bing-start-search",
        r"Software\Microsoft\Windows\CurrentVersion\Search",
        "BingSearchEnabled",
        "Bing in Start search",
        "Include web results from Bing in Start search.",
        Privacy,
        0,
        BIN,
    ),
    user(
        "visual-effects",
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\VisualEffects",
        "VisualFXSetting",
        "Visual effects",
        "0 = let Windows choose, 1 = best appearance, 2 = best performance, 3 = custom.",
        Performance,
        2,
        LEVELS,
    ),
    user(
        "game-dvr",
        r"System\GameConfigStore",
        "GameDVR_Enabled",
        "Game DVR",
        "Record gameplay in the background.",
        Performance,
        0,
        BIN,
    ),
    user(
        "game-mode",
        r"Software\Microsoft\GameBar",
        "AutoGameModeEnabled",
        "Game Mode",
        "Prioritize games when they are running.",
        Performance,
        1,
        BIN,
    ),
    user(
        "transparency",
        r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
        "EnableTransparency",
        "Transparency effects",
        "Translucent windows and taskbar.",
        Performance,
        0,
        BIN,
    ),
    machine(
        "telemetry-level",
        r"SOFTWARE\Policies\Microsoft\Windows\DataCollection",
        "AllowTelemetry",
        "Diagnostic data level",
        "Policy level for diagnostic data. 0 (Security) is honored only on Enterprise and Education; other editions treat it as 1.",
        1,
        LEVELS,
        false,
    ),
    machine(
        "activity-history-publish",
        r"SOFTWARE\Policies\Microsoft\Windows\System",
        "PublishUserActivities",
        "Publish activity history",
        "Allow publishing user activities.",
        0,
        BIN,
        false,
    ),
    machine(
        "activity-history-upload",
        r"SOFTWARE\Policies\Microsoft\Windows\System",
        "UploadUserActivities",
        "Upload activity history",
        "Allow uploading user activities to Microsoft.",
        0,
        BIN,
        false,
    ),
    machine(
        "activity-feed",
        r"SOFTWARE\Policies\Microsoft\Windows\System",
        "EnableActivityFeed",
        "Activity feed",
        "Enable the activity feed.",
        0,
        BIN,
        false,
    ),
    machine(
        "advertising-id-policy",
        r"SOFTWARE\Policies\Microsoft\Windows\AdvertisingInfo",
        "DisabledByGroupPolicy",
        "Disable advertising ID (policy)",
        "Turn off the advertising ID for all users.",
        1,
        BIN,
        false,
    ),
    machine(
        "location-policy",
        r"SOFTWARE\Policies\Microsoft\Windows\LocationAndSensors",
        "DisableLocation",
        "Disable location (policy)",
        "Turn off location services for all users.",
        1,
        BIN,
        false,
    ),
    machine(
        "cortana",
        r"SOFTWARE\Policies\Microsoft\Windows\Windows Search",
        "AllowCortana",
        "Cortana",
        "Allow Cortana.",
        0,
        BIN,
        false,
    ),
    machine(
        "consumer-features",
        r"SOFTWARE\Policies\Microsoft\Windows\CloudContent",
        "DisableWindowsConsumerFeatures",
        "Disable consumer features",
        "Stop Windows from installing promoted apps. Honored only on Pro and higher editions.",
        1,
        BIN,
        true,
    ),
    machine(
        "error-reporting",
        r"SOFTWARE\Microsoft\Windows\Windows Error Reporting",
        "Disabled",
        "Disable error reporting",
        "Turn off Windows Error Reporting.",
        1,
        BIN,
        false,
    ),
];

static TASKS: [TaskEntry; 7] = [
    TaskEntry {
        id: "compat-appraiser",
        path: r"\Microsoft\Windows\Application Experience\Microsoft Compatibility Appraiser",
        label: "Compatibility Appraiser",
        recommended_enabled: false,
    },
    TaskEntry {
        id: "program-data-updater",
        path: r"\Microsoft\Windows\Application Experience\ProgramDataUpdater",
        label: "Program Data Updater",
        recommended_enabled: false,
    },
    TaskEntry {
        id: "ceip-consolidator",
        path: r"\Microsoft\Windows\Customer Experience Improvement Program\Consolidator",
        label: "CEIP Consolidator",
        recommended_enabled: false,
    },
    TaskEntry {
        id: "ceip-usb",
        path: r"\Microsoft\Windows\Customer Experience Improvement Program\UsbCeip",
        label: "CEIP USB",
        recommended_enabled: false,
    },
    TaskEntry {
        id: "feedback-dmclient",
        path: r"\Microsoft\Windows\Feedback\Siuf\DmClient",
        label: "Feedback DmClient",
        recommended_enabled: false,
    },
    TaskEntry {
        id: "feedback-dmclient-download",
        path: r"\Microsoft\Windows\Feedback\Siuf\DmClientOnScenarioDownload",
        label: "Feedback scenario download",
        recommended_enabled: false,
    },
    TaskEntry {
        id: "wer-queue",
        path: r"\Microsoft\Windows\Windows Error Reporting\QueueReporting",
        label: "Error report queue",
        recommended_enabled: false,
    },
];

pub fn settings_catalog() -> &'static [SettingEntry] {
    &SETTINGS
}

pub fn task_catalog() -> &'static [TaskEntry] {
    &TASKS
}

fn find_setting(id: &str) -> Option<&'static SettingEntry> {
    SETTINGS.iter().find(|entry| entry.id == id)
}

fn find_task(id: &str) -> Option<&'static TaskEntry> {
    TASKS.iter().find(|entry| entry.id == id)
}

fn not_present() -> AdapterError {
    AdapterError::Unsupported(UnsupportedReason::NotPresent)
}

fn setting_for(id: &str, hive: SettingHive) -> Result<&'static SettingEntry, AdapterError> {
    find_setting(id)
        .filter(|entry| entry.hive == hive)
        .ok_or_else(not_present)
}

fn check_edition(entry: &SettingEntry, os: &OsFacts) -> Result<(), AdapterError> {
    if entry.requires_policy_support
        && (os.edition == Edition::Home || !os.edition.honors_policies())
    {
        return Err(AdapterError::Unsupported(
            UnsupportedReason::EditionUnsupported,
        ));
    }
    Ok(())
}

fn check_value(entry: &SettingEntry, value: Option<u32>) -> Result<(), AdapterError> {
    match value {
        Some(v) if !entry.allowed.contains(&v) => Err(AdapterError::Failed),
        _ => Ok(()),
    }
}

pub fn observe_setting(
    store: &dyn PrivacyStore,
    os: &OsFacts,
    id: &str,
    hive: SettingHive,
) -> Result<PriorState, AdapterError> {
    let entry = setting_for(id, hive)?;
    check_edition(entry, os)?;
    let value = store.read_value(hive.hive(), entry.key, entry.value_name)?;
    Ok(PriorState::RegistryValue { value })
}

pub fn apply_setting(
    store: &dyn PrivacyStore,
    os: &OsFacts,
    id: &str,
    hive: SettingHive,
    value: Option<u32>,
) -> Result<(), AdapterError> {
    let entry = setting_for(id, hive)?;
    check_edition(entry, os)?;
    check_value(entry, value)?;
    store.write_value(hive.hive(), entry.key, entry.value_name, value)
}

pub fn observe_task(store: &dyn PrivacyStore, id: &str) -> Result<PriorState, AdapterError> {
    let entry = find_task(id).ok_or_else(not_present)?;
    match store.task_enabled(entry.path)? {
        Some(enabled) => Ok(PriorState::Enabled { enabled }),
        None => Err(not_present()),
    }
}

pub fn apply_task(store: &dyn PrivacyStore, id: &str, enabled: bool) -> Result<(), AdapterError> {
    let entry = find_task(id).ok_or_else(not_present)?;
    if store.task_enabled(entry.path)?.is_none() {
        return Err(not_present());
    }
    store.set_task_enabled(entry.path, enabled)
}

pub struct PrivacyAdapter {
    store: Box<dyn PrivacyStore>,
    os: OsFacts,
}

impl Default for PrivacyAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl PrivacyAdapter {
    pub fn new() -> Self {
        Self::with(Box::new(WindowsPrivacyStore), os_info::current())
    }

    pub fn with(store: Box<dyn PrivacyStore>, os: OsFacts) -> Self {
        Self { store, os }
    }
}

fn impact(component: &str, effect: String, risk: RiskLevel) -> ImpactSummary {
    ImpactSummary {
        component: component.to_string(),
        effect,
        restart: RestartRequirement::None,
        risk,
    }
}

impl SystemAdapter for PrivacyAdapter {
    fn describe(&self, change: &SystemChange) -> Result<ImpactSummary, AdapterError> {
        match change {
            SystemChange::SetUserSetting { setting_id, value } => {
                let entry = setting_for(setting_id.as_str(), SettingHive::User)?;
                Ok(impact(
                    entry.label,
                    format!("Set {} to {}", entry.value_name, show(*value)),
                    RiskLevel::Low,
                ))
            }
            SystemChange::SetMachineSetting { setting_id, value } => {
                let entry = setting_for(setting_id.as_str(), SettingHive::Machine)?;
                Ok(impact(
                    entry.label,
                    format!("Set policy {} to {}", entry.value_name, show(*value)),
                    RiskLevel::Medium,
                ))
            }
            SystemChange::SetSystemTaskEnabled {
                catalog_id,
                enabled,
            } => {
                let entry = find_task(catalog_id.as_str()).ok_or_else(not_present)?;
                let verb = if *enabled { "Enable" } else { "Disable" };
                Ok(impact(
                    entry.label,
                    format!("{verb} scheduled task {}", entry.path),
                    RiskLevel::Medium,
                ))
            }
            _ => Err(not_present()),
        }
    }

    fn observe(&self, change: &SystemChange) -> Result<PriorState, AdapterError> {
        let store = self.store.as_ref();
        match change {
            SystemChange::SetUserSetting { setting_id, .. } => {
                observe_setting(store, &self.os, setting_id.as_str(), SettingHive::User)
            }
            SystemChange::SetMachineSetting { setting_id, .. } => {
                observe_setting(store, &self.os, setting_id.as_str(), SettingHive::Machine)
            }
            SystemChange::SetSystemTaskEnabled { catalog_id, .. } => {
                observe_task(store, catalog_id.as_str())
            }
            _ => Err(not_present()),
        }
    }

    fn apply(&self, change: &SystemChange) -> Result<(), AdapterError> {
        match change {
            SystemChange::SetUserSetting { setting_id, value } => apply_setting(
                self.store.as_ref(),
                &self.os,
                setting_id.as_str(),
                SettingHive::User,
                *value,
            ),
            _ => Err(AdapterError::Failed),
        }
    }
}

fn show(value: Option<u32>) -> String {
    value.map_or_else(|| "default (removed)".to_string(), |v| v.to_string())
}

pub mod elevated {
    use super::{
        PrivacyStore, SettingHive, WindowsPrivacyStore, apply_setting, apply_task, observe_setting,
        observe_task,
    };
    use crate::os_info::{self, OsFacts};
    use crate::security::system_changes::HelperChange;
    use crate::system_change::AdapterError;
    use cleanup_core::system_change::PriorState;

    pub fn observe(change: &HelperChange) -> Result<PriorState, AdapterError> {
        observe_with(&WindowsPrivacyStore, &os_info::current(), change)
    }

    pub fn apply(change: &HelperChange) -> Result<(), AdapterError> {
        apply_with(&WindowsPrivacyStore, &os_info::current(), change)
    }

    pub fn observe_with(
        store: &dyn PrivacyStore,
        os: &OsFacts,
        change: &HelperChange,
    ) -> Result<PriorState, AdapterError> {
        match change {
            HelperChange::SetMachinePolicyValue { setting_id, .. } => {
                observe_setting(store, os, setting_id.as_str(), SettingHive::Machine)
            }
            HelperChange::SetSystemTaskEnabled { catalog_id, .. } => {
                observe_task(store, catalog_id.as_str())
            }
            _ => Err(AdapterError::Failed),
        }
    }

    pub fn apply_with(
        store: &dyn PrivacyStore,
        os: &OsFacts,
        change: &HelperChange,
    ) -> Result<(), AdapterError> {
        match change {
            HelperChange::SetMachinePolicyValue { setting_id, value } => {
                apply_setting(store, os, setting_id.as_str(), SettingHive::Machine, *value)
            }
            HelperChange::SetSystemTaskEnabled {
                catalog_id,
                enabled,
            } => apply_task(store, catalog_id.as_str(), *enabled),
            _ => Err(AdapterError::Failed),
        }
    }
}

// ---------------------------------------------------------------- report

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivacySettingReport {
    pub id: &'static str,
    pub hive: SettingHive,
    pub label: &'static str,
    pub description: &'static str,
    pub category: SettingCategory,
    pub current: Option<u32>,
    pub recommended: Option<u32>,
    pub applied: bool,
    pub supported: bool,
    pub unsupported_reason: Option<UnsupportedReason>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivacyTaskReport {
    pub id: &'static str,
    pub path: &'static str,
    pub label: &'static str,
    pub enabled: Option<bool>,
    pub recommended: bool,
    pub present: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivacyReport {
    pub settings: Vec<PrivacySettingReport>,
    pub tasks: Vec<PrivacyTaskReport>,
    pub related_services: Vec<&'static str>,
}

pub fn privacy_report() -> PrivacyReport {
    privacy_report_with(&WindowsPrivacyStore, &os_info::current())
}

pub fn privacy_report_with(store: &dyn PrivacyStore, os: &OsFacts) -> PrivacyReport {
    let settings = SETTINGS
        .iter()
        .map(|entry| {
            let (current, reason) = match observe_setting(store, os, entry.id, entry.hive) {
                Ok(PriorState::RegistryValue { value }) => (value, None),
                Err(AdapterError::Unsupported(reason)) => (None, Some(reason)),
                _ => (None, None),
            };
            PrivacySettingReport {
                id: entry.id,
                hive: entry.hive,
                label: entry.label,
                description: entry.description,
                category: entry.category,
                current,
                recommended: entry.recommended,
                applied: reason.is_none() && current == entry.recommended,
                supported: reason.is_none(),
                unsupported_reason: reason,
            }
        })
        .collect();
    let tasks = TASKS
        .iter()
        .map(|entry| {
            let enabled = store.task_enabled(entry.path).ok().flatten();
            PrivacyTaskReport {
                id: entry.id,
                path: entry.path,
                label: entry.label,
                enabled,
                recommended: entry.recommended_enabled,
                present: enabled.is_some(),
            }
        })
        .collect();
    PrivacyReport {
        settings,
        tasks,
        related_services: RELATED_SERVICES.to_vec(),
    }
}

// ---------------------------------------------------------------- Windows store

pub struct WindowsPrivacyStore;

fn io_error(error: io::Error) -> AdapterError {
    if error.kind() == io::ErrorKind::PermissionDenied {
        AdapterError::Denied
    } else {
        AdapterError::Failed
    }
}

impl PrivacyStore for WindowsPrivacyStore {
    fn read_value(&self, hive: Hive, key: &str, name: &str) -> Result<Option<u32>, AdapterError> {
        let Some(key) = RegistryKey::open_read(hive, key).map_err(io_error)? else {
            return Ok(None);
        };
        match key.value(name).map_err(io_error)? {
            None => Ok(None),
            Some(RegistryData::Dword(value)) => Ok(Some(value)),
            Some(_) => Err(not_present()),
        }
    }

    fn write_value(
        &self,
        hive: Hive,
        key: &str,
        name: &str,
        value: Option<u32>,
    ) -> Result<(), AdapterError> {
        match value {
            Some(value) => RegistryKey::create_write(hive, key)
                .map_err(io_error)?
                .set_dword(name, value)
                .map_err(io_error),
            None => {
                let Some(key) = RegistryKey::open_write(hive, key).map_err(io_error)? else {
                    return Ok(());
                };
                match key.delete_value(name) {
                    Err(error) if is_not_found(&error) => Ok(()),
                    other => other.map_err(io_error),
                }
            }
        }
    }

    fn task_enabled(&self, path: &str) -> Result<Option<bool>, AdapterError> {
        tasks::enabled(path)
    }

    fn set_task_enabled(&self, path: &str, enabled: bool) -> Result<(), AdapterError> {
        tasks::set_enabled(path, enabled)
    }
}

mod tasks {
    use windows::Win32::Foundation::{
        E_ACCESSDENIED, ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, RPC_E_CHANGED_MODE,
        VARIANT_BOOL,
    };
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
        CoUninitialize,
    };
    use windows::Win32::System::TaskScheduler::{IRegisteredTask, ITaskService, TaskScheduler};
    use windows::Win32::System::Variant::VARIANT;
    use windows::core::{BSTR, HRESULT};

    use crate::system_change::AdapterError;

    struct ComGuard(bool);

    impl Drop for ComGuard {
        fn drop(&mut self) {
            if self.0 {
                // SAFETY: balanced with a successful CoInitializeEx on this thread.
                unsafe { CoUninitialize() };
            }
        }
    }

    fn init() -> Result<ComGuard, AdapterError> {
        // SAFETY: plain COM initialization for the current thread.
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if hr.is_ok() {
            Ok(ComGuard(true))
        } else if hr == RPC_E_CHANGED_MODE {
            Ok(ComGuard(false))
        } else {
            Err(AdapterError::Failed)
        }
    }

    fn map(error: windows::core::Error) -> AdapterError {
        if error.code() == E_ACCESSDENIED {
            AdapterError::Denied
        } else {
            AdapterError::Failed
        }
    }

    fn is_missing(error: &windows::core::Error) -> bool {
        let code = error.code();
        code == HRESULT::from_win32(ERROR_FILE_NOT_FOUND.0)
            || code == HRESULT::from_win32(ERROR_PATH_NOT_FOUND.0)
    }

    fn with_task<T>(
        path: &str,
        f: impl FnOnce(&IRegisteredTask) -> windows::core::Result<T>,
    ) -> Result<Option<T>, AdapterError> {
        let _guard = init()?;
        let (folder, name) = match path.rfind('\\') {
            Some(0) => ("\\", &path[1..]),
            Some(index) => (&path[..index], &path[index + 1..]),
            None => ("\\", path),
        };
        // SAFETY: COM calls on interfaces obtained from the Task Scheduler
        // service; every result is checked.
        unsafe {
            let service: ITaskService =
                CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER).map_err(map)?;
            let empty = VARIANT::default();
            service
                .Connect(&empty, &empty, &empty, &empty)
                .map_err(map)?;
            let folder = match service.GetFolder(&BSTR::from(folder)) {
                Ok(folder) => folder,
                Err(error) if is_missing(&error) => return Ok(None),
                Err(error) => return Err(map(error)),
            };
            let task = match folder.GetTask(&BSTR::from(name)) {
                Ok(task) => task,
                Err(error) if is_missing(&error) => return Ok(None),
                Err(error) => return Err(map(error)),
            };
            f(&task).map(Some).map_err(map)
        }
    }

    pub(super) fn enabled(path: &str) -> Result<Option<bool>, AdapterError> {
        // SAFETY: `task` is a live registered task interface.
        with_task(path, |task| unsafe { task.Enabled() }.map(|v| v.as_bool()))
    }

    pub(super) fn set_enabled(path: &str, enabled: bool) -> Result<(), AdapterError> {
        // SAFETY: `task` is a live registered task interface.
        match with_task(path, |task| unsafe {
            task.SetEnabled(VARIANT_BOOL::from(enabled))
        })? {
            Some(()) => Ok(()),
            None => Err(AdapterError::Unsupported(
                cleanup_core::system_change::UnsupportedReason::NotPresent,
            )),
        }
    }
}
