//! Read-only native fixed-drive inventory. Removable, remote, optical, RAM and
//! unknown drive types are excluded; a failed native query is never a zero capacity.
use super::opaque_id;
use cleanup_core::storage::analysis::DriveSummary;
use std::{io, ptr};
use windows_sys::Win32::{
    Storage::FileSystem::{
        GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDriveStringsW, GetVolumeInformationW,
        GetVolumeNameForVolumeMountPointW, GetVolumePathNameW,
    },
    System::{SystemInformation::GetSystemWindowsDirectoryW, WindowsProgramming::DRIVE_FIXED},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct VolumeEvidence {
    mount: String,
    guid: String,
    serial: u32,
    filesystem: String,
}
#[derive(Clone, Debug)]
pub(crate) struct BoundDrive {
    pub summary: DriveSummary,
    evidence: VolumeEvidence,
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn text(buffer: &[u16]) -> io::Result<String> {
    let end = buffer
        .iter()
        .position(|c| *c == 0)
        .ok_or_else(|| invalid("unterminated native string"))?;
    String::from_utf16(&buffer[..end]).map_err(|_| invalid("invalid native UTF-16"))
}
fn mounts() -> io::Result<Vec<String>> {
    let mut size = unsafe { GetLogicalDriveStringsW(0, ptr::null_mut()) };
    if size == 0 {
        return Err(io::Error::other(format!(
            "{}:{}: {}",
            file!(),
            line!(),
            io::Error::last_os_error()
        )));
    }
    for _ in 0..4 {
        if size > 4096 {
            return Err(invalid("drive list exceeds bound"));
        }
        let mut buffer = vec![0u16; size as usize + 1];
        let n = unsafe { GetLogicalDriveStringsW(buffer.len() as u32, buffer.as_mut_ptr()) };
        if n == 0 {
            return Err(io::Error::other(format!(
                "{}:{}: {}",
                file!(),
                line!(),
                io::Error::last_os_error()
            )));
        }
        if n as usize >= buffer.len() {
            size = n;
            continue;
        }
        let mut result = Vec::new();
        for part in buffer[..n as usize]
            .split(|c| *c == 0)
            .filter(|s| !s.is_empty())
        {
            let mount = String::from_utf16(part).map_err(|_| invalid("invalid drive name"))?;
            if mount.len() != 3
                || mount.as_bytes()[1..] != *b":\\"
                || !mount.as_bytes()[0].is_ascii_alphabetic()
            {
                return Err(invalid("unexpected logical drive mount"));
            }
            result.push(mount.to_ascii_uppercase());
        }
        result.sort();
        result.dedup();
        return Ok(result);
    }
    Err(invalid("drive list changed repeatedly"))
}
fn volume(mount: &str) -> io::Result<(VolumeEvidence, String)> {
    let path = wide(mount);
    if unsafe { GetDriveTypeW(path.as_ptr()) } != DRIVE_FIXED {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "drive removed or no longer fixed",
        ));
    }
    let mut guid = [0u16; 50];
    if unsafe {
        GetVolumeNameForVolumeMountPointW(path.as_ptr(), guid.as_mut_ptr(), guid.len() as u32)
    } == 0
    {
        let error = io::Error::last_os_error();
        return Err(io::Error::other(format!(
            "volume identity for {mount:?}: {error}"
        )));
    }
    let mut label = [0u16; 261];
    let mut fs = [0u16; 261];
    let mut serial = 0;
    if unsafe {
        GetVolumeInformationW(
            path.as_ptr(),
            label.as_mut_ptr(),
            label.len() as u32,
            &mut serial,
            ptr::null_mut(),
            ptr::null_mut(),
            fs.as_mut_ptr(),
            fs.len() as u32,
        )
    } == 0
    {
        return Err(io::Error::other(format!(
            "{}:{}: {}",
            file!(),
            line!(),
            io::Error::last_os_error()
        )));
    }
    Ok((
        VolumeEvidence {
            mount: mount.to_owned(),
            guid: text(&guid)?.to_ascii_lowercase(),
            serial,
            filesystem: text(&fs)?,
        },
        text(&label)?,
    ))
}
pub(crate) fn system_guid() -> io::Result<String> {
    let mut windows = [0u16; 32768];
    let n = unsafe { GetSystemWindowsDirectoryW(windows.as_mut_ptr(), windows.len() as u32) };
    if n == 0 {
        return Err(io::Error::other(format!(
            "{}:{}: {}",
            file!(),
            line!(),
            io::Error::last_os_error()
        )));
    }
    if n as usize >= windows.len() {
        return Err(invalid("Windows directory exceeds bound"));
    }
    let mut mount = [0u16; 32768];
    if unsafe { GetVolumePathNameW(windows.as_ptr(), mount.as_mut_ptr(), mount.len() as u32) } == 0
    {
        return Err(io::Error::other(format!(
            "{}:{}: {}",
            file!(),
            line!(),
            io::Error::last_os_error()
        )));
    }
    Ok(volume(&text(&mount)?)?.0.guid)
}
fn query(mount: &str, system: Option<&str>, id: String) -> io::Result<BoundDrive> {
    let (evidence, label) = volume(mount)?;
    let path = wide(mount);
    let (mut available, mut total, mut volume_free) = (0u64, 0u64, 0u64);
    if unsafe { GetDiskFreeSpaceExW(path.as_ptr(), &mut available, &mut total, &mut volume_free) }
        == 0
    {
        return Err(io::Error::other(format!(
            "{}:{}: {}",
            file!(),
            line!(),
            io::Error::last_os_error()
        )));
    }
    // Both fields use caller/quota units. volume_free is intentionally not mixed in.
    let used = total
        .checked_sub(available)
        .ok_or_else(|| invalid("inconsistent caller capacity"))?;
    if volume(mount)?.0 != evidence {
        return Err(invalid("volume changed during inventory"));
    }
    Ok(BoundDrive {
        summary: DriveSummary {
            drive_id: id,
            label,
            filesystem: evidence.filesystem.clone(),
            total_bytes: total,
            free_bytes: available,
            used_bytes: used,
            system: system.map(|system| evidence.guid == system),
        },
        evidence,
    })
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveIssue {
    /// None identifies an inventory-wide issue, not an unreadable drive.
    pub mount: Option<String>,
    pub error: String,
}

pub(crate) fn inventory(
    cancel: &cleanup_core::CancellationToken,
    system: io::Result<String>,
    mut accept: impl FnMut(Result<BoundDrive, DriveIssue>) -> bool,
) -> io::Result<()> {
    let system = match system {
        Ok(system) => Some(system),
        Err(error) => {
            if !accept(Err(DriveIssue {
                mount: None,
                error: error.to_string(),
            })) {
                return Ok(());
            }
            None
        }
    };
    for mount in mounts()? {
        if cancel.is_cancelled() {
            break;
        }
        if unsafe { GetDriveTypeW(wide(&mount).as_ptr()) } != DRIVE_FIXED {
            continue;
        }
        let id = opaque_id().map_err(io::Error::other)?;
        let result = query(&mount, system.as_deref(), id).map_err(|error| DriveIssue {
            mount: Some(mount),
            error: error.to_string(),
        });
        if !accept(result) {
            break;
        }
    }
    Ok(())
}
pub(crate) fn resolve(drive: &BoundDrive) -> io::Result<DriveSummary> {
    if drive.summary.system.is_none() {
        return Err(invalid(
            "system classification unavailable; refresh inventory",
        ));
    }
    let current = query(
        &drive.evidence.mount,
        Some(&system_guid()?),
        drive.summary.drive_id.clone(),
    )?;
    if current.evidence != drive.evidence {
        return Err(invalid("stale drive identity"));
    }
    Ok(current.summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_fixed_inventory_is_read_only_and_identity_bound() {
        let mut drives = Vec::new();
        let mut issues = Vec::new();
        inventory(&Default::default(), system_guid(), |d| {
            match d {
                Ok(d) => drives.push(d),
                Err(e) => issues.push(e),
            };
            true
        })
        .unwrap();
        for issue in issues {
            assert!(!issue.error.is_empty());
            assert!(
                query(
                    issue.mount.as_deref().unwrap(),
                    Some(&system_guid().unwrap()),
                    opaque_id().unwrap()
                )
                .is_err()
            );
        }
        assert!(!drives.is_empty());
        assert!(drives.iter().any(|d| d.summary.system == Some(true)));
        for drive in drives {
            let current = resolve(&drive).unwrap();
            assert_eq!(current.total_bytes - current.free_bytes, current.used_bytes);
            assert_eq!(
                unsafe { GetDriveTypeW(wide(&drive.evidence.mount).as_ptr()) },
                DRIVE_FIXED
            );
            let mut stale = drive;
            stale.evidence.serial ^= 1;
            assert!(resolve(&stale).is_err());
        }
    }
}
