//! Read-only uninstall registry inventory. Registry strings are metadata, never deletion authority.
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use windows_sys::Win32::{
    Foundation::{ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS},
    System::Registry::*,
};

const UNINSTALL: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";
const MAX_KEYS: usize = 4096;
const MAX_VALUE_BYTES: usize = 32 * 1024;
const MAX_TOTAL_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub(crate) enum Hive {
    Machine,
    User,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub(crate) enum View {
    Native64,
    Wow32,
}
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub(crate) struct RegistryLocation {
    pub hive: Hive,
    pub view: View,
    pub subkey: String,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct RegistryValue {
    pub kind: u32,
    pub bytes: Vec<u8>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InventoryError {
    Registry(u32),
    Malformed,
    Limit,
    Entropy,
    Cancelled,
}

/// Injectable read-only boundary; implementations must bound enumeration and value allocation.
pub(crate) trait RegistryReader: Send + Sync {
    // Fixtures may supply a bounded key list; native overrides enumeration to stream each key.
    fn keys(&self, hive: Hive, view: View) -> Result<Vec<String>, InventoryError>;
    fn enumerate(
        &self,
        hive: Hive,
        view: View,
        cancel: &CancellationToken,
        visitor: &mut dyn FnMut(String) -> bool,
    ) -> Result<(), InventoryError> {
        let keys = self.keys(hive, view)?;
        if keys.len() > MAX_KEYS {
            return Err(InventoryError::Limit);
        }
        for key in keys {
            if cancel.is_cancelled() || !visitor(key) {
                break;
            }
        }
        Ok(())
    }

    fn value(
        &self,
        key: &RegistryLocation,
        name: &str,
    ) -> Result<Option<RegistryValue>, InventoryError>;
}

pub use cleanup_core::storage::InstalledProgram;
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProgramQuery {
    #[serde(default)]
    pub name_contains: String,
    #[serde(default)]
    pub largest_first: bool,
}
impl ProgramQuery {
    pub(crate) fn validate(&self) -> Result<(), cleanup_core::storage::StorageError> {
        if self.name_contains.encode_utf16().count() > 128
            || self.name_contains.chars().any(char::is_control)
        {
            return Err(cleanup_core::storage::StorageError::InvalidRequest);
        }
        Ok(())
    }
}
use cleanup_core::CancellationToken;
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ProgramRecord {
    pub display: InstalledProgram,
    pub location: RegistryLocation,
}

/// Streams bounded, deduplicated locations. The sink sees enumeration progress BEFORE
/// metadata reads and may stop immediately. No whole inventory is exposed to IPC.
pub(crate) fn inventory_stream(
    reader: &dyn RegistryReader,
    cancel: &CancellationToken,
    sink: &mut dyn FnMut(usize, Option<Result<ProgramRecord, InventoryError>>) -> bool,
) -> Result<(), InventoryError> {
    let mut seen = HashSet::new();
    let mut total_bytes = 0usize;
    let mut visited = 0usize;
    let mut stopped = false;
    for hive in [Hive::Machine, Hive::User] {
        for view in [View::Native64, View::Wow32] {
            if stopped || cancel.is_cancelled() {
                return Ok(());
            }
            // HKCU Software is shared. Enumerate its one shared location once, never by name.
            if hive == Hive::User && view == View::Wow32 {
                continue;
            }
            reader.enumerate(hive, view, cancel, &mut |subkey| {
                if stopped || cancel.is_cancelled() {
                    return false;
                }
                visited += 1;
                if visited > MAX_KEYS {
                    stopped = true;
                    sink(visited - 1, Some(Err(InventoryError::Limit)));
                    return false;
                }
                if !sink(visited, None) {
                    stopped = true;
                    return false;
                }
                let location = RegistryLocation { hive, view, subkey };
                if location.subkey.is_empty()
                    || location.subkey.len() > 1024
                    || location.subkey.contains(['\\', '/', '\0'])
                {
                    stopped = !sink(visited, Some(Err(InventoryError::Malformed)));
                    return !stopped;
                }
                if !seen.insert((hive, view, location.subkey.to_lowercase())) {
                    return true;
                }
                let result = read_record(reader, &location, cancel, &mut total_bytes);
                match result {
                    Ok(Some(display)) => {
                        stopped = !sink(visited, Some(Ok(ProgramRecord { display, location })))
                    }
                    Ok(None) => {}
                    Err(InventoryError::Cancelled) => stopped = true,
                    Err(error) => {
                        let fatal =
                            matches!(error, InventoryError::Limit | InventoryError::Entropy);
                        stopped = !sink(visited, Some(Err(error))) || fatal;
                    }
                }
                !stopped && !cancel.is_cancelled()
            })?;
        }
    }
    Ok(())
}
fn read_record(
    reader: &dyn RegistryReader,
    location: &RegistryLocation,
    cancel: &CancellationToken,
    total_bytes: &mut usize,
) -> Result<Option<InstalledProgram>, InventoryError> {
    let mut get = |name: &str| -> Result<Option<RegistryValue>, InventoryError> {
        if cancel.is_cancelled() {
            return Err(InventoryError::Cancelled);
        }
        let value = reader.value(location, name)?;
        if let Some(value) = &value {
            *total_bytes = total_bytes
                .checked_add(value.bytes.len())
                .ok_or(InventoryError::Limit)?;
            if value.bytes.len() > MAX_VALUE_BYTES || *total_bytes > MAX_TOTAL_BYTES {
                return Err(InventoryError::Limit);
            }
        }
        Ok(value)
    };
    let Some(name) = text(get("DisplayName")?)? else {
        return Ok(None);
    };
    if name.trim().is_empty() {
        return Ok(None);
    }
    let publisher = text(get("Publisher")?)?;
    let version = text(get("DisplayVersion")?)?;
    let install_date = text(get("InstallDate")?)?;
    let estimated_size_bytes = match get("EstimatedSize")? {
        None => None,
        Some(value) if value.kind == REG_DWORD && value.bytes.len() == 4 => {
            Some(u64::from(u32::from_le_bytes(value.bytes.try_into().unwrap())) * 1024)
        }
        Some(_) => return Err(InventoryError::Malformed),
    };
    Ok(Some(InstalledProgram {
        program_id: super::opaque_id().map_err(|_| InventoryError::Entropy)?,
        name,
        publisher,
        version,
        install_date,
        estimated_size_bytes,
        last_used_at: None,
        leftover_support: "unsupportedUnknownOwnership".into(),
    }))
}

pub(crate) fn text(value: Option<RegistryValue>) -> Result<Option<String>, InventoryError> {
    let Some(value) = value else {
        return Ok(None);
    };
    // Expansion is deliberately unsupported; environment variables are not executable/path authority.
    if value.kind != REG_SZ || value.bytes.len() > MAX_VALUE_BYTES || value.bytes.len() % 2 != 0 {
        return Err(InventoryError::Malformed);
    }
    let mut units: Vec<u16> = value
        .bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    if units.last() == Some(&0) {
        units.pop();
    }
    if units.contains(&0) {
        return Err(InventoryError::Malformed);
    }
    String::from_utf16(&units)
        .map(Some)
        .map_err(|_| InventoryError::Malformed)
}

pub(crate) struct NativeRegistry;
struct OwnedKey(HKEY);
impl Drop for OwnedKey {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
fn open(hive: Hive, view: View, suffix: Option<&str>) -> Result<OwnedKey, InventoryError> {
    let path = wide(&suffix.map_or_else(
        || UNINSTALL.to_owned(),
        |suffix| format!("{UNINSTALL}\\{suffix}"),
    ));
    let mut key = std::ptr::null_mut();
    let status = unsafe {
        RegOpenKeyExW(
            match hive {
                Hive::Machine => HKEY_LOCAL_MACHINE,
                Hive::User => HKEY_CURRENT_USER,
            },
            path.as_ptr(),
            0,
            KEY_READ
                | match view {
                    View::Native64 => KEY_WOW64_64KEY,
                    View::Wow32 => KEY_WOW64_32KEY,
                },
            &mut key,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(InventoryError::Registry(status));
    }
    Ok(OwnedKey(key))
}
impl RegistryReader for NativeRegistry {
    fn keys(&self, hive: Hive, view: View) -> Result<Vec<String>, InventoryError> {
        let mut keys = Vec::new();
        self.enumerate(hive, view, &CancellationToken::new(), &mut |key| {
            keys.push(key);
            true
        })?;
        Ok(keys)
    }
    fn enumerate(
        &self,
        hive: Hive,
        view: View,
        cancel: &CancellationToken,
        visitor: &mut dyn FnMut(String) -> bool,
    ) -> Result<(), InventoryError> {
        let key = match open(hive, view, None) {
            Ok(key) => key,
            Err(InventoryError::Registry(2)) => return Ok(()),
            Err(error) => return Err(error),
        };
        for index in 0..=MAX_KEYS {
            if cancel.is_cancelled() {
                return Ok(());
            }
            // RegEnumKeyExW mutates length: reset both length and buffer every call.
            let mut buffer = [0u16; 1024];
            let mut length = buffer.len() as u32;
            let status = unsafe {
                RegEnumKeyExW(
                    key.0,
                    index as u32,
                    buffer.as_mut_ptr(),
                    &mut length,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if status == ERROR_NO_MORE_ITEMS {
                return Ok(());
            }
            if status == ERROR_MORE_DATA || index == MAX_KEYS {
                return Err(InventoryError::Limit);
            }
            if status != ERROR_SUCCESS {
                return Err(InventoryError::Registry(status));
            }
            if length as usize >= buffer.len() || buffer[..length as usize].contains(&0) {
                return Err(InventoryError::Malformed);
            }
            let key = String::from_utf16(&buffer[..length as usize])
                .map_err(|_| InventoryError::Malformed)?;
            if !visitor(key) {
                return Ok(());
            }
        }
        Err(InventoryError::Limit)
    }
    fn value(
        &self,
        location: &RegistryLocation,
        name: &str,
    ) -> Result<Option<RegistryValue>, InventoryError> {
        let key = open(location.hive, location.view, Some(&location.subkey))?;
        let name = wide(name);
        // A fixed bounded buffer avoids size-query races and unbounded retry/allocation.
        let mut bytes = vec![0; MAX_VALUE_BYTES];
        let mut size = bytes.len() as u32;
        let mut kind = 0;
        let status = unsafe {
            RegQueryValueExW(
                key.0,
                name.as_ptr(),
                std::ptr::null(),
                &mut kind,
                bytes.as_mut_ptr(),
                &mut size,
            )
        };
        if status == 2 {
            return Ok(None);
        }
        if status == ERROR_MORE_DATA || size as usize > bytes.len() {
            return Err(InventoryError::Limit);
        }
        if status != ERROR_SUCCESS {
            return Err(InventoryError::Registry(status));
        }
        bytes.truncate(size as usize);
        Ok(Some(RegistryValue { kind, bytes }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn string(value: &str) -> RegistryValue {
        RegistryValue {
            kind: REG_SZ,
            bytes: value
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect(),
        }
    }
    #[test]
    fn malformed_values_fail_closed() {
        for bytes in [vec![1], vec![0, 216], vec![65, 0, 0, 0, 66, 0]] {
            assert_eq!(
                text(Some(RegistryValue {
                    kind: REG_SZ,
                    bytes
                })),
                Err(InventoryError::Malformed)
            );
        }
        let mut expanded = string("%PATH%");
        expanded.kind = REG_EXPAND_SZ;
        assert_eq!(text(Some(expanded)), Err(InventoryError::Malformed));
        assert_eq!(text(Some(string("hello"))), Ok(Some("hello".into())));
    }
    struct Fixture;
    impl RegistryReader for Fixture {
        fn keys(&self, _: Hive, _: View) -> Result<Vec<String>, InventoryError> {
            Ok(vec!["one".into(), "two".into()])
        }
        fn value(
            &self,
            _: &RegistryLocation,
            name: &str,
        ) -> Result<Option<RegistryValue>, InventoryError> {
            Ok((name == "DisplayName").then(|| string("Same name")))
        }
    }
    #[test]
    fn shared_user_view_deduplicates_locations_not_display_names() {
        let mut programs = Vec::new();
        inventory_stream(&Fixture, &CancellationToken::new(), &mut |_, record| {
            if let Some(record) = record {
                programs.push(record.unwrap().display);
            }
            true
        })
        .unwrap();
        assert_eq!(programs.len(), 6);
        assert_eq!(
            programs
                .iter()
                .map(|p| &p.program_id)
                .collect::<HashSet<_>>()
                .len(),
            6
        );
        assert!(
            programs
                .iter()
                .all(|p| p.last_used_at.is_none() && p.estimated_size_bytes.is_none())
        );
    }
}
