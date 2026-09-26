//! Contained quarantine (ADR 0003, decision 8).
//!
//! Layout: `<app data>/protection/quarantine/<32-hex id>/{record.json,payload.bin}`.
//! Paths are always derived here from a validated ID; a record never supplies
//! a storage path. Payloads are XOR-neutered behind a magic header so they are
//! not directly executable. Quarantine uses one streaming path on every
//! volume: hash + neuter into a new file, verify, then delete the source
//! through the same handle that was hashed, so a swapped file is never
//! deleted. Restore writes with `CREATE_NEW` and never overwrites.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use protection_core::{from_hex, to_hex};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, DELETE, FILE_DISPOSITION_INFO, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_GENERIC_READ, FILE_SHARE_READ, FileDispositionInfo, GetFileInformationByHandle,
    SetFileInformationByHandle,
};

use super::fsutil::{self, FsFault, valid_id};
use super::locations::starts_with_ci;

const MAGIC: &[u8; 8] = b"SDKQRNT1";
const MAX_QUARANTINE_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_RECORD_BYTES: u64 = 16 * 1024;
const MAX_ENTRIES: usize = 10_000;
const CHUNK: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EntryState {
    /// Payload verified; source deletion not yet confirmed.
    SourcePending,
    Complete,
    /// Restore in progress; target may exist.
    Restoring,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuarantineRecord {
    pub id: String,
    pub original_path: String,
    pub sha256: String,
    pub size: u64,
    pub volume_serial: u32,
    pub file_index: u64,
    pub finding: String,
    pub quarantined_at: u64,
    pub state: EntryState,
    xor_key: String,
}

/// What the UI sees: no key, no storage path.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuarantineEntry {
    pub id: String,
    pub original_path: Option<String>,
    pub sha256: Option<String>,
    pub size: Option<u64>,
    pub finding: Option<String>,
    pub quarantined_at: Option<u64>,
    pub damaged: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum QuarantineError {
    InvalidId,
    NotFound,
    InvalidPath,
    OutsideRoot,
    ReparsePoint,
    Protected,
    InUse,
    AccessDenied,
    TooLarge,
    /// The file changed since it was scanned.
    Changed,
    /// Restore target already exists; nothing was overwritten.
    Collision,
    /// The quarantined payload no longer matches its recorded hash.
    HashMismatch,
    Damaged,
    Storage,
}

impl std::fmt::Display for QuarantineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::InvalidId => "invalid quarantine id",
            Self::NotFound => "quarantine entry not found",
            Self::InvalidPath => "path is not an absolute local file path",
            Self::OutsideRoot => "file is outside the scanned folder",
            Self::ReparsePoint => "path contains a junction or symbolic link",
            Self::Protected => "path is protected",
            Self::InUse => "file is in use by another program",
            Self::AccessDenied => "access denied",
            Self::TooLarge => "file is too large to quarantine",
            Self::Changed => "file changed since it was scanned",
            Self::Collision => {
                "a file already exists at the restore location; nothing was overwritten"
            }
            Self::HashMismatch => "quarantined data does not match its recorded hash",
            Self::Damaged => "quarantine entry is damaged",
            Self::Storage => "quarantine storage is unavailable",
        };
        f.write_str(text)
    }
}

impl From<FsFault> for QuarantineError {
    fn from(fault: FsFault) -> Self {
        match fault {
            FsFault::Reparse => Self::ReparsePoint,
            FsFault::TooLarge => Self::TooLarge,
            FsFault::Exists => Self::Collision,
            _ => Self::Storage,
        }
    }
}

fn io_error(error: &std::io::Error) -> QuarantineError {
    match error.raw_os_error() {
        Some(32) | Some(33) => QuarantineError::InUse,
        Some(5) => QuarantineError::AccessDenied,
        Some(80) | Some(183) => QuarantineError::Collision,
        Some(2) | Some(3) => QuarantineError::NotFound,
        _ => QuarantineError::Storage,
    }
}

/// Everything the caller must supply to quarantine a scanned file.
pub struct QuarantineRequest<'a> {
    pub source: &'a Path,
    /// The authorized scan root the file was found under.
    pub scan_root: &'a Path,
    /// SHA-256 from the scan; the file must still match it.
    pub expected_sha256: Option<&'a str>,
    pub finding: &'a str,
}

pub struct Quarantine {
    root: PathBuf,
    #[cfg(test)]
    pub(crate) fail_before_source_delete: bool,
}

/// Absolute, drive-rooted, no `..`, no device or UNC prefixes other than a local disk.
fn validate_absolute(path: &Path) -> Result<(), QuarantineError> {
    use std::path::Prefix;
    let mut components = path.components();
    match components.next() {
        Some(Component::Prefix(prefix))
            if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)) => {}
        _ => return Err(QuarantineError::InvalidPath),
    }
    if components.next() != Some(Component::RootDir) {
        return Err(QuarantineError::InvalidPath);
    }
    if components.any(|c| !matches!(c, Component::Normal(_))) {
        return Err(QuarantineError::InvalidPath);
    }
    Ok(())
}

/// Reject if any existing ancestor (from the drive root) is a reparse point.
fn ensure_no_reparse_ancestors(path: &Path) -> Result<(), QuarantineError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        if matches!(component, Component::Prefix(_) | Component::RootDir) {
            continue;
        }
        match fsutil::is_reparse(&current) {
            Ok(true) => return Err(QuarantineError::ReparsePoint),
            Ok(false) => {}
            Err(_) => break,
        }
    }
    Ok(())
}

fn xor_stream(key: &[u8], offset: u64, data: &mut [u8]) {
    for (index, byte) in data.iter_mut().enumerate() {
        *byte ^= key[((offset + index as u64) % key.len() as u64) as usize];
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn identity(file: &File) -> Option<(u32, u64)> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the handle is open for the lifetime of `file`; `info` is writable.
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) };
    (ok != 0).then(|| {
        (
            info.dwVolumeSerialNumber,
            (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
        )
    })
}

fn delete_by_handle(file: &File) -> Result<(), QuarantineError> {
    let info = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: the handle was opened with DELETE access; the struct outlives the call.
    let ok = unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileDispositionInfo,
            (&raw const info).cast(),
            size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    };
    if ok == 0 {
        Err(io_error(&std::io::Error::last_os_error()))
    } else {
        Ok(())
    }
}

impl Quarantine {
    pub fn open(root: PathBuf) -> Result<Self, QuarantineError> {
        fs::create_dir_all(&root).map_err(|_| QuarantineError::Storage)?;
        if fsutil::is_reparse(&root)? {
            return Err(QuarantineError::ReparsePoint);
        }
        let root = fs::canonicalize(&root).map_err(|_| QuarantineError::Storage)?;
        let quarantine = Self {
            root,
            #[cfg(test)]
            fail_before_source_delete: false,
        };
        quarantine.reconcile();
        Ok(quarantine)
    }

    /// The only way to turn an ID into a storage path.
    fn entry_dir(&self, id: &str) -> Result<PathBuf, QuarantineError> {
        if !valid_id(id) {
            return Err(QuarantineError::InvalidId);
        }
        let dir = self.root.join(id);
        fsutil::ensure_no_reparse_below(&self.root, &dir)?;
        Ok(dir)
    }

    fn read_record(&self, id: &str) -> Result<QuarantineRecord, QuarantineError> {
        let dir = self.entry_dir(id)?;
        if !dir.is_dir() {
            return Err(QuarantineError::NotFound);
        }
        let bytes = fsutil::read_bounded(&dir.join("record.json"), MAX_RECORD_BYTES)
            .map_err(|_| QuarantineError::Damaged)?;
        let record: QuarantineRecord =
            serde_json::from_slice(&bytes).map_err(|_| QuarantineError::Damaged)?;
        let valid = record.id == id
            && record.sha256.len() == 64
            && from_hex(&record.sha256).is_some()
            && from_hex(&record.xor_key).is_some_and(|k| k.len() == 32)
            && record.finding.chars().count() <= 512
            && validate_absolute(Path::new(&record.original_path)).is_ok();
        if valid {
            Ok(record)
        } else {
            Err(QuarantineError::Damaged)
        }
    }

    fn write_record(&self, record: &QuarantineRecord) -> Result<(), QuarantineError> {
        let dir = self.entry_dir(&record.id)?;
        let bytes = serde_json::to_vec(record).map_err(|_| QuarantineError::Storage)?;
        fsutil::atomic_replace(&dir.join("record.json"), &bytes)?;
        Ok(())
    }

    pub fn list(&self) -> Vec<QuarantineEntry> {
        let mut out = Vec::new();
        let Ok(entries) = fs::read_dir(&self.root) else {
            return out;
        };
        for entry in entries.flatten().take(MAX_ENTRIES) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !valid_id(&name) {
                continue;
            }
            match self.read_record(&name) {
                Ok(record) if record.state == EntryState::Complete => out.push(QuarantineEntry {
                    id: record.id,
                    original_path: Some(record.original_path),
                    sha256: Some(record.sha256),
                    size: Some(record.size),
                    finding: Some(record.finding),
                    quarantined_at: Some(record.quarantined_at),
                    damaged: false,
                }),
                Ok(_) => {}
                Err(_) => out.push(QuarantineEntry {
                    id: name,
                    original_path: None,
                    sha256: None,
                    size: None,
                    finding: None,
                    quarantined_at: None,
                    damaged: true,
                }),
            }
        }
        out.sort_by(|a, b| b.quarantined_at.cmp(&a.quarantined_at));
        out
    }

    /// Move a scanned file into quarantine. `is_protected` is the existing
    /// cleanup protection policy.
    pub fn quarantine(
        &self,
        request: &QuarantineRequest<'_>,
        is_protected: &dyn Fn(&Path) -> bool,
    ) -> Result<QuarantineEntry, QuarantineError> {
        let source = request.source;
        validate_absolute(source)?;
        validate_absolute(request.scan_root)?;
        if !starts_with_ci(source, request.scan_root) {
            return Err(QuarantineError::OutsideRoot);
        }
        ensure_no_reparse_ancestors(source)?;
        if is_protected(source) {
            return Err(QuarantineError::Protected);
        }
        if starts_with_ci(source, &self.root) {
            return Err(QuarantineError::InvalidPath);
        }

        // Exclusive against writers; DELETE access so the same handle removes it.
        let mut file = OpenOptions::new()
            .access_mode(FILE_GENERIC_READ | DELETE)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(source)
            .map_err(|e| io_error(&e))?;
        let metadata = file.metadata().map_err(|e| io_error(&e))?;
        if fsutil::is_reparse(source)? || !metadata.is_file() {
            return Err(QuarantineError::ReparsePoint);
        }
        if metadata.len() > MAX_QUARANTINE_BYTES {
            return Err(QuarantineError::TooLarge);
        }
        let (volume_serial, file_index) = identity(&file).ok_or(QuarantineError::Storage)?;

        let id = fsutil::random_hex()?;
        let dir = self.root.join(&id);
        fs::create_dir(&dir).map_err(|_| QuarantineError::Storage)?;
        let cleanup = |dir: &Path| {
            let _ = fsutil::remove_owned_tree(dir);
        };

        let mut key = [0_u8; 32];
        getrandom::fill(&mut key).map_err(|_| QuarantineError::Storage)?;
        let payload_tmp = dir.join("payload.tmp");
        let result = (|| {
            let mut out = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&payload_tmp)
                .map_err(|_| QuarantineError::Storage)?;
            out.write_all(MAGIC).map_err(|_| QuarantineError::Storage)?;
            let mut hasher = Sha256::new();
            let mut buffer = vec![0_u8; CHUNK];
            let mut offset = 0_u64;
            loop {
                let read = file.read(&mut buffer).map_err(|e| io_error(&e))?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
                xor_stream(&key, offset, &mut buffer[..read]);
                out.write_all(&buffer[..read])
                    .map_err(|_| QuarantineError::Storage)?;
                offset += read as u64;
                if offset > MAX_QUARANTINE_BYTES {
                    return Err(QuarantineError::TooLarge);
                }
            }
            out.sync_all().map_err(|_| QuarantineError::Storage)?;
            let digest = to_hex(&hasher.finalize());
            if request
                .expected_sha256
                .is_some_and(|expected| !expected.eq_ignore_ascii_case(&digest))
            {
                return Err(QuarantineError::Changed);
            }
            Ok((digest, offset))
        })();
        let (sha256, size) = match result {
            Ok(value) => value,
            Err(error) => {
                drop(file);
                cleanup(&dir);
                return Err(error);
            }
        };
        // Re-verify what landed on disk before touching the source.
        if self
            .payload_digest(&dir.join("payload.tmp"), &key)
            .ok()
            .as_deref()
            != Some(sha256.as_str())
        {
            drop(file);
            cleanup(&dir);
            return Err(QuarantineError::Storage);
        }
        fs::rename(&payload_tmp, dir.join("payload.bin")).map_err(|_| {
            cleanup(&dir);
            QuarantineError::Storage
        })?;
        let mut record = QuarantineRecord {
            id: id.clone(),
            original_path: source.to_string_lossy().into_owned(),
            sha256,
            size,
            volume_serial,
            file_index,
            finding: request.finding.chars().take(512).collect(),
            quarantined_at: now(),
            state: EntryState::SourcePending,
            xor_key: to_hex(&key),
        };
        if let Err(error) = self.write_record(&record) {
            drop(file);
            cleanup(&dir);
            return Err(error);
        }

        #[cfg(test)]
        if self.fail_before_source_delete {
            return Err(QuarantineError::Storage);
        }
        if let Err(error) = delete_by_handle(&file) {
            drop(file);
            cleanup(&dir);
            return Err(error);
        }
        drop(file);
        record.state = EntryState::Complete;
        self.write_record(&record)?;
        Ok(QuarantineEntry {
            id,
            original_path: Some(record.original_path),
            sha256: Some(record.sha256),
            size: Some(record.size),
            finding: Some(record.finding),
            quarantined_at: Some(record.quarantined_at),
            damaged: false,
        })
    }

    fn payload_reader(&self, path: &Path) -> Result<File, QuarantineError> {
        if fsutil::is_reparse(path)? {
            return Err(QuarantineError::ReparsePoint);
        }
        let mut file = File::open(path).map_err(|_| QuarantineError::Damaged)?;
        let mut magic = [0_u8; 8];
        file.read_exact(&mut magic)
            .map_err(|_| QuarantineError::Damaged)?;
        if &magic != MAGIC {
            return Err(QuarantineError::Damaged);
        }
        Ok(file)
    }

    fn payload_digest(&self, path: &Path, key: &[u8]) -> Result<String, QuarantineError> {
        let mut file = self.payload_reader(path)?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0_u8; CHUNK];
        let mut offset = 0_u64;
        loop {
            let read = file
                .read(&mut buffer)
                .map_err(|_| QuarantineError::Damaged)?;
            if read == 0 {
                break;
            }
            xor_stream(key, offset, &mut buffer[..read]);
            hasher.update(&buffer[..read]);
            offset += read as u64;
        }
        Ok(to_hex(&hasher.finalize()))
    }

    /// The target a restore would write, for the confirmation prompt.
    pub fn restore_target(&self, id: &str) -> Result<String, QuarantineError> {
        Ok(self.read_record(id)?.original_path)
    }

    /// Restore to the original path. Call only after native confirmation.
    pub fn restore(
        &self,
        id: &str,
        is_protected: &dyn Fn(&Path) -> bool,
    ) -> Result<String, QuarantineError> {
        let mut record = self.read_record(id)?;
        if record.state != EntryState::Complete {
            return Err(QuarantineError::Damaged);
        }
        let target = PathBuf::from(&record.original_path);
        validate_absolute(&target)?;
        let parent = target.parent().ok_or(QuarantineError::InvalidPath)?;
        if !parent.is_dir() {
            return Err(QuarantineError::NotFound);
        }
        ensure_no_reparse_ancestors(&target)?;
        if is_protected(&target) {
            return Err(QuarantineError::Protected);
        }
        let dir = self.entry_dir(id)?;
        let payload = dir.join("payload.bin");
        let key = from_hex(&record.xor_key).ok_or(QuarantineError::Damaged)?;
        // Verify before writing anything.
        if self.payload_digest(&payload, &key)? != record.sha256 {
            return Err(QuarantineError::HashMismatch);
        }

        record.state = EntryState::Restoring;
        self.write_record(&record)?;
        let written = (|| {
            let mut out = OpenOptions::new()
                .write(true)
                .create_new(true)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
                .open(&target)
                .map_err(|e| io_error(&e))?;
            let mut input = self.payload_reader(&payload)?;
            let mut hasher = Sha256::new();
            let mut buffer = vec![0_u8; CHUNK];
            let mut offset = 0_u64;
            let copy = (|| {
                loop {
                    let read = input
                        .read(&mut buffer)
                        .map_err(|_| QuarantineError::Damaged)?;
                    if read == 0 {
                        break;
                    }
                    xor_stream(&key, offset, &mut buffer[..read]);
                    hasher.update(&buffer[..read]);
                    out.write_all(&buffer[..read])
                        .map_err(|_| QuarantineError::Storage)?;
                    offset += read as u64;
                }
                out.sync_all().map_err(|_| QuarantineError::Storage)?;
                if to_hex(&hasher.finalize()) != record.sha256 {
                    return Err(QuarantineError::HashMismatch);
                }
                Ok(())
            })();
            if copy.is_err() {
                // We created this file with CREATE_NEW, so removing it cannot
                // touch anything that existed before.
                drop(out);
                let _ = fs::remove_file(&target);
            }
            copy
        })();
        if let Err(error) = written {
            record.state = EntryState::Complete;
            self.write_record(&record)?;
            return Err(error);
        }
        fsutil::remove_owned_tree(&dir)?;
        Ok(record.original_path)
    }

    /// Permanently delete a quarantined payload. Call only after native confirmation.
    pub fn delete(&self, id: &str) -> Result<(), QuarantineError> {
        let dir = self.entry_dir(id)?;
        if !dir.is_dir() {
            return Err(QuarantineError::NotFound);
        }
        fsutil::remove_owned_tree(&dir).map_err(Into::into)
    }

    /// Startup reconciliation of interrupted operations. Never deletes or
    /// overwrites a file outside the quarantine root.
    pub fn reconcile(&self) {
        let Ok(entries) = fs::read_dir(&self.root) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            if name.starts_with(".tmp-") {
                let _ = fs::remove_file(&path);
                continue;
            }
            if !valid_id(&name) {
                continue;
            }
            if fsutil::is_reparse(&path).unwrap_or(true) {
                // A planted junction: remove the link itself only.
                let _ = fs::remove_dir(&path);
                continue;
            }
            match self.read_record(&name) {
                Err(_) => {
                    // No record means the source was never deleted: the
                    // original is intact, so the partial entry is discarded.
                    if !path.join("payload.bin").exists() {
                        let _ = fsutil::remove_owned_tree(&path);
                    }
                }
                Ok(mut record) => match record.state {
                    EntryState::Complete => {}
                    EntryState::SourcePending => self.finish_source_pending(&path, &mut record),
                    EntryState::Restoring => {
                        let target = Path::new(&record.original_path);
                        let restored = file_digest(target).is_some_and(|d| d == record.sha256);
                        if restored {
                            let _ = fsutil::remove_owned_tree(&path);
                        } else {
                            // Leave any file at the target untouched; the user can retry.
                            record.state = EntryState::Complete;
                            let _ = self.write_record(&record);
                        }
                    }
                },
            }
        }
    }

    fn finish_source_pending(&self, dir: &Path, record: &mut QuarantineRecord) {
        let source = Path::new(&record.original_path);
        let same = OpenOptions::new()
            .access_mode(FILE_GENERIC_READ | DELETE)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(source)
            .ok()
            .filter(|file| identity(file) == Some((record.volume_serial, record.file_index)));
        match same {
            Some(file) if file_digest(source).as_deref() == Some(record.sha256.as_str()) => {
                // The confirmed quarantine did not finish deleting the source:
                // the same file is still there, so complete it.
                if delete_by_handle(&file).is_ok() {
                    drop(file);
                    record.state = EntryState::Complete;
                    let _ = self.write_record(record);
                }
            }
            Some(_) => {
                // Same file, different content: the payload is stale. Keep the
                // user's file and drop the entry.
                let _ = fsutil::remove_owned_tree(dir);
            }
            None => {
                record.state = EntryState::Complete;
                let _ = self.write_record(record);
            }
        }
    }
}

fn file_digest(path: &Path) -> Option<String> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .ok()?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; CHUNK];
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Some(to_hex(&hasher.finalize()))
}

#[cfg(test)]
mod tests;
