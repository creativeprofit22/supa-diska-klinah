//! Opt-in, user-approved app self-update (distinct from `updates`, which is the
//! Windows Update policy feature).
//!
//! Flow: `check` fetches the Ed25519-signed manifest over the fixed-host sink;
//! `download` streams the installer to `<app_data>/updates/<name>.part` while
//! hashing, requires the manifest's exact size and SHA-256, then renames it and
//! applies the no-downgrade Authenticode policy; `install` re-verifies the file
//! through a handle that denies writes and deletes, asks natively (default No),
//! records `launched`, and opens the installer without elevation (the
//! per-machine NSIS installer raises its own UAC prompt).
//!
//! State lives in `<app_data>/updates/state.json` (atomic writes). Only files
//! whose names equal `AppVersion::installer_name()` (plus `.part`) are ever
//! touched; paths are never read from the state file.

#[cfg(windows)]
pub mod launch;
#[cfg(windows)]
pub mod native_ui;

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Re-exported so the app crate depends only on windows-platform (ADR 0003).
pub use protection_core::UpdateCheckPolicy;
use protection_core::{
    AppVersion, UpdateManifest, UpdateManifestError, UpdateSigning, UpdateVerifier, from_hex,
    to_hex,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::cleanup::{read_json, write_json};
use crate::protection::authenticode::AuthenticodeVerdict;
use crate::protection::net::{Endpoint, NetClient, NetError, Transport};

/// The embedded update public key (`src-tauri/keys/update.pub`). While it holds
/// the `unconfigured` marker, updates report `NotConfigured`.
pub fn embedded_verifier() -> Result<UpdateVerifier, UpdateManifestError> {
    UpdateVerifier::from_key_file(include_str!("../../../../keys/update.pub"))
}

/// Reports the Authenticode state of a file through an open handle.
pub trait AuthenticodeProbe: Send + Sync {
    fn verdict(&self, path: &Path, file: &File) -> AuthenticodeVerdict;
}

/// `WinVerifyTrust`-backed probe.
pub struct SystemProbe;

impl AuthenticodeProbe for SystemProbe {
    fn verdict(&self, path: &Path, file: &File) -> AuthenticodeVerdict {
        crate::protection::authenticode::signer_thumbprint(path, Some(file))
    }
}

/// Opens a verified installer. Production: `launch::ShellLauncher`.
pub trait InstallerLauncher: Send + Sync {
    #[allow(clippy::result_unit_err)]
    fn launch(&self, installer: &Path) -> Result<(), ()>;
}

/// Typed failures surfaced to the UI as codes; no paths or free text.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UpdateError {
    /// This build has no update key.
    NotConfigured,
    /// Update checks are turned off.
    NotPermitted,
    /// Another update operation is running.
    Busy,
    /// The update server could not be reached.
    Network,
    /// The manifest failed signature or policy checks.
    BadManifest,
    /// Download requested without a successful check.
    NoUpdateChecked,
    /// The download failed or was interrupted.
    DownloadFailed,
    /// Size or SHA-256 does not match the signed manifest.
    IntegrityFailed,
    /// The Windows signature does not satisfy the update policy.
    SignatureRejected,
    /// Local staging could not be written or read.
    Storage,
    /// Install requested without a verified download.
    NothingToInstall,
    /// The user answered No.
    Declined,
    /// Windows could not open the installer.
    LaunchFailed,
}

/// The persisted description of a staged installer (re-verified on every use).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Staged {
    version: String,
    size: u64,
    sha256: String,
    signing: UpdateSigning,
    signer_thumbprint: Option<String>,
}

impl Staged {
    fn from_manifest(manifest: &UpdateManifest) -> Self {
        Self {
            version: manifest.version.to_string(),
            size: manifest.installer_size,
            sha256: to_hex(&manifest.installer_sha256),
            signing: manifest.signing,
            signer_thumbprint: manifest.signer_thumbprint.clone(),
        }
    }

    fn version(&self) -> Option<AppVersion> {
        AppVersion::parse(&self.version)
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase", deny_unknown_fields)]
enum Stored {
    #[default]
    Idle,
    Verified {
        staged: Staged,
    },
    /// The installer was opened; success is confirmed at the next start.
    Launched {
        staged: Staged,
    },
}

/// What the UI shows.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum UpdateState {
    /// Startup recovery has not finished re-checking the persisted state yet;
    /// a staged or interrupted update may still be on disk.
    Recovering,
    Idle,
    UpToDate,
    Available {
        version: String,
        size: u64,
    },
    Downloading {
        version: String,
    },
    Verified {
        version: String,
    },
    Launched {
        version: String,
    },
    /// A launched install did not finish (cancelled UAC, closed installer, power loss).
    Interrupted {
        version: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub current_version: String,
    /// False when this build has no update key.
    pub configured: bool,
    pub update: UpdateState,
    /// The last startup recovery result, until the UI acknowledges it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery: Option<RecoveryOutcome>,
    /// True until startup recovery has run, so the UI knows to ask again.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub recovery_pending: bool,
}

/// Result of startup recovery, for a one-time notice.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "camelCase")]
pub enum RecoveryOutcome {
    /// Nothing pending.
    Clean,
    /// The launched update is now running; staging was cleaned up.
    Updated { version: String },
    /// The launched update did not finish; the verified installer is kept for a retry.
    Interrupted { version: String },
    /// The pending installer no longer verified and was removed.
    Discarded,
}

struct Inner {
    stored: Stored,
    checked: Option<UpdateManifest>,
    up_to_date: bool,
    downloading: Option<String>,
    interrupted: Option<String>,
    running_signer: Option<AuthenticodeVerdict>,
    /// A notable startup recovery result (never `Clean`), shown once.
    recovery: Option<RecoveryOutcome>,
    recovery_pending: bool,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            stored: Stored::Idle,
            checked: None,
            up_to_date: false,
            downloading: None,
            interrupted: None,
            running_signer: None,
            recovery: None,
            recovery_pending: true,
        }
    }
}

pub struct UpdateService<T: Transport> {
    dir: PathBuf,
    running: AppVersion,
    running_exe: PathBuf,
    net: NetClient<T>,
    verifier: Result<UpdateVerifier, UpdateManifestError>,
    clock: fn() -> u64,
    probe: Box<dyn AuthenticodeProbe>,
    launcher: Box<dyn InstallerLauncher>,
    /// Serialises check/download/install/discard/recover.
    operation: Mutex<()>,
    inner: Mutex<Inner>,
}

pub struct UpdateServiceConfig<T: Transport> {
    pub app_data: PathBuf,
    /// The running app's `major.minor.patch`.
    pub running_version: String,
    pub running_exe: PathBuf,
    pub transport: T,
    pub verifier: Result<UpdateVerifier, UpdateManifestError>,
    pub clock: fn() -> u64,
    pub probe: Box<dyn AuthenticodeProbe>,
    pub launcher: Box<dyn InstallerLauncher>,
}

/// Seconds since the Unix epoch (0 if the clock is before 1970).
pub fn system_clock() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// Access mask sharing: others may read (and execute) but never write or delete
/// the installer while this handle is open.
#[cfg(windows)]
fn open_deny_write(path: &Path) -> std::io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;
    OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(path)
}

#[cfg(not(windows))]
fn open_deny_write(path: &Path) -> std::io::Result<File> {
    File::open(path)
}

/// Streams into the staging file while hashing.
struct HashingWriter {
    file: File,
    hasher: Sha256,
}

impl Write for HashingWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let written = self.file.write(bytes)?;
        self.hasher.update(&bytes[..written]);
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

/// The no-downgrade Authenticode policy. The SHA-256 match with the signed
/// manifest is checked first and always required; this decides whether the
/// Windows signature state is acceptable on top of it.
pub fn authenticode_allows(
    signing: UpdateSigning,
    manifest_thumbprint: Option<&str>,
    running: &AuthenticodeVerdict,
    candidate: &AuthenticodeVerdict,
) -> bool {
    let candidate_thumbprint = match candidate {
        AuthenticodeVerdict::Valid { thumbprint } => Some(thumbprint.as_str()),
        AuthenticodeVerdict::Unsigned => None,
        // A broken signature or an unknown state is never accepted.
        AuthenticodeVerdict::Invalid | AuthenticodeVerdict::Unavailable => return false,
    };
    if signing == UpdateSigning::Authenticode
        && (manifest_thumbprint.is_none() || candidate_thumbprint != manifest_thumbprint)
    {
        return false;
    }
    match running {
        // A signed install only accepts the same signer: no downgrade to
        // unsigned and no signer switch, whatever the manifest says.
        AuthenticodeVerdict::Valid { thumbprint } => candidate_thumbprint == Some(thumbprint),
        AuthenticodeVerdict::Unsigned => true,
        // If we cannot tell whether we are signed, we cannot rule out a downgrade.
        AuthenticodeVerdict::Invalid | AuthenticodeVerdict::Unavailable => false,
    }
}

fn is_installer_file(name: &str) -> bool {
    let base = name.strip_suffix(".part").unwrap_or(name);
    base.strip_prefix("Supa-Diska-Klinah_")
        .and_then(|rest| rest.strip_suffix("_x64-setup.exe"))
        .and_then(AppVersion::parse)
        .is_some_and(|version| version.installer_name() == base)
}

impl<T: Transport> UpdateService<T> {
    pub fn new(config: UpdateServiceConfig<T>) -> Result<Self, UpdateError> {
        let running = AppVersion::parse(&config.running_version).ok_or(UpdateError::Storage)?;
        Ok(Self {
            dir: config.app_data.join("updates"),
            running,
            running_exe: config.running_exe,
            net: NetClient::new(config.transport),
            verifier: config.verifier,
            clock: config.clock,
            probe: config.probe,
            launcher: config.launcher,
            operation: Mutex::new(()),
            inner: Mutex::new(Inner::default()),
        })
    }

    fn state_path(&self) -> PathBuf {
        self.dir.join("state.json")
    }

    fn installer_path(&self, version: AppVersion) -> PathBuf {
        self.dir.join(version.installer_name())
    }

    fn part_path(&self, version: AppVersion) -> PathBuf {
        self.dir.join(format!("{}.part", version.installer_name()))
    }

    /// User operations never wait: they report `Busy` while another operation
    /// runs, and until startup recovery has resolved the persisted state (so a
    /// download can never sweep a staged installer the user has not seen).
    fn lock_operation(&self) -> Result<std::sync::MutexGuard<'_, ()>, UpdateError> {
        let guard = self.operation.try_lock().map_err(|_| UpdateError::Busy)?;
        if self.inner().recovery_pending {
            return Err(UpdateError::Busy);
        }
        Ok(guard)
    }

    fn inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        // A poisoned lock only means a panic elsewhere; the data stays usable.
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn persist(&self, stored: Stored) -> Result<(), UpdateError> {
        std::fs::create_dir_all(&self.dir).map_err(|_| UpdateError::Storage)?;
        write_json(&self.state_path(), &stored, true).map_err(|_| UpdateError::Storage)?;
        self.inner().stored = stored;
        Ok(())
    }

    /// Deletes staged installers and partial downloads, except `keep`.
    fn sweep(&self, keep: Option<AppVersion>) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        let kept = keep.map(|version| version.installer_name());
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let is_file = entry.file_type().is_ok_and(|kind| kind.is_file());
            if is_file && is_installer_file(name) && kept.as_deref() != Some(name) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    fn running_signer(&self) -> AuthenticodeVerdict {
        if let Some(verdict) = self.inner().running_signer.clone() {
            return verdict;
        }
        let verdict = File::open(&self.running_exe)
            .map_or(AuthenticodeVerdict::Unavailable, |file| {
                self.probe.verdict(&self.running_exe, &file)
            });
        self.inner().running_signer = Some(verdict.clone());
        verdict
    }

    /// Re-hashes the staged installer through a deny-write handle and applies
    /// the Authenticode policy. The returned handle keeps the file locked.
    fn verify_staged(&self, staged: &Staged) -> Result<(PathBuf, File), UpdateError> {
        let version = staged.version().ok_or(UpdateError::Storage)?;
        let expected: [u8; 32] = from_hex(&staged.sha256)
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or(UpdateError::Storage)?;
        let path = self.installer_path(version);
        let mut file = open_deny_write(&path).map_err(|_| UpdateError::Storage)?;
        let length = file.metadata().map_err(|_| UpdateError::Storage)?.len();
        if length != staged.size {
            return Err(UpdateError::IntegrityFailed);
        }
        let mut hasher = Sha256::new();
        let mut buffer = vec![0_u8; 64 * 1024];
        loop {
            let read = file.read(&mut buffer).map_err(|_| UpdateError::Storage)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        if hasher.finalize().as_slice() != expected {
            return Err(UpdateError::IntegrityFailed);
        }
        file.seek(SeekFrom::Start(0))
            .map_err(|_| UpdateError::Storage)?;
        let candidate = self.probe.verdict(&path, &file);
        if !authenticode_allows(
            staged.signing,
            staged.signer_thumbprint.as_deref(),
            &self.running_signer(),
            &candidate,
        ) {
            return Err(UpdateError::SignatureRejected);
        }
        Ok((path, file))
    }

    pub fn status(&self) -> UpdateStatus {
        let inner = self.inner();
        let update = if inner.recovery_pending {
            UpdateState::Recovering
        } else if let Some(version) = &inner.downloading {
            UpdateState::Downloading {
                version: version.clone(),
            }
        } else if let Some(version) = &inner.interrupted {
            UpdateState::Interrupted {
                version: version.clone(),
            }
        } else {
            match &inner.stored {
                Stored::Launched { staged } => UpdateState::Launched {
                    version: staged.version.clone(),
                },
                Stored::Verified { staged } => UpdateState::Verified {
                    version: staged.version.clone(),
                },
                Stored::Idle => match &inner.checked {
                    Some(manifest) => UpdateState::Available {
                        version: manifest.version.to_string(),
                        size: manifest.installer_size,
                    },
                    None if inner.up_to_date => UpdateState::UpToDate,
                    None => UpdateState::Idle,
                },
            }
        };
        UpdateStatus {
            current_version: self.running.to_string(),
            configured: self.verifier.is_ok(),
            update,
            recovery: inner.recovery.clone(),
            recovery_pending: inner.recovery_pending,
        }
    }

    /// Fetches and verifies the signed manifest. Requires the opt-in capability.
    pub fn check(&self, policy: UpdateCheckPolicy) -> Result<UpdateStatus, UpdateError> {
        let _operation = self.lock_operation()?;
        let verifier = self
            .verifier
            .as_ref()
            .map_err(|_| UpdateError::NotConfigured)?;
        let capability = policy.capability().ok_or(UpdateError::NotPermitted)?;
        let manifest = self
            .net
            .fetch(&capability, Endpoint::UpdateManifest)
            .map_err(|_| UpdateError::Network)?;
        let signature = self
            .net
            .fetch(&capability, Endpoint::UpdateSignature)
            .map_err(|_| UpdateError::Network)?;
        let result = verifier.verify(&manifest, &signature, self.running, (self.clock)());
        {
            let mut inner = self.inner();
            match result {
                Ok(manifest) => {
                    inner.checked = Some(manifest);
                    inner.up_to_date = false;
                }
                Err(UpdateManifestError::NotNewer) => {
                    inner.checked = None;
                    inner.up_to_date = true;
                }
                Err(_) => {
                    inner.checked = None;
                    return Err(UpdateError::BadManifest);
                }
            }
        }
        Ok(self.status())
    }

    /// Downloads and verifies the checked release. Requires the opt-in capability.
    pub fn download(&self, policy: UpdateCheckPolicy) -> Result<UpdateStatus, UpdateError> {
        let _operation = self.lock_operation()?;
        let capability = policy.capability().ok_or(UpdateError::NotPermitted)?;
        let manifest = self
            .inner()
            .checked
            .clone()
            .ok_or(UpdateError::NoUpdateChecked)?;
        let version = manifest.version;
        std::fs::create_dir_all(&self.dir).map_err(|_| UpdateError::Storage)?;
        self.sweep(None);
        self.persist(Stored::Idle)?;

        let part = self.part_path(version);
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&part)
            .map_err(|_| UpdateError::Storage)?;
        let mut writer = HashingWriter {
            file,
            hasher: Sha256::new(),
        };
        self.inner().downloading = Some(version.to_string());
        let result = self.net.download(
            &capability,
            Endpoint::UpdateInstaller(version),
            manifest.installer_size,
            &mut writer,
        );
        self.inner().downloading = None;
        let outcome = match result {
            Ok(written) if written != manifest.installer_size => Err(UpdateError::IntegrityFailed),
            Ok(_) if writer.hasher.clone().finalize().as_slice() != manifest.installer_sha256 => {
                Err(UpdateError::IntegrityFailed)
            }
            Ok(_) => writer.file.sync_all().map_err(|_| UpdateError::Storage),
            Err(NetError::TooLarge) => Err(UpdateError::IntegrityFailed),
            Err(NetError::Storage) => Err(UpdateError::Storage),
            Err(_) => Err(UpdateError::DownloadFailed),
        };
        drop(writer);
        if let Err(error) = outcome {
            let _ = std::fs::remove_file(&part);
            return Err(error);
        }
        let final_path = self.installer_path(version);
        if std::fs::rename(&part, &final_path).is_err() {
            let _ = std::fs::remove_file(&part);
            return Err(UpdateError::Storage);
        }
        let staged = Staged::from_manifest(&manifest);
        if let Err(error) = self.verify_staged(&staged) {
            let _ = std::fs::remove_file(&final_path);
            return Err(error);
        }
        self.persist(Stored::Verified { staged })?;
        Ok(self.status())
    }

    /// Re-verifies, asks the user (native dialog, default No), records the
    /// launch and opens the installer. The app should exit after `Ok`.
    pub fn install(
        &self,
        confirm: &dyn Fn(&str, &str) -> bool,
    ) -> Result<UpdateStatus, UpdateError> {
        let _operation = self.lock_operation()?;
        let Stored::Verified { staged } = self.inner().stored.clone() else {
            return Err(UpdateError::NothingToInstall);
        };
        // The handle denies writes and deletes until the installer is running.
        let (path, handle) = match self.verify_staged(&staged) {
            Ok(verified) => verified,
            Err(error) => {
                self.sweep(None);
                self.persist(Stored::Idle)?;
                return Err(error);
            }
        };
        let strings = crate::i18n::native();
        let body = (strings.update_install_body)(&self.running.to_string(), &staged.version);
        if !confirm(strings.update_install_title, &body) {
            return Err(UpdateError::Declined);
        }
        self.persist(Stored::Launched {
            staged: staged.clone(),
        })?;
        if self.launcher.launch(&path).is_err() {
            drop(handle);
            self.persist(Stored::Verified { staged })?;
            return Err(UpdateError::LaunchFailed);
        }
        drop(handle);
        {
            let mut inner = self.inner();
            inner.interrupted = None;
            inner.recovery = None;
        }
        Ok(self.status())
    }

    /// Removes any staged installer and forgets the pending update.
    pub fn discard(&self) -> Result<UpdateStatus, UpdateError> {
        let _operation = self.lock_operation()?;
        self.sweep(None);
        self.persist(Stored::Idle)?;
        {
            let mut inner = self.inner();
            inner.interrupted = None;
            inner.checked = None;
            inner.recovery = None;
        }
        Ok(self.status())
    }

    /// Forgets the startup recovery result once the UI has shown it.
    pub fn acknowledge_recovery(&self) -> UpdateStatus {
        self.inner().recovery = None;
        self.status()
    }

    /// Runs once at startup: clears partial downloads, re-verifies any staged
    /// installer and resolves a previous launch. A notable outcome is kept for
    /// `status().recovery` until acknowledged or the update is discarded.
    /// Waits for the operation lock rather than skipping, so the persisted state
    /// is always loaded; user operations report `Busy` until this finishes.
    pub fn recover_on_startup(&self) -> RecoveryOutcome {
        let operation = self
            .operation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let outcome = self.recover_locked();
        {
            let mut inner = self.inner();
            inner.recovery = (outcome != RecoveryOutcome::Clean).then(|| outcome.clone());
            inner.recovery_pending = false;
        }
        drop(operation);
        outcome
    }

    fn recover_locked(&self) -> RecoveryOutcome {
        let stored = read_json::<Stored>(&self.state_path()).unwrap_or_default();
        let staged = match &stored {
            Stored::Idle => None,
            Stored::Verified { staged } | Stored::Launched { staged } => Some(staged.clone()),
        };
        let Some(staged) = staged else {
            self.sweep(None);
            self.inner().stored = Stored::Idle;
            return RecoveryOutcome::Clean;
        };
        let Some(target) = staged.version() else {
            self.sweep(None);
            let _ = self.persist(Stored::Idle);
            return RecoveryOutcome::Discarded;
        };
        if self.running >= target {
            // Either the launched update succeeded, or a newer build is already
            // installed; staging is no longer needed.
            self.sweep(None);
            let _ = self.persist(Stored::Idle);
            return match stored {
                Stored::Launched { .. } => RecoveryOutcome::Updated {
                    version: target.to_string(),
                },
                _ => RecoveryOutcome::Clean,
            };
        }
        // Partial downloads never survive a restart.
        self.sweep(Some(target));
        match self.verify_staged(&staged) {
            Ok(_) => {
                let interrupted = matches!(stored, Stored::Launched { .. });
                let _ = self.persist(Stored::Verified { staged });
                if interrupted {
                    self.inner().interrupted = Some(target.to_string());
                    RecoveryOutcome::Interrupted {
                        version: target.to_string(),
                    }
                } else {
                    RecoveryOutcome::Clean
                }
            }
            Err(_) => {
                self.sweep(None);
                let _ = self.persist(Stored::Idle);
                RecoveryOutcome::Discarded
            }
        }
    }
}

#[cfg(test)]
mod tests;
