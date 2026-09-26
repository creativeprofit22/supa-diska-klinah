//! Read-only `ROOT\DEFAULT:SystemRestore` enumeration through typed WMI COM.

use windows::{
    Win32::{
        Foundation::{RPC_E_CHANGED_MODE, S_FALSE, S_OK},
        System::{
            Com::{
                CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
                CoSetProxyBlanket, CoUninitialize, EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL,
                RPC_C_IMP_LEVEL_IMPERSONATE,
            },
            Variant::{VARIANT, VT_BSTR, VT_I4, VT_UI4, VariantClear},
            Wmi::{
                IEnumWbemClassObject, IWbemClassObject, IWbemContext, IWbemLocator,
                WBEM_FLAG_FORWARD_ONLY, WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_S_TIMEDOUT, WbemLocator,
            },
        },
    },
    core::{BSTR, HRESULT, PCWSTR, w},
};

use super::{MAX_RESTORE_POINTS, RawRestorePoint, RestorePointSource, WmiFailure};

const NAMESPACE: &str = "ROOT\\DEFAULT";
const QUERY_LANGUAGE: &str = "WQL";
pub(super) const QUERY: &str =
    "SELECT SequenceNumber, Description, CreationTime, RestorePointType FROM SystemRestore";
/// `RPC_C_AUTHN_WINNT` / `RPC_C_AUTHZ_NONE` (the `Win32_System_Rpc` feature is not enabled).
const RPC_C_AUTHN_WINNT: u32 = 10;
const RPC_C_AUTHZ_NONE: u32 = 0;
/// Per-object wait for the forward-only enumerator, in milliseconds.
const NEXT_TIMEOUT_MS: i32 = 30_000;

pub struct WmiRestorePointSource;

impl RestorePointSource for WmiRestorePointSource {
    fn query(&self) -> Result<Vec<RawRestorePoint>, WmiFailure> {
        let _com = ComApartment::enter()?;
        let enumerator = open_query()?;
        let mut points = Vec::new();
        while points.len() < MAX_RESTORE_POINTS {
            let mut objects: [Option<IWbemClassObject>; 1] = [None];
            let mut returned = 0_u32;
            // SAFETY: objects is a writable one-element slice and returned is a valid out pointer.
            let status = unsafe { enumerator.Next(NEXT_TIMEOUT_MS, &mut objects, &mut returned) };
            if status.is_err() {
                return Err(WmiFailure(status.0));
            }
            let [object] = objects;
            match object {
                Some(object) if returned == 1 => points.push(read_point(&object)),
                _ if status.0 == WBEM_S_TIMEDOUT.0 => return Err(WmiFailure(status.0)),
                _ => break,
            }
        }
        Ok(points)
    }
}

fn open_query() -> Result<IEnumWbemClassObject, WmiFailure> {
    // SAFETY: WbemLocator is the documented in-process CLSID for IWbemLocator.
    let locator: IWbemLocator =
        unsafe { CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER) }.map_err(failure)?;
    let empty = BSTR::new();
    // SAFETY: all BSTR arguments are valid for the call; a null context is documented.
    let services = unsafe {
        locator.ConnectServer(
            &BSTR::from(NAMESPACE),
            &empty,
            &empty,
            &empty,
            0,
            &empty,
            None::<&IWbemContext>,
        )
    }
    .map_err(failure)?;
    // SAFETY: services is a live proxy; the arguments are the documented WMI client defaults.
    unsafe {
        CoSetProxyBlanket(
            &services,
            RPC_C_AUTHN_WINNT,
            RPC_C_AUTHZ_NONE,
            PCWSTR::null(),
            RPC_C_AUTHN_LEVEL_CALL,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            None,
            EOAC_NONE,
        )
    }
    .map_err(failure)?;
    // SAFETY: the query and language are compile-time constants; a null context is documented.
    unsafe {
        services.ExecQuery(
            &BSTR::from(QUERY_LANGUAGE),
            &BSTR::from(QUERY),
            WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
            None::<&IWbemContext>,
        )
    }
    .map_err(failure)
}

fn read_point(object: &IWbemClassObject) -> RawRestorePoint {
    RawRestorePoint {
        sequence_number: property(object, w!("SequenceNumber")).and_then(PropertyValue::number),
        description: property(object, w!("Description")).and_then(PropertyValue::text),
        creation_time: property(object, w!("CreationTime")).and_then(PropertyValue::text),
        restore_point_type: property(object, w!("RestorePointType"))
            .and_then(PropertyValue::number),
    }
}

enum PropertyValue {
    Number(u32),
    Text(String),
}

impl PropertyValue {
    fn number(self) -> Option<u32> {
        match self {
            Self::Number(value) => Some(value),
            Self::Text(_) => None,
        }
    }

    fn text(self) -> Option<String> {
        match self {
            Self::Text(value) => Some(value),
            Self::Number(_) => None,
        }
    }
}

fn property(object: &IWbemClassObject, name: PCWSTR) -> Option<PropertyValue> {
    let mut value = OwnedVariant(VARIANT::default());
    // SAFETY: name is a static NUL-terminated literal and value is a writable, empty VARIANT.
    unsafe { object.Get(name, 0, &mut value.0, None, None) }.ok()?;
    // SAFETY: vt discriminates the union member WMI initialized.
    unsafe {
        let inner = &value.0.Anonymous.Anonymous;
        match inner.vt {
            VT_BSTR => Some(PropertyValue::Text(inner.Anonymous.bstrVal.to_string())),
            VT_I4 => u32::try_from(inner.Anonymous.lVal)
                .ok()
                .map(PropertyValue::Number),
            VT_UI4 => Some(PropertyValue::Number(inner.Anonymous.ulVal)),
            _ => None,
        }
    }
}

struct OwnedVariant(VARIANT);

impl Drop for OwnedVariant {
    fn drop(&mut self) {
        // SAFETY: the VARIANT is either zeroed (VT_EMPTY) or filled by IWbemClassObject::Get.
        let _ = unsafe { VariantClear(&mut self.0) };
    }
}

struct ComApartment {
    owns: bool,
}

impl ComApartment {
    fn enter() -> Result<Self, WmiFailure> {
        // SAFETY: a null reserved pointer and COINIT_MULTITHREADED are documented arguments.
        let result: HRESULT = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if result == S_OK || result == S_FALSE {
            Ok(Self { owns: true })
        } else if result == RPC_E_CHANGED_MODE {
            Ok(Self { owns: false })
        } else {
            Err(WmiFailure(result.0))
        }
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.owns {
            // SAFETY: balances the successful CoInitializeEx on this thread.
            unsafe { CoUninitialize() };
        }
    }
}

fn failure(error: windows::core::Error) -> WmiFailure {
    WmiFailure(error.code().0)
}
