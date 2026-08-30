use cleanup_core::{FileSystem, ProtectionPolicy, ScanSnapshot, revalidate_candidate};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use super::{
    filesystem::WindowsFileSystem,
    preview::{
        CleanupPreview, PrivateCleanupScan, current_protection, scan_temporary_caches,
        temporary_root, temporary_rule,
    },
    recycle::{RecycleBin, WindowsRecycleBin, recycle_exact, restore_exact},
    storage::{
        AutoCleanupPolicy, ByteAccounting, CleanupDisposition, CleanupPlan, CleanupStorage,
        ExecutionItem, ExecutionJournal, ItemState, MAX_ITEMS, PlanItem, StorageError,
    },
};

const MAX_HISTORY: usize = 100;
const MAX_MUTATION_ENTRIES: usize = 250_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CleanupServiceError {
    InvalidInput,
    NotFound,
    Conflict,
    ValidationFailed,
    PersistenceFailed,
    OperationFailed,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupPlanSummary {
    pub plan_id: String,
    pub disposition: CleanupDisposition,
    pub selected_count: usize,
    pub selected_bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupExecutionSummary {
    pub execution_id: String,
    pub plan_id: String,
    pub disposition: CleanupDisposition,
    pub completed: bool,
    pub purge_after: Option<u64>,
    pub items: Vec<CleanupItemOutcome>,
    pub accounting: ByteAccounting,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupItemOutcome {
    pub item_id: String,
    pub state: ItemState,
    pub logical_bytes: u64,
    pub failure: Option<String>,
}

struct RetainedScan {
    snapshot: ScanSnapshot,
    _protection: ProtectionPolicy,
}

pub struct CleanupService {
    storage: CleanupStorage,
    file_system: WindowsFileSystem,
    recycle_bin: Arc<dyn RecycleBin>,
    scans: Mutex<HashMap<String, RetainedScan>>,
    writer: Mutex<()>,
}

impl CleanupService {
    pub fn new(app_data: PathBuf) -> Result<Self, CleanupServiceError> {
        Self::with_recycle_bin(app_data, Arc::new(WindowsRecycleBin))
    }

    fn with_recycle_bin(
        app_data: PathBuf,
        recycle_bin: Arc<dyn RecycleBin>,
    ) -> Result<Self, CleanupServiceError> {
        let service = Self {
            storage: CleanupStorage::open(app_data.join("cleanup")).map_err(map_storage_error)?,
            file_system: WindowsFileSystem,
            recycle_bin,
            scans: Mutex::new(HashMap::new()),
            writer: Mutex::new(()),
        };
        service.reconcile_interrupted()?;
        Ok(service)
    }

    pub fn preview(&self) -> Result<CleanupPreview, CleanupServiceError> {
        let PrivateCleanupScan {
            preview,
            snapshot,
            protection,
        } = scan_temporary_caches().map_err(|_| CleanupServiceError::OperationFailed)?;
        let mut scans = self
            .scans
            .lock()
            .map_err(|_| CleanupServiceError::Conflict)?;
        if scans.len() >= 4 {
            scans.clear();
        }
        scans.insert(
            preview.scan_id.clone(),
            RetainedScan {
                snapshot,
                _protection: protection,
            },
        );
        Ok(preview)
    }

    pub fn create_plan(
        &self,
        scan_id: &str,
        candidate_ids: &[String],
        disposition: CleanupDisposition,
    ) -> Result<CleanupPlanSummary, CleanupServiceError> {
        if !valid_id(scan_id)
            || candidate_ids.is_empty()
            || candidate_ids.len() > MAX_ITEMS
            || candidate_ids.iter().any(|id| !valid_id(id))
        {
            return Err(CleanupServiceError::InvalidInput);
        }
        let scans = self
            .scans
            .lock()
            .map_err(|_| CleanupServiceError::Conflict)?;
        let scan = scans.get(scan_id).ok_or(CleanupServiceError::NotFound)?;
        let mut selected = HashSet::new();
        let mut items = Vec::with_capacity(candidate_ids.len());
        for candidate_id in candidate_ids {
            if !selected.insert(candidate_id) {
                return Err(CleanupServiceError::InvalidInput);
            }
            let proof = scan
                .snapshot
                .resolve(candidate_id)
                .ok_or(CleanupServiceError::NotFound)?
                .clone();
            items.push(PlanItem {
                item_id: candidate_id.clone(),
                proof,
            });
        }
        let selected_bytes = items
            .iter()
            .try_fold(0_u64, |total, item| {
                total.checked_add(item.proof.logical_bytes)
            })
            .ok_or(CleanupServiceError::InvalidInput)?;
        let plan_id = random_id()?;
        let plan = CleanupPlan::new(
            plan_id.clone(),
            scan_id.to_owned(),
            now_seconds()?,
            disposition,
            items,
        )
        .map_err(map_storage_error)?;
        self.storage.create_plan(&plan).map_err(map_storage_error)?;
        Ok(CleanupPlanSummary {
            plan_id,
            disposition,
            selected_count: plan.items.len(),
            selected_bytes,
        })
    }

    pub fn execute(&self, plan_id: &str) -> Result<CleanupExecutionSummary, CleanupServiceError> {
        self.execute_inner(plan_id, false)
    }

    pub fn execute_permanent(
        &self,
        plan_id: &str,
    ) -> Result<CleanupExecutionSummary, CleanupServiceError> {
        self.execute_inner(plan_id, true)
    }

    fn execute_inner(
        &self,
        plan_id: &str,
        permanent_command: bool,
    ) -> Result<CleanupExecutionSummary, CleanupServiceError> {
        let _writer = self
            .writer
            .lock()
            .map_err(|_| CleanupServiceError::Conflict)?;
        let plan = self.storage.read_plan(plan_id).map_err(map_storage_error)?;
        if (plan.disposition == CleanupDisposition::Permanent) != permanent_command {
            return Err(CleanupServiceError::InvalidInput);
        }
        let current_rule = temporary_rule().map_err(|_| CleanupServiceError::ValidationFailed)?;
        let expected_root = temporary_root().map_err(|_| CleanupServiceError::ValidationFailed)?;
        if !plan_matches_current_scope(&self.file_system, &plan, &current_rule, &expected_root) {
            return Err(CleanupServiceError::ValidationFailed);
        }
        let execution_id = random_id()?;
        let started_at = now_seconds()?;
        let policy = self.storage.policy().map_err(map_storage_error)?;
        let purge_after = (plan.disposition == CleanupDisposition::Quarantine)
            .then(|| started_at.saturating_add(u64::from(policy.grace_days) * 86_400));
        let selected_bytes = checked_sum(plan.items.iter().map(|item| item.proof.logical_bytes))?;
        let mut journal = ExecutionJournal {
            schema_version: 1,
            execution_id,
            plan_id: plan.plan_id.clone(),
            started_at,
            completed_at: None,
            disposition: plan.disposition,
            purge_after,
            items: plan
                .items
                .iter()
                .map(|item| ExecutionItem {
                    item_id: item.item_id.clone(),
                    state: ItemState::Pending,
                    logical_bytes: item.proof.logical_bytes,
                    processed: false,
                    occupied_bytes: 0,
                    reclaimed_bytes: 0,
                    quarantine_path: None,
                    recycle_item: None,
                    failure: None,
                })
                .collect(),
            accounting: ByteAccounting {
                selected_bytes,
                ..ByteAccounting::default()
            },
        };
        self.storage
            .write_execution(&journal)
            .map_err(map_storage_error)?;

        for index in 0..journal.items.len() {
            journal.items[index].state = ItemState::Mutating;
            if plan.disposition == CleanupDisposition::Quarantine {
                journal.items[index].quarantine_path = Some(
                    self.storage
                        .quarantine_directory(&journal.execution_id, &journal.items[index].item_id)
                        .map_err(map_storage_error)?,
                );
            }
            self.storage
                .write_execution(&journal)
                .map_err(map_storage_error)?;
            let planned = &plan.items[index];
            let protection = match current_protection() {
                Ok(protection) => protection,
                Err(_) => {
                    fail_item(&mut journal.items[index], "protection-unavailable");
                    persist_accounting(&self.storage, &mut journal)?;
                    continue;
                }
            };
            let measured = match revalidate_candidate(
                &self.file_system,
                &planned.proof,
                &protection,
                SystemTime::now(),
            ) {
                Ok(measured) => measured,
                Err(_) => {
                    fail_item(&mut journal.items[index], "revalidation-rejected");
                    persist_accounting(&self.storage, &mut journal)?;
                    continue;
                }
            };
            journal.items[index].processed = true;
            journal.items[index].logical_bytes = measured.logical_bytes;
            journal.items[index].occupied_bytes = measured.allocated_bytes;
            if let Err(reason) = self.mutate_item(&plan, index, &mut journal) {
                fail_item(&mut journal.items[index], reason);
            }
            persist_accounting(&self.storage, &mut journal)?;
        }
        journal.completed_at = Some(now_seconds()?);
        persist_accounting(&self.storage, &mut journal)?;
        Ok(summary(&journal))
    }

    fn mutate_item(
        &self,
        plan: &CleanupPlan,
        index: usize,
        journal: &mut ExecutionJournal,
    ) -> Result<(), &'static str> {
        let planned = &plan.items[index];
        let item = &mut journal.items[index];
        match plan.disposition {
            CleanupDisposition::RecycleBin => {
                let recycled = recycle_exact(self.recycle_bin.as_ref(), &planned.proof.path)
                    .map_err(|_| "recycle-failed")?;
                item.recycle_item = Some(recycled);
                item.state = ItemState::Recycled;
            }
            CleanupDisposition::Quarantine => {
                let destination = item
                    .quarantine_path
                    .clone()
                    .ok_or("quarantine-unavailable")?;
                let directory = destination
                    .parent()
                    .ok_or("quarantine-unavailable")?
                    .to_path_buf();
                if destination.exists() {
                    return Err("quarantine-collision");
                }
                if self
                    .file_system
                    .same_volume(&planned.proof.path, &directory)
                    .map_err(|_| "volume-check-failed")?
                {
                    fs::rename(&planned.proof.path, &destination)
                        .map_err(|_| "quarantine-move-failed")?;
                } else {
                    let staging = directory.join(format!(".{}.staging", item.item_id));
                    self.file_system
                        .copy_tree_no_follow(
                            &planned.proof.path,
                            &staging,
                            MAX_MUTATION_ENTRIES,
                            item.logical_bytes,
                        )
                        .map_err(|_| "quarantine-copy-failed")?;
                    fs::rename(&staging, &destination).map_err(|_| "quarantine-publish-failed")?;
                    let protection = current_protection().map_err(|_| "protection-unavailable")?;
                    revalidate_candidate(
                        &self.file_system,
                        &planned.proof,
                        &protection,
                        SystemTime::now(),
                    )
                    .map_err(|_| "source-revalidation-failed")?;
                    self.file_system
                        .remove_tree_no_follow(&planned.proof.path, MAX_MUTATION_ENTRIES)
                        .map_err(|_| "source-remove-failed")?;
                }
                item.state = ItemState::Quarantined;
            }
            CleanupDisposition::Permanent => {
                let parent = planned.proof.path.parent().ok_or("invalid-parent")?;
                let before = self
                    .file_system
                    .free_space(parent)
                    .map_err(|_| "space-sample-failed")?;
                self.file_system
                    .remove_tree_no_follow(&planned.proof.path, MAX_MUTATION_ENTRIES)
                    .map_err(|_| "permanent-remove-failed")?;
                let after = self
                    .file_system
                    .free_space(parent)
                    .map_err(|_| "space-sample-failed")?;
                item.reclaimed_bytes = after.saturating_sub(before).min(item.occupied_bytes);
                item.state = ItemState::Purged;
            }
        }
        Ok(())
    }

    pub fn undo(&self, execution_id: &str) -> Result<CleanupExecutionSummary, CleanupServiceError> {
        let _writer = self
            .writer
            .lock()
            .map_err(|_| CleanupServiceError::Conflict)?;
        let mut journal = self
            .storage
            .read_execution(execution_id)
            .map_err(map_storage_error)?;
        let plan = self
            .storage
            .read_plan(&journal.plan_id)
            .map_err(map_storage_error)?;
        for index in 0..journal.items.len() {
            {
                let item = &mut journal.items[index];
                match item.state {
                    ItemState::Recycled => {
                        let recycled = item
                            .recycle_item
                            .as_ref()
                            .ok_or(CleanupServiceError::ValidationFailed)?;
                        if restore_exact(self.recycle_bin.as_ref(), recycled).is_ok() {
                            item.state = ItemState::Restored;
                        } else {
                            item.failure = Some("restore-failed".to_owned());
                        }
                    }
                    ItemState::Quarantined => {
                        let source = &plan.items[index].proof.path;
                        let quarantined = item
                            .quarantine_path
                            .as_ref()
                            .ok_or(CleanupServiceError::ValidationFailed)?;
                        if source.exists() {
                            item.failure = Some("restore-collision".to_owned());
                        } else if fs::rename(quarantined, source).is_ok() {
                            item.state = ItemState::Restored;
                        } else {
                            let staging = source.with_extension("cleanup-restore-staging");
                            let restored = self
                                .file_system
                                .copy_tree_no_follow(
                                    quarantined,
                                    &staging,
                                    MAX_MUTATION_ENTRIES,
                                    item.logical_bytes,
                                )
                                .and_then(|_| {
                                    fs::rename(&staging, source)
                                        .map_err(cleanup_core::FsError::from)
                                })
                                .and_then(|_| {
                                    self.file_system
                                        .remove_tree_no_follow(quarantined, MAX_MUTATION_ENTRIES)
                                });
                            if restored.is_ok() {
                                item.state = ItemState::Restored;
                            } else {
                                item.failure = Some("restore-failed".to_owned());
                            }
                        }
                    }
                    _ => {}
                }
            }
            persist_accounting(&self.storage, &mut journal)?;
        }
        Ok(summary(&journal))
    }

    pub fn history(&self) -> Result<Vec<CleanupExecutionSummary>, CleanupServiceError> {
        Ok(self
            .storage
            .executions()
            .map_err(map_storage_error)?
            .into_iter()
            .take(MAX_HISTORY)
            .map(|journal| summary(&journal))
            .collect())
    }

    pub fn policy(&self) -> Result<AutoCleanupPolicy, CleanupServiceError> {
        self.storage.policy().map_err(map_storage_error)
    }

    pub fn set_policy(
        &self,
        enabled: bool,
        grace_days: u16,
    ) -> Result<AutoCleanupPolicy, CleanupServiceError> {
        let policy = AutoCleanupPolicy {
            schema_version: 1,
            enabled,
            grace_days,
        };
        {
            let _writer = self
                .writer
                .lock()
                .map_err(|_| CleanupServiceError::Conflict)?;
            self.storage
                .write_policy(&policy)
                .map_err(map_storage_error)?;
        }
        if enabled {
            self.run_maintenance()?;
        }
        Ok(policy)
    }

    pub fn run_maintenance(&self) -> Result<(), CleanupServiceError> {
        if !self.policy()?.enabled {
            return Ok(());
        }
        let preview = self.preview()?;
        if !preview.records.is_empty() {
            let candidate_ids: Vec<String> = preview
                .records
                .iter()
                .map(|record| record.id.clone())
                .collect();
            let plan = self.create_plan(
                &preview.scan_id,
                &candidate_ids,
                CleanupDisposition::Quarantine,
            )?;
            self.execute(&plan.plan_id)?;
        }
        self.purge_due()
    }

    pub fn purge_due(&self) -> Result<(), CleanupServiceError> {
        let _writer = self
            .writer
            .lock()
            .map_err(|_| CleanupServiceError::Conflict)?;
        let policy = self.storage.policy().map_err(map_storage_error)?;
        if !policy.enabled {
            return Ok(());
        }
        let now = now_seconds()?;
        for mut journal in self.storage.executions().map_err(map_storage_error)? {
            if journal.disposition != CleanupDisposition::Quarantine
                || journal
                    .purge_after
                    .is_none_or(|purge_after| purge_after > now)
            {
                continue;
            }
            for item in &mut journal.items {
                if item.state != ItemState::Quarantined {
                    continue;
                }
                let Some(path) = item.quarantine_path.as_ref() else {
                    item.state = ItemState::Unknown;
                    continue;
                };
                let parent = path.parent().ok_or(CleanupServiceError::ValidationFailed)?;
                let before = self
                    .file_system
                    .free_space(parent)
                    .map_err(|_| CleanupServiceError::OperationFailed)?;
                if self
                    .file_system
                    .remove_tree_no_follow(path, MAX_MUTATION_ENTRIES)
                    .is_ok()
                {
                    let after = self
                        .file_system
                        .free_space(parent)
                        .map_err(|_| CleanupServiceError::OperationFailed)?;
                    item.reclaimed_bytes = after.saturating_sub(before).min(item.occupied_bytes);
                    item.state = ItemState::Purged;
                } else {
                    item.failure = Some("purge-failed".to_owned());
                }
            }
            persist_accounting(&self.storage, &mut journal)?;
        }
        Ok(())
    }

    fn reconcile_interrupted(&self) -> Result<(), CleanupServiceError> {
        for mut journal in self.storage.executions().map_err(map_storage_error)? {
            if journal.completed_at.is_some() {
                continue;
            }
            let plan = self
                .storage
                .read_plan(&journal.plan_id)
                .map_err(map_storage_error)?;
            self.storage
                .reconcile(&plan, &mut journal)
                .map_err(map_storage_error)?;
        }
        Ok(())
    }
}

fn fail_item(item: &mut ExecutionItem, reason: &str) {
    item.state = ItemState::Failed;
    item.failure = Some(reason.to_owned());
}

fn persist_accounting(
    storage: &CleanupStorage,
    journal: &mut ExecutionJournal,
) -> Result<(), CleanupServiceError> {
    let selected_bytes = journal.accounting.selected_bytes;
    journal.accounting = ByteAccounting {
        selected_bytes,
        processed_bytes: checked_sum(
            journal
                .items
                .iter()
                .filter(|item| item.processed)
                .map(|item| item.logical_bytes),
        )?,
        failed_bytes: checked_sum(
            journal
                .items
                .iter()
                .filter(|item| matches!(item.state, ItemState::Failed | ItemState::Unknown))
                .map(|item| item.logical_bytes),
        )?,
        quarantined_bytes: checked_sum(
            journal
                .items
                .iter()
                .filter(|item| item.state == ItemState::Quarantined)
                .map(|item| item.occupied_bytes),
        )?,
        purged_bytes: checked_sum(
            journal
                .items
                .iter()
                .filter(|item| item.state == ItemState::Purged)
                .map(|item| item.occupied_bytes),
        )?,
        occupied_bytes: checked_sum(
            journal
                .items
                .iter()
                .filter(|item| item.processed)
                .map(|item| item.occupied_bytes),
        )?,
        reclaimed_bytes: checked_sum(journal.items.iter().map(|item| item.reclaimed_bytes))?,
    };
    storage.write_execution(journal).map_err(map_storage_error)
}

fn checked_sum(mut values: impl Iterator<Item = u64>) -> Result<u64, CleanupServiceError> {
    values.try_fold(0_u64, |total, value| {
        total
            .checked_add(value)
            .ok_or(CleanupServiceError::ValidationFailed)
    })
}

fn summary(journal: &ExecutionJournal) -> CleanupExecutionSummary {
    CleanupExecutionSummary {
        execution_id: journal.execution_id.clone(),
        plan_id: journal.plan_id.clone(),
        disposition: journal.disposition,
        completed: journal.completed_at.is_some(),
        purge_after: journal.purge_after,
        items: journal
            .items
            .iter()
            .map(|item| CleanupItemOutcome {
                item_id: item.item_id.clone(),
                state: item.state,
                logical_bytes: item.logical_bytes,
                failure: item.failure.clone(),
            })
            .collect(),
        accounting: journal.accounting.clone(),
    }
}

fn valid_id(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn plan_matches_current_scope(
    file_system: &dyn FileSystem,
    plan: &CleanupPlan,
    current_rule: &cleanup_core::CleanupRule,
    expected_root: &Path,
) -> bool {
    let semantics = file_system.semantics();
    plan.items.iter().all(|item| {
        item.proof.rule == *current_rule
            && semantics.equivalent(&item.proof.scan_root, expected_root)
            && semantics.equivalent(&item.proof.context_root, expected_root)
    })
}

fn random_id() -> Result<String, CleanupServiceError> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| CleanupServiceError::OperationFailed)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn now_seconds() -> Result<u64, CleanupServiceError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| CleanupServiceError::OperationFailed)
}

fn map_storage_error(error: StorageError) -> CleanupServiceError {
    match error {
        StorageError::Invalid | StorageError::TooLarge => CleanupServiceError::ValidationFailed,
        StorageError::Exists => CleanupServiceError::Conflict,
        StorageError::Io => CleanupServiceError::PersistenceFailed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "moves only owned disposable fixtures through the real Windows Recycle Bin"]
    fn disposable_recycle_quarantine_purge_and_permanent_drill() {
        for orphan in WindowsRecycleBin
            .list()
            .unwrap()
            .into_iter()
            .filter(|item| {
                item.original_path().components().any(|component| {
                    component
                        .as_os_str()
                        .to_string_lossy()
                        .starts_with("supa-diska-destructive-drill-")
                })
            })
        {
            let orphan_root = orphan
                .original_path()
                .parent()
                .and_then(Path::parent)
                .map(Path::to_path_buf);
            let _ = restore_exact(&WindowsRecycleBin, &orphan);
            if let Some(orphan_root) = orphan_root {
                let _ = std::fs::remove_dir_all(orphan_root);
            }
        }
        let root = std::env::temp_dir().join(format!(
            "supa-diska-destructive-drill-{}-{}",
            std::process::id(),
            getrandom::u64().unwrap()
        ));
        let app_data = root.join("app-data");
        std::fs::create_dir_all(&app_data).unwrap();
        let candidate = |name: &str| {
            let path = root.join(name).join("cache");
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(path.join("payload"), name).unwrap();
            std::fs::canonicalize(path).unwrap()
        };
        let select = |service: &CleanupService, path: &Path| {
            let preview = service.preview().unwrap();
            let id = preview
                .records
                .iter()
                .find(|record| Path::new(&record.display_path) == path)
                .unwrap()
                .id
                .clone();
            (preview.scan_id, id)
        };

        let service = CleanupService::new(app_data.clone()).unwrap();
        let recycled_path = candidate("recycle");
        let (scan_id, id) = select(&service, &recycled_path);
        let plan = service
            .create_plan(&scan_id, &[id], CleanupDisposition::RecycleBin)
            .unwrap();
        let recycled = service.execute(&plan.plan_id).unwrap();
        assert!(!recycled_path.exists());
        service.undo(&recycled.execution_id).unwrap();
        assert!(recycled_path.exists());

        let quarantined_path = candidate("quarantine");
        let (scan_id, id) = select(&service, &quarantined_path);
        let plan = service
            .create_plan(&scan_id, &[id], CleanupDisposition::Quarantine)
            .unwrap();
        let quarantined = service.execute(&plan.plan_id).unwrap();
        assert!(!quarantined_path.exists());
        drop(service);
        let service = CleanupService::new(app_data.clone()).unwrap();
        service.undo(&quarantined.execution_id).unwrap();
        assert!(quarantined_path.exists());

        let purge_path = candidate("purge");
        let (scan_id, id) = select(&service, &purge_path);
        let plan = service
            .create_plan(&scan_id, &[id], CleanupDisposition::Quarantine)
            .unwrap();
        let purged = service.execute(&plan.plan_id).unwrap();
        let mut journal = service
            .storage
            .read_execution(&purged.execution_id)
            .unwrap();
        journal.purge_after = Some(0);
        service.storage.write_execution(&journal).unwrap();
        service
            .storage
            .write_policy(&AutoCleanupPolicy {
                schema_version: 1,
                enabled: true,
                grace_days: 1,
            })
            .unwrap();
        service.purge_due().unwrap();
        assert!(
            service
                .storage
                .read_execution(&purged.execution_id)
                .unwrap()
                .items
                .iter()
                .all(|item| item.state == ItemState::Purged)
        );

        let permanent_path = candidate("permanent");
        let (scan_id, id) = select(&service, &permanent_path);
        let plan = service
            .create_plan(&scan_id, &[id], CleanupDisposition::Permanent)
            .unwrap();
        service.execute_permanent(&plan.plan_id).unwrap();
        assert!(!permanent_path.exists());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn persisted_plan_cannot_expand_the_fixed_scan_root() {
        let expected_root = PathBuf::from(r"C:\Temp");
        let plan = CleanupPlan::new(
            "1".repeat(32),
            "2".repeat(32),
            1,
            CleanupDisposition::RecycleBin,
            vec![PlanItem {
                item_id: "3".repeat(32),
                proof: cleanup_core::ResolvedCandidate {
                    path: PathBuf::from(r"C:\Other\cache"),
                    scan_root: PathBuf::from(r"C:\Other"),
                    context_root: PathBuf::from(r"C:\Other"),
                    rule: temporary_rule().unwrap(),
                    identity: cleanup_core::FileIdentity { volume: 1, file: 1 },
                    kind: cleanup_core::EntryKind::Directory,
                    logical_bytes: 0,
                    allocated_bytes: 0,
                    scanned_at: SystemTime::UNIX_EPOCH,
                },
            }],
        )
        .unwrap();

        assert!(!plan_matches_current_scope(
            &WindowsFileSystem,
            &plan,
            &temporary_rule().unwrap(),
            &expected_root,
        ));
    }

    #[test]
    fn automatic_policy_defaults_disabled_and_persists_grace() {
        let root = std::env::temp_dir().join(format!(
            "supa-diska-service-{}-{}",
            std::process::id(),
            getrandom::u64().unwrap()
        ));
        std::fs::create_dir(&root).unwrap();
        let service = CleanupService::new(root.clone()).unwrap();
        assert_eq!(service.policy().unwrap(), AutoCleanupPolicy::default());
        service.set_policy(false, 14).unwrap();
        drop(service);
        let reopened = CleanupService::new(root.clone()).unwrap();
        assert_eq!(reopened.policy().unwrap().grace_days, 14);
        assert!(!reopened.policy().unwrap().enabled);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn accounting_separates_failed_recoverable_and_reclaimed_bytes() {
        let root = std::env::temp_dir().join(format!(
            "supa-diska-accounting-{}-{}",
            std::process::id(),
            getrandom::u64().unwrap()
        ));
        let storage = CleanupStorage::open(root.clone()).unwrap();
        let mut journal = ExecutionJournal {
            schema_version: 1,
            execution_id: "1".repeat(32),
            plan_id: "2".repeat(32),
            started_at: 1,
            completed_at: None,
            disposition: CleanupDisposition::Quarantine,
            purge_after: Some(2),
            items: vec![
                ExecutionItem {
                    item_id: "3".repeat(32),
                    state: ItemState::Failed,
                    logical_bytes: 10,
                    processed: false,
                    occupied_bytes: 0,
                    reclaimed_bytes: 0,
                    quarantine_path: None,
                    recycle_item: None,
                    failure: Some("rejected".to_owned()),
                },
                ExecutionItem {
                    item_id: "4".repeat(32),
                    state: ItemState::Quarantined,
                    logical_bytes: 20,
                    processed: true,
                    occupied_bytes: 24,
                    reclaimed_bytes: 0,
                    quarantine_path: Some(root.join("payload")),
                    recycle_item: None,
                    failure: None,
                },
            ],
            accounting: ByteAccounting {
                selected_bytes: 30,
                ..ByteAccounting::default()
            },
        };

        persist_accounting(&storage, &mut journal).unwrap();

        assert_eq!(journal.accounting.selected_bytes, 30);
        assert_eq!(journal.accounting.processed_bytes, 20);
        assert_eq!(journal.accounting.failed_bytes, 10);
        assert_eq!(journal.accounting.quarantined_bytes, 24);
        assert_eq!(journal.accounting.occupied_bytes, 24);
        assert_eq!(journal.accounting.reclaimed_bytes, 0);
        std::fs::remove_dir_all(root).unwrap();
    }
}
