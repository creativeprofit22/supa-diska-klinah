use super::*;
use crate::storage::scans::StorageService;
use cleanup_core::{PathSemantics, ProtectionInputs};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "browser-fixture-{}",
            crate::storage::opaque_id().unwrap()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(crate::WindowsFileSystem.canonicalize(&path).unwrap())
    }
    fn file(&self, relative: &str, old: bool) -> PathBuf {
        let path = self.0.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"disposable fixture").unwrap();
        if old {
            std::fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(SystemTime::now() - Duration::from_secs(86400))
                .unwrap();
        }
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
            KnownFolder::LocalLow => "local-low",
            KnownFolder::Roaming => "roaming",
            KnownFolder::Profile => "home",
            KnownFolder::ProgramFiles | KnownFolder::ProgramFilesX86 | KnownFolder::ProgramData => {
                return Err(StorageError::UnsupportedScope);
            }
        });
        std::fs::create_dir_all(&path).unwrap();
        crate::storage::known_folders::validate_binding(path)
    }
}
struct Probe(BrowserActivity);
impl ActivityProbe for Probe {
    fn activity(&self) -> BrowserActivity {
        self.0
    }
}
fn finish(service: &StorageService, id: &str) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let (status, error) = service.status(id).unwrap();
        if error.is_some() || status.phase == StoragePhase::Failed {
            return false;
        }
        if status.phase == StoragePhase::Complete {
            return true;
        }
        assert!(Instant::now() < deadline, "browser fixture timed out");
        std::thread::yield_now();
    }
}
fn scan(
    fixture: Arc<Fixture>,
    root: &str,
    extra: Option<&Path>,
    opt_in: bool,
    activity: BrowserActivity,
) -> (StorageService, String) {
    let service = StorageService::new();
    let protection = fixture.protection(extra);
    let root = fixture.0.join(root);
    let root_id = service.authorize_root(&root, &protection).unwrap();
    let id = service
        .start_with(
            StorageModule::Browser,
            Some(&root_id),
            StorageLimits::default(),
            Some(protection.clone()),
            move |ctx| {
                discover_with(ctx, &protection, opt_in, fixture.as_ref(), &Probe(activity))
                    .map_err(Into::into)
            },
        )
        .unwrap();
    (service, id)
}
#[test]
fn record_limit_preserves_partial_browser_results() {
    let fixture = Arc::new(Fixture::new());
    let base = "local/Google/Chrome/User Data";
    for index in 0..9 {
        fixture.file(&format!("{base}/Default/Cache/Cache_Data/{index}"), true);
    }
    let worker_fixture = fixture.clone();
    let service = StorageService::new();
    let protection = fixture.protection(None);
    let root = service
        .authorize_root(&fixture.0.join(base), &protection)
        .unwrap();
    let id = service
        .start_with(
            StorageModule::Browser,
            Some(&root),
            StorageLimits {
                retained_records: 8,
                ..StorageLimits::default()
            },
            Some(protection.clone()),
            move |ctx| {
                discover_with(
                    ctx,
                    &protection,
                    false,
                    worker_fixture.as_ref(),
                    &Probe(BrowserActivity::Inactive),
                )
                .map_err(Into::into)
            },
        )
        .unwrap();
    assert_eq!(evidence(&service, &id).len(), 8);
    let (status, error) = service.status(&id).unwrap();
    assert!(error.is_none());
    assert!(
        status
            .completeness
            .reasons
            .contains(&PartialReason::RecordLimit)
    );
    let page = service
        .page(&PageRequest {
            snapshot_id: id,
            module: StorageModule::Browser,
            collection: PageCollection::Files,
            parent_id: None,
            cursor: None,
            page_size: 100,
        })
        .unwrap();
    assert_eq!(page.records.len(), 8);
    assert!(
        page.completeness
            .reasons
            .contains(&PartialReason::RecordLimit)
    );
}

fn evidence(service: &StorageService, id: &str) -> Vec<StorageEvidence> {
    assert!(finish(service, id));
    let request = PageRequest {
        snapshot_id: id.into(),
        module: StorageModule::Browser,
        collection: PageCollection::Files,
        parent_id: None,
        cursor: None,
        page_size: 100,
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    let page = loop {
        if let Ok(page) = service.page(&request) {
            break page;
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
        return vec![];
    }
    service
        .resolve_selection(&StorageSelection {
            snapshot_id: id.into(),
            module: StorageModule::Browser,
            candidate_ids,
        })
        .unwrap()
}
#[test]
fn native_discovery_uses_canonical_roots_and_only_old_unprotected_cache_files() {
    let fixture = Arc::new(Fixture::new());
    let base = "local/Google/Chrome/User Data";
    let old = fixture.file(&format!("{base}/Default/Cache/Cache_Data/old"), true);
    let recent = fixture.file(&format!("{base}/Default/Cache/Cache_Data/recent"), false);
    let personal = fixture.file(&format!("{base}/Default/Cookies"), true);
    let nested = fixture.file(
        &format!("{base}/Default/Cache/Cache_Data/IndexedDB/personal"),
        true,
    );
    let protected = fixture.file(
        &format!("{base}/Default/Cache/Cache_Data/protected/private"),
        true,
    );
    let worker = fixture.file(
        &format!("{base}/Default/Service Worker/CacheStorage/offline"),
        true,
    );
    // Old directory timestamps cannot make the recent descendant eligible.
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_WRITE_ATTRIBUTES,
    };
    std::fs::File::options()
        .access_mode(FILE_WRITE_ATTRIBUTES)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(old.parent().unwrap())
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(86400))
        .unwrap();
    let (service, id) = scan(
        fixture.clone(),
        base,
        protected.parent(),
        false,
        BrowserActivity::Inactive,
    );
    let proofs = evidence(&service, &id);
    assert_eq!(proofs.len(), 1);
    assert_eq!(proofs[0].entry().canonical_path, old);
    validate_with(
        &proofs[0],
        fixture.as_ref(),
        &Probe(BrowserActivity::Inactive),
    )
    .unwrap();
    for p in [&old, &recent, &personal, &nested, &protected, &worker] {
        assert!(p.exists());
    }
    let (service, id) = scan(
        fixture.clone(),
        base,
        protected.parent(),
        true,
        BrowserActivity::Inactive,
    );
    let proofs = evidence(&service, &id);
    assert_eq!(proofs.len(), 2);
    assert!(proofs.iter().any(|p| matches!(
        p,
        StorageEvidence::BrowserCache {
            service_worker: true,
            service_worker_opt_in: true,
            ..
        }
    )));
    for proof in &proofs {
        validate_with(proof, fixture.as_ref(), &Probe(BrowserActivity::Inactive)).unwrap();
        assert!(validate_with(proof, fixture.as_ref(), &Probe(BrowserActivity::Active)).is_err());
        assert!(validate_with(proof, fixture.as_ref(), &Probe(BrowserActivity::Unknown)).is_err());
        let mut forged = proof.clone();
        if let StorageEvidence::BrowserCache { catalog, .. } = &mut forged {
            catalog.revision = "other".into();
        }
        assert!(
            validate_with(&forged, fixture.as_ref(), &Probe(BrowserActivity::Inactive)).is_err()
        );
        let mut forged = proof.clone();
        if let StorageEvidence::BrowserCache { profile, .. } = &mut forged {
            profile.identity.file ^= 1;
        }
        assert!(
            validate_with(&forged, fixture.as_ref(), &Probe(BrowserActivity::Inactive)).is_err()
        );
        if let StorageEvidence::BrowserCache {
            service_worker: true,
            ..
        } = proof
        {
            let mut forged = proof.clone();
            if let StorageEvidence::BrowserCache {
                service_worker_opt_in,
                ..
            } = &mut forged
            {
                *service_worker_opt_in = false;
            }
            assert!(
                validate_with(&forged, fixture.as_ref(), &Probe(BrowserActivity::Inactive))
                    .is_err()
            );
        }
    }
}
#[test]
fn native_opera_shared_profiles_and_firefox_forks() {
    let fixture = Arc::new(Fixture::new());
    for (base, paths) in [
        (
            "roaming/Opera Software/Opera Stable",
            vec!["Cache/Cache_Data/a", "Code Cache/b", "ShaderCache/c"],
        ),
        (
            "local/Google/Chrome/User Data",
            vec![
                "Default/GPUCache/a",
                "Profile 2/Code Cache/b",
                "component_crx_cache/c",
            ],
        ),
        (
            "local/Mozilla/Firefox/Profiles",
            vec!["abc.default/cache2/entries/a"],
        ),
        (
            "local/Zen/Profiles",
            vec!["abc.default/cache2/entries/a", "abc.default/cache2/index"],
        ),
    ] {
        for relative in &paths {
            fixture.file(&format!("{base}/{relative}"), true);
        }
        if base.ends_with("/Profiles") {
            std::fs::create_dir_all(
                fixture
                    .0
                    .join(base.replacen("local/", "roaming/", 1))
                    .join("abc.default"),
            )
            .unwrap();
        }
        fixture.file(&format!("{base}/personal/sessionstore.jsonlz4"), true);
        let (service, id) = scan(
            fixture.clone(),
            base,
            None,
            false,
            BrowserActivity::Inactive,
        );
        let proofs = evidence(&service, &id);
        assert_eq!(proofs.len(), paths.len(), "{base}");
        for proof in &proofs {
            validate_with(proof, fixture.as_ref(), &Probe(BrowserActivity::Inactive)).unwrap();
        }
    }
}
#[test]
fn unknown_activity_and_arbitrary_roots_fail_closed() {
    let fixture = Arc::new(Fixture::new());
    for base in [
        "local/Google/Chrome/User Data",
        "local/attacker/Chrome/User Data",
    ] {
        fixture.file(&format!("{base}/Default/GPUCache/a"), true);
        for state in [BrowserActivity::Unknown, BrowserActivity::Active] {
            let (service, id) = scan(fixture.clone(), base, None, false, state);
            assert!(!finish(&service, &id));
        }
    }
    let (service, id) = scan(
        fixture.clone(),
        "local/attacker/Chrome/User Data",
        None,
        false,
        BrowserActivity::Inactive,
    );
    assert!(!finish(&service, &id));
}
#[test]
fn matcher_accepts_verified_disk_prefix_only_and_rejects_personal_descendants() {
    let c = catalog();
    let layout = Layout {
        id: "chrome".into(),
        base: "C:/Fixture/Chrome/User Data".into(),
        firefox: false,
        direct: false,
        profile_base: None,
    };
    for path in [
        r"\\?\C:\Fixture\Chrome\User Data\Default\GPUCache\a",
        r"c:\fixture\chrome\user data\default\gpucache\a",
    ] {
        assert!(
            match_cache(&layout, Path::new(path), &c).is_some(),
            "{path}"
        );
    }
    for path in [
        r"\\?\GLOBALROOT\C:\Fixture\Chrome\User Data\Default\GPUCache\a",
        r"C:\Fixture\Chrome\User Data2\Default\GPUCache\a",
        r"C:\Fixture\Chrome\User Data\Default\GPUCache\Sessions\a",
    ] {
        assert!(match_cache(&layout, Path::new(path), &c).is_none());
    }
    assert!(
        PathSemantics::CaseInsensitive
            .equivalent(Path::new(r"C:\Fixture"), Path::new(r"\\?\C:\Fixture"))
    );
}
