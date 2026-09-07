use cleanup_core::{storage::*, *};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

#[derive(Default)]
struct EntropyCounter(AtomicU64);
impl Entropy for EntropyCounter {
    fn fill(&self, bytes: &mut [u8]) -> Result<(), FsError> {
        bytes.fill(0);
        bytes[..8].copy_from_slice(&self.0.fetch_add(1, Ordering::Relaxed).to_le_bytes());
        Ok(())
    }
}
fn id(n: u32) -> String {
    format!("{n:032x}")
}
fn file(n: u32) -> StorageRecord {
    StorageRecord::File(large_files::FileRecord {
        record_id: id(n),
        display_path: format!("file-{n}"),
        logical_bytes: n.into(),
        allocated_bytes: None,
        modified_unix_seconds: None,
        eligibility: CandidateEligibility::ReadOnly,
    })
}
fn snapshot(n: u32, entropy: &EntropyCounter) -> StorageSnapshot {
    let mut builder = SnapshotBuilder::new(id(n), StorageModule::LargeFiles, 3).unwrap();
    for n in [3, 1, 2] {
        builder
            .push(
                file(n),
                RecordOrder {
                    numeric: n.into(),
                    text: String::new(),
                },
            )
            .unwrap();
    }
    assert_eq!(
        builder.push(
            file(4),
            RecordOrder {
                numeric: 4,
                text: String::new()
            }
        ),
        Err(StorageError::LimitReached)
    );
    builder.finish(entropy, false).unwrap()
}
fn request(n: u32) -> PageRequest {
    PageRequest {
        snapshot_id: id(n),
        module: StorageModule::LargeFiles,
        collection: PageCollection::Files,
        parent_id: None,
        cursor: None,
        page_size: 2,
    }
}
#[test]
fn storage_paging_is_stable_bounded_expiring_and_snapshot_bound() {
    let entropy = EntropyCounter::default();
    let mut pages = SnapshotPages::default();
    pages
        .insert(snapshot(10, &entropy), Duration::ZERO)
        .unwrap();
    let mut req = request(10);
    let first = pages.page(&req, Duration::ZERO).unwrap();
    assert_eq!(first.records.len(), 2);
    assert_eq!(first.retained_total, 3);
    assert!(
        first
            .completeness
            .reasons
            .contains(&PartialReason::RecordLimit)
    );
    assert!(matches!(&first.records[0], StorageRecord::File(f) if f.logical_bytes == 1));
    assert_eq!(
        serde_json::to_value(&first).unwrap(),
        serde_json::to_value(pages.page(&req, Duration::ZERO).unwrap()).unwrap()
    );
    req.cursor = first.next_cursor.clone();
    let second = pages.page(&req, Duration::from_secs(1)).unwrap();
    assert_eq!(second.records.len(), 1);
    assert!(second.next_cursor.is_none());
    pages
        .insert(snapshot(11, &entropy), Duration::from_secs(1))
        .unwrap();
    req.snapshot_id = id(11);
    assert_eq!(
        pages.page(&req, Duration::from_secs(1)).unwrap_err(),
        StorageError::InvalidCursor
    );
    req.cursor = None;
    req.collection = PageCollection::Drives;
    assert_eq!(
        pages.page(&req, Duration::from_secs(1)).unwrap_err(),
        StorageError::InvalidRequest
    );
    req.collection = PageCollection::Files;
    req.parent_id = Some(id(1));
    assert!(pages.page(&req, Duration::from_secs(1)).is_err());
    req.parent_id = None;
    req.page_size = 201;
    assert!(pages.page(&req, Duration::from_secs(1)).is_err());
    pages
        .insert(snapshot(12, &entropy), Duration::from_secs(1))
        .unwrap();
    assert!(pages.page(&request(10), Duration::from_secs(1)).is_err());
    assert!(pages.page(&request(11), Duration::from_secs(601)).is_err());
    pages.release(&id(12)).unwrap_err(); // expiry removes both records
}
fn directory(n: u32, parent: Option<u32>) -> StorageRecord {
    StorageRecord::Directory(analysis::DirectorySummary {
        node_id: id(n),
        parent_id: parent.map(id),
        display_path: format!("dir-{n}"),
        logical_bytes: 0,
        allocated_bytes: None,
        independent_files: 0,
        hard_link_entries: 0,
        completeness: Completeness::default(),
    })
}
#[test]
fn storage_tree_rejects_missing_parents_cycles_and_cross_parent_cursors() {
    let entropy = EntropyCounter::default();
    for rows in [
        vec![directory(1, Some(1))],
        vec![directory(1, Some(2)), directory(2, Some(1))],
        vec![directory(1, Some(9))],
    ] {
        let mut builder = SnapshotBuilder::new(id(10), StorageModule::DiskAnalyzer, 10).unwrap();
        for row in rows {
            builder
                .push(
                    row,
                    RecordOrder {
                        numeric: 0,
                        text: String::new(),
                    },
                )
                .unwrap();
        }
        assert!(builder.finish(&entropy, false).is_err());
    }
    let mut builder = SnapshotBuilder::new(id(10), StorageModule::DiskAnalyzer, 10).unwrap();
    for row in [
        directory(1, None),
        directory(2, Some(1)),
        directory(3, Some(1)),
        directory(4, None),
    ] {
        builder
            .push(
                row,
                RecordOrder {
                    numeric: 0,
                    text: String::new(),
                },
            )
            .unwrap();
    }
    let mut pages = SnapshotPages::default();
    pages
        .insert(builder.finish(&entropy, false).unwrap(), Duration::ZERO)
        .unwrap();
    let mut req = PageRequest {
        snapshot_id: id(10),
        module: StorageModule::DiskAnalyzer,
        collection: PageCollection::Tree,
        parent_id: Some(id(1)),
        cursor: None,
        page_size: 1,
    };
    req.cursor = pages.page(&req, Duration::ZERO).unwrap().next_cursor;
    req.parent_id = Some(id(4));
    assert_eq!(
        pages.page(&req, Duration::ZERO).unwrap_err(),
        StorageError::InvalidCursor
    );
    req.cursor = None;
    req.parent_id = Some(id(99));
    assert_eq!(
        pages.page(&req, Duration::ZERO).unwrap_err(),
        StorageError::InvalidRequest
    );
}
#[test]
fn storage_candidate_evidence_is_bound_to_rows_and_never_legacy_executable() {
    let (fs, root, policy) = fixture();
    let entry = ObservedEntry {
        canonical_path: root.canonical_path.join("personal.txt"),
        identity: FileIdentity {
            volume: 1,
            file: 200,
        },
        kind: EntryKind::File,
        logical_bytes: 10,
        allocated_bytes: Some(10),
        modified_unix_nanos: 1,
    };
    let evidence = StorageEvidence::UserSelectedFile {
        root: root.clone(),
        entry: entry.clone(),
    };
    evidence.validate().unwrap();
    let mut builder =
        SnapshotBuilder::new(root.snapshot_id.clone(), StorageModule::LargeFiles, 10).unwrap();
    builder.add_candidate(id(90), evidence.clone()).unwrap();
    builder
        .push(
            StorageRecord::File(large_files::FileRecord {
                record_id: id(91),
                display_path: "mismatched-path".into(),
                logical_bytes: 10,
                allocated_bytes: Some(10),
                modified_unix_seconds: None,
                eligibility: CandidateEligibility::Eligible {
                    candidate_id: id(90),
                },
            }),
            RecordOrder {
                numeric: 10,
                text: String::new(),
            },
        )
        .unwrap();
    assert!(builder.finish(&EntropyCounter::default(), false).is_err());
    let catalog = load_catalog(
        std::io::Cursor::new(include_str!("fixtures/catalog-v1.json")),
        CatalogLimits::default(),
    )
    .unwrap();
    let proof = ResolvedCandidate {
        scope: CandidateProofScope::Storage {
            evidence: Box::new(evidence),
        },
        path: entry.canonical_path,
        scan_root: root.canonical_path.clone(),
        context_root: root.canonical_path,
        context_identity: Some(root.identity),
        rule: catalog.rules()[0].clone(),
        identity: entry.identity,
        kind: entry.kind,
        logical_bytes: 10,
        allocated_bytes: 10,
        scanned_at: std::time::SystemTime::UNIX_EPOCH,
    };
    assert_eq!(
        revalidate_candidate(&fs, &proof, &policy, std::time::SystemTime::now()),
        Err(CandidateRejection::InvalidProof)
    );
}

fn evidence_fixtures() -> Vec<StorageEvidence> {
    let fixture = include_str!("../src/storage/fixtures/evidence-v2.json");
    let fixture = if cfg!(windows) {
        fixture.to_owned()
    } else {
        fixture.replace("C:/fixture", "/fixture")
    };
    serde_json::from_str(&fixture).unwrap()
}
#[test]
fn all_six_storage_evidence_variants_round_trip_and_fail_closed() {
    let fixtures = evidence_fixtures();
    assert_eq!(fixtures.len(), 6);
    for evidence in fixtures {
        evidence.validate().unwrap();
        let scope = CandidateProofScope::Storage {
            evidence: Box::new(evidence.clone()),
        };
        assert_eq!(
            serde_json::to_value(&scope).unwrap(),
            serde_json::json!({"kind": "storage", "evidence": evidence})
        );
        let json = serde_json::to_string(&scope).unwrap();
        assert_eq!(
            serde_json::from_str::<CandidateProofScope>(&json).unwrap(),
            scope
        );
        let mut value = serde_json::to_value(&scope).unwrap();
        value["evidence"]["unexpected"] = true.into();
        assert!(serde_json::from_value::<CandidateProofScope>(value).is_err());
        let mut invalid = evidence.clone();
        match &mut invalid {
            StorageEvidence::CatalogTarget { catalog, .. } => {
                catalog.newest_descendant_unix_nanos = u64::MAX
            }
            StorageEvidence::UserSelectedFile { entry, .. } => entry.kind = EntryKind::Directory,
            StorageEvidence::DuplicateMember { entry, keeper, .. } => {
                keeper.keeper.identity = entry.identity
            }
            StorageEvidence::EmptyFolder {
                complete_subtree, ..
            } => *complete_subtree = false,
            StorageEvidence::BrowserCache {
                browser_inactive, ..
            } => *browser_inactive = false,
            StorageEvidence::ConfirmedApplicationLeftover {
                uninstall_confirmation_id,
                ..
            } => uninstall_confirmation_id.clear(),
        }
        assert_eq!(invalid.validate(), Err(StorageError::InvalidEvidence));
    }
}
#[test]
fn duplicate_rows_bind_group_keeper_counts_and_evidence() {
    let evidence = evidence_fixtures().remove(2);
    let StorageEvidence::DuplicateMember {
        root,
        entry,
        keeper,
    } = &evidence
    else {
        unreachable!()
    };
    for corruption in 0..7 {
        let mut builder =
            SnapshotBuilder::new(root.snapshot_id.clone(), StorageModule::Duplicates, 10).unwrap();
        let mut proof = evidence.clone();
        if let StorageEvidence::DuplicateMember { keeper, .. } = &mut proof
            && corruption == 1
        {
            keeper.group_id = id(999);
        }
        builder.add_candidate(id(90), proof).unwrap();
        let group = StorageRecord::DuplicateGroup(duplicates::DuplicateGroup {
            group_id: keeper.group_id.clone(),
            member_count: if corruption == 2 { 3 } else { 2 },
            independent_copies: if corruption == 3 { 3 } else { 2 },
            bytes_per_copy: if corruption == 4 { 11 } else { 10 },
            completeness: Completeness::default(),
        });
        let member = StorageRecord::DuplicateMember(duplicates::DuplicateMember {
            group_id: keeper.group_id.clone(),
            file: large_files::FileRecord {
                record_id: id(91),
                display_path: entry.canonical_path.to_string_lossy().into_owned(),
                logical_bytes: 10,
                allocated_bytes: Some(16),
                modified_unix_seconds: Some(1),
                eligibility: CandidateEligibility::Eligible {
                    candidate_id: id(90),
                },
            },
        });
        let keeper_row = StorageRecord::DuplicateMember(duplicates::DuplicateMember {
            group_id: keeper.group_id.clone(),
            file: large_files::FileRecord {
                record_id: id(92),
                display_path: if corruption == 5 {
                    entry.canonical_path.to_string_lossy().into_owned()
                } else {
                    keeper.keeper.canonical_path.to_string_lossy().into_owned()
                },
                logical_bytes: 10,
                allocated_bytes: Some(16),
                modified_unix_seconds: Some(1),
                eligibility: if corruption == 6 {
                    CandidateEligibility::Ineligible {
                        reason: PartialReason::Changed,
                    }
                } else {
                    CandidateEligibility::ReadOnly
                },
            },
        });
        for row in [group, member, keeper_row] {
            builder
                .push(
                    row,
                    RecordOrder {
                        numeric: 0,
                        text: String::new(),
                    },
                )
                .unwrap();
        }
        let snapshot = builder.finish(&EntropyCounter::default(), false);
        assert_eq!(snapshot.is_ok(), corruption == 0, "corruption {corruption}");
        if let Ok(snapshot) = snapshot {
            let mut pages = SnapshotPages::default();
            pages.insert(snapshot, Duration::ZERO).unwrap();
            let mut request = PageRequest {
                snapshot_id: root.snapshot_id.clone(),
                module: StorageModule::Duplicates,
                collection: PageCollection::DuplicateMembers,
                parent_id: Some(keeper.group_id.clone()),
                cursor: None,
                page_size: 1,
            };
            let first = pages.page(&request, Duration::ZERO).unwrap();
            assert_eq!(first.records.len(), 1);
            assert_eq!(first.retained_total, 2);
            request.cursor = first.next_cursor.clone();
            let second = pages.page(&request, Duration::ZERO).unwrap();
            assert_eq!(second.records.len(), 1);
            let StorageRecord::DuplicateMember(first_member) = &first.records[0] else {
                panic!()
            };
            let StorageRecord::DuplicateMember(second_member) = &second.records[0] else {
                panic!()
            };
            assert_ne!(first_member.file.record_id, second_member.file.record_id);
            assert!(second.next_cursor.is_none());
            request.parent_id = Some(id(999));
            assert!(pages.page(&request, Duration::ZERO).is_err());
            request.parent_id = Some(keeper.group_id.clone());
            request.page_size = 201;
            assert!(pages.page(&request, Duration::ZERO).is_err());
            request.page_size = 1;
            request.parent_id = None;
            assert!(pages.page(&request, Duration::ZERO).is_err());
            request.collection = PageCollection::DuplicateGroups;
            request.cursor = None;
            assert_eq!(
                pages.page(&request, Duration::ZERO).unwrap().records.len(),
                1
            );
        }
    }
}

#[test]
fn storage_requests_reject_unknown_fields_and_limits() {
    let value = serde_json::json!({"snapshotId": id(1), "module": "largeFiles", "collection": "files", "parentId": null, "cursor": null});
    let req: PageRequest = decode_request(&serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(decode_request::<PageRequest>(&vec![b' '; MAX_REQUEST_BYTES + 1]).is_err());
    let mut oversized = value.clone();
    oversized["pageSize"] = 201.into();
    assert!(decode_request::<PageRequest>(&serde_json::to_vec(&oversized).unwrap()).is_err());
    let selection = StorageSelection {
        snapshot_id: id(1),
        module: StorageModule::LargeFiles,
        candidate_ids: (0..=MAX_SELECTION as u32).map(id).collect(),
    };
    assert!(decode_request::<StorageSelection>(&serde_json::to_vec(&selection).unwrap()).is_err());
    assert_eq!(req.page_size, 100);
    let mut bad = value;
    bad["path"] = "C:/anything".into();
    assert!(serde_json::from_value::<PageRequest>(bad).is_err());
    let mut limits = StorageLimits {
        depth: 65,
        ..StorageLimits::default()
    };
    assert!(limits.validate().is_err());
    limits = StorageLimits {
        workers: 5,
        ..StorageLimits::default()
    };
    assert!(limits.validate().is_err());
    assert!(!is_local_storage_path(Path::new(r"\\server\share\file")));
    for path in [
        r"\\?\UNC\server\share\file",
        r"\\?\GLOBALROOT\Device\HarddiskVolume1\file",
        r"\\.\C:\file",
        r"\\?\C:\file:stream",
        r"\\?\Volume{abc}\file",
    ] {
        assert!(!is_local_storage_path(Path::new(path)), "{path}");
    }
    assert!(!is_local_storage_path(Path::new(r"C:\file:stream")));
    assert!(PathSemantics::CaseInsensitive.contains(Path::new("C:/"), Path::new("C:/folder")));
}

#[cfg(windows)]
#[test]
fn storage_accepts_real_canonical_local_disk_paths_without_allowing_device_namespaces() {
    struct OwnedDirectory(PathBuf);
    impl Drop for OwnedDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let path = std::env::temp_dir().join(format!(
        "storage-canonical-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&path).unwrap();
    let _owned = OwnedDirectory(path.clone());
    std::fs::write(path.join("child"), b"fixture").unwrap();
    let canonical = std::fs::canonicalize(&path).unwrap();
    let child = std::fs::canonicalize(path.join("child")).unwrap();
    assert!(canonical.to_str().unwrap().starts_with(r"\\?\"));
    assert!(is_local_storage_path(&canonical));
    assert!(is_local_storage_path(&child));
    // TEMP may use an 8.3 alias. Compare the verified canonical display form,
    // not the uncanonicalized input alias (which must still fail closed).
    let display = Path::new(canonical.to_str().unwrap().strip_prefix(r"\\?\").unwrap());
    assert!(PathSemantics::CaseInsensitive.equivalent(display, &canonical));
    assert!(PathSemantics::CaseInsensitive.contains(display, &child));
    RootAuthorization {
        snapshot_id: id(1),
        root_id: id(2),
        canonical_path: canonical,
        identity: FileIdentity { volume: 1, file: 1 },
    }
    .validate()
    .unwrap();
}

struct FakeFs {
    entries: BTreeMap<PathBuf, EntryMetadata>,
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
    fn read_dir(
        &self,
        p: &Path,
        identity: FileIdentity,
        visitor: &mut dyn FnMut(DirectoryEntry) -> ReadDirControl,
    ) -> Result<(), FsError> {
        if self.metadata_no_follow(p)?.identity != Some(identity) {
            return Err(FsError::new(FsErrorKind::Changed, "changed"));
        }
        for (path, meta) in &self.entries {
            if path.parent() == Some(p)
                && visitor(DirectoryEntry {
                    path: path.clone(),
                    name: path.file_name().unwrap().to_string_lossy().into(),
                    kind: meta.kind,
                    identity: meta.identity,
                }) == ReadDirControl::Stop
            {
                break;
            }
        }
        Ok(())
    }
}
fn fixture() -> (FakeFs, RootAuthorization, ProtectionPolicy) {
    let root = std::env::temp_dir().join("storage-core-fixture");
    let mut entries = BTreeMap::new();
    for (index, path) in (1u64..).zip(root.ancestors().map(Path::to_owned).chain([
        root.join("a"),
        root.join("a/link"),
        root.join("system"),
        root.join("user"),
        root.join("config"),
    ])) {
        let kind = if path.ends_with("link") {
            EntryKind::LinkLike
        } else {
            EntryKind::Directory
        };
        entries.insert(
            path,
            EntryMetadata {
                kind,
                identity: Some(FileIdentity {
                    volume: 1,
                    file: index,
                }),
                size: 0,
                modified: None,
            },
        );
    }
    let fs = FakeFs { entries };
    let policy = ProtectionPolicy::compile(
        &fs,
        ProtectionInputs::new(
            vec![root.join("system")],
            vec![root.join("user")],
            vec![root.join("config")],
        )
        .unwrap(),
    )
    .unwrap();
    let auth = RootAuthorization {
        snapshot_id: id(1),
        root_id: id(2),
        canonical_path: root.clone(),
        identity: fs.entries[&root].identity.unwrap(),
    };
    (fs, auth, policy)
}
#[test]
fn storage_walk_propagates_skipped_contents_to_every_ancestor() {
    let (fs, root, policy) = fixture();
    let mut ancestors = Vec::new();
    let report = walk::walk(
        &fs,
        &root,
        walk::WalkPolicy {
            protection: &policy,
            excluded: &|_| false,
        },
        &CancellationToken::new(),
        StorageLimits::default(),
        &|_| {},
        &mut |event| {
            if let walk::WalkEvent::LeaveDirectory {
                path, completeness, ..
            } = event
            {
                ancestors.push((path.to_owned(), completeness.clone()));
            }
            walk::WalkControl::Continue
        },
    )
    .unwrap();
    assert!(
        report
            .completeness
            .reasons
            .contains(&PartialReason::LinkLike)
    );
    assert!(
        report
            .completeness
            .reasons
            .contains(&PartialReason::Protected)
    );
    assert!(ancestors.iter().all(|(_, status)| !status.is_complete()));
    assert_eq!(ancestors.len(), 2);
}
#[test]
fn storage_walk_cancellation_depth_entry_diagnostic_and_retention_limits_are_partial() {
    let (fs, root, policy) = fixture();
    for (limits, expected) in [
        (
            StorageLimits {
                depth: 0,
                ..StorageLimits::default()
            },
            PartialReason::DepthLimit,
        ),
        (
            StorageLimits {
                visited_entries: 1,
                ..StorageLimits::default()
            },
            PartialReason::EntryLimit,
        ),
        (
            StorageLimits {
                diagnostics: 1,
                ..StorageLimits::default()
            },
            PartialReason::DiagnosticLimit,
        ),
        (
            StorageLimits {
                retained_records: 1,
                ..StorageLimits::default()
            },
            PartialReason::RecordLimit,
        ),
    ] {
        let report = walk::walk(
            &fs,
            &root,
            walk::WalkPolicy {
                protection: &policy,
                excluded: &|_| false,
            },
            &CancellationToken::new(),
            limits,
            &|_| {},
            &mut |_| walk::WalkControl::Continue,
        )
        .unwrap();
        assert!(
            report.completeness.reasons.contains(&expected),
            "{:?}",
            report.completeness
        );
        assert!(report.visited_entries <= limits.visited_entries);
        assert!(report.diagnostics.len() <= limits.diagnostics);
    }
    let cancellation = CancellationToken::new();
    let report = walk::walk(
        &fs,
        &root,
        walk::WalkPolicy {
            protection: &policy,
            excluded: &|_| false,
        },
        &cancellation,
        StorageLimits::default(),
        &|_| {},
        &mut |_| {
            cancellation.cancel();
            walk::WalkControl::Continue
        },
    )
    .unwrap();
    assert!(
        report
            .completeness
            .reasons
            .contains(&PartialReason::Cancelled)
    );
    let mut stale = root;
    stale.identity.file += 100;
    assert!(
        walk::walk(
            &fs,
            &stale,
            walk::WalkPolicy {
                protection: &policy,
                excluded: &|_| false
            },
            &CancellationToken::new(),
            StorageLimits::default(),
            &|_| {},
            &mut |_| walk::WalkControl::Continue
        )
        .is_err()
    );
}

fn step6_fixture() -> (FakeFs, RootAuthorization, ProtectionPolicy) {
    let (mut fs, root, protection) = fixture();
    // Keep mandatory protection fixtures; their skipped rows make totals partial.
    fs.entries.remove(&root.canonical_path.join("a/link"));
    for (name, kind, identity, size) in [
        ("a/deep", EntryKind::Directory, 101, 0),
        ("a/deep/more", EntryKind::Directory, 102, 0),
        ("a/deep/more/one.TXT", EntryKind::File, 200, 10),
        ("a/alias.txt", EntryKind::File, 200, 10),
        ("b", EntryKind::Directory, 103, 0),
        ("b/cross.txt", EntryKind::File, 200, 10),
        ("b/movie.mp4", EntryKind::File, 201, 30),
        ("b/photo.PNG", EntryKind::File, 202, 20),
    ] {
        fs.entries.insert(
            root.canonical_path.join(name),
            EntryMetadata {
                kind,
                identity: Some(FileIdentity {
                    volume: 1,
                    file: identity,
                }),
                size,
                modified: Some(std::time::UNIX_EPOCH + Duration::from_secs(size)),
            },
        );
    }
    (fs, root, protection)
}
fn step6_analyze(
    depth: u16,
    limits: StorageLimits,
    cancel: bool,
) -> (
    Vec<analysis::DirectorySummary>,
    Vec<analysis::ExtensionSummary>,
) {
    let (fs, root, protection) = step6_fixture();
    let mut analyzer =
        analysis::Analyzer::new(&root.canonical_path, depth, limits.retained_records).unwrap();
    let token = CancellationToken::new();
    let report = walk::walk(
        &fs,
        &root,
        walk::WalkPolicy {
            protection: &protection,
            excluded: &|_| false,
        },
        &token,
        limits,
        &|_| {},
        &mut |event| {
            let result = analyzer.observe(event, Some(4096));
            if cancel {
                token.cancel();
            }
            result
        },
    )
    .unwrap();
    analyzer.finish(&report.completeness)
}
#[test]
fn step6_analyzer_full_depth_unique_subtrees_extensions_and_limits() {
    let (rows, types) = step6_analyze(1, StorageLimits::default(), false);
    let root = &rows[0];
    assert_eq!(
        (
            root.logical_bytes,
            root.allocated_bytes,
            root.independent_files,
            root.hard_link_entries
        ),
        (60, Some(12288), 3, 2)
    );
    assert_eq!(rows.len(), 3); // protected mandatory fixtures are omitted
    assert_eq!(rows[1].logical_bytes, 10);
    assert_eq!(rows[1].hard_link_entries, 1);
    assert_eq!(rows[2].logical_bytes, 60); // shared identity counts once in each subtree
    assert_eq!(
        types
            .iter()
            .find(|t| t.extension == "txt")
            .unwrap()
            .file_count,
        1
    );
    assert_eq!(
        step6_analyze(0, StorageLimits::default(), false).0[0].logical_bytes,
        60
    );
    for (limits, cancel, reason) in [
        (
            StorageLimits {
                depth: 1,
                ..StorageLimits::default()
            },
            false,
            PartialReason::DepthLimit,
        ),
        (
            StorageLimits {
                retained_records: 2,
                ..StorageLimits::default()
            },
            false,
            PartialReason::RecordLimit,
        ),
        (
            StorageLimits {
                visited_entries: 2,
                ..StorageLimits::default()
            },
            false,
            PartialReason::EntryLimit,
        ),
        (StorageLimits::default(), true, PartialReason::Cancelled),
    ] {
        let (rows, _) = step6_analyze(0, limits, cancel);
        assert!(
            rows[0].completeness.reasons.contains(&reason),
            "{:?}",
            rows[0]
        );
    }
}
#[test]
fn step6_analyzer_unknown_allocation_and_changed_identity_are_not_complete_estimates() {
    let root = std::env::temp_dir().join("analyzer");
    let mut analyzer = analysis::Analyzer::new(&root, 0, 10).unwrap();
    for (name, size, allocated) in [
        ("a.txt", 10, Some(4096)),
        ("b.txt", 10, None),
        ("c.txt", 11, Some(4096)),
    ] {
        let metadata = EntryMetadata {
            kind: EntryKind::File,
            identity: Some(FileIdentity {
                volume: 1,
                file: if name == "b.txt" { 2 } else { 1 },
            }),
            size,
            modified: None,
        };
        analyzer.observe(
            walk::WalkEvent::Entry {
                path: &root.join(name),
                metadata: &metadata,
                depth: 1,
            },
            allocated,
        );
    }
    let (rows, _) = analyzer.finish(&Completeness::default());
    assert_eq!(rows[0].allocated_bytes, None);
    assert!(
        rows[0]
            .completeness
            .reasons
            .contains(&PartialReason::Changed)
    );
    assert!(
        rows[0]
            .completeness
            .reasons
            .contains(&PartialReason::Unreadable)
    );
}
#[test]
fn step6_analyzer_membership_budget_is_global_and_atomic_not_depth_times_limit() {
    let root = std::env::temp_dir().join("bounded-analyzer");
    let mut analyzer = analysis::Analyzer::new(&root, 64, 128).unwrap();
    let mut path = root.clone();
    for depth in 1..=63 {
        path.push("d");
        let metadata = EntryMetadata {
            kind: EntryKind::Directory,
            identity: Some(FileIdentity {
                volume: 1,
                file: depth.into(),
            }),
            size: 0,
            modified: None,
        };
        assert_eq!(
            analyzer.observe(
                walk::WalkEvent::Entry {
                    path: &path,
                    metadata: &metadata,
                    depth
                },
                None
            ),
            walk::WalkControl::Continue
        );
    }
    for n in 0..2 {
        let metadata = EntryMetadata {
            kind: EntryKind::File,
            identity: Some(FileIdentity {
                volume: 1,
                file: 100 + n,
            }),
            size: 10,
            modified: None,
        };
        let control = analyzer.observe(
            walk::WalkEvent::Entry {
                path: &path.join(format!("{n}.txt")),
                metadata: &metadata,
                depth: 64,
            },
            Some(4096),
        );
        assert_eq!(
            control,
            if n == 0 {
                walk::WalkControl::Continue
            } else {
                walk::WalkControl::Stop
            }
        );
    }
    let (rows, types) = analyzer.finish(&Completeness::default());
    assert_eq!(rows.len(), 64);
    assert!(rows.iter().all(|r| r.logical_bytes == 10
        && r.independent_files == 1
        && r.completeness.reasons.contains(&PartialReason::RecordLimit)));
    assert_eq!(types[0].file_count, 1);
}
#[test]
fn step6_large_files_filters_sorting_evidence_and_bounded_prefix() {
    use large_files::*;
    let (fs, root, protection) = step6_fixture();
    let run = |filter: FileFilter, limit| {
        let mut files = LargeFiles::new(filter, limit).unwrap();
        let report = walk::walk(
            &fs,
            &root,
            walk::WalkPolicy {
                protection: &protection,
                excluded: &|_| false,
            },
            &CancellationToken::new(),
            StorageLimits::default(),
            &|_| {},
            &mut |event| files.observe(event, Some(4096)),
        )
        .unwrap();
        (files.finish(), report)
    };
    let filter = FileFilter {
        minimum_bytes: 10,
        maximum_bytes: Some(30),
        ..FileFilter::default()
    };
    let (rows, _) = run(filter.clone(), 10);
    assert_eq!(
        rows.iter()
            .map(|(r, _)| r.logical_bytes)
            .collect::<Vec<_>>(),
        [30, 20, 10, 10, 10]
    );
    assert!(
        rows.iter()
            .all(|(r, e)| r.eligibility == CandidateEligibility::ReadOnly && e.is_some())
    );
    let (types, _) = run(
        FileFilter {
            extensions: vec!["txt".into()],
            category: FileCategory::Documents,
            ..filter.clone()
        },
        10,
    );
    assert_eq!(types.len(), 3);
    assert!(
        run(
            FileFilter {
                extensions: vec!["txt".into()],
                category: FileCategory::Video,
                ..filter.clone()
            },
            10
        )
        .0
        .is_empty()
    );
    let (rows, _) = run(
        FileFilter {
            sort: FileSort::Modified,
            descending: false,
            ..filter.clone()
        },
        10,
    );
    assert_eq!(rows.first().unwrap().0.logical_bytes, 10);
    let (rows, _) = run(
        FileFilter {
            sort: FileSort::Path,
            descending: true,
            ..filter.clone()
        },
        10,
    );
    assert!(rows[0].0.display_path.ends_with("photo.PNG"));
    let (rows, report) = run(filter.clone(), 2);
    assert_eq!(rows.len(), 2);
    assert!(
        report
            .completeness
            .reasons
            .contains(&PartialReason::RecordLimit)
    );
    for bad in [
        FileFilter {
            maximum_bytes: Some(9),
            ..filter.clone()
        },
        FileFilter {
            extensions: vec!["../exe".into()],
            ..filter.clone()
        },
        FileFilter {
            extensions: vec!["TXT".into()],
            ..filter
        },
    ] {
        assert!(LargeFiles::new(bad, 10).is_err());
    }
}
