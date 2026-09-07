use super::*;
use crate::Entropy;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    time::Duration,
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PageCollection {
    Files,
    Tree,
    Extensions,
    DuplicateGroups,
    DuplicateMembers,
    EmptyFolders,
    Drives,
    Programs,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PageRequest {
    pub snapshot_id: String,
    pub module: StorageModule,
    pub collection: PageCollection,
    pub parent_id: Option<String>,
    pub cursor: Option<String>,
    #[serde(default = "default_page_size")]
    pub page_size: usize,
}
fn default_page_size() -> usize {
    DEFAULT_PAGE_SIZE
}
fn collection_allowed(module: StorageModule, collection: PageCollection) -> bool {
    match collection {
        PageCollection::Files => matches!(
            module,
            StorageModule::Cleaner
                | StorageModule::LargeFiles
                | StorageModule::Browser
                | StorageModule::Uninstaller
        ),
        PageCollection::Tree | PageCollection::Extensions => module == StorageModule::DiskAnalyzer,
        PageCollection::DuplicateGroups | PageCollection::DuplicateMembers => {
            module == StorageModule::Duplicates
        }
        PageCollection::EmptyFolders => module == StorageModule::EmptyFolders,
        PageCollection::Drives => module == StorageModule::Drives,
        PageCollection::Programs => module == StorageModule::Uninstaller,
    }
}
impl PageRequest {
    pub fn validate(&self) -> Result<(), StorageError> {
        if !collection_allowed(self.module, self.collection)
            || !valid_id(&self.snapshot_id)
            || !(1..=MAX_PAGE_SIZE).contains(&self.page_size)
            || self.parent_id.as_ref().is_some_and(|id| !valid_id(id))
            || self.cursor.as_ref().is_some_and(|id| !valid_id(id))
            || (self.parent_id.is_some()
                && !matches!(
                    self.collection,
                    PageCollection::Tree | PageCollection::DuplicateMembers
                ))
            || (self.collection == PageCollection::DuplicateMembers && self.parent_id.is_none())
        {
            return Err(StorageError::InvalidRequest);
        }
        Ok(())
    }
}
/// Read-only program metadata. Opaque IDs resolve only in the retained backend snapshot.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstalledProgram {
    pub program_id: String,
    pub name: String,
    pub publisher: Option<String>,
    pub version: Option<String>,
    pub install_date: Option<String>,
    pub estimated_size_bytes: Option<u64>,
    pub last_used_at: Option<u64>,
    pub leftover_support: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", content = "record", rename_all = "camelCase")]
pub enum StorageRecord {
    File(large_files::FileRecord),
    Program(InstalledProgram),
    Directory(analysis::DirectorySummary),
    Extension(analysis::ExtensionSummary),
    Drive(analysis::DriveSummary),
    DuplicateGroup(duplicates::DuplicateGroup),
    DuplicateMember(duplicates::DuplicateMember),
    EmptyFolder(empty_folders::EmptyFolderRecord),
}
impl StorageRecord {
    fn collection(&self) -> PageCollection {
        match self {
            Self::File(_) => PageCollection::Files,
            Self::Program(_) => PageCollection::Programs,
            Self::Directory(_) => PageCollection::Tree,
            Self::Extension(_) => PageCollection::Extensions,
            Self::Drive(_) => PageCollection::Drives,
            Self::DuplicateGroup(_) => PageCollection::DuplicateGroups,
            Self::DuplicateMember(_) => PageCollection::DuplicateMembers,
            Self::EmptyFolder(_) => PageCollection::EmptyFolders,
        }
    }
    fn parent(&self) -> Option<&str> {
        match self {
            Self::Directory(r) => r.parent_id.as_deref(),
            Self::DuplicateMember(r) => Some(&r.group_id),
            _ => None,
        }
    }
    fn eligibility(&self) -> Option<&CandidateEligibility> {
        match self {
            Self::File(r) => Some(&r.eligibility),
            Self::DuplicateMember(r) => Some(&r.file.eligibility),
            Self::EmptyFolder(r) => Some(&r.eligibility),
            _ => None,
        }
    }
    fn id(&self) -> Option<&str> {
        match self {
            Self::File(r) => Some(&r.record_id),
            Self::Program(r) => Some(&r.program_id),
            Self::Directory(r) => Some(&r.node_id),
            Self::Drive(r) => Some(&r.drive_id),
            Self::DuplicateGroup(r) => Some(&r.group_id),
            Self::DuplicateMember(r) => Some(&r.file.record_id),
            Self::EmptyFolder(r) => Some(&r.record_id),
            Self::Extension(_) => None,
        }
    }
    fn belongs_to(&self, module: StorageModule) -> bool {
        match self {
            Self::File(_) => matches!(
                module,
                StorageModule::Cleaner
                    | StorageModule::LargeFiles
                    | StorageModule::Browser
                    | StorageModule::Uninstaller
            ),
            Self::Directory(_) | Self::Extension(_) => module == StorageModule::DiskAnalyzer,
            Self::Drive(_) => module == StorageModule::Drives,
            Self::Program(_) => module == StorageModule::Uninstaller,
            Self::DuplicateGroup(_) | Self::DuplicateMember(_) => {
                module == StorageModule::Duplicates
            }
            Self::EmptyFolder(_) => module == StorageModule::EmptyFolders,
        }
    }
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoragePage {
    pub snapshot_id: String,
    pub records: Vec<StorageRecord>,
    pub next_cursor: Option<String>,
    pub retained_total: usize,
    pub completeness: Completeness,
}

/// Sort/filter decisions are backend-owned and frozen before publication. The numeric
/// key supports size/date ordering; text supports path/extension ordering. ID breaks ties.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RecordOrder {
    pub numeric: u64,
    pub text: String,
}
struct Row {
    record: StorageRecord,
    order: RecordOrder,
    cursor: String,
}
pub struct SnapshotBuilder {
    id: String,
    module: StorageModule,
    rows: Vec<Row>,
    candidates: HashMap<String, StorageEvidence>,
    completeness: Completeness,
    limit: usize,
}
impl SnapshotBuilder {
    pub fn new(id: String, module: StorageModule, limit: usize) -> Result<Self, StorageError> {
        if !valid_id(&id) || !(1..=MAX_RECORDS).contains(&limit) {
            return Err(StorageError::InvalidRequest);
        }
        Ok(Self {
            id,
            module,
            rows: Vec::new(),
            candidates: HashMap::new(),
            completeness: Completeness::default(),
            limit,
        })
    }
    pub fn mark_partial(&mut self, reason: PartialReason) {
        self.completeness.mark(reason);
    }
    pub fn add_candidate(
        &mut self,
        id: String,
        evidence: StorageEvidence,
    ) -> Result<(), StorageError> {
        evidence.validate()?;
        if !valid_id(&id)
            || evidence.root().snapshot_id != self.id
            || evidence.module() != self.module
            || self.candidates.contains_key(&id)
        {
            return Err(StorageError::InvalidEvidence);
        }
        if self.candidates.len() >= self.limit {
            self.mark_partial(PartialReason::RecordLimit);
            return Err(StorageError::LimitReached);
        }
        self.candidates.insert(id, evidence);
        Ok(())
    }
    pub fn push(&mut self, record: StorageRecord, order: RecordOrder) -> Result<(), StorageError> {
        if !record.belongs_to(self.module)
            || record.id().is_some_and(|id| !valid_id(id))
            || record.parent().is_some_and(|id| !valid_id(id))
            || order.text.len() > 4096
        {
            return Err(StorageError::InvalidRequest);
        }
        // Bounds each retained DTO, including display strings; evidence is stored separately.
        if serde_json::to_vec(&record)
            .map_err(|_| StorageError::InvalidRequest)?
            .len()
            > 16_384
        {
            return Err(StorageError::InvalidRequest);
        }
        if self.rows.len() >= self.limit {
            self.mark_partial(PartialReason::RecordLimit);
            return Err(StorageError::LimitReached);
        }
        self.rows.push(Row {
            record,
            order,
            cursor: String::new(),
        });
        Ok(())
    }
    pub fn finish(
        mut self,
        entropy: &dyn Entropy,
        descending: bool,
    ) -> Result<StorageSnapshot, StorageError> {
        let mut ids = HashMap::new();
        let mut referenced = HashSet::new();
        for row in &self.rows {
            if row
                .record
                .id()
                .is_some_and(|id| ids.insert(id, &row.record).is_some())
            {
                return Err(StorageError::InvalidRequest);
            }
            if let Some(CandidateEligibility::Eligible { candidate_id }) = row.record.eligibility()
            {
                let evidence = self
                    .candidates
                    .get(candidate_id)
                    .ok_or(StorageError::InvalidEvidence)?;
                let (path, bytes, kind) = match &row.record {
                    StorageRecord::File(r) => (&r.display_path, Some(r.logical_bytes), None),
                    StorageRecord::DuplicateMember(r) => (
                        &r.file.display_path,
                        Some(r.file.logical_bytes),
                        Some(crate::EntryKind::File),
                    ),
                    StorageRecord::EmptyFolder(r) if r.completeness.is_complete() => {
                        (&r.display_path, None, Some(crate::EntryKind::Directory))
                    }
                    _ => return Err(StorageError::InvalidEvidence),
                };
                if evidence.entry().canonical_path.to_str() != Some(path.as_str())
                    || bytes.is_some_and(|n| n != evidence.entry().logical_bytes)
                    || kind.is_some_and(|k| k != evidence.entry().kind)
                    || !referenced.insert(candidate_id.clone())
                {
                    return Err(StorageError::InvalidEvidence);
                }
            }
        }
        if referenced.len() != self.candidates.len() {
            return Err(StorageError::InvalidEvidence);
        }
        for row in &self.rows {
            if let Some(parent) = row.record.parent() {
                let parent_record = ids.get(parent).ok_or(StorageError::InvalidRequest)?;
                if !matches!(
                    (&row.record, *parent_record),
                    (StorageRecord::Directory(_), StorageRecord::Directory(_))
                        | (
                            StorageRecord::DuplicateMember(_),
                            StorageRecord::DuplicateGroup(_)
                        )
                ) {
                    return Err(StorageError::InvalidRequest);
                }
                // A parent chain is bounded by the shared depth cap. Reject cycles and self-parenting.
                if matches!(row.record, StorageRecord::Directory(_)) {
                    let mut ancestor = Some(parent);
                    let mut depth = 0;
                    while let Some(id) = ancestor {
                        if Some(id) == row.record.id() || depth >= usize::from(MAX_DEPTH) {
                            return Err(StorageError::InvalidRequest);
                        }
                        ancestor = ids.get(id).ok_or(StorageError::InvalidRequest)?.parent();
                        depth += 1;
                    }
                }
            }
        }
        let mut member_counts = HashMap::<&str, usize>::new();
        let mut members = HashMap::new();
        for row in &self.rows {
            if let StorageRecord::DuplicateMember(member) = &row.record {
                *member_counts.entry(&member.group_id).or_default() += 1;
                let path = crate::PathSemantics::CaseInsensitive
                    .key(std::path::Path::new(&member.file.display_path));
                if members
                    .insert((member.group_id.as_str(), path), &member.file)
                    .is_some()
                {
                    return Err(StorageError::InvalidEvidence);
                }
            }
        }
        let mut keepers = HashMap::new();
        for row in &self.rows {
            if let StorageRecord::DuplicateGroup(group) = &row.record {
                let retained = member_counts
                    .get(group.group_id.as_str())
                    .copied()
                    .unwrap_or(0);
                if !(2..=MAX_RECORDS).contains(&group.member_count)
                    || !(2..=group.member_count).contains(&group.independent_copies)
                    || retained > group.member_count
                    || (group.completeness.is_complete() && retained != group.member_count)
                {
                    return Err(StorageError::InvalidEvidence);
                }
            }
            if let StorageRecord::DuplicateMember(member) = &row.record {
                let Some(StorageRecord::DuplicateGroup(group)) = ids.get(member.group_id.as_str())
                else {
                    return Err(StorageError::InvalidEvidence);
                };
                if member.file.logical_bytes != group.bytes_per_copy {
                    return Err(StorageError::InvalidEvidence);
                }
                if let CandidateEligibility::Eligible { candidate_id } = &member.file.eligibility {
                    let Some(StorageEvidence::DuplicateMember { keeper, .. }) =
                        self.candidates.get(candidate_id)
                    else {
                        return Err(StorageError::InvalidEvidence);
                    };
                    let keeper_path =
                        crate::PathSemantics::CaseInsensitive.key(&keeper.keeper.canonical_path);
                    let Some(keeper_row) = members.get(&(member.group_id.as_str(), keeper_path))
                    else {
                        return Err(StorageError::InvalidEvidence);
                    };
                    if keeper.group_id != member.group_id
                        || !group.completeness.is_complete()
                        || keeper.independent_copies as usize != group.independent_copies
                        || matches!(
                            keeper_row.eligibility,
                            CandidateEligibility::Ineligible { .. }
                        )
                        || keeper_row.logical_bytes != keeper.keeper.logical_bytes
                        || keeper_row.allocated_bytes != keeper.keeper.allocated_bytes
                        || keepers
                            .insert(member.group_id.as_str(), keeper.full_sha256)
                            .is_some_and(|previous| previous != keeper.full_sha256)
                    {
                        return Err(StorageError::InvalidEvidence);
                    }
                }
            }
        }
        let parents = ids
            .into_iter()
            .map(|(id, r)| (id.to_owned(), r.collection()))
            .collect();
        self.rows.sort_by(|a, b| {
            let order = a.order.cmp(&b.order);
            (if descending { order.reverse() } else { order })
                .then_with(|| a.record.id().cmp(&b.record.id()))
        });
        let mut cursors = HashMap::new();
        let mut pages: HashMap<(PageCollection, Option<String>), Vec<usize>> = HashMap::new();
        for (index, row) in self.rows.iter_mut().enumerate() {
            let mut bytes = [0; 16];
            entropy
                .fill(&mut bytes)
                .map_err(|_| StorageError::Entropy)?;
            row.cursor = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
            let key = (
                row.record.collection(),
                row.record.parent().map(str::to_owned),
            );
            let page = pages.entry(key.clone()).or_default();
            if cursors
                .insert(row.cursor.clone(), (key, page.len()))
                .is_some()
            {
                return Err(StorageError::Entropy);
            }
            page.push(index);
        }
        Ok(StorageSnapshot {
            id: self.id,
            module: self.module,
            rows: self.rows,
            cursors,
            candidates: self.candidates,
            completeness: self.completeness,
            pages,
            parents,
        })
    }
}
/// No mutators or raw record/proof-vector exports. Dropping/releasing this value drops all evidence.
pub struct StorageSnapshot {
    id: String,
    module: StorageModule,
    rows: Vec<Row>,
    cursors: HashMap<String, ((PageCollection, Option<String>), usize)>,
    pages: HashMap<(PageCollection, Option<String>), Vec<usize>>,
    parents: HashMap<String, PageCollection>,
    candidates: HashMap<String, StorageEvidence>,
    completeness: Completeness,
}
impl StorageSnapshot {
    pub fn id(&self) -> &str {
        &self.id
    }
    fn page(&self, request: &PageRequest) -> Result<StoragePage, StorageError> {
        request.validate()?;
        if request.snapshot_id != self.id || request.module != self.module {
            return Err(StorageError::SnapshotUnavailable);
        }
        let key = (request.collection, request.parent_id.clone());
        if let Some(parent) = &request.parent_id {
            let expected = if request.collection == PageCollection::Tree {
                PageCollection::Tree
            } else {
                PageCollection::DuplicateGroups
            };
            if self.parents.get(parent) != Some(&expected) {
                return Err(StorageError::InvalidRequest);
            }
        }
        let indices = self.pages.get(&key).map(Vec::as_slice).unwrap_or(&[]);
        let start = if let Some(cursor) = &request.cursor {
            let (cursor_key, offset) = self
                .cursors
                .get(cursor)
                .ok_or(StorageError::InvalidCursor)?;
            if cursor_key != &key {
                return Err(StorageError::InvalidCursor);
            }
            offset + 1
        } else {
            0
        };
        let total = indices.len();
        let end = (start + request.page_size).min(total);
        let rows: Vec<_> = indices[start..end]
            .iter()
            .map(|index| &self.rows[*index])
            .collect();
        let next_cursor = if end < total {
            rows.last().map(|r| r.cursor.clone())
        } else {
            None
        };
        Ok(StoragePage {
            snapshot_id: self.id.clone(),
            records: rows.iter().map(|r| r.record.clone()).collect(),
            next_cursor,
            retained_total: total,
            completeness: self.completeness.clone(),
        })
    }
}

/// Paging retention owner, not a scan job service. Caller supplies monotonic elapsed time.
/// No mutex is held here, and no filesystem work is performed by this owner.
#[derive(Default)]
pub struct SnapshotPages {
    retained: VecDeque<(Duration, StorageSnapshot)>,
}
impl SnapshotPages {
    pub fn expire(&mut self, now: Duration) {
        self.retained.retain(|(access, _)| {
            now.checked_sub(*access)
                .is_some_and(|age| age < Duration::from_secs(SNAPSHOT_IDLE_SECONDS))
        });
    }
    pub fn insert(&mut self, snapshot: StorageSnapshot, now: Duration) -> Result<(), StorageError> {
        self.expire(now);
        if self.retained.iter().any(|(_, s)| s.id == snapshot.id) {
            return Err(StorageError::InvalidRequest);
        }
        if self.retained.len() == MAX_COMPLETED_SNAPSHOTS {
            self.retained.pop_front();
        }
        self.retained.push_back((now, snapshot));
        Ok(())
    }
    pub fn page(
        &mut self,
        request: &PageRequest,
        now: Duration,
    ) -> Result<StoragePage, StorageError> {
        request.validate()?;
        self.expire(now);
        let (access, snapshot) = self
            .retained
            .iter_mut()
            .find(|(_, s)| s.id == request.snapshot_id)
            .ok_or(StorageError::SnapshotUnavailable)?;
        let result = snapshot.page(request)?;
        *access = now;
        Ok(result)
    }
    pub fn resolve(
        &mut self,
        selection: &StorageSelection,
        now: Duration,
    ) -> Result<Vec<StorageEvidence>, StorageError> {
        selection.validate()?;
        self.expire(now);
        let (access, snapshot) = self
            .retained
            .iter_mut()
            .find(|(_, s)| s.id == selection.snapshot_id && s.module == selection.module)
            .ok_or(StorageError::SnapshotUnavailable)?;
        let mut proofs = selection
            .candidate_ids
            .iter()
            .map(|id| {
                snapshot
                    .candidates
                    .get(id)
                    .cloned()
                    .ok_or(StorageError::InvalidEvidence)
            })
            .collect::<Result<Vec<_>, _>>()?;
        super::duplicates::retain_copy(&mut proofs, snapshot.candidates.values().cloned())?;
        *access = now;
        Ok(proofs)
    }
    pub fn release(&mut self, id: &str) -> Result<(), StorageError> {
        if !valid_id(id) {
            return Err(StorageError::InvalidRequest);
        }
        let before = self.retained.len();
        self.retained.retain(|(_, s)| s.id != id);
        if before == self.retained.len() {
            return Err(StorageError::SnapshotUnavailable);
        }
        Ok(())
    }
}
