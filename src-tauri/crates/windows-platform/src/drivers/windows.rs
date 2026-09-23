//! Real SetupAPI-backed reader and writer. No process is ever launched.

use std::{collections::HashMap, ffi::c_void, io, path::PathBuf, ptr};

use cleanup_core::system_change::DriverPackageName;
use windows_sys::Win32::{
    Devices::DeviceAndDriverInstallation::{
        DICS_FLAG_GLOBAL, DIGCF_ALLCLASSES, DIGCF_PRESENT, DIREG_DRV, HDEVINFO, INF_STYLE_WIN4,
        INFCONTEXT, SP_DEVINFO_DATA, SetupCloseInfFile, SetupDiDestroyDeviceInfoList,
        SetupDiEnumDeviceInfo, SetupDiGetClassDevsW, SetupDiOpenDevRegKey, SetupFindFirstLineW,
        SetupGetInfPublishedNameW, SetupGetStringFieldW, SetupOpenInfFileW, SetupUninstallOEMInfW,
    },
    Foundation::{
        ERROR_ACCESS_DENIED, ERROR_INF_IN_USE_BY_DEVICES, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS,
        GetLastError, INVALID_HANDLE_VALUE,
    },
    System::{
        Registry::{HKEY, KEY_READ, REG_EXPAND_SZ, REG_SZ, RegCloseKey, RegQueryValueExW},
        SystemInformation::{GetSystemDirectoryW, GetWindowsDirectoryW},
    },
};

use super::{
    BoundPackages, DriverReader, DriverWriter, MAX_DEVICES, MAX_PACKAGES, RawDriverPackage, bounded,
};
use crate::system_change::AdapterError;

/// Maximum UTF-16 units read from any INF field, store path, or registry value.
const MAX_STRING_UNITS: u32 = 1024;
const MAX_DIRECTORY_ENTRIES: usize = 20_000;

pub struct WindowsDriverReader;
pub struct WindowsDriverWriter;

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn from_wide(buffer: &[u16]) -> String {
    let end = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}

fn map_win32(code: u32) -> AdapterError {
    if code == ERROR_ACCESS_DENIED {
        AdapterError::Denied
    } else {
        AdapterError::Failed
    }
}

fn map_io(error: &io::Error) -> AdapterError {
    if error.kind() == io::ErrorKind::PermissionDenied {
        AdapterError::Denied
    } else {
        AdapterError::Failed
    }
}

fn directory(
    get: unsafe extern "system" fn(*mut u16, u32) -> u32,
) -> Result<PathBuf, AdapterError> {
    let mut buffer = [0_u16; 520];
    // SAFETY: buffer is writable for its full length.
    let length = unsafe { get(buffer.as_mut_ptr(), buffer.len() as u32) };
    if length == 0 || length as usize >= buffer.len() {
        return Err(AdapterError::Failed);
    }
    Ok(PathBuf::from(String::from_utf16_lossy(
        &buffer[..length as usize],
    )))
}

fn inf_directory() -> Result<PathBuf, AdapterError> {
    directory(GetWindowsDirectoryW).map(|windows| windows.join("INF"))
}

fn system_directory() -> Result<PathBuf, AdapterError> {
    directory(GetSystemDirectoryW)
}

/// An open INF handle, closed exactly once.
struct InfFile(*mut c_void);

impl Drop for InfFile {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful SetupOpenInfFileW call.
        unsafe { SetupCloseInfFile(self.0) };
    }
}

impl InfFile {
    fn open(path: &str) -> Option<Self> {
        let path = wide(path);
        let mut error_line = 0_u32;
        // SAFETY: path is NUL-terminated; a null class accepts any class.
        let handle = unsafe {
            SetupOpenInfFileW(path.as_ptr(), ptr::null(), INF_STYLE_WIN4, &mut error_line)
        };
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            None
        } else {
            Some(Self(handle))
        }
    }

    /// Field `index` of the first `entry` line in `section`.
    fn field(&self, section: &str, entry: &str, index: u32) -> Option<String> {
        let section = wide(section);
        let entry = wide(entry);
        let mut context = INFCONTEXT::default();
        // SAFETY: the handle is open; section and entry are NUL-terminated.
        if unsafe { SetupFindFirstLineW(self.0, section.as_ptr(), entry.as_ptr(), &mut context) }
            == 0
        {
            return None;
        }
        let mut buffer = vec![0_u16; MAX_STRING_UNITS as usize];
        let mut required = 0_u32;
        // SAFETY: context was filled by SetupFindFirstLineW; buffer is writable.
        let ok = unsafe {
            SetupGetStringFieldW(
                &context,
                index,
                buffer.as_mut_ptr(),
                MAX_STRING_UNITS,
                &mut required,
            )
        };
        (ok != 0).then(|| from_wide(&buffer))
    }

    /// A `[Version]` value with `%token%` references resolved from `[Strings]`.
    fn version_value(&self, key: &str, index: u32) -> Option<String> {
        let value = self.field("Version", key, index)?;
        let trimmed = value.trim();
        let resolved = match trimmed
            .strip_prefix('%')
            .and_then(|rest| rest.strip_suffix('%'))
        {
            Some(token) if !token.is_empty() && !token.contains('%') => {
                self.field("Strings", token, 1)?
            }
            _ => value,
        };
        bounded(&resolved)
    }
}

/// Map published name → original INF name by walking the driver store.
///
/// `SetupGetInfPublishedNameW` needs the `Win32_System_Diagnostics_Debug`
/// windows-sys feature, which this crate does not enable. The inverse
/// `SetupGetInfPublishedNameW` is available, so each store entry
/// `...\FileRepository\<orig>.inf_<arch>_<hash>\<orig>.inf` is resolved to its
/// published name instead. Failures leave the original name unknown.
fn original_names() -> HashMap<String, String> {
    let mut names = HashMap::new();
    let Ok(repository) =
        system_directory().map(|system| system.join("DriverStore").join("FileRepository"))
    else {
        return names;
    };
    let Ok(entries) = std::fs::read_dir(&repository) else {
        return names;
    };
    for entry in entries.take(MAX_DIRECTORY_ENTRIES).flatten() {
        let Some(directory) = entry.file_name().to_str().map(str::to_ascii_lowercase) else {
            continue;
        };
        let Some(stem) = directory.find(".inf_").map(|end| &directory[..end]) else {
            continue;
        };
        let original = format!("{stem}.inf");
        let Some(original) = bounded(&original) else {
            continue;
        };
        let store_inf = entry.path().join(&original).to_string_lossy().into_owned();
        if let Some(published) = published_name(&store_inf) {
            names.insert(published, original);
        }
    }
    names
}

fn published_name(store_inf: &str) -> Option<String> {
    let path = wide(store_inf);
    let mut buffer = vec![0_u16; MAX_STRING_UNITS as usize];
    let mut required = 0_u32;
    // SAFETY: path is NUL-terminated; buffer is writable for the size given.
    let ok = unsafe {
        SetupGetInfPublishedNameW(
            path.as_ptr(),
            buffer.as_mut_ptr(),
            MAX_STRING_UNITS,
            &mut required,
        )
    };
    if ok == 0 {
        return None;
    }
    let published = from_wide(&buffer);
    let file_name = published
        .rsplit(['/', char::from(0x5c)])
        .next()?
        .to_ascii_lowercase();
    DriverPackageName::parse(file_name).ok().map(String::from)
}

fn read_package(
    directory: &std::path::Path,
    originals: &HashMap<String, String>,
    name: DriverPackageName,
) -> RawDriverPackage {
    let path = directory.join(name.as_str()).to_string_lossy().into_owned();
    let inf = InfFile::open(&path);
    let value =
        |entry: &str, index: u32| inf.as_ref().and_then(|inf| inf.version_value(entry, index));
    RawDriverPackage {
        original_name: originals.get(name.as_str()).cloned(),
        provider: value("Provider", 1),
        class: value("Class", 1),
        driver_date: value("DriverVer", 1),
        driver_version: value("DriverVer", 2),
        published_name: name,
    }
}

/// An owned device information set.
struct DeviceInfoSet(HDEVINFO);

impl Drop for DeviceInfoSet {
    fn drop(&mut self) {
        // SAFETY: the set came from a successful SetupDiGetClassDevsW call.
        unsafe { SetupDiDestroyDeviceInfoList(self.0) };
    }
}

/// An HKEY returned by SetupAPI. `win_registry` only opens keys by path, so
/// this small wrapper owns and reads the foreign handle.
struct DeviceKey(HKEY);

impl Drop for DeviceKey {
    fn drop(&mut self) {
        // SAFETY: the key came from a successful SetupDiOpenDevRegKey call.
        unsafe { RegCloseKey(self.0) };
    }
}

impl DeviceKey {
    fn string(&self, name: &str) -> Option<String> {
        let name = wide(name);
        let mut kind = 0_u32;
        let mut data = vec![0_u16; MAX_STRING_UNITS as usize];
        let mut size = MAX_STRING_UNITS * 2;
        // SAFETY: data has `size` writable bytes and name is NUL-terminated.
        let code = unsafe {
            RegQueryValueExW(
                self.0,
                name.as_ptr(),
                ptr::null(),
                &mut kind,
                data.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if code != ERROR_SUCCESS || !(kind == REG_SZ || kind == REG_EXPAND_SZ) {
            return None;
        }
        data.truncate((size as usize / 2).min(data.len()));
        bounded(&from_wide(&data))
    }
}

impl DriverReader for WindowsDriverReader {
    fn packages(&self) -> Result<Vec<RawDriverPackage>, AdapterError> {
        let directory = inf_directory()?;
        let entries = std::fs::read_dir(&directory).map_err(|error| map_io(&error))?;
        let mut names = Vec::new();
        for entry in entries.take(MAX_DIRECTORY_ENTRIES) {
            let entry = entry.map_err(|error| map_io(&error))?;
            let Some(file_name) = entry.file_name().to_str().map(str::to_ascii_lowercase) else {
                continue;
            };
            if let Ok(name) = DriverPackageName::parse(file_name) {
                names.push(name);
                if names.len() >= MAX_PACKAGES {
                    break;
                }
            }
        }
        names.sort();
        names.dedup();
        let originals = original_names();
        Ok(names
            .into_iter()
            .map(|name| read_package(&directory, &originals, name))
            .collect())
    }

    fn bound_packages(&self) -> Result<BoundPackages, AdapterError> {
        // SAFETY: null class/enumerator/parent are valid with DIGCF_ALLCLASSES.
        let set = unsafe {
            SetupDiGetClassDevsW(
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
                DIGCF_PRESENT | DIGCF_ALLCLASSES,
            )
        };
        if set == INVALID_HANDLE_VALUE as HDEVINFO {
            // SAFETY: reads the calling thread's last-error value.
            return Err(map_win32(unsafe { GetLastError() }));
        }
        let set = DeviceInfoSet(set);
        let mut bound = BoundPackages::default();
        for index in 0..MAX_DEVICES as u32 {
            let mut device = SP_DEVINFO_DATA {
                cbSize: size_of::<SP_DEVINFO_DATA>() as u32,
                ..Default::default()
            };
            // SAFETY: the set is open and device.cbSize is initialized.
            if unsafe { SetupDiEnumDeviceInfo(set.0, index, &mut device) } == 0 {
                // SAFETY: reads the calling thread's last-error value.
                let code = unsafe { GetLastError() };
                if code == ERROR_NO_MORE_ITEMS {
                    bound.complete = true;
                    return Ok(bound);
                }
                return Err(map_win32(code));
            }
            // SAFETY: device was filled by SetupDiEnumDeviceInfo for this set.
            let key = unsafe {
                SetupDiOpenDevRegKey(set.0, &device, DICS_FLAG_GLOBAL, 0, DIREG_DRV, KEY_READ)
            };
            if key.is_null() || std::ptr::eq(key, INVALID_HANDLE_VALUE as HKEY) {
                // Devices without a driver key have no bound package.
                continue;
            }
            if let Some(inf) = DeviceKey(key).string("InfPath") {
                bound.names.insert(inf.to_ascii_lowercase());
            }
        }
        // Enumeration bound reached: bindings are incomplete.
        Ok(bound)
    }
}

impl DriverWriter for WindowsDriverWriter {
    fn uninstall(&self, name: &DriverPackageName) -> Result<(), AdapterError> {
        let name = wide(name.as_str());
        // SAFETY: name is NUL-terminated; flags 0 never forces deletion of a
        // package that devices still use; reserved must be null.
        if unsafe { SetupUninstallOEMInfW(name.as_ptr(), 0, ptr::null()) } != 0 {
            return Ok(());
        }
        // SAFETY: reads the calling thread's last-error value.
        Err(match unsafe { GetLastError() } {
            ERROR_ACCESS_DENIED => AdapterError::Denied,
            ERROR_INF_IN_USE_BY_DEVICES => AdapterError::Failed,
            _ => AdapterError::Failed,
        })
    }
}
