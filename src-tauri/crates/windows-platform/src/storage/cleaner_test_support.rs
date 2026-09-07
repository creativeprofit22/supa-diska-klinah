use super::known_folders::*;
use cleanup_core::storage::*;
use cleanup_core::{FileSystem, ProtectionInputs, ProtectionPolicy};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};
pub struct Fixture(pub PathBuf);
impl Fixture {
    pub fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("cleaner-review-{}", super::opaque_id().unwrap()));
        std::fs::create_dir_all(&path).unwrap();
        Self(crate::WindowsFileSystem.canonicalize(&path).unwrap())
    }
    pub fn file(&self, relative: &str, old: bool) -> PathBuf {
        let path = self.0.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"fixture only").unwrap();
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
    pub fn protection(&self, extra: Option<&Path>) -> ProtectionPolicy {
        let safe = self.0.join("protected");
        std::fs::create_dir_all(&safe).unwrap();
        let mut configured = vec![safe.clone()];
        if let Some(path) = extra {
            configured.push(path.to_path_buf());
        }
        ProtectionPolicy::compile(
            &crate::WindowsFileSystem,
            ProtectionInputs::new(vec![safe.clone()], vec![safe], configured).unwrap(),
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
        });
        std::fs::create_dir_all(&path).unwrap();
        validate_binding(path)
    }
}
pub fn wait(service: &super::scans::StorageService, id: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let (status, error) = service.status(id).unwrap();
        if matches!(
            status.phase,
            StoragePhase::Complete | StoragePhase::Failed | StoragePhase::Cancelled
        ) {
            assert!(error.is_none(), "{error:?}");
            // Last worker progress may precede publication. page() below retries that tiny window.
            return;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
}
pub fn selection(
    service: &super::scans::StorageService,
    id: &str,
    module: StorageModule,
) -> StorageSelection {
    wait(service, id);
    let request = PageRequest {
        snapshot_id: id.into(),
        module,
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
    let candidate_ids = page
        .records
        .iter()
        .filter_map(|record| match record {
            StorageRecord::File(file) => match &file.eligibility {
                CandidateEligibility::Eligible { candidate_id } => Some(candidate_id.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect();
    StorageSelection {
        snapshot_id: id.into(),
        module,
        candidate_ids,
    }
}
