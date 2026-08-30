use cleanup_core::ResolvedCandidate;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use windows_sys::Win32::Storage::FileSystem::{
    MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
};

use super::{filesystem::wide, recycle::RecycleItem};

pub const MAX_ITEMS: usize = 1_000;
const MAX_RECORD_BYTES: u64 = 8 * 1024 * 1024;
const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CleanupDisposition {
    RecycleBin,
    Quarantine,
    Permanent,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanItem {
    pub item_id: String,
    pub proof: ResolvedCandidate,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CleanupPlan {
    pub schema_version: u32,
    pub plan_id: String,
    pub scan_id: String,
    pub created_at: u64,
    pub disposition: CleanupDisposition,
    pub items: Vec<PlanItem>,
}

impl CleanupPlan {
    pub fn new(
        plan_id: String,
        scan_id: String,
        created_at: u64,
        disposition: CleanupDisposition,
        items: Vec<PlanItem>,
    ) -> Result<Self, StorageError> {
        let plan = Self {
            schema_version: SCHEMA_VERSION,
            plan_id,
            scan_id,
            created_at,
            disposition,
            items,
        };
        plan.validate()?;
        Ok(plan)
    }

    pub fn validate(&self) -> Result<(), StorageError> {
        if self.schema_version != SCHEMA_VERSION
            || !valid_id(&self.plan_id)
            || !valid_id(&self.scan_id)
            || self.items.is_empty()
            || self.items.len() > MAX_ITEMS
            || self.items.iter().any(|item| {
                !valid_id(&item.item_id)
                    || !item.proof.path.is_absolute()
                    || !item.proof.scan_root.is_absolute()
                    || !item.proof.context_root.is_absolute()
            })
        {
            return Err(StorageError::Invalid);
        }
        let mut ids = std::collections::HashSet::new();
        if !self.items.iter().all(|item| ids.insert(&item.item_id)) {
            return Err(StorageError::Invalid);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ItemState {
    Pending,
    Mutating,
    Recycled,
    Quarantined,
    Purged,
    Restored,
    Failed,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionItem {
    pub item_id: String,
    pub state: ItemState,
    pub logical_bytes: u64,
    pub processed: bool,
    pub occupied_bytes: u64,
    pub reclaimed_bytes: u64,
    pub quarantine_path: Option<PathBuf>,
    pub recycle_item: Option<RecycleItem>,
    pub failure: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ByteAccounting {
    pub selected_bytes: u64,
    pub processed_bytes: u64,
    pub failed_bytes: u64,
    pub quarantined_bytes: u64,
    pub purged_bytes: u64,
    pub occupied_bytes: u64,
    pub reclaimed_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionJournal {
    pub schema_version: u32,
    pub execution_id: String,
    pub plan_id: String,
    pub started_at: u64,
    pub completed_at: Option<u64>,
    pub disposition: CleanupDisposition,
    pub purge_after: Option<u64>,
    pub items: Vec<ExecutionItem>,
    pub accounting: ByteAccounting,
}

impl ExecutionJournal {
    pub fn validate(&self) -> Result<(), StorageError> {
        if self.schema_version != SCHEMA_VERSION
            || !valid_id(&self.execution_id)
            || !valid_id(&self.plan_id)
            || self.items.is_empty()
            || self.items.len() > MAX_ITEMS
            || self.items.iter().any(|item| !valid_id(&item.item_id))
        {
            Err(StorageError::Invalid)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutoCleanupPolicy {
    pub schema_version: u32,
    pub enabled: bool,
    pub grace_days: u16,
}

impl Default for AutoCleanupPolicy {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            enabled: false,
            grace_days: 7,
        }
    }
}

impl AutoCleanupPolicy {
    pub fn validate(&self) -> Result<(), StorageError> {
        if self.schema_version == SCHEMA_VERSION && (1..=30).contains(&self.grace_days) {
            Ok(())
        } else {
            Err(StorageError::Invalid)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageError {
    Io,
    Invalid,
    TooLarge,
    Exists,
}

#[derive(Clone, Debug)]
pub struct CleanupStorage {
    root: PathBuf,
}

impl CleanupStorage {
    pub fn open(root: PathBuf) -> Result<Self, StorageError> {
        fs::create_dir_all(root.join("plans")).map_err(|_| StorageError::Io)?;
        fs::create_dir_all(root.join("executions")).map_err(|_| StorageError::Io)?;
        fs::create_dir_all(root.join("quarantine")).map_err(|_| StorageError::Io)?;
        let root = fs::canonicalize(root).map_err(|_| StorageError::Io)?;
        Ok(Self { root })
    }

    pub fn create_plan(&self, plan: &CleanupPlan) -> Result<(), StorageError> {
        plan.validate()?;
        write_json(&self.id_path("plans", &plan.plan_id)?, plan, false)
    }

    pub fn read_plan(&self, plan_id: &str) -> Result<CleanupPlan, StorageError> {
        let plan: CleanupPlan = read_json(&self.id_path("plans", plan_id)?)?;
        plan.validate()?;
        if plan.plan_id != plan_id {
            return Err(StorageError::Invalid);
        }
        Ok(plan)
    }

    pub fn write_execution(&self, journal: &ExecutionJournal) -> Result<(), StorageError> {
        journal.validate()?;
        write_json(
            &self.id_path("executions", &journal.execution_id)?,
            journal,
            true,
        )
    }

    pub fn read_execution(&self, execution_id: &str) -> Result<ExecutionJournal, StorageError> {
        let journal: ExecutionJournal = read_json(&self.id_path("executions", execution_id)?)?;
        journal.validate()?;
        if journal.execution_id != execution_id {
            return Err(StorageError::Invalid);
        }
        Ok(journal)
    }

    pub fn reconcile(
        &self,
        plan: &CleanupPlan,
        journal: &mut ExecutionJournal,
    ) -> Result<(), StorageError> {
        plan.validate()?;
        journal.validate()?;
        if plan.plan_id != journal.plan_id || plan.items.len() != journal.items.len() {
            return Err(StorageError::Invalid);
        }
        for item in &mut journal.items {
            if item.state != ItemState::Mutating {
                continue;
            }
            let source = plan
                .items
                .iter()
                .find(|planned| planned.item_id == item.item_id)
                .ok_or(StorageError::Invalid)?
                .proof
                .path
                .as_path();
            item.state = if fs::symlink_metadata(source).is_ok() {
                ItemState::Pending
            } else if item
                .quarantine_path
                .as_ref()
                .is_some_and(|path| fs::symlink_metadata(path).is_ok())
            {
                ItemState::Quarantined
            } else {
                ItemState::Unknown
            };
        }
        self.write_execution(journal)
    }

    pub fn executions(&self) -> Result<Vec<ExecutionJournal>, StorageError> {
        let mut output = Vec::new();
        for entry in fs::read_dir(self.root.join("executions")).map_err(|_| StorageError::Io)? {
            let entry = entry.map_err(|_| StorageError::Io)?;
            if entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                let journal: ExecutionJournal = read_json(&entry.path())?;
                journal.validate()?;
                output.push(journal);
                if output.len() > MAX_ITEMS {
                    return Err(StorageError::TooLarge);
                }
            }
        }
        output.sort_by_key(|journal| std::cmp::Reverse(journal.started_at));
        Ok(output)
    }

    pub fn policy(&self) -> Result<AutoCleanupPolicy, StorageError> {
        let path = self.root.join("policy.json");
        if !path.exists() {
            return Ok(AutoCleanupPolicy::default());
        }
        let policy: AutoCleanupPolicy = read_json(&path)?;
        policy.validate()?;
        Ok(policy)
    }

    pub fn write_policy(&self, policy: &AutoCleanupPolicy) -> Result<(), StorageError> {
        policy.validate()?;
        write_json(&self.root.join("policy.json"), policy, true)
    }

    pub fn quarantine_directory(
        &self,
        execution_id: &str,
        item_id: &str,
    ) -> Result<PathBuf, StorageError> {
        if !valid_id(execution_id) || !valid_id(item_id) {
            return Err(StorageError::Invalid);
        }
        let directory = self.root.join("quarantine").join(execution_id);
        fs::create_dir_all(&directory).map_err(|_| StorageError::Io)?;
        Ok(directory.join(item_id))
    }

    fn id_path(&self, directory: &str, id: &str) -> Result<PathBuf, StorageError> {
        if !valid_id(id) {
            return Err(StorageError::Invalid);
        }
        Ok(self.root.join(directory).join(format!("{id}.json")))
    }
}

fn valid_id(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, StorageError> {
    let file = File::open(path).map_err(|_| StorageError::Io)?;
    if file.metadata().map_err(|_| StorageError::Io)?.len() > MAX_RECORD_BYTES {
        return Err(StorageError::TooLarge);
    }
    let mut bytes = Vec::new();
    file.take(MAX_RECORD_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| StorageError::Io)?;
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return Err(StorageError::TooLarge);
    }
    serde_json::from_slice(&bytes).map_err(|_| StorageError::Invalid)
}

fn write_json<T: Serialize>(path: &Path, value: &T, replace: bool) -> Result<(), StorageError> {
    let bytes = serde_json::to_vec(value).map_err(|_| StorageError::Invalid)?;
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return Err(StorageError::TooLarge);
    }
    if !replace && path.exists() {
        return Err(StorageError::Exists);
    }
    let parent = path.parent().ok_or(StorageError::Invalid)?;
    let mut nonce = [0_u8; 16];
    getrandom::fill(&mut nonce).map_err(|_| StorageError::Io)?;
    let temporary = parent.join(format!(
        ".tmp-{}",
        nonce
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|_| StorageError::Io)?;
    if file
        .write_all(&bytes)
        .and_then(|()| file.sync_all())
        .is_err()
    {
        let _ = fs::remove_file(&temporary);
        return Err(StorageError::Io);
    }
    let result = if replace {
        move_replace(&temporary, path)
    } else {
        fs::rename(&temporary, path).map_err(|_| StorageError::Io)
    };
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
        return result;
    }
    Ok(())
}

fn move_replace(source: &Path, destination: &Path) -> Result<(), StorageError> {
    let source = wide(source.as_os_str()).map_err(|_| StorageError::Invalid)?;
    let destination = wide(destination.as_os_str()).map_err(|_| StorageError::Invalid)?;
    // SAFETY: both paths are valid, NUL-terminated UTF-16 strings.
    if unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        Err(StorageError::Io)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "supa-diska-storage-{}-{}",
            std::process::id(),
            getrandom::u64().unwrap()
        ));
        fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn policy_is_atomic_persistent_and_bounded() {
        let root = temp();
        let storage = CleanupStorage::open(root.clone()).unwrap();
        assert_eq!(storage.policy().unwrap(), AutoCleanupPolicy::default());
        let policy = AutoCleanupPolicy {
            enabled: true,
            grace_days: 14,
            ..AutoCleanupPolicy::default()
        };
        storage.write_policy(&policy).unwrap();
        assert_eq!(
            CleanupStorage::open(root.clone())
                .unwrap()
                .policy()
                .unwrap(),
            policy
        );
        assert!(
            storage
                .write_policy(&AutoCleanupPolicy {
                    grace_days: 0,
                    ..policy
                })
                .is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn interrupted_mutations_reconcile_without_assuming_success() {
        use cleanup_core::{
            CatalogLimits, EntryKind, FileIdentity, ResolvedCandidate, load_catalog,
        };
        use std::{io::Cursor, time::SystemTime};

        let root = temp();
        let source = root.join("source");
        fs::create_dir(&source).unwrap();
        let quarantine = root.join("quarantined");
        let rule_json = r#"{"schemaVersion":1,"rules":[{"id":"cache","ruleVersion":1,"lifecycle":"stable","risk":"safe","provenance":{"source":"test","verifiedAt":"2026-08-30"},"defaultSelected":false,"scanner":"direct","roots":[{"binding":"temp","suffix":""}],"markers":{},"targets":["source"],"targetType":"directory","rootDepth":1}]}"#;
        let rule = load_catalog(Cursor::new(rule_json), CatalogLimits::default())
            .unwrap()
            .rules()[0]
            .clone();
        let item_id = "3".repeat(32);
        let plan = CleanupPlan::new(
            "1".repeat(32),
            "2".repeat(32),
            1,
            CleanupDisposition::Quarantine,
            vec![PlanItem {
                item_id: item_id.clone(),
                proof: ResolvedCandidate {
                    path: source.clone(),
                    scan_root: root.clone(),
                    context_root: root.clone(),
                    rule,
                    identity: FileIdentity { volume: 1, file: 1 },
                    kind: EntryKind::Directory,
                    logical_bytes: 0,
                    allocated_bytes: 0,
                    scanned_at: SystemTime::UNIX_EPOCH,
                },
            }],
        )
        .unwrap();
        let mut journal = ExecutionJournal {
            schema_version: 1,
            execution_id: "4".repeat(32),
            plan_id: plan.plan_id.clone(),
            started_at: 1,
            completed_at: None,
            disposition: CleanupDisposition::Quarantine,
            purge_after: None,
            items: vec![ExecutionItem {
                item_id,
                state: ItemState::Mutating,
                logical_bytes: 0,
                processed: false,
                occupied_bytes: 0,
                reclaimed_bytes: 0,
                quarantine_path: Some(quarantine.clone()),
                recycle_item: None,
                failure: None,
            }],
            accounting: ByteAccounting::default(),
        };
        let storage = CleanupStorage::open(root.join("records")).unwrap();
        storage.reconcile(&plan, &mut journal).unwrap();
        assert_eq!(journal.items[0].state, ItemState::Pending);

        fs::rename(&source, &quarantine).unwrap();
        journal.items[0].state = ItemState::Mutating;
        storage.reconcile(&plan, &mut journal).unwrap();
        assert_eq!(journal.items[0].state, ItemState::Quarantined);

        fs::remove_dir(&quarantine).unwrap();
        journal.items[0].state = ItemState::Mutating;
        storage.reconcile(&plan, &mut journal).unwrap();
        assert_eq!(journal.items[0].state, ItemState::Unknown);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn immutable_records_and_identifier_boundaries_fail_closed() {
        let root = temp();
        let storage = CleanupStorage::open(root.clone()).unwrap();
        assert_eq!(
            storage.read_plan("../escape").unwrap_err(),
            StorageError::Invalid
        );
        let plan = CleanupPlan::new(
            "1".repeat(32),
            "2".repeat(32),
            1,
            CleanupDisposition::RecycleBin,
            vec![],
        );
        assert_eq!(plan.unwrap_err(), StorageError::Invalid);
        fs::remove_dir_all(root).unwrap();
    }
}
