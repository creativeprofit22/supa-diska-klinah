//! Real Windows reader/writer: typed registry helper, known Startup folders,
//! and the Task Scheduler COM API (read-only).

use std::{io, os::windows::ffi::OsStringExt, path::PathBuf};

use cleanup_core::system_change::{StartupScope, UnsupportedReason};
use windows::{
    Win32::{
        Foundation::{E_ACCESSDENIED, REGDB_E_CLASSNOTREG, RPC_E_CHANGED_MODE},
        System::{
            Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx},
            TaskScheduler::{
                ITaskFolder, ITaskService, TASK_ENUM_HIDDEN, TASK_TRIGGER_LOGON,
                TASK_TRIGGER_TYPE2, TaskScheduler,
            },
            Variant::VARIANT,
        },
    },
    core::BSTR,
};
use windows_sys::Win32::{
    Foundation::ERROR_ACCESS_DENIED,
    System::Com::{CoTaskMemFree, CoUninitialize},
    UI::Shell::{FOLDERID_CommonStartup, FOLDERID_Startup, SHGetKnownFolderPath},
};

use super::{
    LogonTask, MAX_FOLDER_ENTRIES, MAX_TASK_DEPTH, MAX_TASKS, SourceEntry, StartupReader,
    StartupWriter,
};
use crate::{
    system_change::AdapterError,
    win_registry::{Hive, RegistryData, RegistryKey},
};

const MAX_TASK_FOLDERS: usize = 2000;

pub struct WindowsStartupStore;

fn io_error(error: io::Error) -> AdapterError {
    if error.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) {
        AdapterError::Denied
    } else {
        AdapterError::Failed
    }
}

fn com_error(error: windows::core::Error) -> AdapterError {
    let code = error.code();
    if code == E_ACCESSDENIED {
        AdapterError::Denied
    } else if code == REGDB_E_CLASSNOTREG {
        AdapterError::Unsupported(UnsupportedReason::ApiUnavailable)
    } else {
        AdapterError::Failed
    }
}

impl StartupReader for WindowsStartupStore {
    fn registry_values(&self, hive: Hive, path: &str) -> Result<Vec<SourceEntry>, AdapterError> {
        let Some(key) = RegistryKey::open_read(hive, path).map_err(io_error)? else {
            return Ok(Vec::new());
        };
        Ok(key
            .values()
            .map_err(io_error)?
            .into_iter()
            .filter(|(name, _)| !name.is_empty())
            .map(|(name, data)| SourceEntry {
                name,
                command: match data {
                    RegistryData::String(command) => command,
                    _ => String::new(),
                },
            })
            .collect())
    }

    fn folder_files(&self, scope: StartupScope) -> Result<Vec<SourceEntry>, AdapterError> {
        let folder = startup_folder(scope)?;
        let entries = match std::fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(io_error(error)),
        };
        let mut files = Vec::new();
        for entry in entries.take(MAX_FOLDER_ENTRIES) {
            let Ok(entry) = entry else { continue };
            if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if name.eq_ignore_ascii_case("desktop.ini") {
                continue;
            }
            files.push(SourceEntry {
                name,
                command: entry.path().display().to_string(),
            });
        }
        Ok(files)
    }

    fn approved(
        &self,
        hive: Hive,
        path: &str,
        name: &str,
    ) -> Result<Option<Vec<u8>>, AdapterError> {
        let Some(key) = RegistryKey::open_read(hive, path).map_err(io_error)? else {
            return Ok(None);
        };
        Ok(match key.value(name).map_err(io_error)? {
            Some(RegistryData::Binary(data)) => Some(data),
            _ => None,
        })
    }

    fn logon_tasks(&self) -> Result<Vec<LogonTask>, AdapterError> {
        let _com = ComApartment::initialize()?;
        // SAFETY: COM is initialized on this thread for the lifetime of `_com`.
        let service: ITaskService =
            unsafe { CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER) }
                .map_err(com_error)?;
        let empty = VARIANT::default();
        // SAFETY: empty VARIANTs request the local machine and current user.
        unsafe { service.Connect(&empty, &empty, &empty, &empty) }.map_err(com_error)?;
        // SAFETY: the root folder path is a valid BSTR.
        let root = unsafe { service.GetFolder(&BSTR::from("\\")) }.map_err(com_error)?;
        Ok(collect_logon_tasks(root))
    }
}

impl StartupWriter for WindowsStartupStore {
    fn set_approved(
        &self,
        hive: Hive,
        path: &str,
        name: &str,
        data: &[u8],
    ) -> Result<(), AdapterError> {
        RegistryKey::create_write(hive, path)
            .map_err(io_error)?
            .set_binary(name, data)
            .map_err(io_error)
    }
}

fn startup_folder(scope: StartupScope) -> Result<PathBuf, AdapterError> {
    let id = match scope {
        StartupScope::User => FOLDERID_Startup,
        StartupScope::Machine => FOLDERID_CommonStartup,
    };
    struct ShellAllocation(*mut u16);
    impl Drop for ShellAllocation {
        fn drop(&mut self) {
            // SAFETY: SHGetKnownFolderPath documents CoTaskMemFree as the matching
            // deallocator; freeing null is permitted.
            unsafe { CoTaskMemFree(self.0.cast()) };
        }
    }
    let mut raw = std::ptr::null_mut();
    // SAFETY: id is a valid KNOWNFOLDERID and raw receives a shell allocation.
    let result = unsafe { SHGetKnownFolderPath(&id, 0, std::ptr::null_mut(), &mut raw) };
    let allocation = ShellAllocation(raw);
    if result < 0 || allocation.0.is_null() {
        return Err(AdapterError::Unsupported(UnsupportedReason::ApiUnavailable));
    }
    let mut len = 0;
    // SAFETY: a successful call returns a NUL-terminated UTF-16 string; the scan is bounded.
    while len < 32_768 && unsafe { *allocation.0.add(len) } != 0 {
        len += 1;
    }
    if len >= 32_768 {
        return Err(AdapterError::Failed);
    }
    // SAFETY: `len` UTF-16 units before the terminator are readable.
    let units = unsafe { std::slice::from_raw_parts(allocation.0, len) };
    Ok(PathBuf::from(std::ffi::OsString::from_wide(units)))
}

/// Per-call COM initialization. Uninitializes only when this call
/// initialized (S_OK/S_FALSE); `RPC_E_CHANGED_MODE` is usable as-is.
struct ComApartment {
    owned: bool,
}

impl ComApartment {
    fn initialize() -> Result<Self, AdapterError> {
        // SAFETY: a null reserved pointer and COINIT_MULTITHREADED are documented arguments.
        let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if result == RPC_E_CHANGED_MODE {
            Ok(Self { owned: false })
        } else if result.is_ok() {
            Ok(Self { owned: true })
        } else {
            Err(AdapterError::Failed)
        }
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.owned {
            // SAFETY: balanced with the successful CoInitializeEx on this thread.
            unsafe { CoUninitialize() };
        }
    }
}

/// Depth-first walk bounded by folder depth, folder count, and task count.
/// Folders or tasks that cannot be read (for example access denied) are
/// skipped.
fn collect_logon_tasks(root: ITaskFolder) -> Vec<LogonTask> {
    let mut found = Vec::new();
    let mut examined = 0_usize;
    let mut visited = 0_usize;
    let mut stack = vec![(root, 0_usize)];
    while let Some((folder, depth)) = stack.pop() {
        visited += 1;
        if visited > MAX_TASK_FOLDERS || examined >= MAX_TASKS {
            break;
        }
        // SAFETY: folder is a live ITaskFolder; COM is initialized by the caller.
        if let Ok(tasks) = unsafe { folder.GetTasks(TASK_ENUM_HIDDEN.0) } {
            // SAFETY: tasks is a live collection.
            let count = unsafe { tasks.Count() }.unwrap_or(0).max(0);
            for index in 1..=count {
                if examined >= MAX_TASKS {
                    break;
                }
                examined += 1;
                // SAFETY: collection indexes are 1-based and within Count.
                let Ok(task) = (unsafe { tasks.get_Item(&VARIANT::from(index)) }) else {
                    continue;
                };
                // SAFETY: task is a live IRegisteredTask.
                let Ok(definition) = (unsafe { task.Definition() }) else {
                    continue;
                };
                if !has_logon_trigger(&definition) {
                    continue;
                }
                // SAFETY: task is a live IRegisteredTask.
                let path = unsafe { task.Path() }
                    .map(|path| path.to_string())
                    .unwrap_or_default();
                // SAFETY: task is a live IRegisteredTask.
                let enabled = unsafe { task.Enabled() }.is_ok_and(|value| value.as_bool());
                if !path.is_empty() {
                    found.push(LogonTask { path, enabled });
                }
            }
        }
        if depth + 1 >= MAX_TASK_DEPTH {
            continue;
        }
        // SAFETY: folder is a live ITaskFolder.
        if let Ok(children) = unsafe { folder.GetFolders(0) } {
            // SAFETY: children is a live collection.
            let count = unsafe { children.Count() }.unwrap_or(0).max(0);
            for index in 1..=count.min(MAX_TASK_FOLDERS as i32) {
                // SAFETY: collection indexes are 1-based and within Count.
                if let Ok(child) = unsafe { children.get_Item(&VARIANT::from(index)) } {
                    stack.push((child, depth + 1));
                }
            }
        }
    }
    found
}

fn has_logon_trigger(definition: &windows::Win32::System::TaskScheduler::ITaskDefinition) -> bool {
    // SAFETY: definition is a live ITaskDefinition.
    let Ok(triggers) = (unsafe { definition.Triggers() }) else {
        return false;
    };
    let mut count = 0_i32;
    // SAFETY: count is writable storage for the collection size.
    if unsafe { triggers.Count(&mut count) }.is_err() {
        return false;
    }
    (1..=count.clamp(0, 64)).any(|index| {
        // SAFETY: trigger indexes are 1-based and within Count.
        let Ok(trigger) = (unsafe { triggers.get_Item(index) }) else {
            return false;
        };
        let mut kind = TASK_TRIGGER_TYPE2(0);
        // SAFETY: kind is writable storage for the trigger type.
        unsafe { trigger.Type(&mut kind) }.is_ok() && kind == TASK_TRIGGER_LOGON
    })
}
