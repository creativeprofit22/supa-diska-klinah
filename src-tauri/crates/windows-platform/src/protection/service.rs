//! Stateful facade used by the Tauri commands. The webview only ever sends
//! opaque IDs and fixed enums; every path is resolved natively here.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use protection_core::{
    Evidence, HEURISTIC_CATALOG, HeuristicInfo, PackError, ProtectionNetworkPolicy,
};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use super::authenticode::SignerCache;
use super::breach::{self, PasswordBreachResult};
use super::defender_history::{self, DefenderHistory};
use super::fsutil::{self, valid_id};
use super::locations::KnownLocations;
use super::net::{NetClient, NetError, SystemTransport, Transport};
use super::process::{self, ProcessInventory};
use super::quarantine::{Quarantine, QuarantineEntry, QuarantineError, QuarantineRequest};
use super::rules_store::{self, RulesError, RulesStatus, RulesStore};
use super::scan::{
    ScanControl, ScanLimits, ScanSummary, ScanTarget, ScannedFinding, Scanner, quick_targets,
};
use super::{amsi, updates};

const MAX_ALLOWLIST: usize = 10_000;
const MAX_SETTINGS_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProtectionSettings {
    pub network: ProtectionNetworkPolicy,
    /// Separate from `network`: the antivirus may use its own cloud settings.
    pub amsi_enabled: bool,
    #[serde(default)]
    pub allowlist: Vec<String>,
}

/// Why a rule pack was refused; serialized as a stable camelCase code.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RuleRejection {
    BadSignature,
    NotNewer,
    RequiresNewerApp,
    UnsupportedFormat,
    Invalid,
    TooLarge,
    NoPrevious,
}

impl From<&PackError> for RuleRejection {
    fn from(error: &PackError) -> Self {
        match error {
            PackError::BadSignature | PackError::MalformedSignature => Self::BadSignature,
            PackError::Rollback { .. } => Self::NotNewer,
            PackError::RequiresNewerApp(_) => Self::RequiresNewerApp,
            PackError::UnsupportedFormat(_) => Self::UnsupportedFormat,
            PackError::TooLarge => Self::TooLarge,
            PackError::NoPrevious => Self::NoPrevious,
            PackError::Empty
            | PackError::MalformedKey
            | PackError::Malformed(_)
            | PackError::Invalid(_)
            | PackError::DuplicateRuleId(_) => Self::Invalid,
        }
    }
}

/// A rejected pack: serializes as just its [`RuleRejection`] code, while the
/// `PackError` text is kept for the user-facing message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RulePackRejection {
    pub reason: RuleRejection,
    detail: PackError,
}

impl From<PackError> for RulePackRejection {
    fn from(detail: PackError) -> Self {
        Self {
            reason: RuleRejection::from(&detail),
            detail,
        }
    }
}

impl Serialize for RulePackRejection {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.reason.serialize(serializer)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProtectionError {
    Busy,
    InvalidInput,
    NotFound,
    Cancelled,
    Storage,
    NetworkDisabled,
    NetworkFailed,
    RulesRejected(RulePackRejection),
    RulePackMissing,
    RulesDisabled,
    ConfirmationDeclined,
    WindowUnavailable,
    Quarantine(QuarantineError),
}

impl std::fmt::Display for ProtectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Busy => f.write_str("another protection task is running"),
            Self::InvalidInput => f.write_str("invalid request"),
            Self::NotFound => f.write_str("item not found; run the scan again"),
            Self::Cancelled => f.write_str("cancelled"),
            Self::Storage => f.write_str("protection storage is unavailable"),
            Self::NetworkDisabled => f.write_str("this network feature is turned off"),
            Self::NetworkFailed => {
                f.write_str("the service could not be reached; nothing was changed")
            }
            Self::RulesRejected(rejection) => {
                write!(
                    f,
                    "the rule pack was rejected: {}; the current pack is still active",
                    rejection.detail
                )
            }
            Self::RulePackMissing => {
                f.write_str("the chosen folder must contain pack.json and pack.sig")
            }
            Self::RulesDisabled => f.write_str(
                "rule-pack import is disabled until a release signing key is configured",
            ),
            Self::ConfirmationDeclined => f.write_str("cancelled in the confirmation dialog"),
            Self::WindowUnavailable => f.write_str("the application window is unavailable"),
            Self::Quarantine(error) => write!(f, "{error}"),
        }
    }
}

impl From<QuarantineError> for ProtectionError {
    fn from(error: QuarantineError) -> Self {
        Self::Quarantine(error)
    }
}

/// Finding sent to the webview; `id` is opaque and resolves only in the backend.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FindingView {
    pub id: String,
    pub path: String,
    pub sha256: Option<String>,
    pub size: Option<u64>,
    pub evidence: Evidence,
    pub can_quarantine: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanReport {
    pub scope: ScanScope,
    pub summary: ScanSummary,
    pub findings: Vec<FindingView>,
    pub finished_at: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ScanScope {
    Quick,
    Folder,
}

/// Whether a scan is running and how far it has got. Counters are zero when idle.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanStatus {
    pub running: bool,
    pub files_scanned: u64,
    pub bytes_hashed: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtectionOverview {
    pub rules: RulesStatus,
    pub settings: ProtectionSettings,
    pub quarantine_count: usize,
    pub last_scan: Option<ScanSummary>,
    pub heuristics: Vec<HeuristicInfo>,
}

pub struct ProtectionService<T: Transport = SystemTransport> {
    root: PathBuf,
    rules: Mutex<RulesStore>,
    signers: SignerCache,
    known: KnownLocations,
    quarantine: Quarantine,
    settings: Mutex<ProtectionSettings>,
    last_scan: Mutex<Option<(ScanReport, HashMap<String, ScannedFinding>)>>,
    control: Mutex<Option<Arc<ScanControl>>>,
    net: NetClient<T>,
}

/// UTC timestamp `YYYY-MM-DDTHH:MM:SSZ` without a date dependency.
pub fn utc_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (days, rem) = ((secs / 86_400) as i64, secs % 86_400);
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem / 60) % 60,
        rem % 60
    )
}

/// Quarantine record label, or `None` when the evidence cannot justify quarantine.
/// External responses (AMSI) only annotate files that already carry app-owned
/// evidence, and may say "did not report"; labelling them "Reported by …" would
/// overstate detection (ADR 0003 decision 7), so only the original row quarantines.
fn quarantine_label(evidence: &Evidence) -> Option<String> {
    match evidence {
        Evidence::Deterministic { rule_name, .. } => Some(format!("Rule match: {rule_name}")),
        Evidence::Heuristic { heuristic_id, .. } => Some(format!("Heuristic: {heuristic_id}")),
        Evidence::External { .. } | Evidence::Unavailable { .. } => None,
    }
}

fn lock<T>(mutex: &Mutex<T>) -> Result<MutexGuard<'_, T>, ProtectionError> {
    mutex.lock().map_err(|_| ProtectionError::Busy)
}

impl ProtectionService<SystemTransport> {
    pub fn new(app_data: &Path) -> Result<Self, ProtectionError> {
        Self::with_transport(app_data, SystemTransport, KnownLocations::native())
    }
}

impl<T: Transport> ProtectionService<T> {
    pub(crate) fn with_transport(
        app_data: &Path,
        transport: T,
        known: KnownLocations,
    ) -> Result<Self, ProtectionError> {
        let root = app_data.join("protection");
        fs::create_dir_all(&root).map_err(|_| ProtectionError::Storage)?;
        let rules = RulesStore::open(root.join("rules")).map_err(|_| ProtectionError::Storage)?;
        let quarantine =
            Quarantine::open(root.join("quarantine")).map_err(ProtectionError::from)?;
        let settings = Self::load_settings(&root.join("settings.json"));
        Ok(Self {
            root,
            rules: Mutex::new(rules),
            signers: SignerCache::default(),
            known,
            quarantine,
            settings: Mutex::new(settings),
            last_scan: Mutex::new(None),
            control: Mutex::new(None),
            net: NetClient::new(transport),
        })
    }

    #[cfg(test)]
    pub(crate) fn net(&self) -> &NetClient<T> {
        &self.net
    }

    /// Unreadable or invalid settings fail closed to defaults (all network off).
    fn load_settings(path: &Path) -> ProtectionSettings {
        fsutil::read_bounded(path, MAX_SETTINGS_BYTES)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<ProtectionSettings>(&bytes).ok())
            .filter(|s| {
                s.allowlist.len() <= MAX_ALLOWLIST && s.allowlist.iter().all(|h| is_sha256(h))
            })
            .unwrap_or_default()
    }

    fn save_settings(&self, settings: &ProtectionSettings) -> Result<(), ProtectionError> {
        let bytes = serde_json::to_vec_pretty(settings).map_err(|_| ProtectionError::Storage)?;
        fsutil::atomic_replace(&self.root.join("settings.json"), &bytes)
            .map_err(|_| ProtectionError::Storage)
    }

    pub fn overview(&self) -> Result<ProtectionOverview, ProtectionError> {
        Ok(ProtectionOverview {
            rules: lock(&self.rules)?.status().clone(),
            settings: lock(&self.settings)?.clone(),
            quarantine_count: self.quarantine.list().len(),
            last_scan: lock(&self.last_scan)?
                .as_ref()
                .map(|(report, _)| report.summary.clone()),
            heuristics: HEURISTIC_CATALOG.to_vec(),
        })
    }

    pub fn set_network_policy(
        &self,
        network: ProtectionNetworkPolicy,
        amsi_enabled: bool,
    ) -> Result<ProtectionSettings, ProtectionError> {
        let mut settings = lock(&self.settings)?;
        let mut next = settings.clone();
        next.network = network;
        next.amsi_enabled = amsi_enabled;
        self.save_settings(&next)?;
        *settings = next.clone();
        Ok(next)
    }

    pub fn processes(&self) -> Result<ProcessInventory, ProtectionError> {
        process::inventory(&self.signers, &self.known).map_err(|_| ProtectionError::Storage)
    }

    pub fn cancel_scan(&self) -> Result<(), ProtectionError> {
        if let Some(control) = lock(&self.control)?.as_ref() {
            control.cancelled.store(true, Ordering::Relaxed);
        }
        Ok(())
    }

    pub fn scan_status(&self) -> Result<ScanStatus, ProtectionError> {
        Ok(lock(&self.control)?
            .as_ref()
            .map_or_else(ScanStatus::default, |control| ScanStatus {
                running: true,
                files_scanned: control.files_scanned.load(Ordering::Relaxed),
                bytes_hashed: control.bytes_hashed.load(Ordering::Relaxed),
            }))
    }

    /// Run a scan. `folder` is supplied only by the native folder picker.
    pub fn scan(
        &self,
        scope: ScanScope,
        folder: Option<PathBuf>,
    ) -> Result<ScanReport, ProtectionError> {
        let control = Arc::new(ScanControl::default());
        {
            let mut slot = lock(&self.control)?;
            if slot.is_some() {
                return Err(ProtectionError::Busy);
            }
            *slot = Some(Arc::clone(&control));
        }
        let result = self.scan_inner(scope, folder, Arc::clone(&control));
        if let Ok(mut slot) = self.control.lock() {
            *slot = None;
        }
        result
    }

    fn scan_inner(
        &self,
        scope: ScanScope,
        folder: Option<PathBuf>,
        control: Arc<ScanControl>,
    ) -> Result<ScanReport, ProtectionError> {
        let (targets, limits) = match (scope, folder) {
            (ScanScope::Quick, None) => {
                let images = process::inventory(&self.signers, &self.known)
                    .map(|inv| {
                        inv.processes
                            .into_iter()
                            .filter_map(|p| p.image_path.map(PathBuf::from))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                (quick_targets(&self.known, images), ScanLimits::QUICK)
            }
            (ScanScope::Folder, Some(folder)) => {
                (vec![ScanTarget { path: folder }], ScanLimits::FOLDER)
            }
            _ => return Err(ProtectionError::InvalidInput),
        };
        let allowlist: HashSet<String> = lock(&self.settings)?.allowlist.iter().cloned().collect();
        let amsi_enabled = lock(&self.settings)?.amsi_enabled;
        // Compile once, then release the lock so rule updates are not blocked by a scan.
        let pack = lock(&self.rules)?.active().clone();
        let outcome = Scanner {
            pack: &pack,
            signers: &self.signers,
            known: &self.known,
            allowlist: &allowlist,
            limits,
            control,
        }
        .run(&targets);
        let mut findings = outcome.findings;
        if amsi_enabled {
            // Ask the installed antivirus about files that already have findings.
            let observed = utc_now();
            let mut extra = Vec::new();
            let mut asked = HashSet::new();
            for finding in findings
                .iter()
                .filter(|f| !matches!(f.evidence, Evidence::Unavailable { .. }))
            {
                if asked.insert(finding.path.clone()) && asked.len() <= 200 {
                    extra.push(ScannedFinding {
                        evidence: amsi::scan_file(true, &finding.path, &observed),
                        ..finding.clone()
                    });
                }
            }
            findings.extend(extra);
        }
        let mut by_id = HashMap::new();
        let mut views = Vec::with_capacity(findings.len());
        for finding in findings {
            let id = fsutil::random_hex().map_err(|_| ProtectionError::Storage)?;
            views.push(FindingView {
                id: id.clone(),
                path: finding.path.to_string_lossy().into_owned(),
                sha256: finding.sha256.clone(),
                size: finding.size,
                can_quarantine: finding.sha256.is_some()
                    && quarantine_label(&finding.evidence).is_some(),
                evidence: finding.evidence.clone(),
            });
            by_id.insert(id, finding);
        }
        let report = ScanReport {
            scope,
            summary: outcome.summary,
            findings: views,
            finished_at: utc_now(),
        };
        *lock(&self.last_scan)? = Some((report.clone(), by_id));
        Ok(report)
    }

    pub fn last_scan(&self) -> Result<Option<ScanReport>, ProtectionError> {
        Ok(lock(&self.last_scan)?
            .as_ref()
            .map(|(report, _)| report.clone()))
    }

    fn finding(&self, id: &str) -> Result<ScannedFinding, ProtectionError> {
        if !valid_id(id) {
            return Err(ProtectionError::InvalidInput);
        }
        lock(&self.last_scan)?
            .as_ref()
            .and_then(|(_, map)| map.get(id).cloned())
            .ok_or(ProtectionError::NotFound)
    }

    /// Describe what quarantining a finding will do, for the native prompt.
    pub fn quarantine_prompt(&self, finding_id: &str) -> Result<String, ProtectionError> {
        let finding = self.finding(finding_id)?;
        Ok(format!(
            "Move this file into quarantine?\n\n{}\n\nThe file is made non-executable and can be restored later from the Quarantine page.",
            finding.path.display()
        ))
    }

    pub fn quarantine_finding(&self, finding_id: &str) -> Result<QuarantineEntry, ProtectionError> {
        let finding = self.finding(finding_id)?;
        let sha = finding
            .sha256
            .clone()
            .ok_or(ProtectionError::InvalidInput)?;
        let policy = crate::storage::current_protection().map_err(|_| ProtectionError::Storage)?;
        let label = quarantine_label(&finding.evidence).ok_or(ProtectionError::InvalidInput)?;
        let entry = self.quarantine.quarantine(
            &QuarantineRequest {
                source: &finding.path,
                scan_root: &finding.root,
                expected_sha256: Some(&sha),
                finding: &label,
            },
            &|path| policy.is_protected(path),
        )?;
        if let Some((report, map)) = lock(&self.last_scan)?.as_mut() {
            let path = finding.path.clone();
            map.retain(|_, f| f.path != path);
            report.findings.retain(|f| Path::new(&f.path) != path);
        }
        Ok(entry)
    }

    pub fn quarantine_list(&self) -> Vec<QuarantineEntry> {
        self.quarantine.list()
    }

    pub fn restore_prompt(&self, id: &str) -> Result<String, ProtectionError> {
        let target = self.quarantine.restore_target(id)?;
        Ok(format!(
            "Restore this file from quarantine?\n\n{target}\n\nIt becomes executable again. If a file already exists there, nothing is overwritten and the restore is cancelled."
        ))
    }

    pub fn restore(&self, id: &str) -> Result<String, ProtectionError> {
        let policy = crate::storage::current_protection().map_err(|_| ProtectionError::Storage)?;
        self.quarantine
            .restore(id, &|path| policy.is_protected(path))
            .map_err(Into::into)
    }

    pub fn delete_prompt(&self, id: &str) -> Result<String, ProtectionError> {
        let target = self
            .quarantine
            .restore_target(id)
            .unwrap_or_else(|_| "(damaged entry)".into());
        Ok(format!(
            "Permanently delete this quarantined file?\n\n{target}\n\nThis cannot be undone."
        ))
    }

    pub fn delete(&self, id: &str) -> Result<(), ProtectionError> {
        self.quarantine.delete(id).map_err(Into::into)
    }

    /// Dismiss heuristic findings for a file by its SHA-256.
    pub fn allow_hash(&self, finding_id: &str) -> Result<ProtectionSettings, ProtectionError> {
        let finding = self.finding(finding_id)?;
        if !matches!(finding.evidence, Evidence::Heuristic { .. }) {
            return Err(ProtectionError::InvalidInput);
        }
        let sha = finding.sha256.ok_or(ProtectionError::InvalidInput)?;
        let mut settings = lock(&self.settings)?;
        let mut next = settings.clone();
        if !next.allowlist.contains(&sha) {
            if next.allowlist.len() >= MAX_ALLOWLIST {
                return Err(ProtectionError::InvalidInput);
            }
            next.allowlist.push(sha);
        }
        self.save_settings(&next)?;
        *settings = next.clone();
        Ok(next)
    }

    pub fn clear_allowlist(&self) -> Result<ProtectionSettings, ProtectionError> {
        let mut settings = lock(&self.settings)?;
        let mut next = settings.clone();
        next.allowlist.clear();
        self.save_settings(&next)?;
        *settings = next.clone();
        Ok(next)
    }

    fn map_rules(error: RulesError) -> ProtectionError {
        match error {
            RulesError::Disabled => ProtectionError::RulesDisabled,
            RulesError::Storage => ProtectionError::Storage,
            RulesError::Pack(error) => ProtectionError::RulesRejected(error.into()),
        }
    }

    /// Import `pack.json` + `pack.sig` from a folder chosen with the native picker.
    pub fn import_rules(&self, folder: &Path) -> Result<RulesStatus, ProtectionError> {
        if !rules_store::external_packs_allowed() {
            return Err(ProtectionError::RulesDisabled);
        }
        let read = |name: &str, max: u64, too_large: PackError| {
            fsutil::read_bounded(&folder.join(name), max).map_err(|fault| match fault {
                fsutil::FsFault::TooLarge => ProtectionError::RulesRejected(too_large.into()),
                _ => ProtectionError::RulePackMissing,
            })
        };
        let pack = read(
            "pack.json",
            protection_core::MAX_PACK_BYTES as u64,
            PackError::TooLarge,
        )?;
        let sig = read("pack.sig", 256, PackError::MalformedSignature)?;
        lock(&self.rules)?
            .install(&pack, &sig)
            .map_err(Self::map_rules)
    }

    pub fn restore_previous_rules(&self) -> Result<RulesStatus, ProtectionError> {
        lock(&self.rules)?
            .restore_previous()
            .map_err(Self::map_rules)
    }

    pub fn download_rules(&self) -> Result<RulesStatus, ProtectionError> {
        let policy = lock(&self.settings)?.network;
        // Fetch without the rules lock: the network can take a long time, and
        // the overview, imports and scans all need that lock.
        let fetched = updates::fetch_rule_pack(&policy, &self.net).map_err(Self::map_download)?;
        updates::install_rule_pack(&fetched, &mut *lock(&self.rules)?).map_err(Self::map_download)
    }

    fn map_download(error: updates::DownloadError) -> ProtectionError {
        match error {
            updates::DownloadError::Net(NetError::NotPermitted) => ProtectionError::NetworkDisabled,
            updates::DownloadError::Net(_) => ProtectionError::NetworkFailed,
            updates::DownloadError::Rules(error) => Self::map_rules(error),
        }
    }

    pub fn check_password(
        &self,
        password: Zeroizing<Vec<u8>>,
    ) -> Result<PasswordBreachResult, ProtectionError> {
        let policy = lock(&self.settings)?.network;
        breach::check_password(&policy, &self.net, password, utc_now()).map_err(|error| match error
        {
            NetError::NotPermitted => ProtectionError::NetworkDisabled,
            NetError::InvalidRequest => ProtectionError::InvalidInput,
            _ => ProtectionError::NetworkFailed,
        })
    }

    pub fn defender_history(&self) -> DefenderHistory {
        defender_history::read(&utc_now())
    }
}

fn is_sha256(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests;
