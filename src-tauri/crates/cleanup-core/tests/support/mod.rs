use cleanup_core::*;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, SystemTime},
};

#[derive(Default)]
struct State {
    metadata: HashMap<PathBuf, EntryMetadata>,
    children: HashMap<PathBuf, Vec<DirectoryEntry>>,
    unreadable: HashSet<PathBuf>,
    changed_after: HashMap<PathBuf, (usize, FixtureChange)>,
    appear_after_read: HashMap<PathBuf, (usize, PathBuf, EntryKind, u64)>,
    directory_reads: HashMap<PathBuf, usize>,
    calls: HashMap<PathBuf, usize>,
    disappear_on_enumeration: HashSet<PathBuf>,
    replace_on_enumeration: HashMap<PathBuf, EntryKind>,
}

#[derive(Clone, Copy)]
pub enum FixtureChange {
    Size,
    Modified,
    Identity,
    Type,
    Disappear,
}

#[derive(Default)]
pub struct FixtureFs {
    state: Mutex<State>,
    next_id: AtomicU64,
    io_calls: AtomicUsize,
    enumerated_entries: AtomicUsize,
    cancel_after: Mutex<Option<(usize, CancellationToken)>>,
    active: AtomicUsize,
    max_active: AtomicUsize,
    delay_ms: AtomicU64,
}

impl FixtureFs {
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            ..Self::default()
        }
    }
    pub fn directory(&self, path: impl Into<PathBuf>) {
        self.insert(path.into(), EntryKind::Directory, 0);
    }
    pub fn file(&self, path: impl Into<PathBuf>, size: u64) {
        self.insert(path.into(), EntryKind::File, size);
    }
    pub fn link(&self, path: impl Into<PathBuf>) {
        self.insert(path.into(), EntryKind::LinkLike, 0);
    }
    fn insert(&self, path: PathBuf, kind: EntryKind, size: u64) {
        let identity = FileIdentity {
            volume: 1,
            file: self.next_id.fetch_add(1, Ordering::Relaxed),
        };
        let metadata = EntryMetadata {
            kind,
            identity: Some(identity),
            size,
            modified: Some(SystemTime::UNIX_EPOCH),
        };
        let mut state = self.state.lock().unwrap();
        state.metadata.insert(path.clone(), metadata.clone());
        if let Some(parent) = path.parent() {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            state
                .children
                .entry(parent.to_path_buf())
                .or_default()
                .push(DirectoryEntry {
                    path,
                    name,
                    kind,
                    identity: metadata.identity,
                });
        }
    }
    pub fn make_unreadable(&self, path: impl Into<PathBuf>) {
        self.state.lock().unwrap().unreadable.insert(path.into());
    }
    pub fn change_after(&self, path: impl Into<PathBuf>, calls: usize, change: FixtureChange) {
        self.state
            .lock()
            .unwrap()
            .changed_after
            .insert(path.into(), (calls, change));
    }
    pub fn appear_after_read(
        &self,
        parent: impl Into<PathBuf>,
        reads: usize,
        path: impl Into<PathBuf>,
        kind: EntryKind,
        size: u64,
    ) {
        self.state
            .lock()
            .unwrap()
            .appear_after_read
            .insert(parent.into(), (reads, path.into(), kind, size));
    }
    pub fn alias_identity(&self, path: &Path, other: &Path) {
        let mut state = self.state.lock().unwrap();
        let identity = state.metadata[other].identity;
        state.metadata.get_mut(path).unwrap().identity = identity;
        if let Some(parent) = path.parent()
            && let Some(entry) = state
                .children
                .get_mut(parent)
                .and_then(|entries| entries.iter_mut().find(|entry| entry.path == path))
        {
            entry.identity = identity;
        }
    }
    pub fn disappear_on_enumeration(&self, path: impl Into<PathBuf>) {
        self.state
            .lock()
            .unwrap()
            .disappear_on_enumeration
            .insert(path.into());
    }
    pub fn replace_on_enumeration(&self, path: impl Into<PathBuf>, kind: EntryKind) {
        self.state
            .lock()
            .unwrap()
            .replace_on_enumeration
            .insert(path.into(), kind);
    }
    pub fn delay_reads(&self, milliseconds: u64) {
        self.delay_ms.store(milliseconds, Ordering::Release);
    }
    pub fn max_active(&self) -> usize {
        self.max_active.load(Ordering::Acquire)
    }
    pub fn cancel_during_enumeration(&self, after: usize, token: CancellationToken) {
        *self.cancel_after.lock().unwrap() = Some((after, token));
    }
    pub fn enumerated_entries(&self) -> usize {
        self.enumerated_entries.load(Ordering::Acquire)
    }
    fn enter(&self) -> Active<'_> {
        self.io_calls.fetch_add(1, Ordering::AcqRel);
        let active = self.active.fetch_add(1, Ordering::AcqRel) + 1;
        self.max_active.fetch_max(active, Ordering::AcqRel);
        let delay = self.delay_ms.load(Ordering::Acquire);
        if delay > 0 {
            thread::sleep(Duration::from_millis(delay));
        }
        Active(self)
    }
}
struct Active<'a>(&'a FixtureFs);
impl Drop for Active<'_> {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::AcqRel);
    }
}

impl FileSystem for FixtureFs {
    fn semantics(&self) -> PathSemantics {
        PathSemantics::CaseInsensitive
    }
    fn metadata_no_follow(&self, path: &Path) -> Result<EntryMetadata, FsError> {
        let _active = self.enter();
        let mut state = self.state.lock().unwrap();
        let changed_after = state.changed_after.get(path).copied();
        let calls = state.calls.entry(path.to_path_buf()).or_default();
        *calls += 1;
        let call = *calls;
        let mut metadata = state
            .metadata
            .get(path)
            .cloned()
            .ok_or_else(|| FsError::new(FsErrorKind::NotFound, "missing fixture path"))?;
        if let Some((limit, change)) = changed_after
            && call >= limit
        {
            match change {
                FixtureChange::Size => metadata.size = metadata.size.saturating_add(1),
                FixtureChange::Modified => {
                    metadata.modified = Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1));
                }
                FixtureChange::Identity => {
                    metadata.identity = Some(FileIdentity {
                        volume: 1,
                        file: u64::MAX,
                    });
                }
                FixtureChange::Type => {
                    metadata.kind = match metadata.kind {
                        EntryKind::File => EntryKind::Directory,
                        EntryKind::Directory | EntryKind::LinkLike => EntryKind::File,
                    };
                }
                FixtureChange::Disappear => {
                    return Err(FsError::new(
                        FsErrorKind::NotFound,
                        "fixture path disappeared",
                    ));
                }
            }
        }
        Ok(metadata)
    }
    fn canonicalize(&self, path: &Path) -> Result<PathBuf, FsError> {
        let _active = self.enter();
        if self.state.lock().unwrap().metadata.contains_key(path) {
            Ok(path.to_path_buf())
        } else {
            Err(FsError::new(FsErrorKind::NotFound, "missing fixture path"))
        }
    }
    fn read_dir(
        &self,
        path: &Path,
        expected_identity: FileIdentity,
        visitor: &mut dyn FnMut(DirectoryEntry) -> ReadDirControl,
    ) -> Result<(), FsError> {
        let _active = self.enter();
        let state = self.state.lock().unwrap();
        if state
            .metadata
            .get(path)
            .and_then(|metadata| metadata.identity)
            != Some(expected_identity)
        {
            return Err(FsError::new(
                FsErrorKind::Changed,
                "fixture directory changed",
            ));
        }
        drop(state);
        let mut index = 0;
        loop {
            let entry = {
                let state = self.state.lock().unwrap();
                if state.unreadable.contains(path) {
                    return Err(FsError::new(
                        FsErrorKind::PermissionDenied,
                        "fixture denied",
                    ));
                }
                state
                    .children
                    .get(path)
                    .and_then(|entries| entries.get(index))
                    .cloned()
            };
            let Some(entry) = entry else {
                let pending = {
                    let mut state = self.state.lock().unwrap();
                    let reads = state.directory_reads.entry(path.to_path_buf()).or_default();
                    *reads += 1;
                    let reads = *reads;
                    if state
                        .appear_after_read
                        .get(path)
                        .is_some_and(|(after, ..)| reads >= *after)
                    {
                        state.appear_after_read.remove(path)
                    } else {
                        None
                    }
                };
                if let Some((_, child_path, kind, size)) = pending {
                    self.insert(child_path, kind, size);
                }
                return Ok(());
            };
            index += 1;
            let enumerated = self.enumerated_entries.fetch_add(1, Ordering::AcqRel) + 1;
            if let Some((after, token)) = &*self.cancel_after.lock().unwrap()
                && enumerated >= *after
            {
                token.cancel();
            }
            let control = visitor(entry.clone());
            let mut state = self.state.lock().unwrap();
            if state.disappear_on_enumeration.remove(&entry.path) {
                state.metadata.remove(&entry.path);
            }
            if let Some(kind) = state.replace_on_enumeration.remove(&entry.path) {
                let replacement_identity = FileIdentity {
                    volume: 1,
                    file: self.next_id.fetch_add(1, Ordering::Relaxed),
                };
                state.metadata.insert(
                    entry.path,
                    EntryMetadata {
                        kind,
                        identity: Some(replacement_identity),
                        size: 0,
                        modified: Some(SystemTime::UNIX_EPOCH),
                    },
                );
            }
            drop(state);
            if control == ReadDirControl::Stop {
                return Ok(());
            }
        }
    }
}

#[derive(Default)]
pub struct CounterEntropy(AtomicU64);
impl Entropy for CounterEntropy {
    fn fill(&self, bytes: &mut [u8]) -> Result<(), FsError> {
        let value = self.0.fetch_add(1, Ordering::AcqRel).to_le_bytes();
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = value[index % value.len()];
        }
        Ok(())
    }
}
