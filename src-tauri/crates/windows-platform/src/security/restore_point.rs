use std::{fmt, io, mem::size_of, ptr::null_mut};

use windows_sys::Win32::{
    Foundation::{ERROR_SUCCESS, FreeLibrary, HLOCAL, HMODULE, LocalFree},
    Security::{
        ACL,
        Authorization::{
            EXPLICIT_ACCESS_W, NO_MULTIPLE_TRUSTEE, SET_ACCESS, SetEntriesInAclW, TRUSTEE_IS_GROUP,
            TRUSTEE_IS_SID, TRUSTEE_W,
        },
        CreateWellKnownSid, InitializeSecurityDescriptor, NO_INHERITANCE, PSECURITY_DESCRIPTOR,
        SECURITY_DESCRIPTOR, SetSecurityDescriptorDacl, SetSecurityDescriptorGroup,
        SetSecurityDescriptorOwner, WELL_KNOWN_SID_TYPE, WinBuiltinAdministratorsSid,
        WinInteractiveSid, WinLocalServiceSid, WinLocalSystemSid, WinNetworkServiceSid,
    },
    System::{
        Com::{
            COINIT_MULTITHREADED, CoInitializeEx, CoInitializeSecurity, CoUninitialize,
            EOAC_DISABLE_AAA, EOAC_NO_CUSTOM_MARSHAL, RPC_C_AUTHN_LEVEL_PKT_PRIVACY,
            RPC_C_IMP_LEVEL_IDENTIFY,
        },
        LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW},
    },
};

use super::RestorePointDescription;

const BEGIN_SYSTEM_CHANGE: u32 = 100;
const END_SYSTEM_CHANGE: u32 = 101;
const MODIFY_SETTINGS: u32 = 12;
const MAX_DESC_W: usize = 256;
const SECURITY_MAX_SID_SIZE: usize = 68;
const COM_RIGHTS_EXECUTE_LOCAL: u32 = 0x3;

type SetRestorePoint =
    unsafe extern "system" fn(*mut RestorePointInfo, *mut StateManagerStatus) -> i32;

#[repr(C, packed)]
struct RestorePointInfo {
    event_type: u32,
    restore_point_type: u32,
    sequence_number: i64,
    description: [u16; MAX_DESC_W],
}

#[repr(C, packed)]
#[derive(Default)]
struct StateManagerStatus {
    status: u32,
    sequence_number: i64,
}

#[derive(Debug)]
pub enum RestorePointError {
    Windows(io::Error),
    Com(i32),
    Status(u32),
    MissingEntryPoint,
}

impl fmt::Display for RestorePointError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Windows(_) => "Windows System Restore is unavailable",
            Self::Com(_) => "COM initialization for System Restore failed",
            Self::Status(_) => "Windows rejected the System Restore operation",
            Self::MissingEntryPoint => "Windows System Restore entry point is unavailable",
        })
    }
}

impl std::error::Error for RestorePointError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Windows(error) => Some(error),
            _ => None,
        }
    }
}

pub trait RestorePointBackend {
    fn create(&self, description: &RestorePointDescription) -> Result<i64, RestorePointError>;
}

pub struct WindowsRestorePointBackend;

impl RestorePointBackend for WindowsRestorePointBackend {
    fn create(&self, description: &RestorePointDescription) -> Result<i64, RestorePointError> {
        create_restore_point(description)
    }
}

fn create_restore_point(description: &RestorePointDescription) -> Result<i64, RestorePointError> {
    let _com = ComInitialization::initialize()?;
    let library = DynamicLibrary::system32("SrClient.dll")?;
    let set_restore_point: SetRestorePoint = library.function(b"SRSetRestorePointW\0")?;
    let mut encoded_description = [0; MAX_DESC_W];
    for (destination, unit) in encoded_description
        .iter_mut()
        .zip(description.as_str().encode_utf16())
    {
        *destination = unit;
    }
    let mut info = RestorePointInfo {
        event_type: BEGIN_SYSTEM_CHANGE,
        restore_point_type: MODIFY_SETTINGS,
        sequence_number: 0,
        description: encoded_description,
    };
    let mut status = StateManagerStatus::default();
    call_restore_point(set_restore_point, &mut info, &mut status)?;
    let sequence_number = status.sequence_number;

    info.event_type = END_SYSTEM_CHANGE;
    info.sequence_number = sequence_number;
    status = StateManagerStatus::default();
    call_restore_point(set_restore_point, &mut info, &mut status)?;
    Ok(sequence_number)
}

fn call_restore_point(
    function: SetRestorePoint,
    info: &mut RestorePointInfo,
    status: &mut StateManagerStatus,
) -> Result<(), RestorePointError> {
    // SAFETY: function is resolved from System32 SrClient.dll and both pointers are valid.
    let succeeded = unsafe { function(info, status) };
    if succeeded != 0 && status.status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(RestorePointError::Status(status.status))
    }
}

struct DynamicLibrary(HMODULE);

impl DynamicLibrary {
    fn system32(name: &str) -> Result<Self, RestorePointError> {
        let wide: Vec<_> = name.encode_utf16().chain(Some(0)).collect();
        // SAFETY: name is NUL-terminated and LOAD_LIBRARY_SEARCH_SYSTEM32 forbids path search.
        let handle =
            unsafe { LoadLibraryExW(wide.as_ptr(), null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32) };
        if handle.is_null() {
            Err(RestorePointError::Windows(io::Error::last_os_error()))
        } else {
            Ok(Self(handle))
        }
    }

    fn function<T: Copy>(&self, name: &[u8]) -> Result<T, RestorePointError> {
        // SAFETY: name is NUL-terminated and self contains a loaded module handle.
        let address = unsafe { GetProcAddress(self.0, name.as_ptr()) };
        let address = address.ok_or(RestorePointError::MissingEntryPoint)?;
        // SAFETY: the caller supplies the documented ABI and signature for the named export.
        Ok(unsafe { std::mem::transmute_copy(&address) })
    }
}

impl Drop for DynamicLibrary {
    fn drop(&mut self) {
        // SAFETY: self owns the successful LoadLibraryExW reference.
        unsafe { FreeLibrary(self.0) };
    }
}

struct ComInitialization;

impl ComInitialization {
    fn initialize() -> Result<Self, RestorePointError> {
        // SAFETY: a null reserved pointer and COINIT_MULTITHREADED are documented arguments.
        let result = unsafe { CoInitializeEx(null_mut(), COINIT_MULTITHREADED as u32) };
        if result < 0 {
            return Err(RestorePointError::Com(result));
        }
        if let Err(error) = initialize_com_security() {
            // SAFETY: CoInitializeEx succeeded on this thread.
            unsafe { CoUninitialize() };
            return Err(error);
        }
        Ok(Self)
    }
}

impl Drop for ComInitialization {
    fn drop(&mut self) {
        // SAFETY: this guard exists only after successful CoInitializeEx.
        unsafe { CoUninitialize() };
    }
}

fn initialize_com_security() -> Result<(), RestorePointError> {
    let mut descriptor = SECURITY_DESCRIPTOR::default();
    // SAFETY: descriptor points to writable storage of the documented structure.
    if unsafe {
        InitializeSecurityDescriptor((&mut descriptor as *mut SECURITY_DESCRIPTOR).cast(), 1)
    } == 0
    {
        return Err(RestorePointError::Windows(io::Error::last_os_error()));
    }

    let sid_types = [
        WinBuiltinAdministratorsSid,
        WinLocalServiceSid,
        WinNetworkServiceSid,
        WinInteractiveSid,
        WinLocalSystemSid,
    ];
    let mut sids = [[0_u8; SECURITY_MAX_SID_SIZE]; 5];
    for (sid, sid_type) in sids.iter_mut().zip(sid_types) {
        create_sid(sid, sid_type)?;
    }
    let mut access = [EXPLICIT_ACCESS_W::default(); 5];
    for (entry, sid) in access.iter_mut().zip(sids.iter_mut()) {
        entry.grfAccessPermissions = COM_RIGHTS_EXECUTE_LOCAL;
        entry.grfAccessMode = SET_ACCESS;
        entry.grfInheritance = NO_INHERITANCE;
        entry.Trustee = TRUSTEE_W {
            pMultipleTrustee: null_mut(),
            MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_GROUP,
            ptstrName: sid.as_mut_ptr().cast(),
        };
    }

    let mut acl: *mut ACL = null_mut();
    // SAFETY: access contains five valid SID trustees; acl receives LocalAlloc-owned memory.
    let acl_status =
        unsafe { SetEntriesInAclW(access.len() as u32, access.as_ptr(), null_mut(), &mut acl) };
    if acl_status != ERROR_SUCCESS || acl.is_null() {
        return Err(RestorePointError::Status(acl_status));
    }
    let acl = LocalAcl(acl);
    let administrator = sids[0].as_mut_ptr().cast();
    // SAFETY: descriptor, administrator SID, and ACL remain alive through CoInitializeSecurity.
    let configured = unsafe {
        SetSecurityDescriptorOwner(descriptor_ptr(&mut descriptor), administrator, 0) != 0
            && SetSecurityDescriptorGroup(descriptor_ptr(&mut descriptor), administrator, 0) != 0
            && SetSecurityDescriptorDacl(descriptor_ptr(&mut descriptor), 1, acl.0, 0) != 0
    };
    if !configured {
        return Err(RestorePointError::Windows(io::Error::last_os_error()));
    }

    // SAFETY: descriptor is absolute and grants local COM execute to required service identities.
    let result = unsafe {
        CoInitializeSecurity(
            descriptor_ptr(&mut descriptor),
            -1,
            null_mut(),
            null_mut(),
            RPC_C_AUTHN_LEVEL_PKT_PRIVACY,
            RPC_C_IMP_LEVEL_IDENTIFY,
            null_mut(),
            (EOAC_DISABLE_AAA | EOAC_NO_CUSTOM_MARSHAL) as u32,
            null_mut(),
        )
    };
    if result < 0 {
        Err(RestorePointError::Com(result))
    } else {
        Ok(())
    }
}

fn create_sid(
    storage: &mut [u8; SECURITY_MAX_SID_SIZE],
    sid_type: WELL_KNOWN_SID_TYPE,
) -> Result<(), RestorePointError> {
    let mut size = storage.len() as u32;
    // SAFETY: storage is writable and large enough for any well-known SID.
    if unsafe { CreateWellKnownSid(sid_type, null_mut(), storage.as_mut_ptr().cast(), &mut size) }
        == 0
    {
        Err(RestorePointError::Windows(io::Error::last_os_error()))
    } else {
        Ok(())
    }
}

fn descriptor_ptr(descriptor: &mut SECURITY_DESCRIPTOR) -> PSECURITY_DESCRIPTOR {
    (descriptor as *mut SECURITY_DESCRIPTOR).cast()
}

struct LocalAcl(*mut ACL);

impl Drop for LocalAcl {
    fn drop(&mut self) {
        // SAFETY: SetEntriesInAclW returns memory released by LocalFree.
        unsafe { LocalFree(self.0 as HLOCAL) };
    }
}

const _: () = assert!(size_of::<RestorePointInfo>() == 16 + MAX_DESC_W * 2);
const _: () = assert!(size_of::<StateManagerStatus>() == 12);

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeBackend;

    impl RestorePointBackend for FakeBackend {
        fn create(&self, description: &RestorePointDescription) -> Result<i64, RestorePointError> {
            assert_eq!(description.as_str(), "Before cleanup");
            Ok(123)
        }
    }

    #[test]
    fn backend_trait_replaces_only_the_irreversible_windows_call() {
        let description = RestorePointDescription::parse("Before cleanup".into()).unwrap();
        assert_eq!(FakeBackend.create(&description).unwrap(), 123);
    }
}
