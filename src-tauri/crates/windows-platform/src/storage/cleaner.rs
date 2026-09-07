//! Pinned declarative filesystem targets. Never executes commands or deletes trees.
//! The same matcher authorizes discovery, persisted plans and pre-mutation revalidation.
use super::{
    browser::{nanos, observe, old_enough},
    known_folders::{KnownFolder, KnownFolderResolver, NativeKnownFolders},
    scans::ScanContext,
};
use cleanup_core::storage::*;
use cleanup_core::{EntryKind, FileSystem, Lifecycle, ProtectionPolicy, Risk};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Policy {
    revision: String,
    rule_version: u32,
    minimum_age_seconds: u64,
    lifecycle: Lifecycle,
    risk_class: Risk,
    risk: String,
    exclusions: Vec<String>,
    protected_roots: BTreeMap<String, String>,
    unsupported_operations: Vec<[String; 2]>,
}
fn policy() -> Policy {
    serde_json::from_str(include_str!(
        "../../../cleanup-core/rules/cleaner-policy.json"
    ))
    .expect("compiled cleaner policy")
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Catalog {
    id: String,
    source: String,
    targets: Vec<Target>,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Target {
    id: String,
    paths: Vec<String>,
    #[serde(default)]
    suffixes: Vec<String>,
    #[serde(default)]
    minimum_age_seconds: Option<u64>,
    #[serde(default)]
    file_patterns: Vec<String>,
    #[serde(default)]
    blockers: Vec<String>,
    #[serde(default)]
    recursive_match: Option<RecursiveMatch>,
    #[serde(default)]
    single_file: bool,
    #[serde(default)]
    risk: Option<String>,
    #[serde(default)]
    unsupported: Option<String>,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecursiveMatch {
    anchor: String,
    targets: Vec<String>,
    excluded_ancestors: Vec<String>,
    max_depth: usize,
    #[serde(default)]
    anchor_paths: Vec<String>,
}
fn catalogs() -> Vec<Catalog> {
    [
        include_str!("../../../cleanup-core/rules/gpu-caches.json"),
        include_str!("../../../cleanup-core/rules/system-caches.json"),
        include_str!("../../../cleanup-core/rules/gaming-caches.json"),
        include_str!("../../../cleanup-core/rules/misc-caches.json"),
        include_str!("../../../cleanup-core/rules/apps-caches.json"),
        include_str!("../../../cleanup-core/rules/steam-caches.json"),
        include_str!("../../../cleanup-core/rules/database-maintenance.json"),
    ]
    .into_iter()
    .map(|s| serde_json::from_str(s).expect("compiled cleaner catalog"))
    .collect()
}
/// Every expanded target has its own exact path, provenance and disposition. This
/// is a backend inventory, not a claim that a cache exists on the current machine.
#[derive(Clone, Debug)]
pub struct CatalogTargetPolicy {
    pub catalog_id: String,
    pub target_id: String,
    pub path: String,
    pub source: String,
    pub revision: String,
    pub rule_version: u32,
    pub minimum_age_seconds: u64,
    pub lifecycle: Lifecycle,
    pub risk: Risk,
    pub consequence: String,
    pub exclusions: Vec<String>,
    pub unsupported_reason: Option<String>,
    pub matcher: String,
}
#[derive(Debug)]
pub struct CleanerCatalog {
    pub targets: Vec<CatalogTargetPolicy>,
    pub unsupported_operations: Vec<[String; 2]>,
}
struct CompiledTarget {
    metadata: CatalogTargetPolicy,
    rule: Target,
}
fn compiled_targets() -> &'static [CompiledTarget] {
    static TARGETS: std::sync::OnceLock<Vec<CompiledTarget>> = std::sync::OnceLock::new();
    TARGETS.get_or_init(build_targets)
}
fn build_targets() -> Vec<CompiledTarget> {
    let p = policy();
    let mut out = Vec::new();
    for c in catalogs() {
        for t in c.targets {
            let paths: Vec<String> = if t.suffixes.is_empty() {
                t.paths.clone()
            } else {
                t.paths
                    .iter()
                    .flat_map(|base| t.suffixes.iter().map(move |s| format!("{base}/{s}")))
                    .collect()
            };
            for (index, path) in paths.into_iter().enumerate() {
                let binding = path.split('/').next().unwrap_or("");
                let reason = t
                    .unsupported
                    .clone()
                    .or_else(|| p.protected_roots.get(binding).cloned());
                let mut exclusions = p.exclusions.clone();
                if let Some(r) = &t.recursive_match {
                    exclusions.extend(r.excluded_ancestors.clone());
                }
                let metadata = CatalogTargetPolicy {
                    catalog_id: c.id.clone(),
                    target_id: format!("{}:{index}", t.id),
                    path,
                    source: c.source.clone(),
                    revision: p.revision.clone(),
                    rule_version: p.rule_version,
                    minimum_age_seconds: t.minimum_age_seconds.unwrap_or(p.minimum_age_seconds),
                    lifecycle: p.lifecycle,
                    risk: p.risk_class,
                    consequence: t.risk.clone().unwrap_or_else(|| p.risk.clone()),
                    exclusions,
                    unsupported_reason: reason,
                    matcher: if t.single_file {
                        "single-file"
                    } else if t.recursive_match.is_some() {
                        "bounded-anchor"
                    } else if !t.file_patterns.is_empty() {
                        "direct-file-allowlist"
                    } else {
                        "literal-or-one-child-cache-files"
                    }
                    .into(),
                };
                out.push(CompiledTarget {
                    metadata,
                    rule: t.clone(),
                });
            }
        }
    }
    out
}
pub fn catalog_inventory() -> CleanerCatalog {
    CleanerCatalog {
        targets: compiled_targets()
            .iter()
            .map(|t| t.metadata.clone())
            .collect(),
        unsupported_operations: policy().unsupported_operations,
    }
}
fn folder(name: &str) -> Result<KnownFolder, StorageError> {
    match name {
        "local" => Ok(KnownFolder::Local),
        "roaming" => Ok(KnownFolder::Roaming),
        "profile" => Ok(KnownFolder::Profile),
        "low" => Ok(KnownFolder::LocalLow),
        _ => Err(StorageError::UnsupportedScope),
    }
}
struct BoundTarget {
    target: &'static CompiledTarget,
    root: PathBuf,
    pattern: Vec<String>,
}
fn bind(
    target: &'static CompiledTarget,
    resolver: &dyn KnownFolderResolver,
) -> Result<BoundTarget, StorageError> {
    if target.metadata.unsupported_reason.is_some() {
        return Err(StorageError::UnsupportedScope);
    }
    let parts: Vec<_> = target.metadata.path.split('/').collect();
    let base = resolver.resolve(folder(parts[0])?)?;
    let tail = &parts[1..];
    if tail
        .iter()
        .any(|s| s.is_empty() || *s == "." || *s == ".." || s.contains(['\\', ':']))
    {
        return Err(StorageError::InvalidEvidence);
    }
    let mut count = tail
        .iter()
        .position(|s| s.contains('*'))
        .unwrap_or(tail.len());
    if target.rule.single_file {
        count = count.checked_sub(1).ok_or(StorageError::InvalidEvidence)?;
    }
    let mut root = base;
    for part in &tail[..count] {
        root.push(part);
    }
    let pattern = tail[count..].iter().map(|s| s.to_string()).collect();
    Ok(BoundTarget {
        target,
        root,
        pattern,
    })
}
fn relative(root: &Path, path: &Path) -> Option<Vec<String>> {
    let semantics = crate::WindowsFileSystem.semantics();
    if !cleanup_core::is_local_storage_path(path) || !semantics.contains(root, path) {
        return None;
    }
    let r = semantics.key(root);
    let p = semantics.key(path);
    Some(
        p.split('/')
            .skip(r.split('/').count())
            .map(String::from)
            .collect(),
    )
}
// Only a single '*' within one name; never path globbing or recursive '**'.
fn name_matches(pattern: &str, name: &str) -> bool {
    let pattern = pattern.to_ascii_lowercase();
    let name = name.to_ascii_lowercase();
    if let Some((a, b)) = pattern.split_once('*') {
        !b.contains('*')
            && name.len() >= a.len() + b.len()
            && name.starts_with(a)
            && name.ends_with(b)
    } else {
        pattern == name
    }
}
fn prefix_matches(pattern: &[String], parts: &[String]) -> bool {
    parts.len() >= pattern.len() && pattern.iter().zip(parts).all(|(a, b)| name_matches(a, b))
}
impl BoundTarget {
    fn excluded(&self, path: &Path, protection: &ProtectionPolicy) -> bool {
        protection.is_protected(path)
            || protection.is_repository_metadata(path)
            || path.components().any(|part| {
                part.as_os_str().to_str().is_some_and(|n| {
                    self.target
                        .metadata
                        .exclusions
                        .iter()
                        .any(|e| e.eq_ignore_ascii_case(n))
                })
            })
    }
    fn matches(&self, path: &Path) -> bool {
        let Some(parts) = relative(&self.root, path) else {
            return false;
        };
        if !prefix_matches(&self.pattern, &parts) {
            return false;
        }
        let rest = &parts[self.pattern.len()..];
        let rule = &self.target.rule;
        if rule.single_file {
            return rest.is_empty();
        }
        if let Some(r) = &rule.recursive_match {
            if r.max_depth == 0 || r.max_depth > 32 || rest.len() < 2 {
                return false;
            }
            // Anchors are explicit paths, or the literal root's final component.
            let anchors: Vec<Vec<String>> = if r.anchor_paths.is_empty() {
                if !self
                    .root
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(&r.anchor))
                {
                    return false;
                }
                vec![vec![]]
            } else {
                r.anchor_paths
                    .iter()
                    .map(|p| p.split('/').map(String::from).collect())
                    .collect()
            };
            return anchors.iter().any(|a| {
                if !a.is_empty() && !a.last().is_some_and(|n| n.eq_ignore_ascii_case(&r.anchor)) {
                    return false;
                }
                if !prefix_matches(a, rest) {
                    return false;
                }
                let below = &rest[a.len()..];
                below
                    .iter()
                    .take(below.len().saturating_sub(1))
                    .enumerate()
                    .any(|(depth, name)| {
                        depth < r.max_depth
                            && r.targets.iter().any(|t| t.eq_ignore_ascii_case(name))
                    })
            });
        }
        if rule.file_patterns.is_empty() {
            return !rest.is_empty();
        }
        rest.len() == 1 && rule.file_patterns.iter().any(|p| name_matches(p, &rest[0]))
    }
    // Prune unrelated trees BEFORE descent, including when a WebView/updater
    // target binds Local AppData. No search-anywhere recursive fallback.
    fn may_contain(&self, path: &Path) -> bool {
        let Some(parts) = relative(&self.root, path) else {
            return false;
        };
        let common = parts.len().min(self.pattern.len());
        if !prefix_matches(&self.pattern[..common], &parts) {
            return false;
        }
        if parts.len() < self.pattern.len() {
            return true;
        }
        let rest = &parts[self.pattern.len()..];
        if let Some(r) = &self.target.rule.recursive_match {
            let anchors: Vec<Vec<String>> = if r.anchor_paths.is_empty() {
                vec![vec![]]
            } else {
                r.anchor_paths
                    .iter()
                    .map(|a| a.split('/').map(String::from).collect())
                    .collect()
            };
            return anchors.iter().any(|a| {
                let common = a.len().min(rest.len());
                if !prefix_matches(&a[..common], rest) {
                    return false;
                }
                if rest.len() < a.len() {
                    return true;
                }
                let below = &rest[a.len()..];
                below.len() <= r.max_depth
                    || below
                        .iter()
                        .take(r.max_depth)
                        .any(|n| r.targets.iter().any(|t| t.eq_ignore_ascii_case(n)))
            });
        }
        if self.target.rule.single_file {
            return rest.is_empty();
        }
        if !self.target.rule.file_patterns.is_empty() {
            return rest.is_empty() || self.matches(path);
        }
        true
    }
    fn blockers_absent(&self, path: &Path) -> bool {
        let Some(parent) = path.parent() else {
            return false;
        };
        self.target.rule.blockers.iter().all(|b| {
            matches!(std::fs::symlink_metadata(parent.join(b)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound)
        })
    }
}
/// Construct storage proof only; authority is checked by execution before persistence/mutation.
pub(crate) fn plan_proof(
    evidence: StorageEvidence,
) -> Result<cleanup_core::ResolvedCandidate, StorageError> {
    use cleanup_core::*;
    let (catalog, source, lifecycle, risk, exclusions) = match &evidence {
        StorageEvidence::CatalogTarget { catalog, .. } => {
            let t = compiled_targets()
                .iter()
                .find(|t| {
                    t.metadata.catalog_id == catalog.catalog_id
                        && t.metadata.target_id == catalog.target_id
                })
                .ok_or(StorageError::UnsupportedScope)?;
            if t.metadata.unsupported_reason.is_some()
                || catalog.revision != t.metadata.revision
                || catalog.rule_version != t.metadata.rule_version
                || catalog.minimum_age_seconds != t.metadata.minimum_age_seconds
            {
                return Err(StorageError::InvalidEvidence);
            }
            (
                catalog,
                t.metadata.source.clone(),
                t.metadata.lifecycle,
                t.metadata.risk,
                t.metadata.exclusions.clone(),
            )
        }
        StorageEvidence::BrowserCache { catalog, .. } => {
            let p = super::browser::policy();
            (catalog, p.source, p.lifecycle, p.risk, vec![])
        }
        _ => return Err(StorageError::UnsupportedScope),
    };
    let entry = evidence.entry();
    let root = evidence.root();
    Ok(ResolvedCandidate {
        path: entry.canonical_path.clone(),
        scan_root: root.canonical_path.clone(),
        context_root: root.canonical_path.clone(),
        context_identity: Some(root.identity),
        identity: entry.identity,
        kind: entry.kind,
        logical_bytes: entry.logical_bytes,
        allocated_bytes: entry.allocated_bytes.ok_or(StorageError::InvalidEvidence)?,
        scanned_at: SystemTime::now(),
        rule: CleanupRule {
            id: catalog.catalog_id.clone(),
            rule_version: catalog.rule_version,
            lifecycle,
            risk,
            provenance: Provenance {
                source: format!(
                    "https://github.com/AdventDevInc/kudu/blob/{}/{source}",
                    catalog.revision
                ),
                verified_at: String::new(),
            },
            default_selected: false,
            artifact: None,
            scanner: ScannerKind::Direct,
            roots: vec![RuleRoot {
                binding: "native-known-folder".into(),
                suffix: Default::default(),
            }],
            markers: Markers::default(),
            targets: vec![catalog.target_id.clone()],
            target_prefixes: vec![],
            target_suffixes: vec![],
            target_type: TargetType::File,
            root_depth: 0,
            project_depth: None,
            target_depth: None,
            minimum_age_seconds: catalog.minimum_age_seconds,
            excluded_names: exclusions,
            excluded_paths: vec![],
        },
        scope: CandidateProofScope::Storage {
            evidence: Box::new(evidence),
        },
    })
}
pub(crate) fn publish(
    context: &mut ScanContext,
    evidence: StorageEvidence,
) -> Result<(), StorageError> {
    let id = super::opaque_id()?;
    let entry = evidence.entry();
    let display_path = entry.canonical_path.to_string_lossy().into_owned();
    let record = large_files::FileRecord {
        record_id: id.clone(),
        display_path: display_path.clone(),
        logical_bytes: entry.logical_bytes,
        allocated_bytes: entry.allocated_bytes,
        modified_unix_seconds: Some(entry.modified_unix_nanos / 1_000_000_000),
        eligibility: CandidateEligibility::Eligible {
            candidate_id: id.clone(),
        },
    };
    context.add_candidate(id, evidence)?;
    context.push(
        StorageRecord::File(record),
        RecordOrder {
            numeric: 0,
            text: display_path.to_lowercase(),
        },
    )
}
pub fn discover(
    context: &mut ScanContext,
    protection: &ProtectionPolicy,
) -> Result<(), StorageError> {
    discover_with(context, protection, &NativeKnownFolders)
}
fn discover_with(
    context: &mut ScanContext,
    protection: &ProtectionPolicy,
    resolver: &dyn KnownFolderResolver,
) -> Result<(), StorageError> {
    let root = context.root.clone().ok_or(StorageError::InvalidEvidence)?;
    let targets: Vec<_> = compiled_targets()
        .iter()
        .filter_map(|t| bind(t, resolver).ok())
        .filter(|t| {
            crate::WindowsFileSystem
                .semantics()
                .equivalent(&t.root, &root.canonical_path)
        })
        .collect();
    if targets.is_empty() {
        return Err(StorageError::UnsupportedScope);
    }
    let current = observe(&super::known_folders::validate_binding(
        root.canonical_path.clone(),
    )?)?;
    if current.identity != root.identity || current.kind != EntryKind::Directory {
        return Err(StorageError::InvalidEvidence);
    }
    let now = nanos(SystemTime::now()).ok_or(StorageError::InvalidEvidence)?;
    let mut failure = None;
    context.walk(
        protection,
        &|path| {
            targets
                .iter()
                .all(|t| t.excluded(path, protection) || !t.may_contain(path))
        },
        &mut |ctx, event| {
            if let walk::WalkEvent::Entry { path, metadata, .. } = event {
                if metadata.kind != EntryKind::File {
                    return walk::WalkControl::Continue;
                }
                let Some(t) = targets.iter().find(|t| {
                    !t.excluded(path, protection) && t.matches(path) && t.blockers_absent(path)
                }) else {
                    return walk::WalkControl::Continue;
                };
                let Ok(entry) = observe(path) else {
                    return walk::WalkControl::Continue;
                };
                let p = &t.target.metadata;
                if !old_enough(now, entry.modified_unix_nanos, p.minimum_age_seconds) {
                    return walk::WalkControl::Continue;
                }
                let evidence = StorageEvidence::CatalogTarget {
                    root: root.clone(),
                    catalog: CatalogEvidence {
                        catalog_id: p.catalog_id.clone(),
                        revision: p.revision.clone(),
                        target_id: p.target_id.clone(),
                        rule_version: p.rule_version,
                        minimum_age_seconds: p.minimum_age_seconds,
                        newest_descendant_unix_nanos: entry.modified_unix_nanos,
                        observed_at_unix_nanos: now,
                    },
                    entry,
                };
                if let Err(error) = publish(ctx, evidence) {
                    if error == StorageError::LimitReached {
                        ctx.mark_partial(PartialReason::RecordLimit);
                    } else {
                        failure = Some(error);
                    }
                    return walk::WalkControl::Stop;
                }
            }
            walk::WalkControl::Continue
        },
    )?;
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(())
}
pub(crate) fn validate_current(
    evidence: &StorageEvidence,
    protection: &ProtectionPolicy,
) -> Result<(), StorageError> {
    validate_with(evidence, protection, &NativeKnownFolders)
}
fn validate_with(
    evidence: &StorageEvidence,
    protection: &ProtectionPolicy,
    resolver: &dyn KnownFolderResolver,
) -> Result<(), StorageError> {
    evidence.validate()?;
    let StorageEvidence::CatalogTarget {
        root,
        entry,
        catalog: proof,
    } = evidence
    else {
        return Err(StorageError::UnsupportedScope);
    };
    let t = compiled_targets()
        .iter()
        .find(|t| {
            t.metadata.catalog_id == proof.catalog_id && t.metadata.target_id == proof.target_id
        })
        .ok_or(StorageError::UnsupportedScope)?;
    let t = bind(t, resolver)?;
    let p = &t.target.metadata;
    let now = nanos(SystemTime::now()).ok_or(StorageError::InvalidEvidence)?;
    if entry.kind != EntryKind::File
        || proof.revision != p.revision
        || proof.rule_version != p.rule_version
        || proof.minimum_age_seconds != p.minimum_age_seconds
        || !crate::WindowsFileSystem
            .semantics()
            .equivalent(&t.root, &root.canonical_path)
        || t.excluded(&entry.canonical_path, protection)
        || !t.matches(&entry.canonical_path)
        || !t.blockers_absent(&entry.canonical_path)
        || proof.newest_descendant_unix_nanos != entry.modified_unix_nanos
        || proof.observed_at_unix_nanos > now
        || !old_enough(now, entry.modified_unix_nanos, p.minimum_age_seconds)
    {
        return Err(StorageError::InvalidEvidence);
    }
    let current_root = observe(&super::known_folders::validate_binding(t.root.clone())?)?;
    // Check EVERY ancestor, not just the cache leaf; junction swaps cannot grant scope.
    super::known_folders::validate_binding(
        entry
            .canonical_path
            .parent()
            .ok_or(StorageError::InvalidEvidence)?
            .to_path_buf(),
    )?;
    let current = observe(&entry.canonical_path)?;
    if current_root.identity != root.identity
        || current_root.kind != EntryKind::Directory
        || &current != entry
    {
        return Err(StorageError::InvalidEvidence);
    }
    Ok(())
}
#[cfg(test)]
#[path = "cleaner_tests.rs"]
mod tests;
