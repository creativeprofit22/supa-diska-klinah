//! Exact pinned browser layouts; file-only candidates, never profile/tree deletion.
use super::{
    known_folders::{KnownFolder, KnownFolderResolver, NativeKnownFolders},
    scans::ScanContext,
};
use cleanup_core::storage::*;
use cleanup_core::{EntryKind, FileSystem, ProtectionPolicy};
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const REVISION: &str = "db09e051d0615121e659db187e3799438acbc9e6";
pub const SERVICE_WORKER_DISCLOSURE: &str =
    "Service-worker cache cleanup may remove offline website content. Explicit opt-in is required.";
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Catalog {
    revision: String,
    source: String,
    minimum_age_seconds: u64,
    risk: String,
    lifecycle: cleanup_core::Lifecycle,
    risk_class: cleanup_core::Risk,
    unsupported: Vec<String>,
    service_worker_disclosure: String,
    profile: Vec<String>,
    shared: Vec<String>,
    chromium: Vec<[String; 3]>,
    firefox: Vec<[String; 3]>,
    exclusions: Vec<String>,
}
fn catalog() -> Catalog {
    serde_json::from_str(include_str!(
        "../../../cleanup-core/rules/browser-caches.json"
    ))
    .expect("compiled browser catalog")
}
/// Backend disclosure/provenance, not a claim of upstream implementation equivalence.
pub struct BrowserPolicy {
    pub source: String,
    pub revision: String,
    pub lifecycle: cleanup_core::Lifecycle,
    pub risk: cleanup_core::Risk,
    pub consequence: String,
    pub service_worker_disclosure: String,
    pub unsupported: Vec<String>,
}
pub fn policy() -> BrowserPolicy {
    let c = catalog();
    BrowserPolicy {
        source: c.source,
        revision: c.revision,
        lifecycle: c.lifecycle,
        risk: c.risk_class,
        consequence: c.risk,
        service_worker_disclosure: c.service_worker_disclosure,
        unsupported: c.unsupported,
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrowserActivity {
    Inactive,
    Active,
    Unknown,
}
pub trait ActivityProbe {
    fn activity(&self) -> BrowserActivity;
}
pub struct NativeBrowserActivity;
impl ActivityProbe for NativeBrowserActivity {
    fn activity(&self) -> BrowserActivity {
        use windows_sys::Win32::{
            Foundation::{CloseHandle, ERROR_NO_MORE_FILES, GetLastError, INVALID_HANDLE_VALUE},
            System::Diagnostics::ToolHelp::*,
        };
        // Conservative cross-brand check, including subprocesses. Enumeration failure is never idle.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return BrowserActivity::Unknown;
        }
        struct Handle(windows_sys::Win32::Foundation::HANDLE);
        impl Drop for Handle {
            fn drop(&mut self) {
                unsafe {
                    CloseHandle(self.0);
                }
            }
        }
        let _handle = Handle(snapshot);
        let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if unsafe { Process32FirstW(snapshot, &mut entry) } == 0 {
            return BrowserActivity::Unknown;
        }
        for _ in 0..100_000 {
            let end = entry
                .szExeFile
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(entry.szExeFile.len());
            let Ok(name) = String::from_utf16(&entry.szExeFile[..end]) else {
                return BrowserActivity::Unknown;
            };
            if [
                "chrome.exe",
                "msedge.exe",
                "brave.exe",
                "opera.exe",
                "vivaldi.exe",
                "arc.exe",
                "chromium.exe",
                "thorium.exe",
                "supermium.exe",
                "helium.exe",
                "cromite.exe",
                "catsxp.exe",
                "firefox.exe",
                "librewolf.exe",
                "waterfox.exe",
                "floorp.exe",
                "zen.exe",
            ]
            .contains(&name.to_ascii_lowercase().as_str())
            {
                return BrowserActivity::Active;
            }
            if unsafe { Process32NextW(snapshot, &mut entry) } == 0 {
                return if unsafe { GetLastError() } == ERROR_NO_MORE_FILES {
                    BrowserActivity::Inactive
                } else {
                    BrowserActivity::Unknown
                };
            }
        }
        BrowserActivity::Unknown
    }
}
struct Layout {
    id: String,
    base: PathBuf,
    firefox: bool,
    direct: bool,
    profile_base: Option<PathBuf>,
}
fn layouts(resolver: &dyn KnownFolderResolver) -> Result<Vec<Layout>, StorageError> {
    let c = catalog();
    let local = resolver.resolve(KnownFolder::Local)?;
    let roaming = resolver.resolve(KnownFolder::Roaming)?;
    let mut result = Vec::new();
    for [id, folder, relative] in c.chromium {
        let base = if folder == "local" { &local } else { &roaming };
        let direct = id == "opera" || id == "operaGX";
        result.push(Layout {
            id,
            base: base.join(relative),
            firefox: false,
            direct,
            profile_base: None,
        });
    }
    for [id, profile, cache] in c.firefox {
        // Roaming is profile-discovery evidence only; pinned Windows cleanup targets
        // the distinct Local cache root, never roaming personal profile contents.
        result.push(Layout {
            id,
            base: local.join(cache),
            firefox: true,
            direct: false,
            profile_base: Some(roaming.join(profile)),
        });
    }
    Ok(result)
}
fn profile_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("Default")
        || name
            .to_ascii_lowercase()
            .strip_prefix("profile ")
            .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
}
struct Match {
    target: String,
    profile: PathBuf,
    service_worker: bool,
}
fn match_cache(layout: &Layout, path: &Path, c: &Catalog) -> Option<Match> {
    let semantics = crate::WindowsFileSystem.semantics();
    if !cleanup_core::is_local_storage_path(path) || !semantics.contains(&layout.base, path) {
        return None;
    }
    // PathSemantics recognizes ONLY native verbatim disk prefixes, not arbitrary devices.
    // Keep original case when reconstructing the profile identity path.
    let normalized = cleanup_core::PathSemantics::CaseSensitive.key(path);
    let base = cleanup_core::PathSemantics::CaseSensitive.key(&layout.base);
    let parts: Vec<_> = normalized
        .split('/')
        .skip(base.split('/').count())
        .collect();
    if parts
        .iter()
        .any(|part| c.exclusions.iter().any(|e| e.eq_ignore_ascii_case(part)))
    {
        return None;
    }
    if layout.firefox {
        let target = if layout.id == "firefox" {
            "cache2/entries"
        } else {
            "cache2"
        };
        let suffix = parts.get(1..)?.join("/");
        if suffix
            .to_ascii_lowercase()
            .strip_prefix(target)
            .is_some_and(|s| s.starts_with('/') && s.len() > 1)
        {
            return Some(Match {
                target: target.into(),
                profile: layout.base.join(parts[0]),
                service_worker: false,
            });
        }
        return None;
    }
    let (profile, rest) = if !layout.direct && parts.first().is_some_and(|p| profile_name(p)) {
        (layout.base.join(parts[0]), parts[1..].join("/"))
    } else {
        (layout.base.clone(), parts.join("/"))
    };
    let dirs = if profile != layout.base || layout.direct {
        &c.profile
    } else {
        &c.shared
    };
    for dir in dirs.iter().chain(if profile == layout.base {
        c.shared.iter()
    } else {
        [].iter()
    }) {
        if rest
            .to_ascii_lowercase()
            .strip_prefix(&dir.to_ascii_lowercase())
            .is_some_and(|s| s.starts_with('/') && s.len() > 1)
        {
            return Some(Match {
                target: dir.clone(),
                profile,
                service_worker: dir == "Service Worker/CacheStorage",
            });
        }
    }
    None
}
// Prune whole personal/unrelated trees before read_dir. Only exact target ancestors
// and descendants are traversed; profile root metadata is not a whole-profile candidate.
fn reachable(layout: &Layout, path: &Path, c: &Catalog) -> bool {
    let semantics = crate::WindowsFileSystem.semantics();
    if !cleanup_core::is_local_storage_path(path) || !semantics.contains(&layout.base, path) {
        return false;
    }
    if semantics.equivalent(path, &layout.base) {
        return true;
    }
    let normalized = cleanup_core::PathSemantics::CaseSensitive.key(path);
    let base = cleanup_core::PathSemantics::CaseSensitive.key(&layout.base);
    let parts: Vec<_> = normalized
        .split('/')
        .skip(base.split('/').count())
        .collect();
    if parts
        .iter()
        .any(|p| c.exclusions.iter().any(|e| e.eq_ignore_ascii_case(p)))
    {
        return false;
    }
    let reaches = |rest: &str, target: &str| {
        let rest = rest.to_ascii_lowercase();
        let target = target.to_ascii_lowercase();
        rest == target
            || rest.starts_with(&format!("{target}/"))
            || target.starts_with(&format!("{rest}/"))
    };
    if layout.firefox {
        return parts.len() == 1
            || reaches(
                &parts[1..].join("/"),
                if layout.id == "firefox" {
                    "cache2/entries"
                } else {
                    "cache2"
                },
            );
    }
    if !layout.direct && parts.first().is_some_and(|p| profile_name(p)) {
        return parts.len() == 1
            || c.profile
                .iter()
                .any(|dir| reaches(&parts[1..].join("/"), dir));
    }
    let rest = parts.join("/");
    c.shared.iter().any(|dir| reaches(&rest, dir))
        || (layout.direct && c.profile.iter().any(|dir| reaches(&rest, dir)))
}
fn verify_companion_profile(layout: &Layout, matched: &Match) -> Result<(), StorageError> {
    if let Some(base) = &layout.profile_base {
        let name = matched
            .profile
            .file_name()
            .ok_or(StorageError::InvalidEvidence)?;
        super::known_folders::validate_binding(base.join(name))?;
    }
    Ok(())
}
pub(crate) fn nanos(time: SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos()
        .try_into()
        .ok()
}
pub(crate) fn observe(path: &Path) -> Result<ObservedEntry, StorageError> {
    let m = crate::WindowsFileSystem
        .metadata_no_follow(path)
        .map_err(|_| StorageError::InvalidEvidence)?;
    if !matches!(m.kind, EntryKind::File | EntryKind::Directory) {
        return Err(StorageError::InvalidEvidence);
    }
    let canonical_path = crate::WindowsFileSystem
        .canonicalize(path)
        .map_err(|_| StorageError::InvalidEvidence)?;
    if !crate::WindowsFileSystem
        .semantics()
        .equivalent(path, &canonical_path)
    {
        return Err(StorageError::InvalidEvidence);
    }
    Ok(ObservedEntry {
        canonical_path,
        identity: m.identity.ok_or(StorageError::InvalidEvidence)?,
        kind: m.kind,
        logical_bytes: m.size,
        allocated_bytes: Some(
            crate::WindowsFileSystem
                .allocated_size(path, &m)
                .map_err(|_| StorageError::InvalidEvidence)?,
        ),
        modified_unix_nanos: nanos(m.modified.ok_or(StorageError::InvalidEvidence)?)
            .ok_or(StorageError::InvalidEvidence)?,
    })
}
/// One exact native browser base per existing job. No renderer path grants catalog authority.
pub fn discover(
    context: &mut ScanContext,
    protection: &ProtectionPolicy,
    service_worker_opt_in: bool,
) -> Result<(), StorageError> {
    discover_with(
        context,
        protection,
        service_worker_opt_in,
        &NativeKnownFolders,
        &NativeBrowserActivity,
    )
}
fn discover_with(
    context: &mut ScanContext,
    protection: &ProtectionPolicy,
    opt_in: bool,
    resolver: &dyn KnownFolderResolver,
    activity: &dyn ActivityProbe,
) -> Result<(), StorageError> {
    if activity.activity() != BrowserActivity::Inactive {
        return Err(StorageError::UnsupportedScope);
    }
    let root = context.root.clone().ok_or(StorageError::InvalidEvidence)?;
    let layout = layouts(resolver)?
        .into_iter()
        .find(|l| {
            crate::WindowsFileSystem
                .semantics()
                .equivalent(&l.base, &root.canonical_path)
        })
        .ok_or(StorageError::UnsupportedScope)?;
    let c = catalog();
    let now = nanos(SystemTime::now()).ok_or(StorageError::InvalidEvidence)?;
    let excluded = |path: &Path| {
        !reachable(&layout, path, &c)
            || protection.is_repository_metadata(path)
            || path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| c.exclusions.iter().any(|e| e.eq_ignore_ascii_case(n)))
            || (!opt_in && path.file_name().is_some_and(|n| n == "Service Worker"))
    };
    let mut failure = None;
    let mut profile_ids = std::collections::HashMap::new();
    context.walk(protection, &excluded, &mut |ctx, event| {
        if let walk::WalkEvent::Entry { path, metadata, .. } = event {
            if metadata.kind != EntryKind::File {
                return walk::WalkControl::Continue;
            }
            let Some(matched) = match_cache(&layout, path, &c) else {
                return walk::WalkControl::Continue;
            };
            if matched.service_worker && !opt_in {
                return walk::WalkControl::Continue;
            }
            if verify_companion_profile(&layout, &matched).is_err() {
                return walk::WalkControl::Continue;
            }
            let Ok(entry) = observe(path) else {
                return walk::WalkControl::Continue;
            };
            if !old_enough(now, entry.modified_unix_nanos, c.minimum_age_seconds) {
                return walk::WalkControl::Continue;
            }
            let result = (|| {
                let profile = observe(&matched.profile)?;
                let profile_id = match profile_ids.get(&profile.canonical_path) {
                    Some(id) => String::clone(id),
                    None => {
                        let id = super::opaque_id()?;
                        profile_ids.insert(profile.canonical_path.clone(), id.clone());
                        id
                    }
                };
                let evidence = StorageEvidence::BrowserCache {
                    root: root.clone(),
                    catalog: CatalogEvidence {
                        catalog_id: "browser-caches".into(),
                        revision: REVISION.into(),
                        target_id: matched.target,
                        rule_version: 1,
                        minimum_age_seconds: c.minimum_age_seconds,
                        newest_descendant_unix_nanos: entry.modified_unix_nanos,
                        observed_at_unix_nanos: now,
                    },
                    profile,
                    browser_id: layout.id.clone(),
                    profile_id,
                    entry,
                    activity_checked_at_unix_nanos: now,
                    browser_inactive: true,
                    service_worker: matched.service_worker,
                    service_worker_opt_in: opt_in,
                };
                super::cleaner::publish(ctx, evidence)
            })();
            if let Err(error) = result {
                if error == StorageError::LimitReached {
                    ctx.mark_partial(PartialReason::RecordLimit);
                } else {
                    failure = Some(error);
                }
                return walk::WalkControl::Stop;
            }
        }
        walk::WalkControl::Continue
    })?;
    if let Some(error) = failure {
        return Err(error);
    }
    if activity.activity() != BrowserActivity::Inactive {
        return Err(StorageError::UnsupportedScope);
    }
    Ok(())
}
pub(crate) fn old_enough(now: u64, modified: u64, age: u64) -> bool {
    now.checked_sub(modified)
        .is_some_and(|n| n / 1_000_000_000 >= age)
}
/// Called at creation, execution, and recovery; persisted strings alone cannot authorize a scope.
pub(crate) fn validate_current(evidence: &StorageEvidence) -> Result<(), StorageError> {
    validate_with(evidence, &NativeKnownFolders, &NativeBrowserActivity)
}
fn validate_with(
    evidence: &StorageEvidence,
    resolver: &dyn KnownFolderResolver,
    activity: &dyn ActivityProbe,
) -> Result<(), StorageError> {
    evidence.validate()?;
    let StorageEvidence::BrowserCache {
        root,
        entry,
        catalog: proof,
        browser_id,
        profile,
        service_worker,
        service_worker_opt_in,
        ..
    } = evidence
    else {
        return Err(StorageError::UnsupportedScope);
    };
    if entry.kind != EntryKind::File || activity.activity() != BrowserActivity::Inactive {
        return Err(StorageError::UnsupportedScope);
    }
    let layout = layouts(resolver)?
        .into_iter()
        .find(|l| {
            &l.id == browser_id
                && crate::WindowsFileSystem
                    .semantics()
                    .equivalent(&l.base, &root.canonical_path)
        })
        .ok_or(StorageError::InvalidEvidence)?;
    let c = catalog();
    let m = match_cache(&layout, &entry.canonical_path, &c).ok_or(StorageError::InvalidEvidence)?;
    verify_companion_profile(&layout, &m)?;
    let current_profile = observe(&m.profile)?;
    let now = nanos(SystemTime::now()).ok_or(StorageError::InvalidEvidence)?;
    if proof.catalog_id != "browser-caches"
        || proof.revision != c.revision
        || proof.rule_version != 1
        || proof.minimum_age_seconds != c.minimum_age_seconds
        || proof.target_id != m.target
        || *service_worker != m.service_worker
        || (m.service_worker && !service_worker_opt_in)
        || current_profile.identity != profile.identity
        || current_profile.kind != EntryKind::Directory
        || current_profile.canonical_path != profile.canonical_path
        || proof.newest_descendant_unix_nanos != entry.modified_unix_nanos
        || !old_enough(now, entry.modified_unix_nanos, c.minimum_age_seconds)
    {
        return Err(StorageError::InvalidEvidence);
    }
    Ok(())
}

#[cfg(test)]
#[path = "browser_tests.rs"]
mod native_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pinned_browser_layouts_and_personal_exclusions() {
        let c = catalog();
        assert_eq!(c.revision, REVISION);
        assert_eq!(c.source, "rules/win32/browsers.json");
        assert!(!c.risk.is_empty());
        assert_eq!(c.service_worker_disclosure, SERVICE_WORKER_DISCLOSURE);
        assert_eq!(c.chromium.len(), 13);
        assert_eq!(c.firefox.len(), 5);
        assert_eq!(c.profile.len(), 7);
        assert_eq!(c.shared.len(), 7);
        let l = Layout {
            id: "chrome".into(),
            base: PathBuf::from("C:/fixture"),
            firefox: false,
            direct: false,
            profile_base: None,
        };
        for dir in &c.profile {
            assert!(
                match_cache(&l, &l.base.join("Profile 12").join(dir).join("file"), &c).is_some()
            );
        }
        for dir in &c.shared {
            assert!(match_cache(&l, &l.base.join(dir).join("file"), &c).is_some());
        }
        for path in [
            "Default/Cookies",
            "Default/History",
            "Default/Extensions/file",
            "Default/Cache/Cache_Data/IndexedDB/file",
            "Profile evil/GPUCache/file",
            "Default/file",
        ] {
            assert!(match_cache(&l, &l.base.join(path), &c).is_none(), "{path}");
        }
        assert!(!old_enough(10, 11, 0));
        assert!(!old_enough(3_600_000_000_000, 1, 3600));
    }
}
