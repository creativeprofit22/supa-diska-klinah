use cleanup_core::*;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

struct MemoryFs {
    entries: HashMap<PathBuf, EntryMetadata>,
}
impl FileSystem for MemoryFs {
    fn semantics(&self) -> PathSemantics {
        PathSemantics::CaseInsensitive
    }
    fn metadata_no_follow(&self, path: &Path) -> Result<EntryMetadata, FsError> {
        self.entries
            .get(path)
            .cloned()
            .ok_or_else(|| FsError::new(FsErrorKind::NotFound, "missing"))
    }
    fn canonicalize(&self, path: &Path) -> Result<PathBuf, FsError> {
        if self.entries.contains_key(path) {
            Ok(path.to_path_buf())
        } else {
            Err(FsError::new(FsErrorKind::NotFound, "missing"))
        }
    }
    fn read_dir(
        &self,
        _: &Path,
        _: FileIdentity,
        _: &mut dyn FnMut(DirectoryEntry) -> ReadDirControl,
    ) -> Result<(), FsError> {
        Ok(())
    }
}
fn metadata(kind: EntryKind, file: u64) -> EntryMetadata {
    EntryMetadata {
        kind,
        identity: Some(FileIdentity { volume: 1, file }),
        size: 0,
        modified: None,
    }
}
fn path(value: &str) -> PathBuf {
    PathBuf::from(value)
}

#[test]
fn protects_supplied_subtrees_and_repository_metadata_at_every_depth() {
    let windows = path(r"C:\Windows");
    let documents = path(r"C:\Users\Me\Documents");
    let configured = path(r"D:\Keep");
    let fs = MemoryFs {
        entries: [
            (windows.clone(), metadata(EntryKind::Directory, 1)),
            (documents.clone(), metadata(EntryKind::Directory, 2)),
            (configured.clone(), metadata(EntryKind::Directory, 3)),
        ]
        .into(),
    };
    let policy = ProtectionPolicy::compile(
        &fs,
        ProtectionInputs::new(
            vec![windows.clone()],
            vec![documents.clone()],
            vec![configured.clone()],
        )
        .unwrap(),
    )
    .unwrap();
    assert!(policy.is_protected(&windows.join("Temp")));
    assert!(policy.is_protected(&documents.join("archive")));
    assert!(policy.is_protected(&configured.join("cache")));
    assert!(policy.is_protected(Path::new(r"C:\work\repo\.git\objects")));
    assert!(!policy.is_protected(Path::new(r"C:\work\repo\target")));
}

#[test]
fn missing_protection_classes_are_rejected_before_a_policy_can_reach_scanning() {
    let path = path(r"C:\protected");
    for (system, durable_user, configured, missing) in [
        (
            vec![],
            vec![path.clone()],
            vec![path.clone()],
            ProtectionClass::System,
        ),
        (
            vec![path.clone()],
            vec![],
            vec![path.clone()],
            ProtectionClass::DurableUser,
        ),
        (
            vec![path.clone()],
            vec![path.clone()],
            vec![],
            ProtectionClass::Configured,
        ),
    ] {
        assert!(matches!(
            ProtectionInputs::new(system, durable_user, configured),
            Err(ProtectionError::MissingClass(class)) if class == missing
        ));
    }
}

#[test]
fn fails_closed_for_missing_relative_link_like_and_ambiguous_paths() {
    let system = path(r"C:\Windows");
    let durable = path(r"C:\Users\Me\Documents");
    let good = path(r"D:\Keep");
    let link = path(r"C:\Cloud");
    let fs = MemoryFs {
        entries: [
            (system.clone(), metadata(EntryKind::Directory, 1)),
            (durable.clone(), metadata(EntryKind::Directory, 2)),
            (good.clone(), metadata(EntryKind::Directory, 3)),
            (link.clone(), metadata(EntryKind::LinkLike, 4)),
        ]
        .into(),
    };
    let inputs = |configured| {
        ProtectionInputs::new(vec![system.clone()], vec![durable.clone()], configured).unwrap()
    };
    assert!(matches!(
        ProtectionPolicy::compile(&fs, inputs(vec![path("relative")])),
        Err(ProtectionError::NotAbsolute(_))
    ));
    assert!(matches!(
        ProtectionPolicy::compile(&fs, inputs(vec![path(r"C:\missing")])),
        Err(ProtectionError::Invalid(_, _))
    ));
    assert!(matches!(
        ProtectionPolicy::compile(&fs, inputs(vec![link])),
        Err(ProtectionError::LinkLike(_))
    ));
    assert!(matches!(
        ProtectionPolicy::compile(&fs, inputs(vec![good.clone(), good])),
        Err(ProtectionError::Ambiguous(_))
    ));
}
