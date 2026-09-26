//! Bounded, configurable scan concurrency (see docs/performance.md, "Concurrency").
//!
//! One persisted setting chooses how many workers scans may use: `ssd`, `hdd`, or `auto`
//! (per volume, from the storage stack's seek-penalty property; unknown media falls back to
//! the HDD count, which is the pre-existing behavior).
//! Every result is clamped to `1..=MAX_WORKERS`.
use cleanup_core::storage::MAX_WORKERS;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const SCAN_SETTINGS_SCHEMA_VERSION: u32 = 1;
/// Worker count for solid-state volumes: the measured best of 1/2/4 in docs/performance.md
/// (project discovery 8.1 s -> 6.2 s versus the previous fixed 2 workers).
pub const SSD_WORKERS: usize = 4;
/// Worker count for rotational (or unknown) volumes: the previous fixed count. Warm-cache
/// HDD runs did not justify fewer workers and cold-cache HDD evidence is not yet available,
/// so this stays unchanged instead of guessing (docs/performance.md, "Limitations").
pub const HDD_WORKERS: usize = 2;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ScanProfile {
    #[default]
    Auto,
    Ssd,
    Hdd,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScanSettings {
    pub schema_version: u32,
    pub profile: ScanProfile,
}

impl Default for ScanSettings {
    fn default() -> Self {
        Self {
            schema_version: SCAN_SETTINGS_SCHEMA_VERSION,
            profile: ScanProfile::Auto,
        }
    }
}

impl ScanSettings {
    pub fn is_valid(&self) -> bool {
        self.schema_version == SCAN_SETTINGS_SCHEMA_VERSION
    }
}

/// Pure mapping from the chosen profile and the volume's seek penalty to a worker count.
/// `seek_penalty`: `Some(true)` rotational, `Some(false)` solid-state, `None` unknown.
pub fn resolve_workers(profile: ScanProfile, seek_penalty: Option<bool>) -> usize {
    let workers = match (profile, seek_penalty) {
        (ScanProfile::Ssd, _) | (ScanProfile::Auto, Some(false)) => SSD_WORKERS,
        (ScanProfile::Hdd, _) | (ScanProfile::Auto, Some(true) | None) => HDD_WORKERS,
    };
    workers.clamp(1, MAX_WORKERS)
}

/// Resolves workers for a scan rooted at `path`; only `auto` queries the device.
pub fn workers_for_path(profile: ScanProfile, path: &Path) -> usize {
    let seek_penalty = match profile {
        ScanProfile::Auto => seek_penalty(path),
        ScanProfile::Ssd | ScanProfile::Hdd => None,
    };
    resolve_workers(profile, seek_penalty)
}

/// Queries `StorageDeviceSeekPenaltyProperty` for the volume that contains `path`.
/// Needs no elevation (the volume is opened with zero access rights). Returns `None` when the
/// volume or device does not answer (network shares, some virtual or RAID devices).
#[cfg(windows)]
pub fn seek_penalty(path: &Path) -> Option<bool> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        Storage::FileSystem::{
            CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, GetVolumeNameForVolumeMountPointW,
            GetVolumePathNameW, OPEN_EXISTING,
        },
        System::{
            IO::DeviceIoControl,
            Ioctl::{
                IOCTL_STORAGE_QUERY_PROPERTY, PropertyStandardQuery, STORAGE_PROPERTY_QUERY,
                StorageDeviceSeekPenaltyProperty,
            },
        },
    };

    const BUFFER: usize = 1_024;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut mount_point = [0_u16; BUFFER];
    let mut volume = [0_u16; BUFFER];
    // SAFETY: NUL-terminated input; output buffers are sized by the counts passed.
    let resolved = unsafe {
        GetVolumePathNameW(wide.as_ptr(), mount_point.as_mut_ptr(), BUFFER as u32) != 0
            && GetVolumeNameForVolumeMountPointW(
                mount_point.as_ptr(),
                volume.as_mut_ptr(),
                BUFFER as u32,
            ) != 0
    };
    if !resolved {
        return None;
    }
    // `\\?\Volume{GUID}\` -> `\\?\Volume{GUID}` opens the volume device itself.
    let length = volume.iter().position(|&unit| unit == 0)?;
    if length == 0 || volume[length - 1] != u16::from(b'\\') {
        return None;
    }
    volume[length - 1] = 0;
    // SAFETY: NUL-terminated device path; zero desired access only permits property queries.
    let handle = unsafe {
        CreateFileW(
            volume.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return None;
    }
    let query = STORAGE_PROPERTY_QUERY {
        PropertyId: StorageDeviceSeekPenaltyProperty,
        QueryType: PropertyStandardQuery,
        AdditionalParameters: [0],
    };
    // DEVICE_SEEK_PENALTY_DESCRIPTOR is {u32 Version, u32 Size, BOOLEAN IncursSeekPenalty}.
    // Read it as raw words so a non-0/1 BOOLEAN can never become an invalid Rust bool.
    let mut output = [0_u32; 3];
    let mut returned = 0_u32;
    // SAFETY: valid handle, input/output pointers with matching sizes, synchronous call.
    let ok = unsafe {
        DeviceIoControl(
            handle,
            IOCTL_STORAGE_QUERY_PROPERTY,
            (&raw const query).cast(),
            std::mem::size_of::<STORAGE_PROPERTY_QUERY>() as u32,
            output.as_mut_ptr().cast(),
            std::mem::size_of_val(&output) as u32,
            &mut returned,
            std::ptr::null_mut(),
        ) != 0
    };
    // SAFETY: handle came from CreateFileW above and is closed exactly once.
    unsafe { CloseHandle(handle) };
    if !ok || returned < 9 || output[1] < 9 {
        return None;
    }
    Some(output[2] & 0xff != 0)
}

#[cfg(not(windows))]
pub fn seek_penalty(_path: &Path) -> Option<bool> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolver_maps_profiles_and_media_within_bounds() {
        let table = [
            (ScanProfile::Ssd, None, SSD_WORKERS),
            (ScanProfile::Ssd, Some(true), SSD_WORKERS),
            (ScanProfile::Hdd, None, HDD_WORKERS),
            (ScanProfile::Hdd, Some(false), HDD_WORKERS),
            (ScanProfile::Auto, Some(false), SSD_WORKERS),
            (ScanProfile::Auto, Some(true), HDD_WORKERS),
            // Unknown media must take the safe (HDD) path.
            (ScanProfile::Auto, None, HDD_WORKERS),
        ];
        for (profile, seek_penalty, expected) in table {
            let workers = resolve_workers(profile, seek_penalty);
            assert_eq!(workers, expected, "{profile:?} {seek_penalty:?}");
            assert!((1..=MAX_WORKERS).contains(&workers));
        }
    }

    #[test]
    fn worker_constants_respect_the_global_bound() {
        const { assert!(SSD_WORKERS >= 1 && SSD_WORKERS <= MAX_WORKERS) };
        const { assert!(HDD_WORKERS >= 1 && HDD_WORKERS <= MAX_WORKERS) };
    }

    #[test]
    fn settings_round_trip_and_reject_unknown_input() {
        let settings = ScanSettings {
            schema_version: 1,
            profile: ScanProfile::Hdd,
        };
        let json = serde_json::to_string(&settings).unwrap();
        assert_eq!(json, r#"{"schemaVersion":1,"profile":"hdd"}"#);
        assert_eq!(
            serde_json::from_str::<ScanSettings>(&json).unwrap(),
            settings
        );
        for bad in [
            r#"{"schemaVersion":1,"profile":"nvme"}"#,
            r#"{"schemaVersion":1,"profile":"ssd","workers":64}"#,
            r#"{"schemaVersion":1}"#,
        ] {
            assert!(serde_json::from_str::<ScanSettings>(bad).is_err(), "{bad}");
        }
        assert!(
            !ScanSettings {
                schema_version: 2,
                profile: ScanProfile::Auto
            }
            .is_valid()
        );
    }

    #[test]
    fn explicit_profiles_never_query_the_device() {
        // A path that cannot resolve still yields the fixed profile counts.
        let missing = Path::new(r"Z:\definitely\missing\perf");
        assert_eq!(workers_for_path(ScanProfile::Ssd, missing), SSD_WORKERS);
        assert_eq!(workers_for_path(ScanProfile::Hdd, missing), HDD_WORKERS);
    }

    #[cfg(windows)]
    #[test]
    fn auto_profile_resolves_the_system_volume_within_bounds() {
        let windows = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
        let workers = workers_for_path(ScanProfile::Auto, Path::new(&windows));
        assert!((1..=MAX_WORKERS).contains(&workers));
    }

    /// Machine-specific check: `SUPA_PERF_SEEK_EXPECT="C:\=ssd;E:\=hdd"` (from Get-PhysicalDisk).
    #[cfg(windows)]
    #[test]
    #[ignore = "opt-in: needs known local media"]
    fn perf_seek_penalty_matches_known_media() {
        let Ok(expectations) = std::env::var("SUPA_PERF_SEEK_EXPECT") else {
            return;
        };
        for pair in expectations.split(';').filter(|pair| !pair.is_empty()) {
            let (path, media) = pair.split_once('=').expect("path=media");
            let expected = match media {
                "ssd" => Some(false),
                "hdd" => Some(true),
                _ => None,
            };
            let actual = seek_penalty(Path::new(path));
            println!("PERF seek_penalty {path} expected={expected:?} actual={actual:?}");
            assert_eq!(actual, expected, "{path}");
        }
    }
}
