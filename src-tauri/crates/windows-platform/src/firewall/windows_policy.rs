//! COM `INetFwPolicy2` implementation of the firewall reader and writer.

use cleanup_core::system_change::{EntryName, FirewallProfile, UnsupportedReason};
use windows::{
    Win32::{
        Foundation::{E_ACCESSDENIED, REGDB_E_CLASSNOTREG, RPC_E_CHANGED_MODE, S_OK},
        NetworkManagement::WindowsFirewall::{
            INetFwPolicy2, INetFwRule, NET_FW_ACTION, NET_FW_ACTION_ALLOW, NET_FW_ACTION_BLOCK,
            NET_FW_PROFILE_TYPE2, NET_FW_RULE_DIR_IN, NET_FW_RULE_DIR_OUT, NET_FW_RULE_DIRECTION,
            NetFwPolicy2,
        },
        System::{
            Com::{
                CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
                CoUninitialize, IDispatch,
            },
            Ole::IEnumVARIANT,
            Variant::{VARIANT, VT_DISPATCH, VT_UNKNOWN, VariantClear},
        },
    },
    core::{BSTR, HRESULT, IUnknown, Interface},
};

use super::{
    ALL_PROFILES, FirewallAction, FirewallPolicyReader, FirewallPolicyWriter,
    FirewallProfileStatus, FirewallRule, MAX_RULES, RuleDirection, RuleInventory, RuleMatch,
    bounded, profile_bit,
};
use crate::system_change::AdapterError;

/// `HRESULT_FROM_WIN32` values that mean the firewall service (MpsSvc/BFE) is
/// not running or not reachable.
const SERVICE_UNAVAILABLE: [u32; 5] = [
    0x8007_06D9, // EPT_S_NOT_REGISTERED
    0x8007_06BA, // RPC_S_SERVER_UNAVAILABLE
    0x8007_0422, // ERROR_SERVICE_DISABLED
    0x8007_0426, // ERROR_SERVICE_NOT_ACTIVE
    0x8007_0015, // ERROR_NOT_READY
];

pub(super) fn map_hresult(code: HRESULT) -> AdapterError {
    // E_ACCESSDENIED == HRESULT_FROM_WIN32(ERROR_ACCESS_DENIED).
    if code == E_ACCESSDENIED {
        AdapterError::Denied
    } else if code == REGDB_E_CLASSNOTREG || SERVICE_UNAVAILABLE.contains(&(code.0 as u32)) {
        AdapterError::Unsupported(UnsupportedReason::ApiUnavailable)
    } else {
        AdapterError::Failed
    }
}

fn map_err(error: windows::core::Error) -> AdapterError {
    map_hresult(error.code())
}

struct ComApartment {
    owned: bool,
}

impl ComApartment {
    fn enter() -> Result<Self, AdapterError> {
        // SAFETY: a null reserved pointer and COINIT_MULTITHREADED are documented arguments.
        let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if result == RPC_E_CHANGED_MODE {
            // The thread already has a compatible-enough apartment; do not uninit it.
            return Ok(Self { owned: false });
        }
        if result.is_err() {
            return Err(AdapterError::Failed);
        }
        Ok(Self { owned: true })
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.owned {
            // SAFETY: CoInitializeEx returned S_OK/S_FALSE on this thread.
            unsafe { CoUninitialize() };
        }
    }
}

/// A VARIANT cleared on drop.
struct OwnedVariant(VARIANT);

impl Drop for OwnedVariant {
    fn drop(&mut self) {
        // SAFETY: self.0 is a VARIANT initialized by IEnumVARIANT::Next or zeroed.
        let _ = unsafe { VariantClear(&mut self.0) };
    }
}

impl OwnedVariant {
    fn interface(&self) -> Option<IUnknown> {
        // SAFETY: the vt tag selects the active union member; the clone AddRefs
        // the pointer, which the variant still owns and releases on drop.
        unsafe {
            let inner = &self.0.Anonymous.Anonymous;
            if inner.vt == VT_DISPATCH {
                let dispatch: Option<IDispatch> = (*inner.Anonymous.pdispVal).clone();
                dispatch.and_then(|dispatch| dispatch.cast().ok())
            } else if inner.vt == VT_UNKNOWN {
                (*inner.Anonymous.punkVal).clone()
            } else {
                None
            }
        }
    }
}

struct Session {
    policy: INetFwPolicy2,
    _apartment: ComApartment,
}

impl Session {
    fn open() -> Result<Self, AdapterError> {
        let apartment = ComApartment::enter()?;
        // SAFETY: COM is initialized on this thread for the lifetime of Session.
        let policy: INetFwPolicy2 =
            unsafe { CoCreateInstance(&NetFwPolicy2, None, CLSCTX_INPROC_SERVER) }
                .map_err(map_err)?;
        Ok(Self {
            policy,
            _apartment: apartment,
        })
    }

    /// Visits up to MAX_RULES rules; returns (total count, visited all).
    fn for_each_rule(
        &self,
        mut visit: impl FnMut(&INetFwRule) -> Result<(), AdapterError>,
    ) -> Result<(usize, bool), AdapterError> {
        // SAFETY: calls on a live INetFwPolicy2 / INetFwRules within the COM apartment.
        let rules = unsafe { self.policy.Rules() }.map_err(map_err)?;
        // SAFETY: as above.
        let total = unsafe { rules.Count() }.map_err(map_err)?;
        let total = usize::try_from(total).unwrap_or(0);
        // SAFETY: as above.
        let enumerator: IEnumVARIANT = unsafe { rules._NewEnum() }
            .map_err(map_err)?
            .cast()
            .map_err(map_err)?;
        let mut visited = 0usize;
        loop {
            if visited >= MAX_RULES {
                // Bound reached: complete only if nothing remains.
                let mut probe = [VARIANT::default()];
                let mut fetched = 0u32;
                // SAFETY: probe is a writable one-element VARIANT array.
                let hr = unsafe { enumerator.Next(&mut probe, &mut fetched) };
                let [probe] = probe;
                drop(OwnedVariant(probe));
                return Ok((total, !(hr == S_OK && fetched == 1)));
            }
            let mut slot = [VARIANT::default()];
            let mut fetched = 0u32;
            // SAFETY: slot is a writable one-element VARIANT array.
            let hr = unsafe { enumerator.Next(&mut slot, &mut fetched) };
            let [slot] = slot;
            let variant = OwnedVariant(slot);
            if hr.is_err() {
                return Err(map_hresult(hr));
            }
            if hr != S_OK || fetched == 0 {
                return Ok((total, true));
            }
            visited += 1;
            let Some(rule) = variant
                .interface()
                .and_then(|unknown| unknown.cast::<INetFwRule>().ok())
            else {
                continue;
            };
            visit(&rule)?;
        }
    }
}

fn action(value: NET_FW_ACTION) -> FirewallAction {
    if value == NET_FW_ACTION_ALLOW {
        FirewallAction::Allow
    } else if value == NET_FW_ACTION_BLOCK {
        FirewallAction::Block
    } else {
        FirewallAction::Unknown
    }
}

fn direction(value: NET_FW_RULE_DIRECTION) -> RuleDirection {
    if value == NET_FW_RULE_DIR_IN {
        RuleDirection::Inbound
    } else if value == NET_FW_RULE_DIR_OUT {
        RuleDirection::Outbound
    } else {
        RuleDirection::Unknown
    }
}

fn text(value: windows::core::Result<BSTR>) -> String {
    value
        .map(|bstr| bounded(&String::from_utf16_lossy(&bstr)))
        .unwrap_or_default()
}

fn optional(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

fn read_rule(rule: &INetFwRule) -> Result<FirewallRule, AdapterError> {
    // SAFETY: property getters on a live INetFwRule within the COM apartment.
    unsafe {
        Ok(FirewallRule {
            name: bounded(&String::from_utf16_lossy(&rule.Name().map_err(map_err)?)),
            enabled: rule.Enabled().map_err(map_err)?.as_bool(),
            direction: direction(rule.Direction().map_err(map_err)?),
            action: action(rule.Action().map_err(map_err)?),
            profiles: rule.Profiles().unwrap_or(0),
            application_name: optional(text(rule.ApplicationName())),
            local_ports: text(rule.LocalPorts()),
            remote_addresses: text(rule.RemoteAddresses()),
            grouping: optional(text(rule.Grouping())),
        })
    }
}

fn name_matches(rule: &INetFwRule, wide_name: &[u16]) -> Result<bool, AdapterError> {
    // SAFETY: property getter on a live INetFwRule within the COM apartment.
    let name = unsafe { rule.Name() }.map_err(map_err)?;
    Ok(*name == *wide_name)
}

fn native_profile(profile: FirewallProfile) -> NET_FW_PROFILE_TYPE2 {
    NET_FW_PROFILE_TYPE2(profile_bit(profile))
}

/// Real firewall policy via COM. Every call initializes COM on the calling
/// thread and creates its own `NetFwPolicy2` instance.
#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsFirewallPolicy;

impl FirewallPolicyReader for WindowsFirewallPolicy {
    fn profiles(&self) -> Result<(Vec<FirewallProfileStatus>, i32), AdapterError> {
        let session = Session::open()?;
        let policy = &session.policy;
        // SAFETY: getters on a live INetFwPolicy2 within the COM apartment.
        let current = unsafe { policy.CurrentProfileTypes() }.map_err(map_err)?;
        let mut profiles = Vec::with_capacity(ALL_PROFILES.len());
        for profile in ALL_PROFILES {
            let native = native_profile(profile);
            // SAFETY: as above.
            let status = unsafe {
                FirewallProfileStatus {
                    profile,
                    enabled: policy
                        .get_FirewallEnabled(native)
                        .map_err(map_err)?
                        .as_bool(),
                    default_inbound_action: action(
                        policy.get_DefaultInboundAction(native).map_err(map_err)?,
                    ),
                    default_outbound_action: action(
                        policy.get_DefaultOutboundAction(native).map_err(map_err)?,
                    ),
                    block_all_inbound_traffic: policy
                        .get_BlockAllInboundTraffic(native)
                        .map_err(map_err)?
                        .as_bool(),
                    active: current & profile_bit(profile) != 0,
                }
            };
            profiles.push(status);
        }
        Ok((profiles, current))
    }

    fn rules(&self) -> Result<RuleInventory, AdapterError> {
        let session = Session::open()?;
        let mut rules = Vec::new();
        let (total, _) = session.for_each_rule(|rule| {
            if let Ok(record) = read_rule(rule) {
                rules.push(record);
            }
            Ok(())
        })?;
        Ok(RuleInventory { total, rules })
    }

    fn rule_matches(&self, name: &EntryName) -> Result<Vec<RuleMatch>, AdapterError> {
        let wide: Vec<u16> = name.as_str().encode_utf16().collect();
        let session = Session::open()?;
        let mut matches = Vec::new();
        let (_, complete) = session.for_each_rule(|rule| {
            if name_matches(rule, &wide)? {
                // SAFETY: getters on a live INetFwRule within the COM apartment.
                let (enabled, rule_action) = unsafe {
                    (
                        rule.Enabled().map_err(map_err)?.as_bool(),
                        action(rule.Action().map_err(map_err)?),
                    )
                };
                matches.push(RuleMatch {
                    enabled,
                    action: rule_action,
                });
            }
            Ok(())
        })?;
        if !complete {
            return Err(AdapterError::Failed);
        }
        Ok(matches)
    }
}

impl FirewallPolicyWriter for WindowsFirewallPolicy {
    fn set_rule_enabled(&self, name: &EntryName, enabled: bool) -> Result<usize, AdapterError> {
        let wide: Vec<u16> = name.as_str().encode_utf16().collect();
        let session = Session::open()?;
        // Resolve the full matching set first so a bounded enumeration never
        // leaves the rules half-changed.
        let mut targets = Vec::new();
        let (_, complete) = session.for_each_rule(|rule| {
            if name_matches(rule, &wide)? {
                targets.push(rule.clone());
            }
            Ok(())
        })?;
        if !complete {
            return Err(AdapterError::Failed);
        }
        for rule in &targets {
            // SAFETY: setter on a live INetFwRule within the COM apartment.
            unsafe { rule.SetEnabled(enabled.into()) }.map_err(map_err)?;
        }
        Ok(targets.len())
    }

    fn set_profile_enabled(
        &self,
        profile: FirewallProfile,
        enabled: bool,
    ) -> Result<(), AdapterError> {
        let session = Session::open()?;
        // SAFETY: setter on a live INetFwPolicy2 within the COM apartment.
        unsafe {
            session
                .policy
                .put_FirewallEnabled(native_profile(profile), enabled.into())
        }
        .map_err(map_err)
    }
}
