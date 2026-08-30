use cleanup_core::{
    CancellationToken, CatalogLimits, Entropy, FileSystem, FsError, FsErrorKind, PreviewRecord,
    ProtectionInputs, ProtectionPolicy, ScanDiagnostic, ScanEngine, ScanLimits, ScanRequest,
    ScanSnapshot, load_catalog,
};
use serde::Serialize;
use std::{
    collections::HashMap, ffi::OsString, io::Cursor, os::windows::ffi::OsStringExt, path::PathBuf,
    ptr, sync::Arc,
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

const PREVIEW_LIMITS: ScanLimits = ScanLimits {
    max_workers: 2,
    max_visited_entries: 50_000,
    max_candidates: 1_000,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CleanupPreviewError {
    TemporaryRootUnavailable,
    ProtectionUnavailable,
    ProtectionInvalid,
    CatalogInvalid,
    ScanFailed,
}

pub fn preview_temporary_caches() -> Result<CleanupPreview, CleanupPreviewError> {
    scan_temporary_caches().map(|scan| scan.preview)
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
