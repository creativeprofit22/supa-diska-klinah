use crate::{EntryKind, FileSystem, FsError, PathSemantics};
use std::{
    collections::HashSet,
    fmt,
    path::{Component, Path, PathBuf},
};

#[derive(Clone, Debug)]
pub struct ProtectionInputs {
    system: Vec<PathBuf>,
    durable_user: Vec<PathBuf>,
    configured: Vec<PathBuf>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtectionClass {
    System,
    DurableUser,
    Configured,
}

impl ProtectionInputs {
    pub fn new(
        system: Vec<PathBuf>,
        durable_user: Vec<PathBuf>,
        configured: Vec<PathBuf>,
    ) -> Result<Self, ProtectionError> {
        for (class, paths) in [
            (ProtectionClass::System, &system),
            (ProtectionClass::DurableUser, &durable_user),
            (ProtectionClass::Configured, &configured),
        ] {
            if paths.is_empty() {
                return Err(ProtectionError::MissingClass(class));
            }
        }
        Ok(Self {
            system,
            durable_user,
            configured,
        })
    }
}

#[derive(Clone, Debug)]
pub struct ProtectionPolicy {
    paths: Vec<PathBuf>,
    semantics: PathSemantics,
}

#[derive(Debug)]
pub enum ProtectionError {
    MissingClass(ProtectionClass),
    NotAbsolute(PathBuf),
    Invalid(PathBuf, FsError),
    LinkLike(PathBuf),
    WrongType(PathBuf),
    MissingIdentity(PathBuf),
    Ambiguous(PathBuf),
}
impl fmt::Display for ProtectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::MissingClass(_) => "mandatory protection class is missing",
            Self::NotAbsolute(_) => "protected path is not absolute",
            Self::Invalid(_, _) => "protected path cannot be verified",
            Self::LinkLike(_) => "protected path is link-like",
            Self::WrongType(_) => "protected path is not a directory",
            Self::MissingIdentity(_) => "protected path has no stable identity",
            Self::Ambiguous(_) => "protected paths overlap ambiguously",
        })
    }
}
impl std::error::Error for ProtectionError {}

impl ProtectionPolicy {
    pub fn compile(fs: &dyn FileSystem, inputs: ProtectionInputs) -> Result<Self, ProtectionError> {
        let semantics = fs.semantics();
        let mut paths = Vec::new();
        let mut keys = HashSet::new();
        for path in inputs
            .system
            .into_iter()
            .chain(inputs.durable_user)
            .chain(inputs.configured)
        {
            if !path.is_absolute() {
                return Err(ProtectionError::NotAbsolute(path));
            }
            let before = fs
                .metadata_no_follow(&path)
                .map_err(|error| ProtectionError::Invalid(path.clone(), error))?;
            if before.kind == EntryKind::LinkLike {
                return Err(ProtectionError::LinkLike(path));
            }
            if before.kind != EntryKind::Directory {
                return Err(ProtectionError::WrongType(path));
            }
            let identity = before
                .identity
                .ok_or_else(|| ProtectionError::MissingIdentity(path.clone()))?;
            let canonical = fs
                .canonicalize(&path)
                .map_err(|error| ProtectionError::Invalid(path.clone(), error))?;
            let after = fs
                .metadata_no_follow(&canonical)
                .map_err(|error| ProtectionError::Invalid(canonical.clone(), error))?;
            if after.kind == EntryKind::LinkLike {
                return Err(ProtectionError::LinkLike(canonical));
            }
            if after.identity != Some(identity) {
                return Err(ProtectionError::Ambiguous(path));
            }
            if !keys.insert(semantics.key(&canonical)) {
                return Err(ProtectionError::Ambiguous(canonical));
            }
            paths.push(canonical);
        }
        paths.sort_by_key(|path| semantics.key(path));
        Ok(Self { paths, semantics })
    }

    pub fn is_protected(&self, path: &Path) -> bool {
        self.is_repository_metadata(path)
            || self
                .paths
                .iter()
                .any(|root| self.semantics.contains(root, path))
    }
    pub fn is_repository_metadata(&self, path: &Path) -> bool {
        path.components().any(|component| match component {
            Component::Normal(name) => matches!(
                name.to_string_lossy().to_ascii_lowercase().as_str(),
                ".git" | ".hg" | ".svn" | ".bzr"
            ),
            _ => false,
        })
    }
    pub fn protected_paths(&self) -> &[PathBuf] {
        &self.paths
    }
    pub fn semantics(&self) -> PathSemantics {
        self.semantics
    }
}
