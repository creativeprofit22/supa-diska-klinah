use std::{io, mem::size_of};

use windows_sys::Win32::{
    Foundation::CloseHandle,
    Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation},
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

pub trait PrivilegeProbe {
    fn is_elevated(&self) -> io::Result<bool>;
}

pub struct ProcessPrivilege;

impl PrivilegeProbe for ProcessPrivilege {
    fn is_elevated(&self) -> io::Result<bool> {
        is_process_elevated()
    }
}

pub fn is_process_elevated() -> io::Result<bool> {
    let mut token = std::ptr::null_mut();
    // SAFETY: The pseudo-process handle is always valid; token receives an owned handle.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }

    let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
    let mut returned = 0;
    // SAFETY: elevation points to a correctly sized writable TOKEN_ELEVATION value.
    let result = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
    };
    // SAFETY: token is the owned handle returned by OpenProcessToken above.
    unsafe { CloseHandle(token) };

    if result == 0 {
        Err(io::Error::last_os_error())
    } else if returned != size_of::<TOKEN_ELEVATION>() as u32 {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Windows returned an invalid token elevation result",
        ))
    } else {
        Ok(elevation.TokenIsElevated != 0)
    }
}

pub fn require_standard_user() -> io::Result<()> {
    require_standard_user_with(&ProcessPrivilege)
}

pub fn require_standard_user_with(probe: &impl PrivilegeProbe) -> io::Result<()> {
    if probe.is_elevated()? {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Supa Diska Klinah must run without administrator privileges",
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{PrivilegeProbe, require_standard_user_with};
    use std::io;

    struct FixedProbe(bool);

    impl PrivilegeProbe for FixedProbe {
        fn is_elevated(&self) -> io::Result<bool> {
            Ok(self.0)
        }
    }

    #[test]
    fn standard_user_is_allowed() {
        assert!(require_standard_user_with(&FixedProbe(false)).is_ok());
    }

    #[test]
    fn elevated_process_is_rejected() {
        let error = require_standard_user_with(&FixedProbe(true)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }
}
