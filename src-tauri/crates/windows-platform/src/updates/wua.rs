//! Windows Update Agent COM access. COM is initialized per call.

use cleanup_core::system_change::UnsupportedReason;
use windows::{
    Win32::{
        Foundation::{
            E_ACCESSDENIED, E_NOINTERFACE, REGDB_E_CLASSNOTREG, RPC_E_CHANGED_MODE, VARIANT_BOOL,
        },
        System::{
            Com::{
                CLSCTX_INPROC_SERVER, CLSCTX_LOCAL_SERVER, COINIT_MULTITHREADED, CoCreateInstance,
                CoInitializeEx, CoUninitialize,
            },
            UpdateAgent::{
                AutomaticUpdates, IAutomaticUpdates, IAutomaticUpdates2, ISystemInformation,
                SystemInformation,
            },
            Variant::{VARIANT, VT_DATE, VariantClear},
        },
    },
    core::{HRESULT, Interface},
};

use super::{AutoUpdateInfo, UpdateAgent, ole_date_to_unix, reboot_key_exists};
use crate::system_change::AdapterError;

/// The real Windows Update Agent.
pub struct WuaAgent;

struct ComGuard {
    uninitialize: bool,
}

impl ComGuard {
    fn new() -> Result<Self, AdapterError> {
        // SAFETY: no reserved pointer; MTA is a documented concurrency model.
        let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if result == RPC_E_CHANGED_MODE {
            // COM is already usable on this thread in another apartment.
            Ok(Self {
                uninitialize: false,
            })
        } else if result.is_ok() {
            Ok(Self { uninitialize: true })
        } else {
            Err(AdapterError::Failed)
        }
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.uninitialize {
            // SAFETY: balances a CoInitializeEx that returned S_OK/S_FALSE on this thread.
            unsafe { CoUninitialize() };
        }
    }
}

pub(super) fn classify(code: HRESULT) -> AdapterError {
    if code == REGDB_E_CLASSNOTREG || code == E_NOINTERFACE {
        AdapterError::Unsupported(UnsupportedReason::ApiUnavailable)
    } else if code == E_ACCESSDENIED {
        AdapterError::Denied
    } else {
        AdapterError::Failed
    }
}

fn create<T: Interface>(class: &windows::core::GUID) -> Result<T, AdapterError> {
    // SAFETY: class is a compiled WUA CLSID and T a WUA interface.
    unsafe { CoCreateInstance(class, None, CLSCTX_INPROC_SERVER | CLSCTX_LOCAL_SERVER) }
        .map_err(|error| classify(error.code()))
}

fn flag(value: VARIANT_BOOL) -> bool {
    value.0 != 0
}

/// Extract a `VT_DATE` as unix seconds, then clear the VARIANT.
fn variant_date(mut variant: VARIANT) -> Option<i64> {
    // SAFETY: the discriminant is read first and `date` is read only for VT_DATE.
    let result = unsafe {
        let inner = &variant.Anonymous.Anonymous;
        if inner.vt == VT_DATE {
            ole_date_to_unix(inner.Anonymous.date)
        } else {
            None
        }
    };
    // SAFETY: variant is an initialized VARIANT owned by this function.
    let _ = unsafe { VariantClear(&mut variant) };
    result
}

impl UpdateAgent for WuaAgent {
    fn automatic_updates(&self) -> Result<AutoUpdateInfo, AdapterError> {
        let _com = ComGuard::new()?;
        let updates: IAutomaticUpdates2 = create(&AutomaticUpdates)?;
        // SAFETY: calls on a live WUA interface pointer.
        unsafe {
            let service_enabled = updates.ServiceEnabled().ok().map(flag);
            let results = updates.Results().ok();
            let last_search_success = results
                .as_ref()
                .and_then(|results| results.LastSearchSuccessDate().ok())
                .and_then(variant_date);
            let last_install_success = results
                .as_ref()
                .and_then(|results| results.LastInstallationSuccessDate().ok())
                .and_then(variant_date);
            Ok(AutoUpdateInfo {
                service_enabled,
                last_search_success,
                last_install_success,
            })
        }
    }

    fn system_reboot_required(&self) -> Result<bool, AdapterError> {
        let _com = ComGuard::new()?;
        let info: ISystemInformation = create(&SystemInformation)?;
        // SAFETY: call on a live WUA interface pointer.
        unsafe { info.RebootRequired() }
            .map(flag)
            .map_err(|error| classify(error.code()))
    }

    fn reboot_pending_key(&self) -> bool {
        reboot_key_exists()
    }

    fn detect_now(&self) -> Result<(), AdapterError> {
        let _com = ComGuard::new()?;
        let updates: IAutomaticUpdates = create(&AutomaticUpdates)?;
        // SAFETY: call on a live WUA interface pointer.
        unsafe { updates.DetectNow() }.map_err(|error| classify(error.code()))
    }
}
