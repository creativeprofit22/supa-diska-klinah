use cleanup_core::{
    CancellationToken, CatalogLimits, Entropy, EntryKind, FileSystem, FsError, FsErrorKind,
    Lifecycle, PreviewRecord, ProtectionInputs, ProtectionPolicy, RuleCatalog, ScanDiagnostic,
    ScanEngine, ScanLimits, ScanRequest, ScanSnapshot, load_catalog,
};
use serde::Serialize;
use std::{
    collections::HashMap,
    ffi::OsString,
    io::Cursor,
    os::windows::ffi::OsStringExt,
    path::{Component, PathBuf},
    ptr,
    sync::Arc,
};
use windows_sys::Win32::{
    System::Com::CoTaskMemFree,
    UI::Shell::{FOLDERID_Documents, FOLDERID_Windows, SHGetKnownFolderPath},
};

use super::WindowsFileSystem;

const TEMPORARY_CACHE_RULE: &str = r#"{
  "schemaVersion": 1,
  "rules": [{
    "id": "temporary-caches",
    "ruleVersion": 1,
    "lifecycle": "stable",
    "risk": "safe",
    "provenance": { "source": "built-in temporary preview", "verifiedAt": "2026-08-30" },
    "defaultSelected": false,
    "scanner": "direct",
    "roots": [{ "binding": "temp", "suffix": "" }],
    "targets": ["cache", "tmp"],
    "targetType": "directory",
    "rootDepth": 4
  }]
}"#;

const PROJECT_ARTIFACT_RULES: &[u8] = include_bytes!("project-artifact-rules.json");

const MAX_PROJECT_ROOT_BYTES: usize = 4_096;

const PREVIEW_LIMITS: ScanLimits = ScanLimits {
    max_workers: 2,
    max_visited_entries: 50_000,
    max_candidates: 1_000,
    max_diagnostics: 100,
    max_measurement_entries: 250_000,
};

pub(crate) const PROJECT_DISCOVERY_LIMITS: ScanLimits = ScanLimits {
    max_workers: 2,
    max_visited_entries: 100_000,
    max_candidates: 2_000,
    max_diagnostics: 100,
    max_measurement_entries: 250_000,
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupPreview {
    pub scan_id: String,
    pub records: Vec<PreviewRecord>,
    pub diagnostics: Vec<ScanDiagnostic>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectArtifactDiscovery {
    pub records: Vec<PreviewRecord>,
    pub diagnostics: Vec<ScanDiagnostic>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CleanupPreviewError {
    TemporaryRootUnavailable,
    ProjectRootInvalid,
    ProtectionUnavailable,
    ProtectionInvalid,
    CatalogInvalid,
    ScanFailed,
}

pub fn preview_temporary_caches() -> Result<CleanupPreview, CleanupPreviewError> {
    scan_temporary_caches().map(|scan| scan.preview)
}

pub fn discover_project_artifacts(
    root: &str,
) -> Result<ProjectArtifactDiscovery, CleanupPreviewError> {
    let file_system: Arc<dyn FileSystem> = Arc::new(WindowsFileSystem);
    let protection = current_protection()?;
    let root = validate_project_root(file_system.as_ref(), &protection, root)?;
    discover_project_artifacts_with_context(file_system, root, protection, &WindowsEntropy)
}

pub(crate) struct PrivateCleanupScan {
    pub preview: CleanupPreview,
    pub snapshot: ScanSnapshot,
    pub protection: ProtectionPolicy,
}

pub(crate) fn current_protection() -> Result<ProtectionPolicy, CleanupPreviewError> {
    let file_system = WindowsFileSystem;
    let inputs = ProtectionInputs::new(
        vec![known_folder(&FOLDERID_Windows)?],
        vec![known_folder(&FOLDERID_Documents)?],
        vec![executable_directory()?],
    )
    .map_err(|_| CleanupPreviewError::ProtectionInvalid)?;
    ProtectionPolicy::compile(&file_system, inputs)
        .map_err(|_| CleanupPreviewError::ProtectionInvalid)
}

pub(crate) fn temporary_rule() -> Result<cleanup_core::CleanupRule, CleanupPreviewError> {
    load_catalog(
        Cursor::new(TEMPORARY_CACHE_RULE.as_bytes()),
        CatalogLimits::default(),
    )
    .map_err(|_| CleanupPreviewError::CatalogInvalid)?
    .rules()
    .first()
    .cloned()
    .ok_or(CleanupPreviewError::CatalogInvalid)
}

pub(crate) fn temporary_root() -> Result<PathBuf, CleanupPreviewError> {
    WindowsFileSystem
        .canonicalize(&std::env::temp_dir())
        .map_err(|_| CleanupPreviewError::TemporaryRootUnavailable)
}

pub(crate) fn scan_temporary_caches() -> Result<PrivateCleanupScan, CleanupPreviewError> {
    let file_system = Arc::new(WindowsFileSystem);
    let temporary_root = temporary_root()?;
    let protection = current_protection()?;
    scan_with_context(file_system, temporary_root, protection, &WindowsEntropy)
}

pub(crate) fn validate_project_root(
    file_system: &dyn FileSystem,
    protection: &ProtectionPolicy,
    root: &str,
) -> Result<PathBuf, CleanupPreviewError> {
    let root = root.trim();
    if root.is_empty() || root.len() > MAX_PROJECT_ROOT_BYTES || root.chars().any(char::is_control)
    {
        return Err(CleanupPreviewError::ProjectRootInvalid);
    }
    let path = PathBuf::from(root);
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(CleanupPreviewError::ProjectRootInvalid);
    }
    let metadata = file_system
        .metadata_no_follow(&path)
        .map_err(|_| CleanupPreviewError::ProjectRootInvalid)?;
    let canonical = file_system
        .canonicalize(&path)
        .map_err(|_| CleanupPreviewError::ProjectRootInvalid)?;
    let canonical_metadata = file_system
        .metadata_no_follow(&canonical)
        .map_err(|_| CleanupPreviewError::ProjectRootInvalid)?;
    let canonical_display = canonical.to_string_lossy();
    if canonical_display.len() > MAX_PROJECT_ROOT_BYTES
        || canonical_display.chars().any(char::is_control)
        || metadata.kind != EntryKind::Directory
        || metadata.identity.is_none()
        || canonical_metadata.kind != EntryKind::Directory
        || canonical_metadata.identity != metadata.identity
        || canonical.parent().is_none()
        || protection.is_protected(&canonical)
    {
        return Err(CleanupPreviewError::ProjectRootInvalid);
    }
    Ok(canonical)
}

pub(crate) fn discover_project_artifacts_with_context(
    file_system: Arc<dyn FileSystem>,
    root: PathBuf,
    protection: ProtectionPolicy,
    entropy: &dyn Entropy,
) -> Result<ProjectArtifactDiscovery, CleanupPreviewError> {
    discover_project_artifacts_with_limits(
        file_system,
        root,
        protection,
        entropy,
        PROJECT_DISCOVERY_LIMITS,
    )
}

pub(crate) fn discover_project_artifacts_with_limits(
    file_system: Arc<dyn FileSystem>,
    root: PathBuf,
    protection: ProtectionPolicy,
    entropy: &dyn Entropy,
    limits: ScanLimits,
) -> Result<ProjectArtifactDiscovery, CleanupPreviewError> {
    let catalog = load_catalog(
        Cursor::new(PROJECT_ARTIFACT_RULES),
        CatalogLimits::default(),
    )
    .map_err(|_| CleanupPreviewError::CatalogInvalid)?;
    discover_project_artifacts_from_catalog(
        file_system,
        root,
        protection,
        entropy,
        &catalog,
        limits,
    )
}

fn discover_project_artifacts_from_catalog(
    file_system: Arc<dyn FileSystem>,
    root: PathBuf,
    protection: ProtectionPolicy,
    entropy: &dyn Entropy,
    catalog: &RuleCatalog,
    limits: ScanLimits,
) -> Result<ProjectArtifactDiscovery, CleanupPreviewError> {
    let roots = HashMap::from([("projectRoot".to_owned(), root)]);
    let selected: Vec<_> = catalog
        .rules()
        .iter()
        .filter(|rule| matches!(rule.lifecycle, Lifecycle::Verified | Lifecycle::Stable))
        .map(|rule| rule.id.clone())
        .collect();
    let result = ScanEngine::new(file_system)
        .scan(ScanRequest {
            catalog,
            selected_rule_ids: &selected,
            root_bindings: &roots,
            protection: &protection,
            limits,
            cancellation: CancellationToken::new(),
            entropy,
            progress: &|_| {},
        })
        .map_err(|_| CleanupPreviewError::ScanFailed)?;

    Ok(ProjectArtifactDiscovery {
        records: result.snapshot.records().to_vec(),
        diagnostics: result.diagnostics,
    })
}

#[cfg(test)]
fn preview_with_context(
    file_system: Arc<WindowsFileSystem>,
    temporary_root: PathBuf,
    protection_inputs: ProtectionInputs,
    entropy: &dyn Entropy,
) -> Result<CleanupPreview, CleanupPreviewError> {
    let protection = ProtectionPolicy::compile(file_system.as_ref(), protection_inputs)
        .map_err(|_| CleanupPreviewError::ProtectionInvalid)?;
    scan_with_context(file_system, temporary_root, protection, entropy).map(|scan| scan.preview)
}

fn scan_with_context(
    file_system: Arc<WindowsFileSystem>,
    temporary_root: PathBuf,
    protection: ProtectionPolicy,
    entropy: &dyn Entropy,
) -> Result<PrivateCleanupScan, CleanupPreviewError> {
    let catalog = load_catalog(
        Cursor::new(TEMPORARY_CACHE_RULE.as_bytes()),
        CatalogLimits::default(),
    )
    .map_err(|_| CleanupPreviewError::CatalogInvalid)?;
    let roots = HashMap::from([("temp".to_owned(), temporary_root)]);
    let selected = vec!["temporary-caches".to_owned()];
    let result = ScanEngine::new(file_system)
        .scan(ScanRequest {
            catalog: &catalog,
            selected_rule_ids: &selected,
            root_bindings: &roots,
            protection: &protection,
            limits: PREVIEW_LIMITS,
            cancellation: CancellationToken::new(),
            entropy,
            progress: &|_| {},
        })
        .map_err(|_| CleanupPreviewError::ScanFailed)?;

    Ok(PrivateCleanupScan {
        preview: CleanupPreview {
            scan_id: result.snapshot.scan_id().to_owned(),
            records: result.snapshot.records().to_vec(),
            diagnostics: result.diagnostics,
        },
        snapshot: result.snapshot,
        protection,
    })
}

fn known_folder(id: &windows_sys::core::GUID) -> Result<PathBuf, CleanupPreviewError> {
    let mut raw = ptr::null_mut();
    // SAFETY: Windows owns the returned NUL-terminated allocation until CoTaskMemFree below.
    let result = unsafe { SHGetKnownFolderPath(id, 0, ptr::null_mut(), &mut raw) };
    if result < 0 || raw.is_null() {
        return Err(CleanupPreviewError::ProtectionUnavailable);
    }

    let mut length = 0;
    // SAFETY: A successful SHGetKnownFolderPath call returns a valid NUL-terminated UTF-16 string.
    while unsafe { *raw.add(length) } != 0 {
        length += 1;
    }
    // SAFETY: `length` was established by scanning the valid allocation to its terminator.
    let path = PathBuf::from(OsString::from_wide(unsafe {
        std::slice::from_raw_parts(raw, length)
    }));
    // SAFETY: SHGetKnownFolderPath documents CoTaskMemFree as the matching deallocator.
    unsafe { CoTaskMemFree(raw.cast()) };
    Ok(path)
}

fn executable_directory() -> Result<PathBuf, CleanupPreviewError> {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(PathBuf::from))
        .ok_or(CleanupPreviewError::ProtectionUnavailable)
}

struct WindowsEntropy;

impl Entropy for WindowsEntropy {
    fn fill(&self, bytes: &mut [u8]) -> Result<(), FsError> {
        getrandom::fill(bytes)
            .map_err(|_| FsError::new(FsErrorKind::Other, "system entropy unavailable"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::HashSet,
        env, fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = env::temp_dir().join(format!(
                "supa-diska-preview-{}-{}",
                std::process::id(),
                getrandom::u64().unwrap()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn directory(&self, name: &str) -> PathBuf {
            let path = self.0.join(name);
            fs::create_dir(&path).unwrap();
            path
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[derive(Default)]
    struct TestEntropy(AtomicU64);

    impl Entropy for TestEntropy {
        fn fill(&self, bytes: &mut [u8]) -> Result<(), FsError> {
            let value = self.0.fetch_add(1, Ordering::Relaxed).to_le_bytes();
            for (index, byte) in bytes.iter_mut().enumerate() {
                *byte = value[index % value.len()];
            }
            Ok(())
        }
    }

    fn test_context(temp: &TestDirectory) -> (PathBuf, ProtectionInputs) {
        let scan = temp.directory("scan");
        let system = temp.directory("system");
        let documents = temp.directory("documents");
        let executable = temp.directory("executable");
        (
            scan,
            ProtectionInputs::new(vec![system], vec![documents], vec![executable]).unwrap(),
        )
    }

    #[test]
    fn eligible_cache_directory_is_returned_through_real_adapter() {
        let temp = TestDirectory::new();
        let (scan, protection) = test_context(&temp);
        let cache = scan.join("cache");
        fs::create_dir(&cache).unwrap();
        fs::write(cache.join("entry.bin"), b"preview").unwrap();

        let preview = preview_with_context(
            Arc::new(WindowsFileSystem),
            scan,
            protection,
            &TestEntropy::default(),
        )
        .unwrap();

        assert_eq!(preview.records.len(), 1);
        assert_eq!(preview.records[0].rule_id, "temporary-caches");
        assert!(preview.records[0].display_path.ends_with("cache"));
        assert_eq!(preview.records[0].bytes, 7);
    }

    #[test]
    fn empty_temporary_root_returns_no_records() {
        let temp = TestDirectory::new();
        let (scan, protection) = test_context(&temp);

        let preview = preview_with_context(
            Arc::new(WindowsFileSystem),
            scan,
            protection,
            &TestEntropy::default(),
        )
        .unwrap();

        assert!(preview.records.is_empty());
    }

    #[test]
    fn protected_temporary_root_fails_closed() {
        let temp = TestDirectory::new();
        let scan = temp.directory("scan");
        let documents = temp.directory("documents");
        let executable = temp.directory("executable");
        let protection =
            ProtectionInputs::new(vec![scan.clone()], vec![documents], vec![executable]).unwrap();

        let error = preview_with_context(
            Arc::new(WindowsFileSystem),
            scan,
            protection,
            &TestEntropy::default(),
        )
        .unwrap_err();

        assert_eq!(error, CleanupPreviewError::ScanFailed);
    }

    #[test]
    fn project_artifacts_require_markers_and_return_intelligence_without_scan_id() {
        let temp = TestDirectory::new();
        let (scan, protection_inputs) = test_context(&temp);
        let app = scan.join("app");
        fs::create_dir(&app).unwrap();
        fs::write(app.join("package.json"), b"{}").unwrap();
        fs::create_dir(app.join("node_modules")).unwrap();
        fs::write(app.join("node_modules/dependency.bin"), b"dependencies").unwrap();
        let scratch = scan.join("scratch");
        fs::create_dir(&scratch).unwrap();
        fs::create_dir(scratch.join("node_modules")).unwrap();
        let file_system: Arc<dyn FileSystem> = Arc::new(WindowsFileSystem);
        let protection =
            ProtectionPolicy::compile(file_system.as_ref(), protection_inputs).unwrap();

        let discovery = discover_project_artifacts_with_context(
            file_system,
            scan,
            protection,
            &TestEntropy::default(),
        )
        .unwrap();

        assert_eq!(discovery.records.len(), 1);
        let record = &discovery.records[0];
        assert_eq!(record.project_name.as_deref(), Some("app"));
        assert!(record.project_path.as_deref().unwrap().ends_with("app"));
        assert_eq!(record.bytes, 12);
        assert_eq!(record.default_selected, Some(false));
        assert_eq!(
            serde_json::to_value(&discovery).unwrap()["records"][0]["artifact"],
            serde_json::json!({
                "ecosystem": "nodeJs",
                "artifactType": "installedDependencies",
                "confidence": "high",
                "recoverability": "rebuildable",
                "rebuildConsequence": "networkDownloadRequired"
            })
        );
        assert!(
            serde_json::to_value(discovery)
                .unwrap()
                .get("scanId")
                .is_none()
        );
    }

    #[test]
    fn every_production_catalog_rule_discovers_its_documented_matcher_shape() {
        let temp = TestDirectory::new();
        let (scan, protection_inputs) = test_context(&temp);
        let catalog = load_catalog(
            Cursor::new(PROJECT_ARTIFACT_RULES),
            CatalogLimits::default(),
        )
        .unwrap();
        let expected: HashSet<_> = catalog
            .rules()
            .iter()
            .filter(|rule| matches!(rule.lifecycle, Lifecycle::Verified | Lifecycle::Stable))
            .map(|rule| rule.id.clone())
            .collect();
        for rule in catalog.rules() {
            let project = scan.join(&rule.id);
            fs::create_dir(&project).unwrap();
            for marker in &rule.markers.all {
                fs::write(project.join(marker), b"marker").unwrap();
            }
            if let Some(marker) = rule.markers.any.first() {
                fs::write(project.join(marker), b"marker").unwrap();
            }
            if let Some(suffix) = rule.markers.any_suffix.first() {
                fs::write(project.join(format!("fixture{suffix}")), b"marker").unwrap();
            }
            let target = rule
                .targets
                .first()
                .cloned()
                .or_else(|| {
                    rule.target_prefixes
                        .first()
                        .map(|prefix| format!("{prefix}fixture"))
                })
                .or_else(|| {
                    rule.target_suffixes
                        .first()
                        .map(|suffix| format!("fixture{suffix}"))
                })
                .unwrap();
            fs::create_dir(project.join(target)).unwrap();
        }
        let file_system: Arc<dyn FileSystem> = Arc::new(WindowsFileSystem);
        let protection =
            ProtectionPolicy::compile(file_system.as_ref(), protection_inputs).unwrap();

        let discovery = discover_project_artifacts_with_context(
            file_system,
            scan,
            protection,
            &TestEntropy::default(),
        )
        .unwrap();
        let actual: HashSet<_> = discovery
            .records
            .iter()
            .map(|record| record.rule_id.clone())
            .collect();
        assert_eq!(actual, expected, "{:?}", discovery.diagnostics);
        assert!(
            discovery
                .records
                .iter()
                .all(|record| record.default_selected == Some(false))
        );
    }

    #[test]
    fn candidate_project_rules_are_never_selected_by_the_adapter() {
        let temp = TestDirectory::new();
        let (scan, protection_inputs) = test_context(&temp);
        fs::write(scan.join("package.json"), b"{}").unwrap();
        fs::create_dir(scan.join("candidate-output")).unwrap();
        let file_system: Arc<dyn FileSystem> = Arc::new(WindowsFileSystem);
        let protection =
            ProtectionPolicy::compile(file_system.as_ref(), protection_inputs).unwrap();
        let catalog = load_catalog(
            Cursor::new(
                br#"{"schemaVersion":1,"rules":[{"id":"candidate","ruleVersion":1,"lifecycle":"candidate","risk":"recoverable","provenance":{"source":"test","verifiedAt":"2026-09-03"},"defaultSelected":false,"artifact":{"ecosystem":"nodeJs","artifactType":"buildOutput","confidence":"medium","recoverability":"rebuildable","rebuildConsequence":"localRebuild"},"scanner":"projectArtifacts","roots":[{"binding":"projectRoot","suffix":""}],"markers":{"all":["package.json"]},"targets":["candidate-output"],"targetType":"directory","rootDepth":0,"projectDepth":1,"targetDepth":0}] }"#,
            ),
            CatalogLimits::default(),
        )
        .unwrap();

        let discovery = discover_project_artifacts_from_catalog(
            file_system,
            scan,
            protection,
            &TestEntropy::default(),
            &catalog,
            PROJECT_DISCOVERY_LIMITS,
        )
        .unwrap();
        assert!(discovery.records.is_empty());
    }

    #[test]
    fn project_artifacts_invalid_roots_are_rejected_before_discovery() {
        let temp = TestDirectory::new();
        let directory = temp.directory("directory");
        let file = temp.0.join("file.txt");
        fs::write(&file, b"file").unwrap();
        let file_system = WindowsFileSystem;
        let (_, protection_inputs) = test_context(&temp);
        let protection = ProtectionPolicy::compile(&file_system, protection_inputs).unwrap();

        for root in [
            "",
            "relative",
            r"C:\work\..\secret",
            file.to_str().unwrap(),
            temp.0.join("missing").to_str().unwrap(),
        ] {
            assert_eq!(
                validate_project_root(&file_system, &protection, root),
                Err(CleanupPreviewError::ProjectRootInvalid)
            );
        }
        let oversized = format!(r"C:\{}", "x".repeat(MAX_PROJECT_ROOT_BYTES));
        assert_eq!(
            validate_project_root(&file_system, &protection, &oversized),
            Err(CleanupPreviewError::ProjectRootInvalid)
        );
        assert_eq!(
            validate_project_root(&file_system, &protection, directory.to_str().unwrap()),
            file_system
                .canonicalize(&directory)
                .map_err(|_| CleanupPreviewError::ProjectRootInvalid)
        );
    }

    #[test]
    fn project_artifacts_protected_root_fails_closed() {
        let temp = TestDirectory::new();
        let scan = temp.directory("scan");
        fs::write(scan.join("package.json"), b"{}").unwrap();
        fs::create_dir(scan.join("node_modules")).unwrap();
        let documents = temp.directory("documents");
        let executable = temp.directory("executable");
        let file_system: Arc<dyn FileSystem> = Arc::new(WindowsFileSystem);
        let protection = ProtectionPolicy::compile(
            file_system.as_ref(),
            ProtectionInputs::new(vec![scan.clone()], vec![documents], vec![executable]).unwrap(),
        )
        .unwrap();

        let error = discover_project_artifacts_with_context(
            file_system,
            scan,
            protection,
            &TestEntropy::default(),
        )
        .unwrap_err();
        assert_eq!(error, CleanupPreviewError::ScanFailed);
    }

    #[test]
    fn response_serializes_only_public_preview_data() {
        let temp = TestDirectory::new();
        let (scan, protection) = test_context(&temp);
        fs::create_dir(scan.join("tmp")).unwrap();
        let preview = preview_with_context(
            Arc::new(WindowsFileSystem),
            scan,
            protection,
            &TestEntropy::default(),
        )
        .unwrap();

        let json = serde_json::to_value(preview).unwrap();

        assert!(json.get("scanId").is_some());
        assert!(json.get("records").is_some());
        assert!(json.get("diagnostics").is_some());
        assert!(json.get("resolved").is_none());
        assert!(!json.to_string().contains("resolved"));
    }

    #[test]
    fn adapter_errors_are_bounded_and_do_not_expose_paths() {
        let temp = TestDirectory::new();
        let scan = temp.directory("scan");
        let documents = temp.directory("documents");
        let executable = temp.directory("executable");
        let missing = temp.0.join("secret-missing-protection");
        let protection =
            ProtectionInputs::new(vec![missing], vec![documents], vec![executable]).unwrap();

        let error = preview_with_context(
            Arc::new(WindowsFileSystem),
            scan,
            protection,
            &TestEntropy::default(),
        )
        .unwrap_err();
        let rendered = format!("{error:?}");

        assert_eq!(error, CleanupPreviewError::ProtectionInvalid);
        assert_eq!(rendered, "ProtectionInvalid");
        assert!(!rendered.contains("secret-missing-protection"));
    }
}
