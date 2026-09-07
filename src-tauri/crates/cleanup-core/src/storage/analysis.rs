use super::{
    Completeness, PartialReason, StorageError,
    walk::{WalkControl, WalkEvent},
};
use crate::{EntryKind, FileIdentity};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
};

/// Aggregate records deliberately have no eligibility/candidate field.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DirectorySummary {
    pub node_id: String,
    pub parent_id: Option<String>,
    pub display_path: String,
    pub logical_bytes: u64,
    pub allocated_bytes: Option<u64>,
    pub independent_files: u64,
    pub hard_link_entries: u64,
    pub completeness: Completeness,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionSummary {
    pub extension: String,
    pub file_count: u64,
    pub logical_bytes: u64,
    pub allocated_bytes: Option<u64>,
    pub completeness: Completeness,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DriveSummary {
    pub drive_id: String,
    pub label: String,
    pub filesystem: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub used_bytes: u64,
    /// None means the system-volume identity could not be established.
    pub system: Option<bool>,
}
/// Unique identities are counted once *within each subtree/type*. Sibling totals
/// need not add to their parent when hard links cross siblings; type totals need
/// not add to the root when aliases have different extensions. No first-path wins.
/// Rows, unique identities, and the *combined* subtree/type identity memberships
/// each have a separate retention-limit budget: O(limit), not O(depth * limit).
/// Deep trees can therefore reach RecordLimit before the row limit. Admission is
/// atomic per file; a rejected file contributes to no totals.
/// Unknown allocation stays unknown, never substituted with logical size.
pub struct Analyzer {
    root: PathBuf,
    depth: u16,
    limit: usize,
    directories: BTreeMap<PathBuf, (DirectorySummary, HashSet<FileIdentity>)>,
    extensions: BTreeMap<String, (ExtensionSummary, HashSet<FileIdentity>)>,
    identities: HashMap<FileIdentity, (u64, Option<u64>)>,
    partial: Completeness,
    memberships: usize,
}
impl Analyzer {
    pub fn new(root: &Path, displayed_depth: u16, limit: usize) -> Result<Self, StorageError> {
        if displayed_depth > super::MAX_DEPTH || !(1..=super::MAX_RECORDS).contains(&limit) {
            return Err(StorageError::InvalidRequest);
        }
        let mut result = Self {
            root: root.into(),
            depth: displayed_depth,
            limit,
            directories: BTreeMap::new(),
            extensions: BTreeMap::new(),
            identities: HashMap::new(),
            partial: Completeness::default(),
            memberships: 0,
        };
        result.directory(root);
        Ok(result)
    }
    fn directory(&mut self, path: &Path) {
        self.directories.insert(
            path.into(),
            (
                DirectorySummary {
                    node_id: String::new(),
                    parent_id: None,
                    display_path: path.to_string_lossy().into_owned(),
                    logical_bytes: 0,
                    allocated_bytes: Some(0),
                    independent_files: 0,
                    hard_link_entries: 0,
                    completeness: Completeness::default(),
                },
                HashSet::new(),
            ),
        );
    }
    pub fn observe(&mut self, event: WalkEvent<'_>, allocated: Option<u64>) -> WalkControl {
        match event {
            WalkEvent::Entry {
                path,
                metadata,
                depth,
            } if metadata.kind == EntryKind::Directory && depth <= self.depth => {
                if self.directories.len() + self.extensions.len() >= self.limit {
                    return self.stop();
                }
                self.directory(path);
            }
            WalkEvent::Entry { path, metadata, .. } if metadata.kind == EntryKind::File => {
                let Some(identity) = metadata.identity else {
                    self.partial.mark(PartialReason::MissingIdentity);
                    return WalkControl::Continue;
                };
                let extension = super::large_files::extension(path);
                if (!self.identities.contains_key(&identity) && self.identities.len() >= self.limit)
                    || (!self.extensions.contains_key(&extension)
                        && self.directories.len() + self.extensions.len() >= self.limit)
                {
                    return self.stop();
                }
                let needed = path
                    .ancestors()
                    .skip(1)
                    .filter(|p| {
                        self.directories
                            .get(*p)
                            .is_some_and(|(_, seen)| !seen.contains(&identity))
                    })
                    .count()
                    + usize::from(
                        self.extensions
                            .get(&extension)
                            .is_none_or(|(_, seen)| !seen.contains(&identity)),
                    );
                if needed > self.limit - self.memberships {
                    return self.stop();
                }
                self.memberships += needed;
                if allocated.is_none() {
                    self.partial.mark(PartialReason::Unreadable);
                }
                if let Some(previous) = self.identities.insert(identity, (metadata.size, allocated))
                    && previous != (metadata.size, allocated)
                {
                    self.partial.mark(PartialReason::Changed);
                }
                for ancestor in path.ancestors().skip(1) {
                    if let Some((summary, seen)) = self.directories.get_mut(ancestor) {
                        if allocated.is_none() {
                            summary.allocated_bytes = None;
                        }
                        if seen.insert(identity) {
                            summary.independent_files += 1;
                            add(
                                &mut summary.logical_bytes,
                                &mut summary.allocated_bytes,
                                metadata.size,
                                allocated,
                                &mut summary.completeness,
                            );
                        } else {
                            summary.hard_link_entries += 1;
                        }
                    }
                }
                let (summary, seen) =
                    self.extensions.entry(extension.clone()).or_insert_with(|| {
                        (
                            ExtensionSummary {
                                extension,
                                file_count: 0,
                                logical_bytes: 0,
                                allocated_bytes: Some(0),
                                completeness: Completeness::default(),
                            },
                            HashSet::new(),
                        )
                    });
                if allocated.is_none() {
                    summary.allocated_bytes = None;
                }
                if seen.insert(identity) {
                    summary.file_count += 1;
                    add(
                        &mut summary.logical_bytes,
                        &mut summary.allocated_bytes,
                        metadata.size,
                        allocated,
                        &mut summary.completeness,
                    );
                }
            }
            WalkEvent::LeaveDirectory {
                path, completeness, ..
            } => {
                for ancestor in path.ancestors() {
                    if let Some((summary, _)) = self.directories.get_mut(ancestor) {
                        for reason in &completeness.reasons {
                            summary.completeness.mark(*reason);
                        }
                    }
                }
            }
            _ => {}
        }
        WalkControl::Continue
    }
    fn stop(&mut self) -> WalkControl {
        self.partial.mark(PartialReason::RecordLimit);
        WalkControl::Stop
    }
    /// IDs are assigned only by the backend adapter after aggregation. Every row
    /// is conservative: global interruption marks even already-completed rows.
    pub fn finish(
        mut self,
        completeness: &Completeness,
    ) -> (Vec<DirectorySummary>, Vec<ExtensionSummary>) {
        for reason in &completeness.reasons {
            self.partial.mark(*reason);
        }
        for (summary, _) in self.directories.values_mut() {
            for reason in &self.partial.reasons {
                summary.completeness.mark(*reason);
            }
        }
        for (summary, _) in self.extensions.values_mut() {
            for reason in &self.partial.reasons {
                summary.completeness.mark(*reason);
            }
        }
        debug_assert!(self.directories.contains_key(&self.root));
        (
            self.directories.into_values().map(|(s, _)| s).collect(),
            self.extensions.into_values().map(|(s, _)| s).collect(),
        )
    }
}
fn add(
    logical: &mut u64,
    allocated: &mut Option<u64>,
    size: u64,
    allocation: Option<u64>,
    completeness: &mut Completeness,
) {
    if let Some(sum) = logical.checked_add(size) {
        *logical = sum;
    } else {
        *logical = u64::MAX;
        completeness.mark(PartialReason::EntryLimit);
    }
    let sum = allocated.and_then(|a| allocation.and_then(|b| a.checked_add(b)));
    if allocated.is_some() && allocation.is_some() && sum.is_none() {
        completeness.mark(PartialReason::EntryLimit);
    }
    *allocated = sum;
}
