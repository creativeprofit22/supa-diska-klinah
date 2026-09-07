use std::{
    fmt,
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(
    Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, serde::Deserialize, serde::Serialize,
)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileIdentity {
    pub volume: u64,
    pub file: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EntryKind {
    File,
    Directory,
    LinkLike,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntryMetadata {
    pub kind: EntryKind,
    pub identity: Option<FileIdentity>,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirectoryEntry {
    pub path: PathBuf,
    pub name: String,
    pub kind: EntryKind,
    pub identity: Option<FileIdentity>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FsError {
    pub kind: FsErrorKind,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FsErrorKind {
    NotFound,
    PermissionDenied,
    InvalidData,
    Changed,
    Other,
}

impl FsError {
    pub fn new(kind: FsErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}
impl fmt::Display for FsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for FsError {}
impl From<std::io::Error> for FsError {
    fn from(error: std::io::Error) -> Self {
        let kind = match error.kind() {
            std::io::ErrorKind::NotFound => FsErrorKind::NotFound,
            std::io::ErrorKind::PermissionDenied => FsErrorKind::PermissionDenied,
            std::io::ErrorKind::InvalidData => FsErrorKind::InvalidData,
            _ => FsErrorKind::Other,
        };
        Self::new(kind, error.to_string())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PathSemantics {
    CaseSensitive,
    CaseInsensitive,
}

impl PathSemantics {
    pub fn key(self, path: &Path) -> String {
        let normalized = path.to_string_lossy().replace('\\', "/");
        let mut value = local_verbatim_disk(&normalized)
            .unwrap_or(&normalized)
            .to_owned();
        while value.len() > 1 && value.ends_with('/') && !value.ends_with(":/") {
            value.pop();
        }
        match self {
            Self::CaseSensitive => value,
            Self::CaseInsensitive => value.to_lowercase(),
        }
    }
    pub fn equivalent(self, left: &Path, right: &Path) -> bool {
        self.key(left) == self.key(right)
    }
    pub fn contains(self, root: &Path, candidate: &Path) -> bool {
        let root = self.key(root);
        let candidate = self.key(candidate);
        candidate == root
            || candidate
                .strip_prefix(&root)
                .is_some_and(|tail| root.ends_with('/') || tail.starts_with('/'))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadDirControl {
    Continue,
    Stop,
}

// Accept only the verbatim *disk* form returned by Windows canonicalize; never
// strip arbitrary device, GLOBALROOT, volume GUID or verbatim UNC namespaces.
fn local_verbatim_disk(normalized: &str) -> Option<&str> {
    let disk = normalized.strip_prefix("//?/")?;
    let bytes = disk.as_bytes();
    (bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/')
        .then_some(disk)
}

/// Storage authorization accepts local absolute paths only. Canonicalization and
/// no-follow identity checks are still required; this is a syntax gate, not authority.
pub fn is_local_storage_path(path: &Path) -> bool {
    let Some(text) = path.to_str() else {
        return false;
    };
    let normalized = text.replace('\\', "/");
    let normalized = local_verbatim_disk(&normalized).unwrap_or(&normalized);
    path.is_absolute()
        && text.len() <= 4096
        && !text.chars().any(char::is_control)
        && !normalized.starts_with("//")
        && !normalized
            .split('/')
            .any(|part| part == "." || part == "..")
        && !normalized.char_indices().any(|(index, ch)| {
            ch == ':' && !(index == 1 && normalized.as_bytes()[0].is_ascii_alphabetic())
        })
}

pub trait FileSystem: Send + Sync {
    /// Identity-bound native visibility attributes. Unknown must block empty-folder proofs.
    fn hidden_or_system(&self, _path: &Path, _metadata: &EntryMetadata) -> Option<bool> {
        None
    }
    fn semantics(&self) -> PathSemantics;
    fn metadata_no_follow(&self, path: &Path) -> Result<EntryMetadata, FsError>;
    fn canonicalize(&self, path: &Path) -> Result<PathBuf, FsError>;
    fn allocated_size(&self, path: &Path, metadata: &EntryMetadata) -> Result<u64, FsError> {
        let _ = path;
        Ok(metadata.size)
    }
    fn ensure_inactive(&self, path: &Path) -> Result<(), FsError> {
        let _ = path;
        Ok(())
    }
    fn read_dir(
        &self,
        path: &Path,
        expected_identity: FileIdentity,
        visitor: &mut dyn FnMut(DirectoryEntry) -> ReadDirControl,
    ) -> Result<(), FsError>;
}

pub trait Entropy: Send + Sync {
    fn fill(&self, bytes: &mut [u8]) -> Result<(), FsError>;
}
