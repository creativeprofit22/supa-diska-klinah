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
use crate::restore::{RestorePointSource, WmiRestorePointSource};

/// Bounded wait for a newly ended restore point to appear in WMI.
const VERIFY_ATTEMPTS: u32 = 5;
const VERIFY_DELAY: std::time::Duration = std::time::Duration::from_secs(1);

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
    /// Windows reported success, but no restore point with the returned
    /// sequence number exists (for example a failed shadow copy).
    NotCreated,
}

impl fmt::Display for RestorePointError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Windows(_) => "Windows System Restore is unavailable",
            Self::Com(_) => "COM initialization for System Restore failed",
            Self::Status(_) => "Windows rejected the System Restore operation",
            Self::MissingEntryPoint => "Windows System Restore entry point is unavailable",
            Self::NotCreated => "Windows did not create the restore point",
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
        let sequence_number = create_restore_point(description)?;
        verify_created(sequence_number, &WmiRestorePointSource, || {
            std::thread::sleep(VERIFY_DELAY)
        })
    }
}

/// `SRSetRestorePointW` can report success without a usable point, so
/// success requires the returned sequence number to be listed by WMI. Any
/// listing failure also fails closed.
fn verify_created(
    sequence_number: i64,
    source: &impl RestorePointSource,
    mut wait: impl FnMut(),
) -> Result<i64, RestorePointError> {
    for attempt in 0..VERIFY_ATTEMPTS {
        if attempt > 0 {
            wait();
        }
        let listed = source
            .query()
            .map_err(|_| RestorePointError::NotCreated)?
            .iter()
            .any(|point| point.sequence_number.map(i64::from) == Some(sequence_number));
        if listed {
            return Ok(sequence_number);
        }
    }
    Err(RestorePointError::NotCreated)
}

fn create_restore_point(description: &RestorePointDescription) -> Result<i64, RestorePointError> {
    let _com = ComInitialization::initialize()?;
    let library = DynamicLibrary::system32("SrClient.dll")?;
    let set_restore_point: SetRestorePoint = library.function(b"SRSetRestorePointW\0")?;
    sequence_restore_point(description, |info, status| {
        call_restore_point(set_restore_point, info, status)
    })
}

fn sequence_restore_point(
    description: &RestorePointDescription,
    mut set_restore_point: impl FnMut(
        &mut RestorePointInfo,
        &mut StateManagerStatus,
    ) -> Result<(), RestorePointError>,
) -> Result<i64, RestorePointError> {
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
    set_restore_point(&mut info, &mut status)?;
    let sequence_number = status.sequence_number;

    info.event_type = END_SYSTEM_CHANGE;
    info.sequence_number = sequence_number;
    status = StateManagerStatus::default();
    set_restore_point(&mut info, &mut status)?;
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

    fn description() -> RestorePointDescription {
        RestorePointDescription::parse("Before cleanup".into()).unwrap()
    }

    #[test]
    fn begin_precedes_end_and_its_sequence_is_returned_and_propagated() {
        let mut calls = Vec::new();
        let sequence_number = sequence_restore_point(&description(), |info, status| {
            calls.push((info.event_type, info.sequence_number));
            if info.event_type == BEGIN_SYSTEM_CHANGE {
                status.sequence_number = 123;
            }
            Ok(())
        })
        .unwrap();

        assert_eq!(sequence_number, 123);
        assert_eq!(
            calls,
            vec![(BEGIN_SYSTEM_CHANGE, 0), (END_SYSTEM_CHANGE, 123)]
        );
    }

    #[test]
    fn begin_failure_prevents_end_call() {
        let mut calls = Vec::new();
        let error = sequence_restore_point(&description(), |info, _| {
            calls.push(info.event_type);
            Err(RestorePointError::Status(5))
        })
        .unwrap_err();

        assert!(matches!(error, RestorePointError::Status(5)));
        assert_eq!(calls, vec![BEGIN_SYSTEM_CHANGE]);
    }

    struct Listing(Vec<Result<Vec<u32>, i32>>, std::cell::Cell<usize>);

    impl RestorePointSource for Listing {
        fn query(
            &self,
        ) -> Result<Vec<crate::restore::RawRestorePoint>, crate::restore::WmiFailure> {
            let call = self.1.get();
            self.1.set(call + 1);
            let response = self.0[call.min(self.0.len() - 1)].clone();
            response
                .map(|sequences| {
                    sequences
                        .into_iter()
                        .map(|sequence| crate::restore::RawRestorePoint {
                            sequence_number: Some(sequence),
                            ..Default::default()
                        })
                        .collect()
                })
                .map_err(crate::restore::WmiFailure)
        }
    }

    #[test]
    fn listed_sequence_is_verified() {
        let source = Listing(vec![Ok(vec![7, 123])], Default::default());
        assert_eq!(verify_created(123, &source, || {}).unwrap(), 123);
        assert_eq!(source.1.get(), 1);
    }

    #[test]
    fn reported_success_without_a_listed_point_is_not_created() {
        let source = Listing(vec![Ok(vec![7])], Default::default());
        let mut waits = 0;
        let error = verify_created(123, &source, || waits += 1).unwrap_err();
        assert!(matches!(error, RestorePointError::NotCreated));
        assert_eq!(source.1.get(), VERIFY_ATTEMPTS as usize);
        assert_eq!(waits, VERIFY_ATTEMPTS - 1);
    }

    #[test]
    fn a_point_that_appears_late_is_verified() {
        let source = Listing(vec![Ok(vec![]), Ok(vec![123])], Default::default());
        assert_eq!(verify_created(123, &source, || {}).unwrap(), 123);
    }

    #[test]
    fn listing_failure_fails_closed() {
        let source = Listing(vec![Err(-1)], Default::default());
        assert!(matches!(
            verify_created(123, &source, || {}),
            Err(RestorePointError::NotCreated)
        ));
    }

    #[test]
    fn end_failure_is_propagated() {
        let mut calls = Vec::new();
        let error = sequence_restore_point(&description(), |info, status| {
            calls.push(info.event_type);
            if info.event_type == BEGIN_SYSTEM_CHANGE {
                status.sequence_number = 123;
                Ok(())
            } else {
                Err(RestorePointError::Status(6))
            }
        })
        .unwrap_err();

        assert!(matches!(error, RestorePointError::Status(6)));
        assert_eq!(calls, vec![BEGIN_SYSTEM_CHANGE, END_SYSTEM_CHANGE]);
    }
}
