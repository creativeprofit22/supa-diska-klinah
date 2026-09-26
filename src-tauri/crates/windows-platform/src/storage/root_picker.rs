//! Native-owned scope selection. Display paths never round-trip as authority.
use super::{
    StorageError, StorageModule, opaque_id,
    scans::{JobError, StorageService},
};
use serde::Serialize;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{ERROR_CANCELLED, HWND},
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
            CoTaskMemFree, CoUninitialize,
        },
        UI::Shell::{
            FOS_DONTADDTORECENT, FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR, FOS_NODEREFERENCELINKS,
            FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH,
        },
    },
    core::HRESULT,
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RootChoice {
    pub root_id: String,
    pub module: StorageModule,
    pub display_path: String,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeScope {
    pub scope_id: String,
    pub module: StorageModule,
    pub label: String,
    pub display_path: Option<String>,
    pub available: bool,
}
struct ScopeBinding {
    module: StorageModule,
    path: Option<PathBuf>,
    created: Instant,
}
#[derive(Default)]
pub struct ScopeService {
    picker_open: AtomicBool,
    scopes: Mutex<HashMap<String, ScopeBinding>>,
}

pub fn personal_module(module: StorageModule) -> bool {
    matches!(
        module,
        StorageModule::DiskAnalyzer
            | StorageModule::LargeFiles
            | StorageModule::Duplicates
            | StorageModule::EmptyFolders
    )
}
fn authorize(
    service: &StorageService,
    module: StorageModule,
    path: PathBuf,
) -> Result<RootChoice, JobError> {
    let protection = super::current_protection()?;
    let root_id = service.authorize_root_for(&path, module, &protection)?;
    Ok(RootChoice {
        root_id,
        module,
        display_path: path.to_string_lossy().into_owned(),
    })
}
impl ScopeService {
    /// HWND comes only from the application's native window, never IPC data.
    /// Run on a dedicated blocking thread: COM and dialog objects stay on that STA.
    pub fn choose(
        &self,
        service: &StorageService,
        module: StorageModule,
        owner: isize,
    ) -> Result<Option<RootChoice>, JobError> {
        if !personal_module(module) || owner == 0 {
            return Err(StorageError::InvalidRequest.into());
        }
        if self
            .picker_open
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(JobError::Busy);
        }
        struct Permit<'a>(&'a AtomicBool);
        impl Drop for Permit<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::Release);
            }
        }
        let _permit = Permit(&self.picker_open);
        selected(service, module, pick_folder(owner)?)
    }

    /// Refresh invalidates previous scope IDs. Inventory is small, native-resolved,
    /// expires after ten minutes and never expands arbitrary renderer paths.
    pub fn list(&self, module: StorageModule) -> Result<Vec<NativeScope>, JobError> {
        let paths = match module {
            StorageModule::Cleaner => super::cleaner::native_scope_paths(),
            StorageModule::Browser => super::browser::native_scope_paths(),
            _ => return Err(StorageError::InvalidRequest.into()),
        }?;
        let bound = if module == StorageModule::Cleaner {
            super::cleaner::scope_inventory_bound()
        } else {
            64
        };
        if bound > 1024 || paths.len() > bound {
            return Err(StorageError::LimitReached.into());
        }
        let mut bindings = self
            .scopes
            .lock()
            .map_err(|_| StorageError::SnapshotUnavailable)?;
        bindings.clear();
        let mut result = Vec::with_capacity(paths.len());
        for (label, path) in paths {
            let id = opaque_id()?;
            let path = path.filter(|p| p.is_dir());
            result.push(NativeScope {
                scope_id: id.clone(),
                module,
                label,
                display_path: path.as_ref().map(|p| p.to_string_lossy().into_owned()),
                available: path.is_some(),
            });
            bindings.insert(
                id,
                ScopeBinding {
                    module,
                    path,
                    created: Instant::now(),
                },
            );
        }
        Ok(result)
    }
    pub fn authorize_scope(
        &self,
        service: &StorageService,
        module: StorageModule,
        id: &str,
    ) -> Result<RootChoice, JobError> {
        let path = {
            let mut scopes = self
                .scopes
                .lock()
                .map_err(|_| StorageError::SnapshotUnavailable)?;
            scopes.retain(|_, s| s.created.elapsed() < Duration::from_secs(600));
            let scope = scopes
                .get(id)
                .filter(|s| s.module == module)
                .ok_or(StorageError::SnapshotUnavailable)?;
            scope.path.clone().ok_or(StorageError::UnsupportedScope)?
        };
        authorize(service, module, path)
    }
}
fn selected(
    service: &StorageService,
    module: StorageModule,
    path: Option<PathBuf>,
) -> Result<Option<RootChoice>, JobError> {
    path.map(|path| authorize(service, module, path))
        .transpose()
}
fn pick_folder(owner: isize) -> Result<Option<PathBuf>, JobError> {
    pick_folder_titled(owner, crate::i18n::native().pick_storage_folder_title)
}

/// Native folder picker shared with the protection scanner. Run on a dedicated
/// blocking thread; `owner` must come from the application's own window.
pub(crate) fn pick_folder_titled(owner: isize, title: &str) -> Result<Option<PathBuf>, JobError> {
    // Owned NUL-terminated UTF-16; it outlives the modal `show` call below.
    let wide: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
    let title = windows::core::PCWSTR(wide.as_ptr());
    fn show(owner: isize, title: windows::core::PCWSTR) -> windows::core::Result<Option<PathBuf>> {
        // SAFETY: COM is initialized and balanced on this thread; all interfaces
        // drop before the apartment. The owner is supplied by the native app.
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
            struct Apartment;
            impl Drop for Apartment {
                fn drop(&mut self) {
                    unsafe {
                        CoUninitialize();
                    }
                }
            }
            let _apartment = Apartment;
            let dialog: IFileOpenDialog =
                CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
            dialog.SetOptions(
                dialog.GetOptions()?
                    | FOS_PICKFOLDERS
                    | FOS_FORCEFILESYSTEM
                    | FOS_PATHMUSTEXIST
                    | FOS_NODEREFERENCELINKS
                    | FOS_NOCHANGEDIR
                    | FOS_DONTADDTORECENT,
            )?;
            dialog.SetTitle(title)?;
            if let Err(error) = dialog.Show(Some(HWND(owner as *mut _))) {
                if error.code() == HRESULT::from_win32(ERROR_CANCELLED.0) {
                    return Ok(None);
                }
                return Err(error);
            }
            let path = dialog.GetResult()?.GetDisplayName(SIGDN_FILESYSPATH)?;
            // GetDisplayName allocates with the COM task allocator. Free even if
            // UTF-16 conversion fails; malformed text never becomes a path.
            let text = path.to_string();
            CoTaskMemFree(Some(path.0.cast()));
            Ok(Some(PathBuf::from(text?)))
        }
    }
    show(owner, title).map_err(|_| JobError::Native("folder_picker_unavailable".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_creates_no_authorization() {
        assert!(
            selected(&StorageService::new(), StorageModule::LargeFiles, None)
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn unavailable_expired_scopes_and_overlapping_dialogs_fail_closed() {
        let scopes = ScopeService::default();
        let storage = StorageService::new();
        let id = "a".repeat(32);
        scopes.scopes.lock().unwrap().insert(
            id.clone(),
            ScopeBinding {
                module: StorageModule::Cleaner,
                path: None,
                created: Instant::now(),
            },
        );
        assert!(matches!(
            scopes.authorize_scope(&storage, StorageModule::Cleaner, &id),
            Err(JobError::Storage(StorageError::UnsupportedScope))
        ));
        scopes.scopes.lock().unwrap().get_mut(&id).unwrap().created =
            Instant::now() - Duration::from_secs(601);
        assert!(matches!(
            scopes.authorize_scope(&storage, StorageModule::Cleaner, &id),
            Err(JobError::Storage(StorageError::SnapshotUnavailable))
        ));
        scopes.picker_open.store(true, Ordering::Release);
        assert!(matches!(
            scopes.choose(&storage, StorageModule::LargeFiles, 1),
            Err(JobError::Busy)
        ));
        assert!(scopes.picker_open.load(Ordering::Acquire));
    }
    #[test]
    fn scopes_reject_arbitrary_and_cross_module_ids() {
        let scopes = ScopeService::default();
        let storage = StorageService::new();
        assert!(scopes.list(StorageModule::LargeFiles).is_err());
        assert!(
            scopes
                .authorize_scope(&storage, StorageModule::Cleaner, "C:\\Windows")
                .is_err()
        );
        let inventory = scopes.list(StorageModule::Cleaner).unwrap();
        assert!(!inventory.is_empty());
        assert!(
            scopes
                .authorize_scope(&storage, StorageModule::Browser, &inventory[0].scope_id)
                .is_err()
        );
        scopes.list(StorageModule::Cleaner).unwrap();
        assert!(
            scopes
                .authorize_scope(&storage, StorageModule::Cleaner, &inventory[0].scope_id)
                .is_err()
        );
    }
}
