//! Headless entry for `--scheduled-scan <uuid>`. Strictly read-only: it
//! previews temporary caches and records a bounded summary; it never deletes,
//! moves, or changes system settings.

use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use cleanup_core::system_change::ScheduleId;
use serde::{Deserialize, Serialize};

use crate::cleanup::{CleanupPreview, preview_temporary_caches};

const MAX_SUMMARIES: usize = 50;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledScanSummary {
    pub schedule_id: String,
    pub finished_at: u64,
    pub reclaimable_bytes: u64,
    pub item_count: u64,
    pub diagnostic_count: u64,
    pub succeeded: bool,
}

pub fn summarize(
    schedule_id: &ScheduleId,
    preview: Option<&CleanupPreview>,
    finished_at: u64,
) -> ScheduledScanSummary {
    ScheduledScanSummary {
        schedule_id: schedule_id.to_string(),
        finished_at,
        reclaimable_bytes: preview.map_or(0, |p| p.records.iter().map(|r| r.bytes).sum()),
        item_count: preview.map_or(0, |p| p.records.len() as u64),
        diagnostic_count: preview.map_or(0, |p| p.diagnostics.len() as u64),
        succeeded: preview.is_some(),
    }
}

fn summaries_path(app_data: &Path) -> PathBuf {
    app_data.join("scheduled-scans").join("summaries.json")
}

/// Most recent summaries, newest first.
pub fn read_summaries(app_data: &Path) -> Vec<ScheduledScanSummary> {
    fs::read(summaries_path(app_data))
        .ok()
        .filter(|bytes| bytes.len() <= 256 * 1024)
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn record_summary(app_data: &Path, summary: ScheduledScanSummary) -> io::Result<()> {
    let path = summaries_path(app_data);
    let parent = path.parent().ok_or_else(|| io::Error::other("no parent"))?;
    fs::create_dir_all(parent)?;
    let mut summaries = read_summaries(app_data);
    summaries.insert(0, summary);
    summaries.truncate(MAX_SUMMARIES);
    let bytes = serde_json::to_vec(&summaries).map_err(io::Error::other)?;
    // Unique per writer: a scheduled run and the open app may record at once.
    let temporary = parent.join(format!(
        "summaries-{}.tmp",
        crate::system_change::random_id()?
    ));
    let result = write_new(&temporary, &bytes).and_then(|()| fs::rename(&temporary, &path));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// Run one read-only scheduled scan and record its summary.
pub fn run_scheduled_scan(
    schedule_id: &ScheduleId,
    app_data: &Path,
) -> io::Result<ScheduledScanSummary> {
    let preview = preview_temporary_caches().ok();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let summary = summarize(schedule_id, preview.as_ref(), now);
    record_summary(app_data, summary.clone())?;
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id() -> ScheduleId {
        ScheduleId::parse("0f8fad5b-d9cb-469f-a165-70867728950e").unwrap()
    }

    #[test]
    fn summaries_are_bounded_and_newest_first() {
        let root = std::env::temp_dir().join(format!("sdk-sched-{}", std::process::id()));
        for n in 0..(MAX_SUMMARIES as u64 + 5) {
            record_summary(&root, summarize(&id(), None, n)).unwrap();
        }
        let summaries = read_summaries(&root);
        assert_eq!(summaries.len(), MAX_SUMMARIES);
        assert_eq!(summaries[0].finished_at, MAX_SUMMARIES as u64 + 4);
        assert!(!summaries[0].succeeded);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn concurrent_records_do_not_collide_on_the_temporary_file() {
        let root =
            std::env::temp_dir().join(format!("sdk-sched-concurrent-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let barrier = std::sync::Barrier::new(8);
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8_u64)
                .map(|n| {
                    let (root, barrier) = (&root, &barrier);
                    scope.spawn(move || {
                        barrier.wait();
                        (0..40).try_for_each(|m| {
                            record_summary(root, summarize(&id(), None, n * 100 + m))
                        })
                    })
                })
                .collect();
            for handle in handles {
                handle.join().unwrap().unwrap();
            }
        });
        assert!(!read_summaries(&root).is_empty());
        let leftovers: Vec<_> = fs::read_dir(root.join("scheduled-scans"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter(|name| name != "summaries.json")
            .collect();
        assert!(leftovers.is_empty(), "stray files: {leftovers:?}");
        fs::remove_dir_all(root).unwrap();
    }
}
