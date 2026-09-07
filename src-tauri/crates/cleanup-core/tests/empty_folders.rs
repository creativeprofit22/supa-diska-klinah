use cleanup_core::{storage::*, *};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
struct FakeFs {
    entries: BTreeMap<PathBuf, EntryMetadata>,
    unreadable: Option<PathBuf>,
    hidden: Option<PathBuf>,
    unknown: bool,
}
impl FileSystem for FakeFs {
    fn semantics(&self) -> PathSemantics {
        PathSemantics::CaseInsensitive
    }
    fn metadata_no_follow(&self, p: &Path) -> Result<EntryMetadata, FsError> {
        self.entries
            .get(p)
            .cloned()
            .ok_or_else(|| FsError::new(FsErrorKind::NotFound, "missing"))
    }
    fn canonicalize(&self, p: &Path) -> Result<PathBuf, FsError> {
        Ok(p.to_owned())
    }
    fn hidden_or_system(&self, p: &Path, _: &EntryMetadata) -> Option<bool> {
        if self.unknown {
            None
        } else {
            Some(self.hidden.as_deref() == Some(p))
        }
    }
    fn read_dir(
        &self,
        p: &Path,
        _: FileIdentity,
        visitor: &mut dyn FnMut(DirectoryEntry) -> ReadDirControl,
    ) -> Result<(), FsError> {
        if self.unreadable.as_deref() == Some(p) {
            return Err(FsError::new(FsErrorKind::PermissionDenied, "unreadable"));
        }
        for (path, m) in &self.entries {
            if path.parent() == Some(p)
                && visitor(DirectoryEntry {
                    path: path.clone(),
                    name: path.file_name().unwrap().to_string_lossy().into_owned(),
                    kind: m.kind,
                    identity: m.identity,
                }) == ReadDirControl::Stop
            {
                break;
            }
        }
        Ok(())
    }
}
fn fixture() -> (FakeFs, RootAuthorization, ProtectionPolicy) {
    let base = std::env::temp_dir().join("fake-empty-folders");
    let root = base.join("scan");
    let mut fs = FakeFs {
        entries: BTreeMap::new(),
        unreadable: None,
        hidden: None,
        unknown: false,
    };
    for (n, path) in root
        .ancestors()
        .map(Path::to_owned)
        .chain([
            base.join("system"),
            base.join("documents"),
            base.join("app"),
            root.join("a"),
            root.join("a/b"),
            root.join("a/b/c"),
        ])
        .enumerate()
    {
        fs.entries.insert(
            path,
            EntryMetadata {
                kind: EntryKind::Directory,
                identity: Some(FileIdentity {
                    volume: 1,
                    file: n as u64 + 1,
                }),
                size: 0,
                modified: None,
            },
        );
    }
    let protection = ProtectionPolicy::compile(
        &fs,
        ProtectionInputs::new(
            vec![base.join("system")],
            vec![base.join("documents")],
            vec![base.join("app")],
        )
        .unwrap(),
    )
    .unwrap();
    let root = RootAuthorization {
        snapshot_id: format!("{:032x}", 1),
        root_id: format!("{:032x}", 2),
        identity: fs.entries[&root].identity.unwrap(),
        canonical_path: root,
    };
    (fs, root, protection)
}
#[test]
fn empty_folders_recursive_complete_evidence_and_all_blockers() {
    for mode in [
        "empty",
        "file",
        "hidden-file",
        "hidden-dir",
        "excluded",
        "protected",
        "unreadable",
        "link",
        "cloud",
        "depth",
        "cancel",
        "unknown",
    ] {
        let (mut fs, root, policy) = fixture();
        let child = root.canonical_path.join("a/b/c");
        match mode {
            "file" | "hidden-file" => fs.entries.get_mut(&child).unwrap().kind = EntryKind::File,
            "link" | "cloud" => fs.entries.get_mut(&child).unwrap().kind = EntryKind::LinkLike,
            "unreadable" => fs.unreadable = Some(child.clone()),
            "unknown" => fs.unknown = true,
            _ => {}
        }
        if mode.starts_with("hidden") {
            fs.hidden = Some(child.clone());
        }
        if mode == "protected" {
            let m = fs.entries.remove(&child).unwrap();
            fs.entries.insert(root.canonical_path.join("a/b/.git"), m);
        }
        let cancellation = CancellationToken::new();
        let mut folders = empty_folders::EmptyFolders::default();
        let limits = StorageLimits {
            depth: if mode == "depth" { 2 } else { 64 },
            ..Default::default()
        };
        let result = walk::walk(
            &fs,
            &root,
            walk::WalkPolicy {
                protection: &policy,
                excluded: &|path| {
                    (mode == "excluded" && path == child)
                        || empty_folders::blocks_visibility(&fs, path)
                },
            },
            &cancellation,
            limits,
            &|_| {},
            &mut |event| {
                if mode == "cancel"
                    && matches!(&event, walk::WalkEvent::Entry { path, .. } if *path == child)
                {
                    cancellation.cancel();
                }
                folders.observe(&root, event)
            },
        );
        let rows = folders.finish();
        if mode == "empty" {
            assert!(result.unwrap().completeness.is_complete());
            assert_eq!(rows.len(), 3);
            for (index, (row, evidence)) in rows.iter().enumerate() {
                assert_eq!(row.depth, 3 - index as u16);
                assert_eq!(row.descendant_directories, index);
                assert_ne!(evidence.entry().canonical_path, root.canonical_path);
                evidence.validate().unwrap();
            }
        } else {
            assert!(rows.is_empty(), "{mode}");
        }
    }
}
