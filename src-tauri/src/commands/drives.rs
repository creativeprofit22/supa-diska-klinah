// Read-only IPC facade. Snapshot authority and limits never cross IPC.
use serde::Serialize;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use windows_platform::storage::{
    DriveSummary, PageCollection, PageRequest, StorageLimits, StorageModule, StoragePhase,
    StorageRecord,
    drives::DriveIssue,
    scans::{JobError, StorageService},
};

const TIMEOUT: Duration = Duration::from_secs(10);
const DRIVE_LIMIT: usize = 26;

#[derive(Default)]
pub(crate) struct DriveInventoryState {
    service: StorageService,
    busy: AtomicBool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DriveInventory {
    drives: Vec<DriveSummary>,
    partial: bool,
    warnings: Vec<DriveWarning>,
}
#[derive(Debug, Serialize)]
struct DriveWarning {
    drive: Option<String>,
    code: &'static str,
}
#[derive(Debug, PartialEq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub(crate) enum InventoryError {
    Busy,
    Timeout,
    InventoryUnavailable,
}
fn sanitized(error: JobError) -> InventoryError {
    match error {
        JobError::Busy => InventoryError::Busy,
        _ => InventoryError::InventoryUnavailable,
    }
}
struct Permit(Arc<DriveInventoryState>);
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.busy.store(false, Ordering::Release);
    }
}
struct Snapshot<'a> {
    service: &'a StorageService,
    id: String,
}
impl Drop for Snapshot<'_> {
    fn drop(&mut self) {
        // release also cancels active work. The service retains its one-worker cap
        // until a stalled native call returns; timed-out requests cannot spawn more.
        let _ = self.service.release(&self.id);
    }
}
fn warning(issue: DriveIssue) -> DriveWarning {
    // Only a drive-letter display hint is allowed, never native diagnostic text.
    let Some(mount) = issue.mount else {
        return DriveWarning {
            drive: None,
            code: "inventory_partial",
        };
    };
    let bytes = mount.as_bytes();
    let drive = (bytes.len() == 3 && bytes[0].is_ascii_alphabetic() && &bytes[1..] == b":\\")
        .then(|| mount.to_ascii_uppercase());
    DriveWarning {
        drive,
        code: "drive_unavailable",
    }
}
fn collect(
    service: &StorageService,
    id: String,
    deadline: Instant,
) -> Result<DriveInventory, InventoryError> {
    let snapshot = Snapshot { service, id };
    loop {
        if Instant::now() >= deadline {
            return Err(InventoryError::Timeout);
        }
        let (status, error) = service.status(&snapshot.id).map_err(sanitized)?;
        if error.is_some() {
            return Err(InventoryError::InventoryUnavailable);
        }
        match status.phase {
            StoragePhase::Complete => {
                let page = service
                    .page(&PageRequest {
                        snapshot_id: snapshot.id.clone(),
                        module: StorageModule::Drives,
                        collection: PageCollection::Drives,
                        parent_id: None,
                        cursor: None,
                        page_size: DRIVE_LIMIT,
                    })
                    .map_err(sanitized)?;
                let mut drives = Vec::new();
                for record in page.records {
                    match record {
                        StorageRecord::Drive(drive) => drives.push(drive),
                        _ => return Err(InventoryError::InventoryUnavailable),
                    }
                }
                let mut warnings: Vec<_> = service
                    .drive_issues(&snapshot.id)
                    .map_err(sanitized)?
                    .into_iter()
                    .take(DRIVE_LIMIT)
                    .map(warning)
                    .collect();
                let partial = !status.completeness.is_complete()
                    || !page.completeness.is_complete()
                    || page.next_cursor.is_some()
                    || !warnings.is_empty();
                if partial
                    && !warnings
                        .iter()
                        .any(|warning| warning.code == "inventory_partial")
                {
                    warnings.push(DriveWarning {
                        drive: None,
                        code: "inventory_partial",
                    });
                }
                return Ok(DriveInventory {
                    drives,
                    partial,
                    warnings,
                });
            }
            StoragePhase::Failed | StoragePhase::Cancelled => {
                return Err(InventoryError::InventoryUnavailable);
            }
            _ => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}

#[tauri::command]
pub(crate) async fn list_drive_inventory(
    state: tauri::State<'_, Arc<DriveInventoryState>>,
) -> Result<DriveInventory, InventoryError> {
    let state = Arc::clone(state.inner());
    state
        .busy
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| InventoryError::Busy)?;
    let permit = Permit(state);
    let deadline = Instant::now() + TIMEOUT;
    let worker = tauri::async_runtime::spawn_blocking(move || {
        let service = &permit.0.service;
        let id = service
            .start_drives(StorageLimits {
                workers: 1,
                visited_entries: DRIVE_LIMIT,
                retained_records: DRIVE_LIMIT,
                diagnostics: DRIVE_LIMIT,
                depth: 0,
            })
            .map_err(sanitized)?;
        collect(service, id, deadline)
    });
    // A dropped waiter cannot free the permit while its blocking task still runs.
    tokio::time::timeout(TIMEOUT, worker)
        .await
        .map_err(|_| InventoryError::Timeout)?
        .map_err(|_| InventoryError::InventoryUnavailable)?
}
