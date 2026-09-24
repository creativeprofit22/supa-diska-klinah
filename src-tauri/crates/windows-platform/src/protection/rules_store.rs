//! Versioned, atomically installed and recoverable rule packs (ADR 0003).
//!
//! Layout under `<app data>/protection/rules`:
//!
//! ```text
//! packs/<sequence>/pack.json + pack.sig   verified, installed packs
//! current.json                            {"current":N,"previous":M}
//! staging-<hex>/                          in-flight install, deleted on startup
//! ```
//!
//! Install: verify → validate → compile → stage + flush → rename into
//! `packs/<sequence>` → atomic write-through replace of `current.json`.
//! Anything that fails before the pointer replace leaves the old state
//! active. Startup recovery removes staging directories and falls back from
//! `current` to `previous` to the signed pack embedded in the binary.

use std::fs;
use std::path::{Path, PathBuf};

use protection_core::{
    CompiledPack, MAX_PACK_BYTES, PackError, PackVerifier, VerifiedPack, check_install_sequence,
    check_restore_previous,
};
use serde::{Deserialize, Serialize};

use super::fsutil::{self, FsFault};

/// The trusted public key, compiled in (ADR 0003, decision 2).
pub const RULE_PACK_PUBLIC_KEY: &str = include_str!("../../../../keys/rule-pack.pub");
const TEST_PUBLIC_KEY: &str = include_str!("fixtures/test-rule-pack.pub");
const BASELINE_PACK: &[u8] = include_bytes!("baseline/pack.json");
const BASELINE_SIG: &[u8] = include_bytes!("baseline/pack.sig");
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Release builds refuse imports and downloads while the compiled key is the
/// committed test key, whose private half is public.
pub fn external_packs_allowed() -> bool {
    cfg!(debug_assertions) || RULE_PACK_PUBLIC_KEY.trim() != TEST_PUBLIC_KEY.trim()
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Pointer {
    current: Option<u64>,
    previous: Option<u64>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RulesSource {
    Installed,
    PreviousFallback,
    EmbeddedBaseline,
}

/// What the UI shows about the active pack.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RulesStatus {
    pub source: RulesSource,
    pub sequence: u64,
    pub created: String,
    pub description: String,
    pub rule_count: usize,
    pub previous_sequence: Option<u64>,
    /// Why recovery fell back, if it did.
    pub recovery_note: Option<String>,
    pub external_packs_allowed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RulesError {
    Pack(PackError),
    Storage,
    Disabled,
}

impl From<PackError> for RulesError {
    fn from(error: PackError) -> Self {
        Self::Pack(error)
    }
}

impl From<FsFault> for RulesError {
    fn from(_: FsFault) -> Self {
        Self::Storage
    }
}

impl std::fmt::Display for RulesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pack(error) => write!(f, "{error}"),
            Self::Storage => write!(f, "rule storage is unavailable"),
            Self::Disabled => {
                write!(
                    f,
                    "rule-pack import is disabled until a release signing key is configured"
                )
            }
        }
    }
}

/// Injected failures for interruption tests.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InstallStep {
    Stage,
    Rename,
    Pointer,
    Prune,
}

pub struct RulesStore {
    root: PathBuf,
    verifier: PackVerifier,
    baseline: (Vec<u8>, Vec<u8>),
    active: CompiledPack,
    status: RulesStatus,
    #[cfg(test)]
    fail_at: Option<InstallStep>,
}

impl RulesStore {
    /// Open with the compiled key and embedded baseline, running recovery.
    pub fn open(root: PathBuf) -> Result<Self, RulesError> {
        let verifier = PackVerifier::from_key_file(RULE_PACK_PUBLIC_KEY)?;
        Self::open_with(
            root,
            verifier,
            BASELINE_PACK.to_vec(),
            BASELINE_SIG.to_vec(),
        )
    }

    pub(crate) fn open_with(
        root: PathBuf,
        verifier: PackVerifier,
        baseline_pack: Vec<u8>,
        baseline_sig: Vec<u8>,
    ) -> Result<Self, RulesError> {
        fs::create_dir_all(root.join("packs")).map_err(|_| RulesError::Storage)?;
        let root = fs::canonicalize(root).map_err(|_| RulesError::Storage)?;
        // A missing or broken baseline is a build defect; fail closed.
        let baseline = verifier.verify(&baseline_pack, &baseline_sig, APP_VERSION)?;
        let mut store = Self {
            active: CompiledPack::compile(&baseline),
            status: status_for(&baseline, RulesSource::EmbeddedBaseline, None, None),
            root,
            verifier,
            baseline: (baseline_pack, baseline_sig),
            #[cfg(test)]
            fail_at: None,
        };
        store.recover();
        Ok(store)
    }

    pub fn active(&self) -> &CompiledPack {
        &self.active
    }

    pub fn status(&self) -> &RulesStatus {
        &self.status
    }

    fn pointer_path(&self) -> PathBuf {
        self.root.join("current.json")
    }

    fn pack_dir(&self, sequence: u64) -> PathBuf {
        self.root.join("packs").join(sequence.to_string())
    }

    fn read_pointer(&self) -> Result<Pointer, ()> {
        let path = self.pointer_path();
        if !path.exists() {
            return Ok(Pointer::default());
        }
        let bytes = fsutil::read_bounded(&path, 4096).map_err(|_| ())?;
        serde_json::from_slice(&bytes).map_err(|_| ())
    }

    fn load_installed(&self, sequence: u64) -> Result<VerifiedPack, RulesError> {
        let dir = self.pack_dir(sequence);
        fsutil::ensure_no_reparse_below(&self.root, &dir)?;
        let pack = fsutil::read_bounded(&dir.join("pack.json"), MAX_PACK_BYTES as u64)?;
        let sig = fsutil::read_bounded(&dir.join("pack.sig"), 256)?;
        let verified = self.verifier.verify(&pack, &sig, APP_VERSION)?;
        if verified.sequence() != sequence {
            return Err(RulesError::Pack(PackError::Invalid(
                "pack sequence does not match its directory".into(),
            )));
        }
        Ok(verified)
    }

    fn baseline(&self) -> VerifiedPack {
        self.verifier
            .verify(&self.baseline.0, &self.baseline.1, APP_VERSION)
            .expect("baseline verified at open")
    }

    /// Startup recovery. Never fails: the embedded baseline always loads.
    fn recover(&mut self) {
        if let Ok(entries) = fs::read_dir(&self.root) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with("staging-") || name.starts_with(".tmp-") {
                    let path = entry.path();
                    let _ = if path.is_dir() || fsutil::is_reparse(&path).unwrap_or(false) {
                        fsutil::remove_owned_tree(&path)
                    } else {
                        fs::remove_file(&path).map_err(|_| FsFault::Io)
                    };
                }
            }
        }
        let pointer = match self.read_pointer() {
            Ok(pointer) => pointer,
            Err(()) => {
                self.activate(
                    self.baseline(),
                    RulesSource::EmbeddedBaseline,
                    None,
                    Some(
                        "The rule pointer was unreadable; using the embedded baseline pack.".into(),
                    ),
                );
                return;
            }
        };
        let mut note = None;
        if let Some(current) = pointer.current {
            match self.load_installed(current) {
                Ok(pack) => {
                    let previous = pointer.previous.filter(|p| self.load_installed(*p).is_ok());
                    self.activate(pack, RulesSource::Installed, previous, None);
                    return;
                }
                Err(error) => {
                    note = Some(format!(
                        "Installed pack {current} failed verification ({error})."
                    ));
                }
            }
        }
        if let Some(previous) = pointer.previous
            && let Ok(pack) = self.load_installed(previous)
        {
            let message = format!(
                "{} Using the previous pack {previous}.",
                note.clone().unwrap_or_default()
            );
            self.activate(
                pack,
                RulesSource::PreviousFallback,
                None,
                Some(message.trim().into()),
            );
            return;
        }
        let note = note.map(|n| format!("{n} Using the embedded baseline pack."));
        self.activate(self.baseline(), RulesSource::EmbeddedBaseline, None, note);
    }

    fn activate(
        &mut self,
        pack: VerifiedPack,
        source: RulesSource,
        previous: Option<u64>,
        note: Option<String>,
    ) {
        self.active = CompiledPack::compile(&pack);
        self.status = status_for(&pack, source, previous, note);
    }

    #[cfg(test)]
    fn fault(&self, step: InstallStep) -> Result<(), RulesError> {
        if self.fail_at == Some(step) {
            Err(RulesError::Storage)
        } else {
            Ok(())
        }
    }

    /// Verify and atomically install a pack from user-supplied bytes.
    pub fn install(
        &mut self,
        pack_bytes: &[u8],
        sig_bytes: &[u8],
    ) -> Result<RulesStatus, RulesError> {
        if !external_packs_allowed() {
            return Err(RulesError::Disabled);
        }
        self.install_unchecked(pack_bytes, sig_bytes)
    }

    pub(crate) fn install_unchecked(
        &mut self,
        pack_bytes: &[u8],
        sig_bytes: &[u8],
    ) -> Result<RulesStatus, RulesError> {
        let verified = self.verifier.verify(pack_bytes, sig_bytes, APP_VERSION)?;
        let floor = self.status.sequence.max(self.baseline().sequence());
        check_install_sequence(verified.sequence(), floor)?;
        let compiled = CompiledPack::compile(&verified);

        let staging = self.root.join(format!("staging-{}", fsutil::random_hex()?));
        fs::create_dir(&staging).map_err(|_| RulesError::Storage)?;
        let staged = (|| {
            #[cfg(test)]
            self.fault(InstallStep::Stage)?;
            fsutil::write_new_flushed(&staging.join("pack.json"), pack_bytes)?;
            fsutil::write_new_flushed(&staging.join("pack.sig"), sig_bytes)?;
            #[cfg(test)]
            self.fault(InstallStep::Rename)?;
            let target = self.pack_dir(verified.sequence());
            if target.exists() {
                // A leftover from an interrupted install; never referenced,
                // because its sequence is above the active one.
                fsutil::remove_owned_tree(&target)?;
            }
            fs::rename(&staging, &target).map_err(|_| RulesError::Storage)
        })();
        if let Err(error) = staged {
            let _ = fsutil::remove_owned_tree(&staging);
            return Err(error);
        }

        #[cfg(test)]
        self.fault(InstallStep::Pointer)?;
        let previous =
            (self.status.source == RulesSource::Installed).then_some(self.status.sequence);
        let pointer = Pointer {
            current: Some(verified.sequence()),
            previous,
        };
        let bytes = serde_json::to_vec(&pointer).map_err(|_| RulesError::Storage)?;
        fsutil::atomic_replace(&self.pointer_path(), &bytes)?;

        self.active = compiled;
        self.status = status_for(&verified, RulesSource::Installed, previous, None);
        #[cfg(test)]
        if self.fault(InstallStep::Prune).is_err() {
            return Ok(self.status.clone());
        }
        self.prune(&pointer);
        Ok(self.status.clone())
    }

    /// Explicitly return to the retained previous pack. This is the only
    /// path that may lower the active sequence.
    pub fn restore_previous(&mut self) -> Result<RulesStatus, RulesError> {
        let previous = check_restore_previous(self.status.previous_sequence)?;
        let verified = self.load_installed(previous)?;
        let pointer = Pointer {
            current: Some(previous),
            previous: None,
        };
        let bytes = serde_json::to_vec(&pointer).map_err(|_| RulesError::Storage)?;
        fsutil::atomic_replace(&self.pointer_path(), &bytes)?;
        self.activate(verified, RulesSource::Installed, None, None);
        self.prune(&pointer);
        Ok(self.status.clone())
    }

    /// Remove pack directories that the pointer no longer references.
    fn prune(&self, pointer: &Pointer) {
        let keep = [pointer.current, pointer.previous];
        if let Ok(entries) = fs::read_dir(self.root.join("packs")) {
            for entry in entries.flatten() {
                let keep_it = entry
                    .file_name()
                    .to_str()
                    .and_then(|name| name.parse::<u64>().ok())
                    .is_some_and(|sequence| keep.contains(&Some(sequence)));
                if !keep_it {
                    let _ = fsutil::remove_owned_tree(&entry.path());
                }
            }
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

fn status_for(
    pack: &VerifiedPack,
    source: RulesSource,
    previous: Option<u64>,
    recovery_note: Option<String>,
) -> RulesStatus {
    let inner = pack.pack();
    RulesStatus {
        source,
        sequence: inner.sequence,
        created: inner.created.clone(),
        description: inner.description.clone(),
        rule_count: inner.rules.len(),
        previous_sequence: previous,
        recovery_note,
        external_packs_allowed: external_packs_allowed(),
    }
}

#[cfg(test)]
impl RulesStore {
    pub(crate) fn fail_next(&mut self, step: InstallStep) {
        self.fail_at = Some(step);
    }
}

#[cfg(test)]
mod tests;
