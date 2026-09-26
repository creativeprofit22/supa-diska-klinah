//! Opt-in disk-reclamation benchmark through the real `CleanupService`.
//!
//! Run: `SUPA_PERF_DISK_MIB=256 cargo test -p windows-platform --lib perf_disk -- --ignored --nocapture`
//! Add `SUPA_PERF_ALLOW_RECYCLE=1` to include the Recycle Bin scenario; it recycles only its own
//! disposable payload and restores it with the app's undo afterwards.
//! Payloads live under `%TEMP%\supa-diska-perf-disk-*` (the Temporary cleanup scope, the only
//! scope that supports Recycle Bin, Quarantine, purge and Permanent), and are always removed.
//! Output: JSON lines (sorted keys), also appended to `SUPA_PERF_OUT` when set.
use super::*;
use std::{
    io::Write,
    os::windows::ffi::OsStrExt,
    time::{Duration, Instant},
};
use windows_sys::Win32::UI::Shell::{SHQUERYRBINFO, SHQueryRecycleBinW};

const FILES_PER_PAYLOAD: u64 = 8;
const SPACE_SAMPLES: usize = 3;

fn emit(record: serde_json::Value) {
    let line = record.to_string();
    println!("PERF {line}");
    if let Ok(path) = std::env::var("SUPA_PERF_OUT") {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("open SUPA_PERF_OUT");
        writeln!(file, "{line}").expect("write SUPA_PERF_OUT");
    }
}

/// Median plus min..max noise band of several free-space samples.
fn sample_free(path: &Path) -> (u64, u64) {
    let fs = WindowsFileSystem;
    let mut samples: Vec<u64> = (0..SPACE_SAMPLES)
        .map(|index| {
            if index > 0 {
                std::thread::sleep(Duration::from_millis(200));
            }
            fs.free_space(path).expect("free space")
        })
        .collect();
    samples.sort_unstable();
    (
        samples[samples.len() / 2],
        samples[samples.len() - 1] - samples[0],
    )
}

fn recycle_bin_bytes(path: &Path) -> Option<i64> {
    let drive: Vec<u16> = path
        .components()
        .next()?
        .as_os_str()
        .encode_wide()
        .chain("\\".encode_utf16())
        .chain(Some(0))
        .collect();
    // SAFETY: NUL-terminated drive root and a correctly sized, zeroed info struct.
    unsafe {
        let mut info: SHQUERYRBINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHQUERYRBINFO>() as u32;
        (SHQueryRecycleBinW(drive.as_ptr(), &mut info) >= 0).then_some(info.i64Size)
    }
}

/// Deterministic incompressible-ish payload (xorshift) so NTFS stores every byte.
fn write_payload(dir: &Path, total: u64, seed: u64) -> u64 {
    std::fs::create_dir_all(dir).unwrap();
    let per_file = total / FILES_PER_PAYLOAD;
    let mut state = seed | 1;
    let mut chunk = vec![0_u8; 1 << 20];
    for index in 0..FILES_PER_PAYLOAD {
        let mut file = std::fs::File::create(dir.join(format!("payload{index}.bin"))).unwrap();
        let mut remaining = per_file;
        while remaining > 0 {
            for word in chunk.chunks_exact_mut(8) {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                word.copy_from_slice(&state.to_le_bytes());
            }
            let count = remaining.min(chunk.len() as u64) as usize;
            file.write_all(&chunk[..count]).unwrap();
            remaining -= count as u64;
        }
        file.sync_all().unwrap();
    }
    per_file * FILES_PER_PAYLOAD
}

struct Fixture {
    root: PathBuf,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn plan_for(
    service: &CleanupService,
    target: &Path,
    disposition: CleanupDisposition,
) -> CleanupPlanSummary {
    let preview = service.preview().unwrap();
    let id = preview
        .records
        .iter()
        .find(|record| Path::new(&record.display_path) == target)
        .unwrap_or_else(|| {
            panic!(
                "payload {} not offered by Temporary preview",
                target.display()
            )
        })
        .id
        .clone();
    service
        .create_plan(&preview.scan_id, &[id], disposition)
        .unwrap()
}

fn sum(items: &[ExecutionItem], state: ItemState, bytes: fn(&ExecutionItem) -> u64) -> u64 {
    items
        .iter()
        .filter(|item| item.state == state)
        .map(bytes)
        .sum()
}

#[test]
#[ignore = "opt-in disk benchmark: set SUPA_PERF_DISK_MIB"]
fn perf_disk_reclamation() {
    let Ok(mib) = std::env::var("SUPA_PERF_DISK_MIB") else {
        eprintln!("SUPA_PERF_DISK_MIB not set; skipping");
        return;
    };
    let mib: u64 = mib.parse().expect("SUPA_PERF_DISK_MIB must be an integer");
    assert!(
        (8..=8192).contains(&mib),
        "SUPA_PERF_DISK_MIB must be 8..=8192"
    );
    let allow_recycle = std::env::var("SUPA_PERF_ALLOW_RECYCLE").as_deref() == Ok("1");
    let fixture = Fixture {
        root: std::env::temp_dir().join(format!(
            "supa-diska-perf-disk-{}-{}",
            std::process::id(),
            getrandom::u64().unwrap()
        )),
    };
    let app_data = fixture.root.join("app-data");
    std::fs::create_dir_all(&app_data).unwrap();
    let service = CleanupService::new(app_data.clone()).unwrap();
    let mut dispositions = vec![
        CleanupDisposition::Quarantine,
        CleanupDisposition::Permanent,
    ];
    if allow_recycle {
        dispositions.insert(0, CleanupDisposition::RecycleBin);
    }

    for (index, disposition) in dispositions.into_iter().enumerate() {
        let name = format!("{disposition:?}").to_lowercase();
        let target = fixture.root.join(&name).join("cache");
        let written = write_payload(&target, mib << 20, 0x5eed_0000 + index as u64);
        let target = std::fs::canonicalize(&target).unwrap();
        let plan = plan_for(&service, &target, disposition);
        let (free_before, noise_before) = sample_free(&fixture.root);
        let bin_before = recycle_bin_bytes(&target);
        let started = Instant::now();
        let summary = if disposition == CleanupDisposition::Permanent {
            service.execute_permanent(&plan.plan_id).unwrap()
        } else {
            service.execute(&plan.plan_id).unwrap()
        };
        let elapsed_ms = started.elapsed().as_secs_f64() * 1e3;
        let (free_after, noise_after) = sample_free(&fixture.root);
        let bin_after = recycle_bin_bytes(&target);
        let journal = service
            .storage
            .read_execution(&summary.execution_id)
            .unwrap();
        let mut record = serde_json::json!({
            "kind": "disk",
            "scenario": name,
            "payloadMiB": mib,
            "writtenBytes": written,
            "selectedBytes": plan.selected_bytes,
            "processedBytes": summary.accounting.processed_bytes,
            "failedBytes": summary.accounting.failed_bytes,
            "quarantinedBytes": summary.accounting.quarantined_bytes,
            "purgedBytes": summary.accounting.purged_bytes,
            "recycledBytes": sum(&journal.items, ItemState::Recycled, |item| item.logical_bytes),
            "occupiedBytes": summary.accounting.occupied_bytes,
            "appReclaimedBytes": summary.accounting.reclaimed_bytes,
            "externalReclaimedBytes": free_after as i128 - free_before as i128,
            "freeSpaceNoiseBytes": noise_before.max(noise_after),
            "recycleBinDeltaBytes": bin_before.zip(bin_after).map(|(before, after)| after - before),
            "executeMs": (elapsed_ms * 10.0).round() / 10.0,
        });

        match disposition {
            CleanupDisposition::Quarantine => {
                // Measure what purge later gives back (the app's own purge path).
                let mut due = journal.clone();
                due.purge_after = Some(0);
                service.storage.write_execution(&due).unwrap();
                service
                    .storage
                    .write_policy(&AutoCleanupPolicy {
                        schema_version: 1,
                        enabled: true,
                        grace_days: 1,
                    })
                    .unwrap();
                let (purge_before, _) = sample_free(&fixture.root);
                service.purge_due().unwrap();
                let (purge_after, purge_noise) = sample_free(&fixture.root);
                let purged = service
                    .storage
                    .read_execution(&summary.execution_id)
                    .unwrap();
                record["afterPurge"] = serde_json::json!({
                    "purgedBytes": sum(&purged.items, ItemState::Purged, |item| item.logical_bytes),
                    "appReclaimedBytes": purged.items.iter().map(|item| item.reclaimed_bytes).sum::<u64>(),
                    "externalReclaimedBytes": purge_after as i128 - purge_before as i128,
                    "freeSpaceNoiseBytes": purge_noise,
                });
            }
            CleanupDisposition::RecycleBin => {
                service.undo(&summary.execution_id).unwrap();
                let restored: u64 = std::fs::read_dir(&target)
                    .unwrap()
                    .map(|entry| entry.unwrap().metadata().unwrap().len())
                    .sum();
                assert_eq!(restored, written, "recycled payload was not fully restored");
                record["restoredBytes"] = restored.into();
            }
            CleanupDisposition::Permanent => {}
        }
        assert!(!summary.items.is_empty() && summary.accounting.failed_bytes == 0);
        emit(record);
    }
}
