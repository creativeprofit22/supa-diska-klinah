//! Local-first protection (ADR 0003): read-only process inventory, signer-aware
//! scanning, signed rule packs, contained quarantine, and opt-in network use.

pub mod amsi;
pub mod authenticode;
pub mod breach;
pub mod defender_history;
pub(crate) mod fsutil;
pub mod locations;
pub mod native_ui;
pub mod net;
pub mod process;
pub mod quarantine;
pub mod rules_store;
pub mod scan;
pub mod service;
pub mod updates;

#[cfg(test)]
pub(crate) mod test_support;

/// Re-exported so the app crate keeps a single workspace edge (windows-platform).
pub use protection_core::ProtectionNetworkPolicy;
pub use zeroize::Zeroizing;

/// Strict JSON decoding for IPC inputs; callers enforce size and shape first.
pub fn decode_request<T: serde::de::DeserializeOwned>(
    bytes: &[u8],
) -> Result<T, service::ProtectionError> {
    serde_json::from_slice(bytes).map_err(|_| service::ProtectionError::InvalidInput)
}
