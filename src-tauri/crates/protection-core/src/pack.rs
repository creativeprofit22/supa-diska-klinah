//! Signed rule-pack format (ADR 0003, decision 2).
//!
//! A pack is `pack.json` plus `pack.sig`, a detached Ed25519 signature over the
//! exact bytes of `pack.json`, stored as 128 hex characters. The signature is
//! verified before any JSON parsing happens.

use std::collections::HashSet;
use std::fmt;

use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::evidence::{Severity, from_hex};

pub const PACK_FORMAT: u32 = 1;
pub const MAX_PACK_BYTES: usize = 1024 * 1024;
pub const MAX_RULES: usize = 4096;
pub const MAX_ID_CHARS: usize = 64;
pub const MAX_NAME_CHARS: usize = 128;
pub const MAX_PROVENANCE_CHARS: usize = 256;
pub const MIN_PATTERN_BYTES: usize = 4;
pub const MAX_PATTERN_BYTES: usize = 256;
/// Byte patterns may only anchor within the first MiB of a file. This bounds
/// the head buffer the scanner keeps per file.
pub const MAX_PATTERN_OFFSET: u64 = 1024 * 1024;
pub const MAX_FILE_NAME_CHARS: usize = 255;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PackError {
    TooLarge,
    Empty,
    MalformedSignature,
    MalformedKey,
    BadSignature,
    Malformed(String),
    UnsupportedFormat(u32),
    RequiresNewerApp(String),
    Invalid(String),
    DuplicateRuleId(String),
    Rollback { candidate: u64, floor: u64 },
    NoPrevious,
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => write!(f, "rule pack exceeds {MAX_PACK_BYTES} bytes"),
            Self::Empty => write!(f, "rule pack is empty"),
            Self::MalformedSignature => write!(f, "signature file is not 128 hex characters"),
            Self::MalformedKey => write!(f, "public key is malformed"),
            Self::BadSignature => write!(f, "signature does not match the trusted key"),
            Self::Malformed(detail) => write!(f, "rule pack is not valid JSON: {detail}"),
            Self::UnsupportedFormat(format) => write!(f, "unsupported rule-pack format {format}"),
            Self::RequiresNewerApp(version) => {
                write!(f, "rule pack requires app {version} or newer")
            }
            Self::Invalid(detail) => write!(f, "rule pack is invalid: {detail}"),
            Self::DuplicateRuleId(id) => write!(f, "duplicate rule id {id}"),
            Self::Rollback { candidate, floor } => {
                write!(
                    f,
                    "rule pack sequence {candidate} is not newer than {floor}"
                )
            }
            Self::NoPrevious => write!(f, "no previous rule pack is retained"),
        }
    }
}

impl std::error::Error for PackError {}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RulePack {
    pub format: u32,
    pub sequence: u64,
    pub created: String,
    pub min_app_version: String,
    pub description: String,
    pub rules: Vec<Rule>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Rule {
    pub id: String,
    pub name: String,
    pub severity: Severity,
    pub provenance: String,
    #[serde(rename = "match")]
    pub matcher: RuleMatch,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RuleMatch {
    /// Exact SHA-256 of the whole file (lowercase hex).
    Sha256 { sha256: String },
    /// Byte pattern (hex) whose start lies in `offset_min..=offset_max`.
    Bytes {
        pattern: String,
        offset_min: u64,
        offset_max: u64,
        max_file_size: Option<u64>,
    },
    /// Case-insensitive exact file name. Always heuristic evidence.
    FileName { name: String },
}

/// A pack whose signature and bounds have been checked.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedPack {
    pack: RulePack,
}

impl VerifiedPack {
    pub fn pack(&self) -> &RulePack {
        &self.pack
    }

    pub fn sequence(&self) -> u64 {
        self.pack.sequence
    }
}

/// The trusted rule-pack public key.
#[derive(Clone, Debug)]
pub struct PackVerifier {
    key: VerifyingKey,
}

impl PackVerifier {
    /// Parse a key file: 64 hex characters of the raw Ed25519 public key,
    /// optionally followed by one line ending.
    pub fn from_key_file(text: &str) -> Result<Self, PackError> {
        let trimmed = strip_single_newline(text.as_bytes());
        let hex = std::str::from_utf8(trimmed).map_err(|_| PackError::MalformedKey)?;
        let bytes: [u8; 32] = from_hex(hex)
            .ok_or(PackError::MalformedKey)?
            .try_into()
            .map_err(|_| PackError::MalformedKey)?;
        let key = VerifyingKey::from_bytes(&bytes).map_err(|_| PackError::MalformedKey)?;
        if key.is_weak() {
            return Err(PackError::MalformedKey);
        }
        Ok(Self { key })
    }

    /// Verify the detached signature over the exact bytes, then parse and
    /// validate the pack. Nothing is parsed before the signature passes.
    pub fn verify(
        &self,
        pack_bytes: &[u8],
        signature_file: &[u8],
        app_version: &str,
    ) -> Result<VerifiedPack, PackError> {
        if pack_bytes.len() > MAX_PACK_BYTES {
            return Err(PackError::TooLarge);
        }
        if pack_bytes.is_empty() {
            return Err(PackError::Empty);
        }
        let signature = parse_signature(signature_file)?;
        self.key
            .verify_strict(pack_bytes, &signature)
            .map_err(|_| PackError::BadSignature)?;
        let pack: RulePack = serde_json::from_slice(pack_bytes)
            .map_err(|error| PackError::Malformed(error.to_string()))?;
        validate(&pack, app_version)?;
        Ok(VerifiedPack { pack })
    }
}

fn strip_single_newline(bytes: &[u8]) -> &[u8] {
    bytes
        .strip_suffix(b"\r\n")
        .or_else(|| bytes.strip_suffix(b"\n"))
        .unwrap_or(bytes)
}

fn parse_signature(file: &[u8]) -> Result<Signature, PackError> {
    let trimmed = strip_single_newline(file);
    if trimmed.len() != 128 {
        return Err(PackError::MalformedSignature);
    }
    let text = std::str::from_utf8(trimmed).map_err(|_| PackError::MalformedSignature)?;
    let bytes: [u8; 64] = from_hex(text)
        .ok_or(PackError::MalformedSignature)?
        .try_into()
        .map_err(|_| PackError::MalformedSignature)?;
    Ok(Signature::from_bytes(&bytes))
}

fn parse_version(text: &str) -> Option<(u32, u32, u32)> {
    let mut parts = text.split('.');
    let parsed = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    parts.next().is_none().then_some(parsed)
}

fn is_timestamp(text: &str) -> bool {
    // YYYY-MM-DDTHH:MM:SSZ
    let bytes = text.as_bytes();
    bytes.len() == 20
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            4 | 7 => *byte == b'-',
            10 => *byte == b'T',
            13 | 16 => *byte == b':',
            19 => *byte == b'Z',
            _ => byte.is_ascii_digit(),
        })
}

fn bounded_text(field: &str, value: &str, max: usize) -> Result<(), PackError> {
    let count = value.chars().count();
    if count == 0 || count > max || value.chars().any(char::is_control) {
        return Err(PackError::Invalid(format!(
            "{field} must be 1..={max} printable characters"
        )));
    }
    Ok(())
}

fn validate(pack: &RulePack, app_version: &str) -> Result<(), PackError> {
    if pack.format != PACK_FORMAT {
        return Err(PackError::UnsupportedFormat(pack.format));
    }
    if pack.sequence == 0 {
        return Err(PackError::Invalid("sequence must be at least 1".into()));
    }
    if !is_timestamp(&pack.created) {
        return Err(PackError::Invalid(
            "created must be YYYY-MM-DDTHH:MM:SSZ".into(),
        ));
    }
    let required = parse_version(&pack.min_app_version)
        .ok_or_else(|| PackError::Invalid("minAppVersion must be MAJOR.MINOR.PATCH".into()))?;
    let running = parse_version(app_version)
        .ok_or_else(|| PackError::Invalid("app version is not MAJOR.MINOR.PATCH".into()))?;
    if running < required {
        return Err(PackError::RequiresNewerApp(pack.min_app_version.clone()));
    }
    bounded_text("description", &pack.description, MAX_PROVENANCE_CHARS)?;
    if pack.rules.is_empty() || pack.rules.len() > MAX_RULES {
        return Err(PackError::Invalid(format!(
            "rules must contain 1..={MAX_RULES} entries"
        )));
    }
    let mut ids = HashSet::new();
    for rule in &pack.rules {
        let valid_id = !rule.id.is_empty()
            && rule.id.len() <= MAX_ID_CHARS
            && rule.id.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
            });
        if !valid_id {
            return Err(PackError::Invalid(format!(
                "rule id {:?} is not [a-z0-9._-]{{1,64}}",
                rule.id
            )));
        }
        if !ids.insert(rule.id.as_str()) {
            return Err(PackError::DuplicateRuleId(rule.id.clone()));
        }
        bounded_text("rule name", &rule.name, MAX_NAME_CHARS)?;
        bounded_text("rule provenance", &rule.provenance, MAX_PROVENANCE_CHARS)?;
        match &rule.matcher {
            RuleMatch::Sha256 { sha256 } => {
                let lowercase = sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
                if sha256.len() != 64 || !lowercase {
                    return Err(PackError::Invalid(format!(
                        "rule {} sha256 must be 64 lowercase hex",
                        rule.id
                    )));
                }
            }
            RuleMatch::Bytes {
                pattern,
                offset_min,
                offset_max,
                max_file_size,
            } => {
                let bytes = from_hex(pattern).ok_or_else(|| {
                    PackError::Invalid(format!("rule {} pattern is not hex", rule.id))
                })?;
                if !(MIN_PATTERN_BYTES..=MAX_PATTERN_BYTES).contains(&bytes.len()) {
                    return Err(PackError::Invalid(format!(
                        "rule {} pattern must be {MIN_PATTERN_BYTES}..={MAX_PATTERN_BYTES} bytes",
                        rule.id
                    )));
                }
                if offset_min > offset_max || *offset_max > MAX_PATTERN_OFFSET {
                    return Err(PackError::Invalid(format!(
                        "rule {} offsets must satisfy min <= max <= {MAX_PATTERN_OFFSET}",
                        rule.id
                    )));
                }
                if matches!(max_file_size, Some(0)) {
                    return Err(PackError::Invalid(format!(
                        "rule {} maxFileSize must be positive",
                        rule.id
                    )));
                }
            }
            RuleMatch::FileName { name } => {
                bounded_text("rule file name", name, MAX_FILE_NAME_CHARS)?;
                if name.contains(['\\', '/', ':']) {
                    return Err(PackError::Invalid(format!(
                        "rule {} file name must not contain separators",
                        rule.id
                    )));
                }
            }
        }
    }
    Ok(())
}

/// Anti-rollback for normal installs: the candidate must be newer than every
/// pack ever installed (and the embedded baseline).
pub fn check_install_sequence(candidate: u64, highest_installed: u64) -> Result<(), PackError> {
    if candidate <= highest_installed {
        return Err(PackError::Rollback {
            candidate,
            floor: highest_installed,
        });
    }
    Ok(())
}

/// The only permitted downgrade: explicitly restoring the retained previous pack.
pub fn check_restore_previous(previous: Option<u64>) -> Result<u64, PackError> {
    previous.ok_or(PackError::NoPrevious)
}
