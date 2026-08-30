use std::{
    fmt,
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FileIdentity {
    pub volume: u64,
    pub file: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
        let mut value = path.to_string_lossy().replace('\\', "/");
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
                .is_some_and(|tail| tail.starts_with('/'))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadDirControl {
    Continue,
    Stop,
}

pub trait FileSystem: Send + Sync {
    fn semantics(&self) -> PathSemantics;
    fn metadata_no_follow(&self, path: &Path) -> Result<EntryMetadata, FsError>;
    fn canonicalize(&self, path: &Path) -> Result<PathBuf, FsError>;
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
