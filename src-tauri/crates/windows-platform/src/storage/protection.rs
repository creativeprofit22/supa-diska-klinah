//! Additional generic-storage exclusions; never replaces the existing Windows,
//! Documents, executable, or configured protection policy. Native resolution
//! failures fail closed, including during persisted personal-file validation.
use super::known_folders::{KnownFolder, KnownFolderResolver, NativeKnownFolders};
use cleanup_core::{PathSemantics, storage::StorageError};
use std::path::{Component, Path, PathBuf};

pub(crate) struct MachineRoots(Vec<PathBuf>);
impl MachineRoots {
    pub(crate) fn resolve() -> Result<Self, StorageError> {
        [
            KnownFolder::ProgramFiles,
            KnownFolder::ProgramFilesX86,
            KnownFolder::ProgramData,
        ]
        .into_iter()
        .map(|folder| NativeKnownFolders.resolve(folder))
        .collect::<Result<Vec<_>, _>>()
        .map(Self)
    }
    pub(crate) fn excludes(&self, path: &Path) -> bool {
        self.0
            .iter()
            .any(|root| PathSemantics::CaseInsensitive.contains(root, path))
            || path
                .components()
                .find_map(|c| match c {
                    Component::Normal(name) => Some(name.to_string_lossy().to_lowercase()),
                    _ => None,
                })
                .is_some_and(|name| {
                    matches!(
                        name.as_str(),
                        "windows"
                            | "program files"
                            | "program files (x86)"
                            | "programdata"
                            | "system volume information"
                            | "$recycle.bin"
                            | "recovery"
                            | "config.msi"
                    )
                })
    }
}
pub(crate) fn personal_path_allowed(path: &Path) -> bool {
    MachineRoots::resolve().is_ok_and(|roots| !roots.excludes(path))
}
