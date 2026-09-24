//! Opt-in AMSI scan through the installed antivirus (external evidence).
//!
//! AMSI hands content to whatever provider is registered (Microsoft Defender
//! or a third-party product). That provider may apply its own cloud settings,
//! so this is a separate opt-in toggle. Many providers ignore AMSI calls from
//! non-script hosts; no provider or no response is reported as `Unavailable`,
//! never as a clean result.

use std::path::Path;

use protection_core::{Evidence, UnavailableReason};
use windows_sys::Win32::System::Antimalware::{
    AMSI_RESULT, AMSI_RESULT_BLOCKED_BY_ADMIN_END, AMSI_RESULT_BLOCKED_BY_ADMIN_START,
    AMSI_RESULT_DETECTED, AMSI_RESULT_NOT_DETECTED, AmsiCloseSession, AmsiInitialize,
    AmsiOpenSession, AmsiScanBuffer, AmsiUninitialize, HAMSICONTEXT, HAMSISESSION,
};

use super::fsutil::{self, wide};

pub const PROVIDER: &str = "Installed antivirus (AMSI)";
/// AMSI is intended for script-sized content; larger files are not submitted.
pub const MAX_AMSI_BYTES: u64 = 16 * 1024 * 1024;

/// Map an AMSI result code to evidence. `AMSI_RESULT_CLEAN` is deliberately
/// not reported as a verdict: many providers return it without inspecting.
pub(crate) fn classify(result: AMSI_RESULT, observed_at: &str) -> Evidence {
    if result >= AMSI_RESULT_DETECTED {
        Evidence::External {
            provider: PROVIDER.into(),
            observed_at: observed_at.into(),
            detail: "The installed antivirus reported this content as malware.".into(),
        }
    } else if (AMSI_RESULT_BLOCKED_BY_ADMIN_START..=AMSI_RESULT_BLOCKED_BY_ADMIN_END)
        .contains(&result)
    {
        Evidence::External {
            provider: PROVIDER.into(),
            observed_at: observed_at.into(),
            detail: "The installed antivirus blocked this content by administrator policy.".into(),
        }
    } else if result == AMSI_RESULT_NOT_DETECTED {
        Evidence::External {
            provider: PROVIDER.into(),
            observed_at: observed_at.into(),
            detail: "The installed antivirus did not report this content. This is not a guarantee."
                .into(),
        }
    } else {
        Evidence::Unavailable {
            reason: UnavailableReason::ProviderNoResponse,
        }
    }
}

struct Context(HAMSICONTEXT);
impl Drop for Context {
    fn drop(&mut self) {
        // SAFETY: context came from a successful AmsiInitialize.
        unsafe { AmsiUninitialize(self.0) }
    }
}

/// Scan a file's bytes. Returns evidence; never an error.
pub fn scan_file(enabled: bool, path: &Path, observed_at: &str) -> Evidence {
    if !enabled {
        return Evidence::Unavailable {
            reason: UnavailableReason::ProviderAbsent,
        };
    }
    let bytes = match fsutil::read_bounded(path, MAX_AMSI_BYTES) {
        Ok(bytes) => bytes,
        Err(fsutil::FsFault::TooLarge) => {
            return Evidence::Unavailable {
                reason: UnavailableReason::TooLarge,
            };
        }
        Err(fsutil::FsFault::Reparse) => {
            return Evidence::Unavailable {
                reason: UnavailableReason::ReparsePoint,
            };
        }
        Err(_) => {
            return Evidence::Unavailable {
                reason: UnavailableReason::ReadFailed,
            };
        }
    };
    let name = wide(path.as_os_str()).unwrap_or_else(|_| vec![0]);
    scan_buffer(&bytes, &name, observed_at)
}

fn scan_buffer(bytes: &[u8], name: &[u16], observed_at: &str) -> Evidence {
    let unavailable = Evidence::Unavailable {
        reason: UnavailableReason::ProviderAbsent,
    };
    let app = wide(std::ffi::OsStr::new("SupaDiskaKlinah")).unwrap_or_default();
    let mut raw: HAMSICONTEXT = std::ptr::null_mut();
    // SAFETY: `app` is NUL-terminated; `raw` receives an owned context.
    if unsafe { AmsiInitialize(app.as_ptr(), &mut raw) } < 0 || raw.is_null() {
        return unavailable;
    }
    let context = Context(raw);
    let mut session: HAMSISESSION = std::ptr::null_mut();
    // SAFETY: valid context; session closed below.
    if unsafe { AmsiOpenSession(context.0, &mut session) } < 0 {
        return unavailable;
    }
    let mut result: AMSI_RESULT = 0;
    // SAFETY: buffer/length describe `bytes`; `name` is NUL-terminated.
    let hr = unsafe {
        AmsiScanBuffer(
            context.0,
            bytes.as_ptr().cast(),
            bytes.len() as u32,
            name.as_ptr(),
            session,
            &mut result,
        )
    };
    // SAFETY: closes the session opened above, before the context drops.
    unsafe { AmsiCloseSession(context.0, session) };
    if hr < 0 {
        return Evidence::Unavailable {
            reason: UnavailableReason::ProviderNoResponse,
        };
    }
    classify(result, observed_at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protection::test_support::temp_dir;

    #[test]
    fn clean_is_never_reported_as_a_verdict() {
        assert_eq!(
            classify(0, "t"),
            Evidence::Unavailable {
                reason: UnavailableReason::ProviderNoResponse
            }
        );
        assert!(matches!(
            classify(AMSI_RESULT_DETECTED, "t"),
            Evidence::External { .. }
        ));
        assert!(matches!(
            classify(AMSI_RESULT_BLOCKED_BY_ADMIN_START, "t"),
            Evidence::External { .. }
        ));
        let Evidence::External { detail, .. } = classify(AMSI_RESULT_NOT_DETECTED, "t") else {
            panic!()
        };
        assert!(detail.contains("not a guarantee"));
    }

    #[test]
    fn disabled_missing_and_oversized_are_unavailable() {
        let dir = temp_dir("amsi");
        let file = dir.join("a.txt");
        std::fs::write(&file, b"hello").unwrap();
        assert_eq!(
            scan_file(false, &file, "t"),
            Evidence::Unavailable {
                reason: UnavailableReason::ProviderAbsent
            }
        );
        assert_eq!(
            scan_file(true, &dir.join("missing"), "t"),
            Evidence::Unavailable {
                reason: UnavailableReason::ReadFailed
            }
        );
        let big = dir.join("big.bin");
        std::fs::File::create(&big)
            .unwrap()
            .set_len(MAX_AMSI_BYTES + 1)
            .unwrap();
        assert_eq!(
            scan_file(true, &big, "t"),
            Evidence::Unavailable {
                reason: UnavailableReason::TooLarge
            }
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn enabled_scan_of_benign_text_never_claims_clean() {
        let dir = temp_dir("amsi-live");
        let file = dir.join("benign.txt");
        std::fs::write(&file, b"hello world").unwrap();
        match scan_file(true, &file, "t") {
            Evidence::External { detail, .. } => assert!(!detail.contains("malware")),
            Evidence::Unavailable { .. } => {}
            other => panic!("unexpected {other:?}"),
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
