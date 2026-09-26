use serde::{Deserialize, Serialize};

/// How serious a finding is if it is a true positive. This is not a
/// confidence level; confidence is carried by [`Evidence`].
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    Low,
    Medium,
    High,
}

/// Which signed-rule mechanism produced a match.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MatchMethod {
    Sha256,
    Bytes,
    FileName,
}

/// Why evidence could not be collected. Never interpreted as "no match".
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UnavailableReason {
    AccessDenied,
    InUse,
    TooLarge,
    ReparsePoint,
    NotFound,
    SignerNotCheckable,
    ProviderAbsent,
    ProviderNoResponse,
    Cancelled,
    ReadFailed,
}

/// Typed evidence for every finding (ADR 0003, decision 7).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Evidence {
    /// A SHA-256 or byte-pattern match from a signed rule pack.
    Deterministic {
        rule_id: String,
        rule_name: String,
        pack_sequence: u64,
        method: MatchMethod,
        severity: Severity,
    },
    /// A local heuristic or a signed file-name rule. May be a false positive.
    Heuristic {
        heuristic_id: String,
        reason: String,
        false_positive_note: String,
        severity: Severity,
    },
    /// Evidence could not be gathered; this is not a clean result.
    Unavailable { reason: UnavailableReason },
    /// A response from a component this app does not control.
    External {
        provider: String,
        observed_at: String,
        detail: String,
    },
}

impl Evidence {
    pub fn is_deterministic(&self) -> bool {
        matches!(self, Self::Deterministic { .. })
    }
}

/// Lowercase hex encoding helper shared by the crate and its callers.
pub fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

/// Strict lowercase-or-uppercase hex decoding; rejects odd length and non-hex.
pub fn from_hex(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    fn nibble(value: u8) -> Option<u8> {
        match value {
            b'0'..=b'9' => Some(value - b'0'),
            b'a'..=b'f' => Some(value - b'a' + 10),
            b'A'..=b'F' => Some(value - b'A' + 10),
            _ => None,
        }
    }
    bytes
        .chunks_exact(2)
        .map(|pair| Some((nibble(pair[0])? << 4) | nibble(pair[1])?))
        .collect()
}
