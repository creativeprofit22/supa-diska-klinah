use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::Mutex,
};

use cleanup_core::system_change::{JOURNAL_VERSION, Journal};
use windows_sys::Win32::Storage::FileSystem::{
    MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
};

const MAX_JOURNAL_BYTES: u64 = 8 * 1024 * 1024;

pub trait JournalStore: Send + Sync {
    fn load(&self) -> io::Result<Journal>;
    fn save(&self, journal: &Journal) -> io::Result<()>;
}

/// Versioned JSON journal at `<app data>\system-changes\journal.json`,
/// replaced atomically with write-through.
pub struct FileJournalStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl FileJournalStore {
    pub fn open(app_data: &Path) -> io::Result<Self> {
        let directory = app_data.join("system-changes");
        fs::create_dir_all(&directory)?;
        Ok(Self {
            path: directory.join("journal.json"),
            lock: Mutex::new(()),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Renames the journal to `journal.corrupt-<unix-seconds>[-<random>].json`
    /// without replacing any existing file.
    fn move_aside(&self) -> io::Result<PathBuf> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| io::Error::other("journal has no parent"))?;
        let stamp = super::unix_now();
        let first = parent.join(format!("journal.corrupt-{stamp}.json"));
        match move_file(&self.path, &first, MOVEFILE_WRITE_THROUGH) {
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let fallback = parent.join(format!(
                    "journal.corrupt-{stamp}-{}.json",
                    super::random_id()?
                ));
                move_file(&self.path, &fallback, MOVEFILE_WRITE_THROUGH).map(|()| fallback)
            }
            result => result.map(|()| first),
        }
    }
}

fn written_by_newer_version(bytes: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(bytes)
        .ok()
        .and_then(|value| value.get("version")?.as_u64())
        .is_some_and(|version| version > u64::from(JOURNAL_VERSION))
}

impl JournalStore for FileJournalStore {
    fn load(&self) -> io::Result<Journal> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| io::Error::other("journal lock poisoned"))?;
        match fs::metadata(&self.path) {
            Ok(metadata) if metadata.len() > MAX_JOURNAL_BYTES => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "journal too large",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Journal::empty()),
            Err(error) => return Err(error),
        }
        let bytes = fs::read(&self.path)?;
        match Journal::from_json(&bytes) {
            Ok(journal) => Ok(journal),
            // A newer app wrote it: fail closed so this build never
            // overwrites undo data it cannot read.
            Err(error) if written_by_newer_version(&bytes) => {
                Err(io::Error::new(io::ErrorKind::InvalidData, error))
            }
            // Damaged or older: keep the evidence beside the journal and
            // start a fresh one. If moving it aside fails, stay closed.
            Err(_) => {
                self.move_aside()?;
                Ok(Journal::empty())
            }
        }
    }

    fn save(&self, journal: &Journal) -> io::Result<()> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| io::Error::other("journal lock poisoned"))?;
        let bytes = serde_json::to_vec(journal).map_err(io::Error::other)?;
        if bytes.len() as u64 > MAX_JOURNAL_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "journal too large",
            ));
        }
        let parent = self
            .path
            .parent()
            .ok_or_else(|| io::Error::other("journal has no parent"))?;
        let temporary = parent.join(format!(".journal-{}.tmp", super::random_id()?));
        let written = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .and_then(|mut file| file.write_all(&bytes).and_then(|()| file.sync_all()));
        if let Err(error) = written.and_then(|()| move_replace(&temporary, &self.path)) {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        Ok(())
    }
}

fn move_replace(source: &Path, destination: &Path) -> io::Result<()> {
    move_file(
        source,
        destination,
        MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
    )
}

fn move_file(
    source: &Path,
    destination: &Path,
    flags: windows_sys::Win32::Storage::FileSystem::MOVE_FILE_FLAGS,
) -> io::Result<()> {
    let wide =
        |path: &Path| -> Vec<u16> { path.as_os_str().encode_wide().chain(Some(0)).collect() };
    let (source, destination) = (wide(source), wide(destination));
    // SAFETY: both are valid NUL-terminated UTF-16 strings alive for the call.
    if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), flags) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// In-memory journal for tests; can be told to fail saves.
#[derive(Default)]
pub struct MemoryJournalStore {
    journal: Mutex<Option<Journal>>,
    fail_saves_after: Mutex<Option<usize>>,
}

impl MemoryJournalStore {
    pub fn failing_after(saves: usize) -> Self {
        Self {
            journal: Mutex::default(),
            fail_saves_after: Mutex::new(Some(saves)),
        }
    }

    pub fn snapshot(&self) -> Journal {
        self.journal
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(Journal::empty)
    }
}

impl JournalStore for MemoryJournalStore {
    fn load(&self) -> io::Result<Journal> {
        Ok(self.snapshot())
    }

    fn save(&self, journal: &Journal) -> io::Result<()> {
        let mut remaining = self.fail_saves_after.lock().unwrap();
        if let Some(count) = remaining.as_mut() {
            if *count == 0 {
                return Err(io::Error::other("injected save failure"));
            }
            *count -= 1;
        }
        *self.journal.lock().unwrap() = Some(journal.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cleanup_core::system_change::{ChangeOutcome, JournalEntry, PriorState, SystemChange};

    #[test]
    fn file_store_round_trips_and_starts_empty() {
        let root = std::env::temp_dir().join(format!(
            "sdk-journal-{}",
            super::super::random_id().unwrap()
        ));
        let store = FileJournalStore::open(&root).unwrap();
        assert_eq!(store.load().unwrap(), Journal::empty());
        let change = SystemChange::SetHibernation { enabled: false };
        let mut journal = Journal::empty();
        journal.record_intent(JournalEntry {
            id: "a".into(),
            plan_id: "p".into(),
            recorded_at: 1,
            reversibility: change.reversibility(),
            inverse: None,
            change,
            prior: PriorState::Enabled { enabled: true },
            outcome: Some(ChangeOutcome::Applied),
            rolled_back_by: None,
        });
        store.save(&journal).unwrap();
        store.save(&journal).unwrap();
        assert_eq!(store.load().unwrap(), journal);
        fs::write(store.path(), b"{\"version\":9,\"entries\":[]}").unwrap();
        assert!(store.load().is_err());
        fs::remove_dir_all(root).unwrap();
    }

    fn temp_store() -> (PathBuf, FileJournalStore) {
        let root = std::env::temp_dir().join(format!(
            "sdk-journal-{}",
            super::super::random_id().unwrap()
        ));
        let store = FileJournalStore::open(&root).unwrap();
        (root, store)
    }

    fn corrupt_files(store: &FileJournalStore) -> Vec<(String, Vec<u8>)> {
        let mut found: Vec<_> = fs::read_dir(store.path().parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap())
            .filter_map(|entry| {
                let name = entry.file_name().into_string().unwrap();
                (name.starts_with("journal.corrupt-") && name.ends_with(".json"))
                    .then(|| (name, fs::read(entry.path()).unwrap()))
            })
            .collect();
        found.sort();
        found
    }

    #[test]
    fn malformed_journal_is_moved_aside_and_loads_empty() {
        let (root, store) = temp_store();
        fs::write(store.path(), b"{not json").unwrap();
        assert_eq!(store.load().unwrap(), Journal::empty());
        assert!(!store.path().exists());
        assert_eq!(
            corrupt_files(&store)
                .into_iter()
                .map(|(_, bytes)| bytes)
                .collect::<Vec<_>>(),
            vec![b"{not json".to_vec()]
        );
        // A second damaged file in the same second never overwrites the first.
        fs::write(store.path(), b"\0\0").unwrap();
        assert_eq!(store.load().unwrap(), Journal::empty());
        let files = corrupt_files(&store);
        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|(_, bytes)| bytes == b"{not json"));
        assert!(files.iter().any(|(_, bytes)| bytes == b"\0\0"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn older_version_journal_is_moved_aside() {
        let (root, store) = temp_store();
        fs::write(store.path(), b"{\"version\":0,\"entries\":[]}").unwrap();
        assert_eq!(store.load().unwrap(), Journal::empty());
        assert_eq!(corrupt_files(&store).len(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn newer_version_journal_fails_closed_and_is_left_untouched() {
        let (root, store) = temp_store();
        let newer = b"{\"version\":99,\"entries\":[]}";
        fs::write(store.path(), newer).unwrap();
        let error = store.load().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(fs::read(store.path()).unwrap(), newer);
        assert!(corrupt_files(&store).is_empty());
        fs::remove_dir_all(root).unwrap();
    }
}
