//! Real Windows implementations of the power reader and writer.

use std::{
    io,
    path::{Path, PathBuf},
    ptr,
};

use cleanup_core::system_change::{PowerSchemeId, UnsupportedReason};
use windows_sys::{
    Win32::{
        Foundation::{
            ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, HLOCAL,
            LocalFree,
        },
        Storage::FileSystem::{
            GetFileAttributesExW, GetFileExInfoStandard, WIN32_FILE_ATTRIBUTE_DATA,
        },
        System::{
            Power::{
                ACCESS_SCHEME, GetPwrCapabilities, PowerEnumerate, PowerGetActiveScheme,
                PowerReadFriendlyName, PowerSetActiveScheme, SYSTEM_POWER_CAPABILITIES,
            },
            SystemInformation::GetSystemDirectoryW,
        },
    },
    core::GUID,
};

use super::{
    HibernationFacts, MAX_SCHEME_NAME_CHARS, MAX_SCHEMES, PowerReader, PowerSchemeEntry,
    PowerWriter,
};
use crate::{
    system_change::AdapterError,
    win_registry::{Hive, RegistryKey},
};

const POWER_KEY: &str = r"SYSTEM\CurrentControlSet\Control\Power";
const HIBERNATE_ENABLED: &str = "HibernateEnabled";
const NAME_BUFFER_BYTES: usize = 1024;

#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsPowerReader;

#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsPowerWriter;

pub(crate) fn win32_error(code: u32) -> AdapterError {
    match code {
        ERROR_ACCESS_DENIED => AdapterError::Denied,
        ERROR_FILE_NOT_FOUND => AdapterError::Unsupported(UnsupportedReason::NotPresent),
        _ => AdapterError::Failed,
    }
}

fn io_error(error: &io::Error) -> AdapterError {
    match error.raw_os_error() {
        Some(code) => win32_error(code as u32),
        None => AdapterError::Failed,
    }
}

impl PowerReader for WindowsPowerReader {
    fn hibernation(&self) -> Result<HibernationFacts, AdapterError> {
        let mut caps = SYSTEM_POWER_CAPABILITIES::default();
        // SAFETY: caps is a writable, correctly sized SYSTEM_POWER_CAPABILITIES.
        if !unsafe { GetPwrCapabilities(&mut caps) } {
            return Err(AdapterError::Unsupported(UnsupportedReason::ApiUnavailable));
        }
        let hibernate_enabled = match RegistryKey::open_read(Hive::LocalMachine, POWER_KEY) {
            Ok(Some(key)) => key.dword(HIBERNATE_ENABLED).map_err(|e| io_error(&e))?,
            Ok(None) => None,
            Err(error) => return Err(io_error(&error)),
        };
        Ok(HibernationFacts {
            system_s4: caps.SystemS4,
            hiberfile_present: caps.HiberFilePresent,
            hibernate_enabled,
            hiberfile_bytes: hiberfile_bytes(),
        })
    }

    fn schemes(&self) -> Result<Vec<PowerSchemeEntry>, AdapterError> {
        let mut schemes = Vec::new();
        for index in 0..MAX_SCHEMES as u32 {
            let mut guid = GUID::default();
            let mut size = size_of::<GUID>() as u32;
            // SAFETY: guid provides `size` writable bytes; null root/subgroup
            // enumerate the schemes of the current user.
            let code = unsafe {
                PowerEnumerate(
                    ptr::null_mut(),
                    ptr::null(),
                    ptr::null(),
                    ACCESS_SCHEME,
                    index,
                    (&mut guid as *mut GUID).cast(),
                    &mut size,
                )
            };
            match code {
                ERROR_SUCCESS => {}
                ERROR_NO_MORE_ITEMS => break,
                code => return Err(win32_error(code)),
            }
            let id = super::parse_scheme(&format_guid(&guid)).ok_or(AdapterError::Failed)?;
            let name = friendly_name(&guid).unwrap_or_else(|| id.as_str().to_owned());
            schemes.push(PowerSchemeEntry { id, name });
        }
        Ok(schemes)
    }

    fn active_scheme(&self) -> Result<PowerSchemeId, AdapterError> {
        let mut active: *mut GUID = ptr::null_mut();
        // SAFETY: active receives a LocalAlloc'd GUID pointer freed below.
        let code = unsafe { PowerGetActiveScheme(ptr::null_mut(), &mut active) };
        if code != ERROR_SUCCESS {
            return Err(win32_error(code));
        }
        if active.is_null() {
            return Err(AdapterError::Failed);
        }
        // SAFETY: on success `active` points to a valid GUID.
        let guid = unsafe { *active };
        // SAFETY: the pointer was allocated by PowerGetActiveScheme and is freed once.
        unsafe { LocalFree(active as HLOCAL) };
        super::parse_scheme(&format_guid(&guid)).ok_or(AdapterError::Failed)
    }
}

impl PowerWriter for WindowsPowerWriter {
    fn set_active_scheme(&self, scheme: &PowerSchemeId) -> Result<(), AdapterError> {
        let guid = parse_guid(scheme.as_str()).ok_or(AdapterError::Failed)?;
        // SAFETY: guid is a valid GUID for the duration of the call.
        match unsafe { PowerSetActiveScheme(ptr::null_mut(), &guid) } {
            ERROR_SUCCESS => Ok(()),
            code => Err(win32_error(code)),
        }
    }
}

fn friendly_name(guid: &GUID) -> Option<String> {
    let mut buffer = [0_u8; NAME_BUFFER_BYTES];
    let mut size = buffer.len() as u32;
    // SAFETY: buffer has `size` writable bytes and guid is valid.
    let code = unsafe {
        PowerReadFriendlyName(
            ptr::null_mut(),
            guid,
            ptr::null(),
            ptr::null(),
            buffer.as_mut_ptr(),
            &mut size,
        )
    };
    if code != ERROR_SUCCESS {
        return None;
    }
    let len = (size as usize).min(buffer.len());
    let units: Vec<u16> = buffer[..len]
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .take_while(|unit| *unit != 0)
        .collect();
    let name: String = String::from_utf16_lossy(&units)
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_SCHEME_NAME_CHARS)
        .collect();
    let name = name.trim().to_owned();
    (!name.is_empty()).then_some(name)
}

pub(crate) fn format_guid(guid: &GUID) -> String {
    let d = guid.data4;
    format!(
        "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        guid.data1, guid.data2, guid.data3, d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]
    )
}

pub(crate) fn parse_guid(value: &str) -> Option<GUID> {
    let id = super::parse_scheme(value)?;
    let hex: String = id.as_str().chars().filter(|c| *c != '-').collect();
    u128::from_str_radix(&hex, 16).ok().map(GUID::from_u128)
}

/// `GetSystemDirectoryW`, validated to be an absolute drive path.
pub(crate) fn system_directory() -> Result<PathBuf, AdapterError> {
    let mut buffer = [0_u16; 1024];
    // SAFETY: buffer has the stated number of writable UTF-16 units.
    let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    if length == 0 || length >= buffer.len() {
        return Err(AdapterError::Failed);
    }
    let path = String::from_utf16(&buffer[..length]).map_err(|_| AdapterError::Failed)?;
    let path = PathBuf::from(path);
    if !path.is_absolute() {
        return Err(AdapterError::Failed);
    }
    Ok(path)
}

/// `X:` of the system drive: `%SystemDrive%` when well-formed, otherwise the
/// drive of the system directory.
fn system_drive() -> Option<String> {
    let valid = |value: &str| {
        let bytes = value.as_bytes();
        bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
    };
    if let Some(value) = std::env::var_os("SystemDrive").and_then(|v| v.into_string().ok())
        && valid(&value)
    {
        return Some(value);
    }
    let dir = system_directory().ok()?;
    let prefix: String = dir.to_str()?.chars().take(2).collect();
    valid(&prefix).then_some(prefix)
}

fn hiberfile_bytes() -> Option<u64> {
    let path = Path::new(&format!("{}\\", system_drive()?)).join("hiberfil.sys");
    let wide: Vec<u16> = path
        .to_str()?
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut data = WIN32_FILE_ATTRIBUTE_DATA::default();
    // SAFETY: wide is NUL-terminated and data is a writable
    // WIN32_FILE_ATTRIBUTE_DATA as required by GetFileExInfoStandard.
    let ok = unsafe {
        GetFileAttributesExW(
            wide.as_ptr(),
            GetFileExInfoStandard,
            (&mut data as *mut WIN32_FILE_ATTRIBUTE_DATA).cast(),
        )
    };
    (ok != 0).then(|| (u64::from(data.nFileSizeHigh) << 32) | u64::from(data.nFileSizeLow))
}
