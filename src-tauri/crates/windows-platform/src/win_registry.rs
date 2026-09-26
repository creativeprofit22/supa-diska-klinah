//! Minimal typed registry access shared by system-management adapters.
//! Callers pass compile-time key paths from their catalogs; this module never
//! accepts keys from the webview.

use std::{io, ptr};

use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS},
    System::Registry::{
        HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_READ, KEY_SET_VALUE,
        KEY_WOW64_64KEY, REG_BINARY, REG_DWORD, REG_EXPAND_SZ, REG_OPTION_NON_VOLATILE, REG_SZ,
        RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegEnumValueW, RegOpenKeyExW,
        RegQueryValueExW, RegSetValueExW,
    },
};

const MAX_VALUE_BYTES: usize = 64 * 1024;
const MAX_VALUES: usize = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Hive {
    CurrentUser,
    LocalMachine,
}

impl Hive {
    fn raw(self) -> HKEY {
        match self {
            Self::CurrentUser => HKEY_CURRENT_USER,
            Self::LocalMachine => HKEY_LOCAL_MACHINE,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RegistryData {
    Dword(u32),
    String(String),
    Binary(Vec<u8>),
    Other,
}

pub struct RegistryKey(HKEY);

impl Drop for RegistryKey {
    fn drop(&mut self) {
        // SAFETY: the handle was opened by this wrapper and is closed once.
        unsafe { RegCloseKey(self.0) };
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn status(code: u32) -> io::Result<()> {
    if code == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(code as i32))
    }
}

pub fn is_not_found(error: &io::Error) -> bool {
    error.raw_os_error() == Some(ERROR_FILE_NOT_FOUND as i32)
}

impl RegistryKey {
    /// Open for reading. `Ok(None)` when the key does not exist.
    pub fn open_read(hive: Hive, path: &str) -> io::Result<Option<Self>> {
        Self::open(hive, path, KEY_READ | KEY_QUERY_VALUE)
    }

    pub fn open_write(hive: Hive, path: &str) -> io::Result<Option<Self>> {
        Self::open(hive, path, KEY_READ | KEY_SET_VALUE)
    }

    /// Open for writing, creating the key when missing.
    pub fn create_write(hive: Hive, path: &str) -> io::Result<Self> {
        let path = wide(path);
        let mut key = ptr::null_mut();
        // SAFETY: path is NUL-terminated and key receives an owned handle.
        status(unsafe {
            RegCreateKeyExW(
                hive.raw(),
                path.as_ptr(),
                0,
                ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_READ | KEY_SET_VALUE | KEY_WOW64_64KEY,
                ptr::null(),
                &mut key,
                ptr::null_mut(),
            )
        })?;
        Ok(Self(key))
    }

    fn open(hive: Hive, path: &str, access: u32) -> io::Result<Option<Self>> {
        let path = wide(path);
        let mut key = ptr::null_mut();
        // SAFETY: path is NUL-terminated and key receives an owned handle.
        match unsafe {
            RegOpenKeyExW(
                hive.raw(),
                path.as_ptr(),
                0,
                access | KEY_WOW64_64KEY,
                &mut key,
            )
        } {
            ERROR_SUCCESS => Ok(Some(Self(key))),
            ERROR_FILE_NOT_FOUND => Ok(None),
            code => Err(io::Error::from_raw_os_error(code as i32)),
        }
    }

    /// Read a value. `Ok(None)` when it does not exist.
    pub fn value(&self, name: &str) -> io::Result<Option<RegistryData>> {
        let name = wide(name);
        let mut kind = 0;
        let mut size = 0_u32;
        // SAFETY: querying the size with null data is permitted.
        match unsafe {
            RegQueryValueExW(
                self.0,
                name.as_ptr(),
                ptr::null(),
                &mut kind,
                ptr::null_mut(),
                &mut size,
            )
        } {
            ERROR_SUCCESS => {}
            ERROR_FILE_NOT_FOUND => return Ok(None),
            code => return Err(io::Error::from_raw_os_error(code as i32)),
        }
        if size as usize > MAX_VALUE_BYTES {
            return Ok(Some(RegistryData::Other));
        }
        let mut data = vec![0_u8; size as usize];
        // SAFETY: data has `size` writable bytes.
        status(unsafe {
            RegQueryValueExW(
                self.0,
                name.as_ptr(),
                ptr::null(),
                &mut kind,
                data.as_mut_ptr(),
                &mut size,
            )
        })?;
        data.truncate(size as usize);
        Ok(Some(decode(kind, data)))
    }

    pub fn dword(&self, name: &str) -> io::Result<Option<u32>> {
        Ok(match self.value(name)? {
            Some(RegistryData::Dword(value)) => Some(value),
            _ => None,
        })
    }

    pub fn string(&self, name: &str) -> io::Result<Option<String>> {
        Ok(match self.value(name)? {
            Some(RegistryData::String(value)) => Some(value),
            _ => None,
        })
    }

    /// Enumerate value names and data (bounded).
    pub fn values(&self) -> io::Result<Vec<(String, RegistryData)>> {
        let mut values = Vec::new();
        for index in 0.. {
            if index as usize >= MAX_VALUES {
                break;
            }
            let mut name = vec![0_u16; 16_384];
            let mut name_len = name.len() as u32;
            let mut kind = 0;
            let mut data = vec![0_u8; 8192];
            let mut data_len = data.len() as u32;
            // SAFETY: buffers and lengths describe writable storage.
            let code = unsafe {
                RegEnumValueW(
                    self.0,
                    index,
                    name.as_mut_ptr(),
                    &mut name_len,
                    ptr::null(),
                    &mut kind,
                    data.as_mut_ptr(),
                    &mut data_len,
                )
            };
            match code {
                ERROR_SUCCESS => {
                    data.truncate(data_len as usize);
                    let name = String::from_utf16_lossy(&name[..name_len as usize]);
                    values.push((name, decode(kind, data)));
                }
                ERROR_MORE_DATA => {
                    let name = String::from_utf16_lossy(&name[..name_len.min(16_383) as usize]);
                    values.push((name, RegistryData::Other));
                }
                ERROR_NO_MORE_ITEMS => break,
                code => return Err(io::Error::from_raw_os_error(code as i32)),
            }
        }
        Ok(values)
    }

    pub fn set_dword(&self, name: &str, value: u32) -> io::Result<()> {
        self.set_raw(name, REG_DWORD, &value.to_le_bytes())
    }

    pub fn set_binary(&self, name: &str, value: &[u8]) -> io::Result<()> {
        self.set_raw(name, REG_BINARY, value)
    }

    fn set_raw(&self, name: &str, kind: u32, data: &[u8]) -> io::Result<()> {
        let name = wide(name);
        // SAFETY: name is NUL-terminated and data is a valid slice.
        status(unsafe {
            RegSetValueExW(
                self.0,
                name.as_ptr(),
                0,
                kind,
                data.as_ptr(),
                data.len() as u32,
            )
        })
    }

    /// Delete a value; succeeds when it is already absent.
    pub fn delete_value(&self, name: &str) -> io::Result<()> {
        let name = wide(name);
        // SAFETY: name is NUL-terminated.
        match unsafe { RegDeleteValueW(self.0, name.as_ptr()) } {
            ERROR_SUCCESS | ERROR_FILE_NOT_FOUND => Ok(()),
            code => Err(io::Error::from_raw_os_error(code as i32)),
        }
    }
}

fn decode(kind: u32, data: Vec<u8>) -> RegistryData {
    match kind {
        REG_DWORD if data.len() == 4 => {
            RegistryData::Dword(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
        }
        REG_SZ | REG_EXPAND_SZ => {
            let units: Vec<u16> = data
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .take_while(|unit| *unit != 0)
                .collect();
            RegistryData::String(String::from_utf16_lossy(&units))
        }
        REG_BINARY => RegistryData::Binary(data),
        _ => RegistryData::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_well_known_machine_values_and_missing_keys() {
        let key = RegistryKey::open_read(
            Hive::LocalMachine,
            r"SOFTWARE\Microsoft\Windows NT\CurrentVersion",
        )
        .unwrap()
        .unwrap();
        assert!(key.string("CurrentBuildNumber").unwrap().is_some());
        assert!(key.value("SupaDiskaKlinahMissingValue").unwrap().is_none());
        assert!(
            RegistryKey::open_read(Hive::CurrentUser, r"Software\SupaDiskaKlinah\Missing\Key")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn decodes_dword_strings_and_binary() {
        assert_eq!(
            decode(REG_DWORD, 7_u32.to_le_bytes().to_vec()),
            RegistryData::Dword(7)
        );
        assert_eq!(
            decode(REG_SZ, vec![b'a', 0, 0, 0]),
            RegistryData::String("a".into())
        );
        assert_eq!(
            decode(REG_BINARY, vec![2, 0]),
            RegistryData::Binary(vec![2, 0])
        );
        assert_eq!(decode(REG_DWORD, vec![1]), RegistryData::Other);
    }
}
