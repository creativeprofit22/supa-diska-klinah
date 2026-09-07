use super::*;
use crate::storage::scans::StorageService;
use cleanup_core::ProtectionInputs;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "cleaner-fixture-{}",
            crate::storage::opaque_id().unwrap()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(crate::WindowsFileSystem.canonicalize(&path).unwrap())
    }
    fn file(&self, relative: &str, age: u64) -> PathBuf {
        let path = self.0.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"fixture only").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(age))
            .unwrap();
        path
    }
    fn protection(&self, extra: Option<&Path>) -> ProtectionPolicy {
        let system = self.0.join("protected-system");
        let user = self.0.join("protected-user");
        let config = self.0.join("protected-config");
        for path in [&system, &user, &config] {
            std::fs::create_dir_all(path).unwrap();
        }
        let mut configured = vec![config];
        if let Some(p) = extra {
            configured.push(p.into());
        }
        ProtectionPolicy::compile(
            &crate::WindowsFileSystem,
            ProtectionInputs::new(vec![system], vec![user], configured).unwrap(),
        )
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
impl KnownFolderResolver for Fixture {
    fn resolve(&self, folder: KnownFolder) -> Result<PathBuf, StorageError> {
        let path = self.0.join(match folder {
            KnownFolder::Local => "local",
            KnownFolder::Roaming => "roaming",
            KnownFolder::Profile => "home",
            KnownFolder::LocalLow => "low",
            KnownFolder::ProgramFiles | KnownFolder::ProgramFilesX86 | KnownFolder::ProgramData => {
                return Err(StorageError::UnsupportedScope);
            }
        });
        std::fs::create_dir_all(&path).unwrap();
        super::super::known_folders::validate_binding(path)
    }
}
fn scan(
    f: Arc<Fixture>,
    root: &str,
    p: ProtectionPolicy,
) -> Result<Vec<StorageEvidence>, crate::storage::scans::JobError> {
    let s = StorageService::new();
    let root_id = s.authorize_root(&f.0.join(root), &p)?;
    let id = s.start_with(
        StorageModule::Cleaner,
        Some(&root_id),
        StorageLimits::default(),
        Some(p.clone()),
        move |ctx| discover_with(ctx, &p, f.as_ref()).map_err(Into::into),
    )?;
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let (status, error) = s.status(&id)?;
        if error.is_some() || status.phase == StoragePhase::Failed {
            return Err(StorageError::UnsupportedScope.into());
        }
        if status.phase == StoragePhase::Complete {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    let req = PageRequest {
        snapshot_id: id.clone(),
        module: StorageModule::Cleaner,
        collection: PageCollection::Files,
        parent_id: None,
        cursor: None,
        page_size: 100,
    };
    let page = loop {
        if let Ok(p) = s.page(&req) {
            break p;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    };
    let candidate_ids: Vec<_> = page
        .records
        .iter()
        .filter_map(|r| match r {
            StorageRecord::File(f) => match &f.eligibility {
                CandidateEligibility::Eligible { candidate_id } => Some(candidate_id.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect();
    if candidate_ids.is_empty() {
        return Ok(vec![]);
    }
    s.resolve_selection(&StorageSelection {
        snapshot_id: id,
        module: StorageModule::Cleaner,
        candidate_ids,
    })
}
#[test]
fn record_limit_preserves_partial_cleaner_results() {
    let f = Arc::new(Fixture::new());
    f.file("local/NVIDIA/DXCache/one", 90000);
    f.file("local/NVIDIA/DXCache/two", 90000);
    let protection = f.protection(None);
    let service = StorageService::new();
    let root = service
        .authorize_root(&f.0.join("local/NVIDIA/DXCache"), &protection)
        .unwrap();
    let id = service
        .start_with(
            StorageModule::Cleaner,
            Some(&root),
            StorageLimits {
                retained_records: 1,
                ..StorageLimits::default()
            },
            Some(protection.clone()),
            move |ctx| discover_with(ctx, &protection, f.as_ref()).map_err(Into::into),
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let (status, error) = service.status(&id).unwrap();
        assert!(error.is_none(), "{error:?}");
        assert_ne!(status.phase, StoragePhase::Failed);
        if let Ok(page) = service.page(&PageRequest {
            snapshot_id: id.clone(),
            module: StorageModule::Cleaner,
            collection: PageCollection::Files,
            parent_id: None,
            cursor: None,
            page_size: 100,
        }) {
            let (status, error) = service.status(&id).unwrap();
            assert!(error.is_none());
            assert_eq!(status.phase, StoragePhase::Complete);
            assert_eq!(page.records.len(), 1);
            assert!(
                status
                    .completeness
                    .reasons
                    .contains(&PartialReason::RecordLimit)
            );
            assert!(
                page.completeness
                    .reasons
                    .contains(&PartialReason::RecordLimit)
            );
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
}

#[test]
fn pinned_catalog_provenance_and_excluded_inventory() {
    let c = catalog_inventory();
    let mut keys = std::collections::BTreeSet::new();
    for t in &c.targets {
        assert!(keys.insert((&t.catalog_id, &t.target_id)));
        assert_eq!(t.revision, super::super::browser::REVISION);
        assert_eq!(t.lifecycle, Lifecycle::Candidate);
        assert_eq!(t.risk, Risk::HighImpact);
        assert!(
            t.minimum_age_seconds >= 3600 && !t.consequence.is_empty() && !t.exclusions.is_empty()
        );
        if matches!(t.catalog_id.as_str(), "steam" | "databases" | "misc") {
            assert!(t.unsupported_reason.is_some());
        }
    }
    // Counts were compared to COMPLETE pinned JSON, including excluded paths.
    for (id, count) in [
        ("system", 55),
        ("apps", 334),
        ("gaming", 39),
        ("gpu-caches", 9),
        ("misc", 17),
        ("steam", 23),
        ("databases", 108),
    ] {
        assert_eq!(
            c.targets
                .iter()
                .filter(|t| t.catalog_id == id)
                .map(|t| &t.path)
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            count,
            "{id}"
        );
    }
    // Pinned misc has NO cache roots: protected event logs and null trash only.
    let actual: std::collections::BTreeSet<_> = c
        .targets
        .iter()
        .filter(|t| t.catalog_id == "misc")
        .map(|t| t.path.clone())
        .collect();
    let mut expected: std::collections::BTreeSet<_> = [
        "microsoft-windows-diagnostics-performance%4operational.evtx",
        "security.evtx",
        "system.evtx",
        "application.evtx",
        "setup.evtx",
        "microsoft-windows-windows defender%4operational.evtx",
        "microsoft-windows-powershell%4operational.evtx",
        "microsoft-windows-sysmon%4operational.evtx",
        "microsoft-windows-taskscheduler%4operational.evtx",
        "microsoft-windows-wmi-activity%4operational.evtx",
        "microsoft-windows-bits-client%4operational.evtx",
        "microsoft-windows-ntlm%4operational.evtx",
        "microsoft-windows-dns-client%4operational.evtx",
        "microsoft-windows-groupPolicy%4operational.evtx",
        "microsoft-windows-codeintegrity%4operational.evtx",
        "microsoft-windows-appLocker%4exe and dll.evtx",
    ]
    .into_iter()
    .map(|n| format!("windows/System32/winevt/Logs/{n}"))
    .collect();
    expected.insert("unsupported/native-recycle-bin".into());
    assert_eq!(actual, expected);
    assert!(std::ptr::eq(compiled_targets(), compiled_targets()));
}
#[test]
fn literal_native_recency_protection_and_scope_revalidation() {
    let f = Arc::new(Fixture::new());
    let old = f.file("local/NVIDIA/DXCache/old", 90000);
    let recent = f.file("local/NVIDIA/DXCache/recent", 0);
    let personal = f.file("local/NVIDIA/DXCache/IndexedDB/private", 90000);
    let protected = f.file("local/NVIDIA/DXCache/protected/private", 90000);
    let repo = f.file("local/NVIDIA/DXCache/.git/config", 90000);
    let p = f.protection(protected.parent());
    let proofs = scan(f.clone(), "local/NVIDIA/DXCache", p.clone()).unwrap();
    assert_eq!(proofs.len(), 1);
    assert_eq!(proofs[0].entry().canonical_path, old);
    validate_with(&proofs[0], &p, f.as_ref()).unwrap();
    let proof = plan_proof(proofs[0].clone()).unwrap();
    assert_eq!(proof.rule.lifecycle, Lifecycle::Candidate);
    assert!(
        proof
            .rule
            .provenance
            .source
            .ends_with("rules/win32/gpu-cache.json")
    );
    for change in 0..7 {
        let mut forged = proofs[0].clone();
        if let StorageEvidence::CatalogTarget {
            root,
            catalog,
            entry,
        } = &mut forged
        {
            match change {
                0 => catalog.revision = "stale".into(),
                1 => catalog.rule_version += 1,
                2 => catalog.target_id = "nvidia-gl:0".into(),
                3 => catalog.catalog_id = "system".into(),
                4 => catalog.minimum_age_seconds = 0,
                5 => root.canonical_path = f.0.join("local"),
                _ => entry.kind = EntryKind::Directory,
            }
        }
        assert!(
            validate_with(&forged, &p, f.as_ref()).is_err(),
            "forgery {change}"
        );
    }
    assert!(validate_with(&proofs[0], &f.protection(old.parent()), f.as_ref()).is_err());
    std::fs::write(&old, b"changed").unwrap();
    assert!(validate_with(&proofs[0], &p, f.as_ref()).is_err());
    for path in [&old, &recent, &personal, &protected, &repo] {
        assert!(path.exists());
    }
    f.file("local/arbitrary/old", 90000);
    assert!(scan(f.clone(), "local/arbitrary", p).is_err());
}
#[test]
fn one_child_and_per_target_age() {
    let f = Arc::new(Fixture::new());
    let p = f.protection(None);
    for path in [
        "local/JetBrains/IDE/caches/old",
        "local/JetBrains/IDE/extra/caches/no",
        "local/JetBrains/caches/no",
        "local/JetBrains/IDE/settings/no",
        "local/JetBrains/IDE/caches/Sessions/no",
    ] {
        f.file(path, 90000);
    }
    let proofs = scan(f.clone(), "local/JetBrains", p.clone()).unwrap();
    assert_eq!(proofs.len(), 1);
    validate_with(&proofs[0], &p, f.as_ref()).unwrap();
    assert!(
        plan_proof(proofs[0].clone())
            .unwrap()
            .rule
            .provenance
            .source
            .ends_with("rules/win32/apps.json")
    );
    f.file("roaming/Claude/logs/week-old", 8 * 86400);
    f.file("roaming/Claude/logs/day-old", 86400);
    let proofs = scan(f.clone(), "roaming/Claude/logs", p.clone()).unwrap();
    assert_eq!(proofs.len(), 1);
    validate_with(&proofs[0], &p, f.as_ref()).unwrap();
    f.file("low/Sun/Java/Deployment/cache/old", 90000);
    assert_eq!(
        scan(f.clone(), "low/Sun/Java/Deployment/cache", p)
            .unwrap()
            .len(),
        1
    );
}
#[test]
fn direct_file_allowlists_and_updater_pending_revalidation() {
    let f = Arc::new(Fixture::new());
    let p = f.protection(None);
    for name in [
        "thumbcache_32.db",
        "iconcache_16.db",
        "settings.dat",
        "thumbcache_32.db.bak",
        "nested/thumbcache_16.db",
    ] {
        f.file(&format!("local/Microsoft/Windows/Explorer/{name}"), 90000);
    }
    let proofs = scan(f.clone(), "local/Microsoft/Windows/Explorer", p.clone()).unwrap();
    assert_eq!(proofs.len(), 2);
    for proof in proofs {
        validate_with(&proof, &p, f.as_ref()).unwrap();
    }
    for name in [
        "ok-updater/installer.exe",
        "ok-updater/current.blockmap",
        "ok-updater/nested/installer.exe",
        "other/installer.exe",
        "busy-updater/installer.exe",
        "recent-updater/installer.exe",
    ] {
        f.file(
            &format!("local/{name}"),
            if name.starts_with("recent") {
                86400
            } else {
                15 * 86400
            },
        );
    }
    std::fs::create_dir_all(f.0.join("local/busy-updater/pending")).unwrap();
    let proofs = scan(f.clone(), "local", p.clone()).unwrap();
    assert_eq!(proofs.len(), 2);
    for proof in &proofs {
        validate_with(proof, &p, f.as_ref()).unwrap();
    }
    std::fs::create_dir_all(f.0.join("local/ok-updater/pending")).unwrap();
    for proof in &proofs {
        assert!(validate_with(proof, &p, f.as_ref()).is_err());
    }
}
#[test]
fn bounded_anchors_prune_unrelated_and_personal_trees() {
    let f = Arc::new(Fixture::new());
    let p = f.protection(None);
    for path in [
        "local/App/EBWebView/Default/Cache/Cache_Data/yes",
        "local/Microsoft/App/EBWebView/Default/GPUCache/yes",
        "local/App/EBWebView/Default/IndexedDB/Cache/no",
        "local/App/EBWebView/Default/History",
        "local/Unrelated/deep/EBWebView/Cache/no",
        "local/NoAnchor/Cache/no",
    ] {
        f.file(path, 90000);
    }
    let t = bind(
        compiled_targets()
            .iter()
            .find(|t| t.metadata.target_id == "webview2:0")
            .unwrap(),
        f.as_ref(),
    )
    .unwrap();
    assert!(!t.may_contain(&f.0.join("local/Unrelated/deep")));
    assert!(t.may_contain(&f.0.join("local/Microsoft/App")));
    let proofs = scan(f.clone(), "local", p.clone()).unwrap();
    assert_eq!(proofs.len(), 2);
    for proof in proofs {
        validate_with(&proof, &p, f.as_ref()).unwrap();
    }
    f.file("roaming/Claude/Partitions/preview/Cache/old", 90000);
    f.file(
        "roaming/Claude/Partitions/preview/Session Storage/Cache/no",
        90000,
    );
    f.file("roaming/Claude/Partitions/preview/Cache/recent", 0);
    f.file("roaming/Claude/Partitions/a/b/c/d/e/f/g/h/Cache/no", 90000);
    let proofs = scan(f.clone(), "roaming/Claude/Partitions", p.clone()).unwrap();
    assert_eq!(proofs.len(), 1);
    validate_with(&proofs[0], &p, f.as_ref()).unwrap();
}
#[test]
fn every_supported_declaration_has_a_bounded_positive_and_sibling_negative() {
    let f = Fixture::new();
    let p = f.protection(None);
    for target in compiled_targets() {
        if target.metadata.unsupported_reason.is_some() {
            assert!(bind(target, &f).is_err());
            continue;
        }
        let t = bind(target, &f).unwrap();
        let mut path = t.root.clone();
        for part in &t.pattern {
            path.push(part.replace('*', "fixture"));
        }
        if let Some(r) = &target.rule.recursive_match {
            if let Some(anchor) = r.anchor_paths.first() {
                for part in anchor.split('/') {
                    path.push(part.replace('*', "fixture"));
                }
            }
            path.push(&r.targets[0]);
            path.push("fixture.bin");
        } else if let Some(pattern) = target.rule.file_patterns.first() {
            path.push(pattern.replace('*', "fixture"));
        } else if !target.rule.single_file {
            path.push("fixture.bin");
        }
        assert!(
            t.matches(&path),
            "{} {}",
            target.metadata.catalog_id,
            target.metadata.target_id
        );
        assert!(t.may_contain(&path));
        assert!(!t.excluded(&path, &p));
        assert!(!t.matches(&f.0.join("unrelated/fixture.bin")));
    }
}
#[test]
fn gaming_native_file_scope() {
    let f = Arc::new(Fixture::new());
    let p = f.protection(None);
    f.file("local/Riot Games/Riot Client/Logs/old", 90000);
    f.file("local/Riot Games/Riot Client/Logs/saves/private", 90000);
    let proofs = scan(f.clone(), "local/Riot Games/Riot Client/Logs", p.clone()).unwrap();
    assert_eq!(proofs.len(), 1);
    validate_with(&proofs[0], &p, f.as_ref()).unwrap();
    assert!(
        plan_proof(proofs[0].clone())
            .unwrap()
            .rule
            .provenance
            .source
            .ends_with("rules/win32/gaming.json")
    );
}
#[test]
fn reparse_ancestor_swap_is_rejected() {
    let f = Arc::new(Fixture::new());
    let p = f.protection(None);
    f.file("local/NVIDIA/DXCache/old", 90000);
    let proofs = scan(f.clone(), "local/NVIDIA/DXCache", p.clone()).unwrap();
    std::fs::rename(f.0.join("local/NVIDIA/DXCache"), f.0.join("elsewhere")).unwrap();
    let link = f.0.join("local/NVIDIA/DXCache");
    junction::create(f.0.join("elsewhere"), &link).unwrap();
    assert!(validate_with(&proofs[0], &p, f.as_ref()).is_err());
    assert!(scan(f.clone(), "local/NVIDIA/DXCache", p).is_err());
    junction::delete(&link).unwrap();
    assert!(f.0.join("elsewhere/old").exists());
}
