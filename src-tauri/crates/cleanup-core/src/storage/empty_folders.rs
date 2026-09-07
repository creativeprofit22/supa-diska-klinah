use super::walk::{WalkControl, WalkEvent};
use super::{
    CandidateEligibility, Completeness, ObservedEntry, RootAuthorization, StorageEvidence,
};
use crate::{EntryKind, FileSystem};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Unknown attributes are not permission; apply this through the shared walker's exclusion gate.
pub fn blocks_visibility(fs: &dyn FileSystem, path: &Path) -> bool {
    !fs.metadata_no_follow(path)
        .ok()
        .is_some_and(|metadata| fs.hidden_or_system(path, &metadata) == Some(false))
}

struct Directory {
    entry: ObservedEntry,
    empty: bool,
    descendants: u32,
}

/// Only LeaveDirectory with complete subtree evidence can authorize a row.
/// The shared walker bounds entries/depth/retention; the stack contains at most depth frames.
#[derive(Default)]
pub struct EmptyFolders {
    stack: Vec<Directory>,
    rows: Vec<(EmptyFolderRecord, StorageEvidence)>,
}
impl EmptyFolders {
    pub fn observe(&mut self, root: &RootAuthorization, event: WalkEvent<'_>) -> WalkControl {
        match event {
            WalkEvent::Entry {
                path,
                metadata,
                depth,
            } => {
                // A directory Entry may be followed by Blocked rather than Leave
                // (duplicate identity or retention limit). Discard such unfinished frames.
                while self.stack.len() >= depth as usize {
                    self.stack.pop();
                    if let Some(parent) = self.stack.last_mut() {
                        parent.empty = false;
                    }
                }
                if metadata.kind == EntryKind::Directory {
                    self.stack.push(Directory {
                        entry: ObservedEntry {
                            canonical_path: path.to_owned(),
                            identity: metadata.identity.expect("walker requires identity"),
                            kind: EntryKind::Directory,
                            logical_bytes: 0,
                            allocated_bytes: Some(0),
                            modified_unix_nanos: 0,
                        },
                        empty: true,
                        descendants: 0,
                    });
                } else if let Some(parent) = self.stack.last_mut() {
                    parent.empty = false;
                }
            }
            WalkEvent::Blocked { .. } => {
                if let Some(parent) = self.stack.last_mut() {
                    parent.empty = false;
                }
            }
            WalkEvent::LeaveDirectory {
                path,
                depth,
                completeness,
            } => {
                if depth == 0 {
                    return WalkControl::Continue;
                }
                while self
                    .stack
                    .last()
                    .is_some_and(|d| d.entry.canonical_path != path)
                {
                    self.stack.pop();
                    if let Some(parent) = self.stack.last_mut() {
                        parent.empty = false;
                    }
                }
                let Some(directory) = self.stack.pop() else {
                    return WalkControl::Stop;
                };
                if directory.entry.canonical_path != path {
                    return WalkControl::Stop;
                }
                let empty = directory.empty && completeness.is_complete();
                if let Some(parent) = self.stack.last_mut() {
                    parent.empty &= empty;
                    parent.descendants += 1 + directory.descendants;
                }
                if empty {
                    self.rows.push((
                        EmptyFolderRecord {
                            record_id: String::new(),
                            display_path: path.to_string_lossy().into_owned(),
                            depth,
                            descendant_directories: directory.descendants as usize,
                            completeness: completeness.clone(),
                            eligibility: CandidateEligibility::ReadOnly,
                        },
                        StorageEvidence::EmptyFolder {
                            root: root.clone(),
                            entry: directory.entry,
                            complete_subtree: true,
                            descendant_directories: directory.descendants,
                        },
                    ));
                }
            }
        }
        WalkControl::Continue
    }
    pub fn finish(self) -> Vec<(EmptyFolderRecord, StorageEvidence)> {
        self.rows
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EmptyFolderRecord {
    pub record_id: String,
    pub display_path: String,
    pub depth: u16,
    pub descendant_directories: usize,
    pub completeness: Completeness,
    pub eligibility: CandidateEligibility,
}
