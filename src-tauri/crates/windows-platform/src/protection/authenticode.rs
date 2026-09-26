//! Offline Authenticode evaluation (ADR 0003, decision 4).
//!
//! `WinVerifyTrust` runs with revocation checks off and cache-only URL
//! retrieval, so it never contacts the network. Only embedded signatures are
//! evaluated; catalog-signed files report `Unsigned`, which the heuristics
//! document as a false-positive source.

use std::collections::HashMap;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use protection_core::SignerStatus;
use windows_sys::Win32::Foundation::{
    CRYPT_E_SECURITY_SETTINGS, TRUST_E_NOSIGNATURE, TRUST_E_PROVIDER_UNKNOWN,
    TRUST_E_SUBJECT_FORM_UNKNOWN,
};
use windows_sys::Win32::Security::Cryptography::{
    CERT_CONTEXT, CERT_NAME_SIMPLE_DISPLAY_TYPE, CERT_SHA1_HASH_PROP_ID,
    CertGetCertificateContextProperty, CertGetNameStringW,
};
use windows_sys::Win32::Security::WinTrust::{
    WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_FILE_INFO,
    WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_FILE, WTD_REVOCATION_CHECK_NONE, WTD_REVOKE_NONE,
    WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY, WTD_UI_NONE, WTHelperGetProvCertFromChain,
    WTHelperGetProvSignerFromChain, WTHelperProvDataFromStateData, WinVerifyTrust,
};

use super::fsutil::wide;

/// Exact leaf-certificate subjects Microsoft uses for OS and product binaries.
const MICROSOFT_SUBJECTS: [&str; 4] = [
    "Microsoft Windows",
    "Microsoft Corporation",
    "Microsoft Windows Publisher",
    "Microsoft Windows Hardware Compatibility Publisher",
];
const CACHE_LIMIT: usize = 4096;

/// Map a `WinVerifyTrust` result code to a signer status.
pub(crate) fn classify_trust_result(code: i32) -> SignerStatus {
    match code {
        0 => SignerStatus::Valid {
            subject: String::new(),
            microsoft: false,
        },
        TRUST_E_NOSIGNATURE => SignerStatus::Unsigned,
        TRUST_E_SUBJECT_FORM_UNKNOWN => SignerStatus::NotApplicable,
        TRUST_E_PROVIDER_UNKNOWN | CRYPT_E_SECURITY_SETTINGS => SignerStatus::Unavailable,
        // FACILITY_WIN32 errors (file not found, access denied, sharing) are not verdicts.
        code if (code as u32) & 0xFFFF_0000 == 0x8007_0000 => SignerStatus::Unavailable,
        _ => SignerStatus::Invalid,
    }
}

fn verify_uncached(path: &Path) -> SignerStatus {
    with_trust_state(path, None, |code, data| {
        let mut status = classify_trust_result(code);
        if let SignerStatus::Valid { subject, microsoft } = &mut status {
            // SAFETY: state data is valid for the duration of this callback.
            *subject = unsafe { signer_subject(data) }.unwrap_or_default();
            *microsoft = MICROSOFT_SUBJECTS.contains(&subject.as_str());
        }
        status
    })
    .unwrap_or(SignerStatus::Unavailable)
}

/// Authenticode verdict with the signer certificate's SHA-1 thumbprint, used
/// to pin update installers to the running app's (or the manifest's) signer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuthenticodeVerdict {
    Unsigned,
    /// Trusted signature; `thumbprint` is 40 uppercase hex characters.
    Valid {
        thumbprint: String,
    },
    /// A signature is present but broken, untrusted, or the file is not a
    /// signable image.
    Invalid,
    /// The file could not be evaluated (I/O, provider or policy failure).
    Unavailable,
}

/// Evaluates `path` offline and reports the leaf signer thumbprint. When
/// `file` is given, `WinVerifyTrust` reads through that handle so the verdict
/// covers exactly the bytes the caller holds open.
pub fn signer_thumbprint(path: &Path, file: Option<&std::fs::File>) -> AuthenticodeVerdict {
    use std::os::windows::io::AsRawHandle;
    // Without a caller handle, open one: a file that cannot be opened is not a
    // signature verdict (WinVerifyTrust reports missing files inconsistently
    // across Windows builds), and verifying through a handle pins the bytes.
    let owned;
    let file = match file {
        Some(file) => file,
        None => match std::fs::File::open(path) {
            Ok(opened) => {
                owned = opened;
                &owned
            }
            Err(_) => return AuthenticodeVerdict::Unavailable,
        },
    };
    let handle = Some(file.as_raw_handle());
    with_trust_state(path, handle, |code, data| {
        match classify_trust_result(code) {
            SignerStatus::Valid { .. } => {
                // SAFETY: state data is valid for the duration of this callback.
                unsafe { signer_certificate(data) }
                    .and_then(|cert| unsafe { certificate_sha1(cert) })
                    .map_or(AuthenticodeVerdict::Unavailable, |thumbprint| {
                        AuthenticodeVerdict::Valid { thumbprint }
                    })
            }
            SignerStatus::Unsigned => AuthenticodeVerdict::Unsigned,
            SignerStatus::Unavailable => AuthenticodeVerdict::Unavailable,
            SignerStatus::Invalid | SignerStatus::NotApplicable => AuthenticodeVerdict::Invalid,
        }
    })
    .unwrap_or(AuthenticodeVerdict::Unavailable)
}

/// Runs `WinVerifyTrust` (verify), hands the result code and live state to
/// `inspect`, then always closes the state. `None` when the path is unusable.
fn with_trust_state<R>(
    path: &Path,
    handle: Option<std::os::windows::io::RawHandle>,
    inspect: impl FnOnce(i32, &WINTRUST_DATA) -> R,
) -> Option<R> {
    let wide_path = wide(path.as_os_str()).ok()?;
    let mut file_info = WINTRUST_FILE_INFO {
        cbStruct: size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: wide_path.as_ptr(),
        hFile: handle.unwrap_or(std::ptr::null_mut()),
        ..Default::default()
    };
    let mut data = WINTRUST_DATA {
        cbStruct: size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        dwStateAction: WTD_STATEACTION_VERIFY,
        dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL | WTD_REVOCATION_CHECK_NONE,
        ..Default::default()
    };
    data.Anonymous.pFile = &mut file_info;
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    // SAFETY: `data` and `file_info` are fully initialized and outlive both calls;
    // the verify state is always released with WTD_STATEACTION_CLOSE.
    let code = unsafe { WinVerifyTrust(std::ptr::null_mut(), &mut action, (&raw mut data).cast()) };
    let result = inspect(code, &data);
    data.dwStateAction = WTD_STATEACTION_CLOSE;
    // SAFETY: closes the state opened by the verify call above.
    unsafe { WinVerifyTrust(std::ptr::null_mut(), &mut action, (&raw mut data).cast()) };
    Some(result)
}

/// Leaf signer certificate from a live verify state.
unsafe fn signer_certificate(data: &WINTRUST_DATA) -> Option<*const CERT_CONTEXT> {
    // SAFETY: caller guarantees the state handle is live; each pointer is null-checked.
    unsafe {
        let provider = WTHelperProvDataFromStateData(data.hWVTStateData);
        if provider.is_null() {
            return None;
        }
        let signer = WTHelperGetProvSignerFromChain(provider, 0, 0, 0);
        if signer.is_null() {
            return None;
        }
        let cert = WTHelperGetProvCertFromChain(signer, 0);
        if cert.is_null() || (*cert).pCert.is_null() {
            return None;
        }
        Some((*cert).pCert)
    }
}

/// Uppercase hex SHA-1 hash of a certificate (its thumbprint).
unsafe fn certificate_sha1(cert: *const CERT_CONTEXT) -> Option<String> {
    let mut hash = [0_u8; 20];
    let mut len = hash.len() as u32;
    // SAFETY: `cert` is a live certificate context; the buffer holds `len` bytes.
    let ok = unsafe {
        CertGetCertificateContextProperty(
            cert,
            CERT_SHA1_HASH_PROP_ID,
            hash.as_mut_ptr().cast(),
            &mut len,
        )
    };
    (ok != 0 && len == 20).then(|| hash.iter().map(|byte| format!("{byte:02X}")).collect())
}

unsafe fn signer_subject(data: &WINTRUST_DATA) -> Option<String> {
    // SAFETY: caller guarantees the state handle is live.
    unsafe {
        let cert = signer_certificate(data)?;
        let mut buffer = [0_u16; 256];
        let written = CertGetNameStringW(
            cert,
            CERT_NAME_SIMPLE_DISPLAY_TYPE,
            0,
            std::ptr::null(),
            buffer.as_mut_ptr(),
            buffer.len() as u32,
        ) as usize;
        (written > 1).then(|| String::from_utf16_lossy(&buffer[..written - 1]))
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct CacheKey {
    path: PathBuf,
    size: u64,
    modified: u64,
}

/// Results cached per (path, size, last-write time).
#[derive(Default)]
pub struct SignerCache {
    entries: Mutex<HashMap<CacheKey, SignerStatus>>,
}

impl SignerCache {
    pub fn verify(&self, path: &Path) -> SignerStatus {
        let Ok(metadata) = std::fs::metadata(path) else {
            return SignerStatus::Unavailable;
        };
        let key = CacheKey {
            path: PathBuf::from(path.as_os_str().to_string_lossy().to_lowercase()),
            size: metadata.len(),
            modified: metadata.last_write_time(),
        };
        if let Some(hit) = self
            .entries
            .lock()
            .ok()
            .and_then(|map| map.get(&key).cloned())
        {
            return hit;
        }
        let status = verify_uncached(path);
        if let Ok(mut map) = self.entries.lock() {
            if map.len() >= CACHE_LIMIT {
                map.clear();
            }
            map.insert(key, status.clone());
        }
        status
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protection::test_support::temp_dir;

    #[test]
    fn trust_codes_map_to_honest_states() {
        assert!(matches!(
            classify_trust_result(0),
            SignerStatus::Valid { .. }
        ));
        assert_eq!(
            classify_trust_result(TRUST_E_NOSIGNATURE),
            SignerStatus::Unsigned
        );
        assert_eq!(
            classify_trust_result(TRUST_E_SUBJECT_FORM_UNKNOWN),
            SignerStatus::NotApplicable
        );
        assert_eq!(
            classify_trust_result(TRUST_E_PROVIDER_UNKNOWN),
            SignerStatus::Unavailable
        );
        assert_eq!(
            classify_trust_result(0x8007_0005_u32 as i32),
            SignerStatus::Unavailable
        );
        assert_eq!(
            classify_trust_result(0x8009_6010_u32 as i32),
            SignerStatus::Invalid
        ); // TRUST_E_BAD_DIGEST
    }

    #[test]
    fn unsigned_text_and_missing_files_are_not_valid() {
        let dir = temp_dir("authenticode");
        let text = dir.join("notes.txt");
        std::fs::write(&text, b"hello").unwrap();
        assert!(!matches!(
            SignerCache::default().verify(&text),
            SignerStatus::Valid { .. }
        ));
        assert_eq!(
            SignerCache::default().verify(&dir.join("missing.exe")),
            SignerStatus::Unavailable
        );
        // A PE file with a corrupted, unsigned body.
        let fake = dir.join("fake.exe");
        std::fs::write(&fake, b"MZ\x90\x00not really a pe").unwrap();
        assert!(!matches!(
            SignerCache::default().verify(&fake),
            SignerStatus::Valid { .. }
        ));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn thumbprint_verdicts_fail_closed_for_unsigned_and_missing_files() {
        let dir = temp_dir("authenticode-thumbprint");
        let exe = std::env::current_exe().unwrap();
        assert_eq!(signer_thumbprint(&exe, None), AuthenticodeVerdict::Unsigned);
        let held = std::fs::File::open(&exe).unwrap();
        assert_eq!(
            signer_thumbprint(&exe, Some(&held)),
            AuthenticodeVerdict::Unsigned
        );
        assert_eq!(
            signer_thumbprint(&dir.join("missing.exe"), None),
            AuthenticodeVerdict::Unavailable
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_test_binary_itself_is_unsigned_and_cached() {
        let exe = std::env::current_exe().unwrap();
        let cache = SignerCache::default();
        assert_eq!(cache.verify(&exe), SignerStatus::Unsigned);
        assert_eq!(cache.entries.lock().unwrap().len(), 1);
        assert_eq!(cache.verify(&exe), SignerStatus::Unsigned);
        assert_eq!(cache.entries.lock().unwrap().len(), 1);
    }
}
