//! Read-only Microsoft Defender detection history (external evidence).
//!
//! Queries `ROOT\Microsoft\Windows\Defender:MSFT_MpThreatDetection` through WMI.
//! This reads what Defender already recorded; it triggers no scan and no
//! network use. If Defender is not the active antivirus, or the namespace is
//! not accessible, the result is `Unavailable`.

use protection_core::{Evidence, UnavailableReason};
use serde::Serialize;
use windows::Win32::Foundation::{RPC_E_CHANGED_MODE, S_FALSE, S_OK};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
    CoSetProxyBlanket, CoUninitialize, EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL,
    RPC_C_IMP_LEVEL_IMPERSONATE,
};
use windows::Win32::System::Variant::{VARENUM, VARIANT, VT_ARRAY, VT_BSTR, VariantClear};
use windows::Win32::System::Wmi::{
    IEnumWbemClassObject, IWbemClassObject, IWbemContext, IWbemLocator, WBEM_FLAG_FORWARD_ONLY,
    WBEM_FLAG_RETURN_IMMEDIATELY, WbemLocator,
};
use windows::core::{BSTR, HRESULT, PCWSTR, w};

pub const PROVIDER: &str = "Microsoft Defender detection history";
const NAMESPACE: &str = "ROOT\\Microsoft\\Windows\\Defender";
const QUERY: &str = "SELECT ThreatID, InitialDetectionTime, Resources FROM MSFT_MpThreatDetection";
const MAX_DETECTIONS: usize = 500;
const MAX_TEXT: usize = 1024;
const NEXT_TIMEOUT_MS: i32 = 15_000;
const RPC_C_AUTHN_WINNT: u32 = 10;
const RPC_C_AUTHZ_NONE: u32 = 0;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DefenderDetection {
    pub threat_id: Option<String>,
    pub detected_at: Option<String>,
    pub resources: Vec<String>,
    pub evidence: Evidence,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DefenderHistory {
    pub detections: Vec<DefenderDetection>,
    /// Set when history could not be read; never means "no threats".
    pub unavailable: Option<Evidence>,
    pub truncated: bool,
}

impl DefenderHistory {
    pub(crate) fn unavailable(reason: UnavailableReason) -> Self {
        Self {
            detections: Vec::new(),
            unavailable: Some(Evidence::Unavailable { reason }),
            truncated: false,
        }
    }
}

struct Apartment(bool);
impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: balances a successful CoInitializeEx on this thread.
            unsafe { CoUninitialize() };
        }
    }
}

fn clip(text: String) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .take(MAX_TEXT)
        .collect()
}

pub fn read(observed_at: &str) -> DefenderHistory {
    // SAFETY: documented arguments; balanced by `Apartment`.
    let init: HRESULT = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let _apartment = if init == S_OK || init == S_FALSE {
        Apartment(true)
    } else if init == RPC_E_CHANGED_MODE {
        Apartment(false)
    } else {
        return DefenderHistory::unavailable(UnavailableReason::ProviderAbsent);
    };
    let Ok(enumerator) = open_query() else {
        return DefenderHistory::unavailable(UnavailableReason::ProviderAbsent);
    };
    let mut history = DefenderHistory {
        detections: Vec::new(),
        unavailable: None,
        truncated: false,
    };
    loop {
        if history.detections.len() >= MAX_DETECTIONS {
            history.truncated = true;
            break;
        }
        let mut objects: [Option<IWbemClassObject>; 1] = [None];
        let mut returned = 0_u32;
        // SAFETY: one-element writable slice and a valid out pointer.
        let status = unsafe { enumerator.Next(NEXT_TIMEOUT_MS, &mut objects, &mut returned) };
        if status.is_err() {
            history.unavailable = Some(Evidence::Unavailable {
                reason: UnavailableReason::ProviderNoResponse,
            });
            break;
        }
        let [object] = objects;
        let Some(object) = object.filter(|_| returned == 1) else {
            break;
        };
        let threat_id = text_property(&object, w!("ThreatID")).map(clip);
        let detected_at = text_property(&object, w!("InitialDetectionTime")).map(clip);
        let resources: Vec<String> = string_array(&object, w!("Resources"))
            .into_iter()
            .take(32)
            .map(clip)
            .collect();
        history.detections.push(DefenderDetection {
            evidence: Evidence::External {
                provider: PROVIDER.into(),
                observed_at: observed_at.into(),
                detail: format!(
                    "Defender recorded threat {}",
                    threat_id.as_deref().unwrap_or("(unknown id)")
                ),
            },
            threat_id,
            detected_at,
            resources,
        });
    }
    history
}

fn open_query() -> windows::core::Result<IEnumWbemClassObject> {
    // SAFETY: WbemLocator is the documented in-process CLSID; all arguments
    // are valid BSTRs or documented nulls; the query is a constant.
    unsafe {
        let locator: IWbemLocator = CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER)?;
        let empty = BSTR::new();
        let services = locator.ConnectServer(
            &BSTR::from(NAMESPACE),
            &empty,
            &empty,
            &empty,
            0,
            &empty,
            None::<&IWbemContext>,
        )?;
        CoSetProxyBlanket(
            &services,
            RPC_C_AUTHN_WINNT,
            RPC_C_AUTHZ_NONE,
            PCWSTR::null(),
            RPC_C_AUTHN_LEVEL_CALL,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            None,
            EOAC_NONE,
        )?;
        services.ExecQuery(
            &BSTR::from("WQL"),
            &BSTR::from(QUERY),
            WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
            None::<&IWbemContext>,
        )
    }
}

struct OwnedVariant(VARIANT);
impl Drop for OwnedVariant {
    fn drop(&mut self) {
        // SAFETY: zeroed or filled by IWbemClassObject::Get.
        let _ = unsafe { VariantClear(&mut self.0) };
    }
}

fn text_property(object: &IWbemClassObject, name: PCWSTR) -> Option<String> {
    let mut value = OwnedVariant(VARIANT::default());
    // SAFETY: static name and an empty writable VARIANT.
    unsafe { object.Get(name, 0, &mut value.0, None, None) }.ok()?;
    // SAFETY: vt discriminates the initialized union member.
    unsafe {
        let inner = &value.0.Anonymous.Anonymous;
        (inner.vt == VT_BSTR).then(|| inner.Anonymous.bstrVal.to_string())
    }
}

fn string_array(object: &IWbemClassObject, name: PCWSTR) -> Vec<String> {
    use windows::Win32::System::Ole::{
        SafeArrayGetElement, SafeArrayGetLBound, SafeArrayGetUBound,
    };
    let mut value = OwnedVariant(VARIANT::default());
    // SAFETY: static name and an empty writable VARIANT.
    if unsafe { object.Get(name, 0, &mut value.0, None, None) }.is_err() {
        return Vec::new();
    }
    let mut out = Vec::new();
    // SAFETY: only reads a BSTR SAFEARRAY after checking vt; bounds are queried
    // from the array itself; each element is copied into an owned BSTR.
    unsafe {
        let inner = &value.0.Anonymous.Anonymous;
        if inner.vt != VARENUM(VT_ARRAY.0 | VT_BSTR.0) {
            return out;
        }
        let array = inner.Anonymous.parray;
        if array.is_null() {
            return out;
        }
        let (Ok(lower), Ok(upper)) = (SafeArrayGetLBound(array, 1), SafeArrayGetUBound(array, 1))
        else {
            return out;
        };
        for index in lower..=upper.min(lower + 63) {
            let mut element = BSTR::new();
            if SafeArrayGetElement(array, &index, (&raw mut element).cast()).is_ok() {
                out.push(element.to_string());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_is_read_only_or_honestly_unavailable() {
        let history = read("2026-09-23T00:00:00Z");
        // Either outcome is legitimate on a test machine; what matters is that
        // detections are external evidence and failure is typed unavailable.
        assert!(
            history
                .detections
                .iter()
                .all(|d| matches!(d.evidence, Evidence::External { .. }))
        );
        assert!(matches!(
            history.unavailable,
            None | Some(Evidence::Unavailable { .. })
        ));
        assert!(history.detections.len() <= MAX_DETECTIONS);
    }

    #[test]
    fn query_is_a_constant_select() {
        assert!(QUERY.starts_with("SELECT "));
        let body = include_str!("defender_history.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        for forbidden in [
            "ExecMethod",
            "PutInstance",
            "DeleteInstance",
            "MSFT_MpScan",
            "Start-Mp",
        ] {
            assert!(!body.contains(forbidden), "{forbidden}");
        }
    }

    #[test]
    fn unavailable_constructor_never_reports_no_threats_as_clean() {
        let history = DefenderHistory::unavailable(UnavailableReason::ProviderAbsent);
        assert!(history.detections.is_empty());
        assert!(matches!(
            history.unavailable,
            Some(Evidence::Unavailable { .. })
        ));
    }
}
