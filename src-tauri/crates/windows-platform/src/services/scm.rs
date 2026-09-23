//! Real Service Control Manager access through typed `windows-sys` calls.

use cleanup_core::system_change::{ServiceStartState, ServiceStartType, UnsupportedReason};
use windows_sys::Win32::{
    Foundation::{
        ERROR_ACCESS_DENIED, ERROR_INSUFFICIENT_BUFFER, ERROR_SERVICE_DOES_NOT_EXIST, GetLastError,
    },
    System::Services::{
        ChangeServiceConfigW, CloseServiceHandle, OpenSCManagerW, OpenServiceW,
        QUERY_SERVICE_CONFIGW, QueryServiceConfig2W, QueryServiceConfigW, QueryServiceStatusEx,
        SC_HANDLE, SC_MANAGER_CONNECT, SC_STATUS_PROCESS_INFO, SERVICE_AUTO_START,
        SERVICE_BOOT_START, SERVICE_CHANGE_CONFIG, SERVICE_CONFIG_DELAYED_AUTO_START_INFO,
        SERVICE_DELAYED_AUTO_START_INFO, SERVICE_DEMAND_START, SERVICE_DISABLED, SERVICE_NO_CHANGE,
        SERVICE_QUERY_CONFIG, SERVICE_QUERY_STATUS, SERVICE_RUNNING, SERVICE_STATUS_PROCESS,
        SERVICE_SYSTEM_START,
    },
};

use super::{ServiceReader, ServiceStatus, ServiceWriter};
use crate::system_change::AdapterError;

/// `QueryServiceConfigW` never needs more than 8 KiB (documented maximum).
const MAX_CONFIG_BYTES: u32 = 8 * 1024;

/// The machine's Service Control Manager.
#[derive(Clone, Copy, Debug, Default)]
pub struct ScmServices;

struct ScHandle(SC_HANDLE);

impl Drop for ScHandle {
    fn drop(&mut self) {
        // SAFETY: the handle was returned non-null by OpenSCManagerW/OpenServiceW
        // and is closed exactly once here.
        unsafe {
            CloseServiceHandle(self.0);
        }
    }
}

fn last_error() -> AdapterError {
    // SAFETY: GetLastError only reads the calling thread's last-error value.
    match unsafe { GetLastError() } {
        ERROR_ACCESS_DENIED => AdapterError::Denied,
        ERROR_SERVICE_DOES_NOT_EXIST => AdapterError::Unsupported(UnsupportedReason::NotPresent),
        _ => AdapterError::Failed,
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn open_service(service_name: &str, access: u32) -> Result<(ScHandle, ScHandle), AdapterError> {
    // SAFETY: null machine/database names select the local active database.
    let manager = unsafe { OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT) };
    if manager.is_null() {
        return Err(match last_error() {
            AdapterError::Denied => AdapterError::Denied,
            _ => AdapterError::Unsupported(UnsupportedReason::ApiUnavailable),
        });
    }
    let manager = ScHandle(manager);
    let name = wide(service_name);
    // SAFETY: `manager` is a live SCM handle and `name` is NUL-terminated.
    let service = unsafe { OpenServiceW(manager.0, name.as_ptr(), access) };
    if service.is_null() {
        return Err(last_error());
    }
    Ok((ScHandle(service), manager))
}

fn start_state(raw: u32) -> Result<ServiceStartState, AdapterError> {
    Ok(match raw {
        SERVICE_BOOT_START => ServiceStartState::Boot,
        SERVICE_SYSTEM_START => ServiceStartState::System,
        SERVICE_AUTO_START => ServiceStartState::Automatic,
        SERVICE_DEMAND_START => ServiceStartState::Manual,
        SERVICE_DISABLED => ServiceStartState::Disabled,
        _ => return Err(AdapterError::Failed),
    })
}

fn query_start(service: &ScHandle) -> Result<ServiceStartState, AdapterError> {
    let mut needed = 0_u32;
    // SAFETY: a zero-sized query only reports the required size in `needed`.
    let ok = unsafe { QueryServiceConfigW(service.0, std::ptr::null_mut(), 0, &mut needed) };
    if ok == 0 {
        // SAFETY: reads the thread's last-error value.
        if unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER {
            return Err(last_error());
        }
    }
    if needed == 0 || needed > MAX_CONFIG_BYTES {
        return Err(AdapterError::Failed);
    }
    // u64 storage keeps the buffer aligned for QUERY_SERVICE_CONFIGW.
    let mut buffer = vec![0_u64; (needed as usize).div_ceil(8)];
    let size = u32::try_from(buffer.len() * 8).map_err(|_| AdapterError::Failed)?;
    // SAFETY: `buffer` is `size` bytes, aligned, and writable.
    let ok = unsafe {
        QueryServiceConfigW(
            service.0,
            buffer.as_mut_ptr().cast::<QUERY_SERVICE_CONFIGW>(),
            size,
            &mut needed,
        )
    };
    if ok == 0 {
        return Err(last_error());
    }
    // SAFETY: the call succeeded, so the buffer starts with an initialized
    // QUERY_SERVICE_CONFIGW; only the scalar start type is read.
    let raw = unsafe { (*buffer.as_ptr().cast::<QUERY_SERVICE_CONFIGW>()).dwStartType };
    start_state(raw)
}

fn query_delayed(service: &ScHandle) -> bool {
    let mut info = SERVICE_DELAYED_AUTO_START_INFO::default();
    let mut needed = 0_u32;
    // SAFETY: `info` is a correctly sized, writable buffer for this info level.
    let ok = unsafe {
        QueryServiceConfig2W(
            service.0,
            SERVICE_CONFIG_DELAYED_AUTO_START_INFO,
            (&raw mut info).cast::<u8>(),
            size_of::<SERVICE_DELAYED_AUTO_START_INFO>() as u32,
            &mut needed,
        )
    };
    ok != 0 && info.fDelayedAutostart != 0
}

fn query_running(service: &ScHandle) -> bool {
    let mut status = SERVICE_STATUS_PROCESS::default();
    let mut needed = 0_u32;
    // SAFETY: `status` is a correctly sized, writable SERVICE_STATUS_PROCESS.
    let ok = unsafe {
        QueryServiceStatusEx(
            service.0,
            SC_STATUS_PROCESS_INFO,
            (&raw mut status).cast::<u8>(),
            size_of::<SERVICE_STATUS_PROCESS>() as u32,
            &mut needed,
        )
    };
    ok != 0 && status.dwCurrentState == SERVICE_RUNNING
}

impl ServiceReader for ScmServices {
    fn query(&self, service_name: &str) -> Result<ServiceStatus, AdapterError> {
        let (service, _manager) =
            open_service(service_name, SERVICE_QUERY_CONFIG | SERVICE_QUERY_STATUS)?;
        let start = query_start(&service)?;
        Ok(ServiceStatus {
            start,
            delayed_auto_start: start == ServiceStartState::Automatic && query_delayed(&service),
            running: query_running(&service),
        })
    }
}

impl ServiceWriter for ScmServices {
    fn set_start_type(
        &self,
        service_name: &str,
        start_type: ServiceStartType,
    ) -> Result<(), AdapterError> {
        let (service, _manager) =
            open_service(service_name, SERVICE_CHANGE_CONFIG | SERVICE_QUERY_CONFIG)?;
        // Re-check on the write handle: boot and system drivers are never written.
        if query_start(&service)?.writable().is_none() {
            return Err(AdapterError::Failed);
        }
        let raw = match start_type {
            ServiceStartType::Automatic => SERVICE_AUTO_START,
            ServiceStartType::Manual => SERVICE_DEMAND_START,
            ServiceStartType::Disabled => SERVICE_DISABLED,
        };
        // SAFETY: `service` holds SERVICE_CHANGE_CONFIG; every other field is
        // SERVICE_NO_CHANGE or null, so only the start type is modified.
        let ok = unsafe {
            ChangeServiceConfigW(
                service.0,
                SERVICE_NO_CHANGE,
                raw,
                SERVICE_NO_CHANGE,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        if ok == 0 {
            return Err(last_error());
        }
        Ok(())
    }
}
