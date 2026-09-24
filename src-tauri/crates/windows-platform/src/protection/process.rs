//! Read-only process inventory (ADR 0003, decision 5).
//!
//! Lists processes with image path, parent and signer. Nothing here opens a
//! process with more than `PROCESS_QUERY_LIMITED_INFORMATION`, and nothing
//! terminates, suspends or writes to a process. Command lines are reported as
//! unavailable: reading them needs memory-read access to the target process,
//! which this module never requests.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use protection_core::{
    Evidence, LocationClass, ProcessFacts, SignerStatus, UnavailableReason, evaluate_process,
};
use serde::Serialize;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};

use super::authenticode::SignerCache;
use super::locations::KnownLocations;

pub const MAX_PROCESSES: usize = 8192;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessEntry {
    pub pid: u32,
    pub parent_pid: u32,
    pub name: String,
    pub thread_count: u32,
    /// `None` when access was denied (protected or other-user processes).
    pub image_path: Option<String>,
    pub image_location: Option<LocationClass>,
    pub signer: SignerStatus,
    /// Always unavailable; see the module docs.
    pub command_line: Evidence,
    pub findings: Vec<Evidence>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessInventory {
    pub processes: Vec<ProcessEntry>,
    pub truncated: bool,
    pub image_unavailable_count: usize,
}

struct Snapshot(HANDLE);
impl Drop for Snapshot {
    fn drop(&mut self) {
        // SAFETY: handle came from CreateToolhelp32Snapshot and is closed once.
        unsafe { CloseHandle(self.0) };
    }
}

/// Raw ToolHelp rows: (pid, parent, threads, exe name).
fn snapshot_rows() -> std::io::Result<Vec<(u32, u32, u32, String)>> {
    // SAFETY: plain snapshot of the process list; the handle is owned by `Snapshot`.
    let handle = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if handle == INVALID_HANDLE_VALUE || handle.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    let snapshot = Snapshot(handle);
    let mut rows = Vec::new();
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    // SAFETY: `entry.dwSize` is set; the snapshot handle is valid.
    let mut more = unsafe { Process32FirstW(snapshot.0, &mut entry) } != 0;
    while more && rows.len() <= MAX_PROCESSES {
        let len = entry
            .szExeFile
            .iter()
            .position(|c| *c == 0)
            .unwrap_or(entry.szExeFile.len());
        rows.push((
            entry.th32ProcessID,
            entry.th32ParentProcessID,
            entry.cntThreads,
            String::from_utf16_lossy(&entry.szExeFile[..len]),
        ));
        // SAFETY: as above.
        more = unsafe { Process32NextW(snapshot.0, &mut entry) } != 0;
    }
    Ok(rows)
}

/// Query the image path with the least access right that allows it.
pub(crate) fn image_path(pid: u32) -> Option<PathBuf> {
    if pid == 0 {
        return None;
    }
    // SAFETY: requesting only limited query rights; handle closed below.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return None;
    }
    let mut buffer = vec![0_u16; 32_768];
    let mut size = buffer.len() as u32;
    // SAFETY: buffer and size describe a valid writable region.
    let ok = unsafe {
        QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, buffer.as_mut_ptr(), &mut size)
    };
    // SAFETY: handle is valid and owned here.
    unsafe { CloseHandle(handle) };
    (ok != 0 && size > 0).then(|| PathBuf::from(OsString::from_wide(&buffer[..size as usize])))
}

/// Build one entry from gathered facts. Split out for testing.
pub(crate) fn build_entry(
    row: (u32, u32, u32, String),
    image: Option<&Path>,
    image_exists: bool,
    location: Option<LocationClass>,
    signer: SignerStatus,
) -> ProcessEntry {
    let (pid, parent_pid, thread_count, name) = row;
    let mut findings = Vec::new();
    match (image, location) {
        (Some(_), Some(image_location)) => {
            findings = evaluate_process(&ProcessFacts {
                image_name: &name,
                image_location,
                image_deleted: !image_exists,
                signer: &signer,
            });
        }
        _ => findings.push(Evidence::Unavailable {
            reason: UnavailableReason::AccessDenied,
        }),
    }
    ProcessEntry {
        pid,
        parent_pid,
        name,
        thread_count,
        image_path: image.map(|p| p.to_string_lossy().into_owned()),
        image_location: location,
        signer,
        command_line: Evidence::Unavailable {
            reason: UnavailableReason::AccessDenied,
        },
        findings,
    }
}

pub fn inventory(
    signers: &SignerCache,
    known: &KnownLocations,
) -> std::io::Result<ProcessInventory> {
    let mut rows = snapshot_rows()?;
    let truncated = rows.len() > MAX_PROCESSES;
    rows.truncate(MAX_PROCESSES);
    let mut unavailable = 0;
    let processes = rows
        .into_iter()
        .map(|row| {
            let image = image_path(row.0);
            if image.is_none() {
                unavailable += 1;
            }
            let (exists, location, signer) = match &image {
                Some(path) => {
                    let exists = path.is_file();
                    let signer = if exists {
                        signers.verify(path)
                    } else {
                        SignerStatus::Unavailable
                    };
                    (exists, Some(known.classify(path)), signer)
                }
                None => (false, None, SignerStatus::Unavailable),
            };
            build_entry(row, image.as_deref(), exists, location, signer)
        })
        .collect();
    Ok(ProcessInventory {
        processes,
        truncated,
        image_unavailable_count: unavailable,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inventory_includes_this_process_with_its_image() {
        let signers = SignerCache::default();
        let inventory = inventory(&signers, &KnownLocations::native()).unwrap();
        let me = std::process::id();
        let entry = inventory
            .processes
            .iter()
            .find(|p| p.pid == me)
            .expect("own process listed");
        let exe = std::env::current_exe().unwrap();
        assert_eq!(
            entry.image_path.as_deref().map(str::to_lowercase),
            Some(exe.to_string_lossy().to_lowercase())
        );
        assert_eq!(entry.signer, SignerStatus::Unsigned);
        assert!(matches!(entry.command_line, Evidence::Unavailable { .. }));
        // System Idle Process (pid 0) never has an image.
        let idle = inventory.processes.iter().find(|p| p.pid == 0).unwrap();
        assert!(idle.image_path.is_none());
        assert!(idle.findings.contains(&Evidence::Unavailable {
            reason: UnavailableReason::AccessDenied
        }));
    }

    #[test]
    fn deleted_image_is_flagged_and_denied_access_is_unavailable() {
        let entry = build_entry(
            (10, 4, 1, "app.exe".into()),
            Some(Path::new(r"C:\Program Files\App\app.exe")),
            false,
            Some(LocationClass::ProgramFiles),
            SignerStatus::Unavailable,
        );
        assert!(entry.findings.iter().any(|f| matches!(f, Evidence::Heuristic { heuristic_id, .. } if heuristic_id.starts_with("H006"))));
        let denied = build_entry(
            (11, 4, 1, "lsass.exe".into()),
            None,
            false,
            None,
            SignerStatus::Unavailable,
        );
        assert_eq!(
            denied.findings,
            vec![Evidence::Unavailable {
                reason: UnavailableReason::AccessDenied
            }]
        );
    }
    // Read-only access (no terminate/suspend/memory rights) is enforced for the
    // whole protection module by scripts/check-architecture.mjs.
}
