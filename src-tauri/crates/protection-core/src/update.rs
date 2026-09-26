//! App self-update primitives: release versions and the Ed25519-signed update
//! manifest (`update.json` + detached `update.json.sig`, 128 hex characters over
//! the exact bytes). Platform-neutral; downloading, hashing the installer,
//! Authenticode and launching live in windows-platform.

use std::fmt;

use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::evidence::from_hex;

/// A strict `major.minor.patch` release version (no pre-release or build
/// suffix, no leading zeros), ordered numerically.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AppVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl AppVersion {
    pub fn parse(text: &str) -> Option<Self> {
        if text.len() > 32 {
            return None;
        }
        let mut parts = text.split('.');
        let mut next = || -> Option<u32> {
            let part = parts.next()?;
            let valid = !part.is_empty()
                && part.bytes().all(|b| b.is_ascii_digit())
                && (part == "0" || !part.starts_with('0'));
            if valid { part.parse().ok() } else { None }
        };
        let version = Self {
            major: next()?,
            minor: next()?,
            patch: next()?,
        };
        parts.next().is_none().then_some(version)
    }

    /// The installer asset name published for this version. Release assets
    /// are renamed to this hyphenated form because GitHub rewrites spaces.
    pub fn installer_name(&self) -> String {
        format!("Supa-Diska-Klinah_{self}_x64-setup.exe")
    }
}

impl fmt::Display for AppVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Upper bound for any installer, whatever a manifest claims.
pub const MAX_UPDATE_INSTALLER_BYTES: u64 = 512 * 1024 * 1024;
/// Upper bound for the manifest file itself.
pub const MAX_UPDATE_MANIFEST_BYTES: usize = 16 * 1024;
/// A manifest may be valid for at most 90 days, which bounds how long a
/// captured old manifest can be replayed.
pub const MAX_UPDATE_VALIDITY_SECONDS: u64 = 90 * 24 * 60 * 60;
/// Allowed clock skew when comparing `notBefore` with the local clock.
const CLOCK_SKEW_SECONDS: u64 = 10 * 60;
/// The contents of an update key file before a real key is generated. The
/// app then reports updates as unavailable, and release builds refuse to publish.
pub const UNCONFIGURED_UPDATE_KEY: &str = "unconfigured";

/// Whether the installer must carry a Windows Authenticode signature.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UpdateSigning {
    /// Releases are currently unsigned; the manifest's SHA-256 is the integrity root.
    None,
    Authenticode,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ManifestInstallerFile {
    name: String,
    size: u64,
    sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ManifestFile {
    format: u32,
    version: String,
    minimum_version: String,
    installer: ManifestInstallerFile,
    not_before: u64,
    expires: u64,
    signing: UpdateSigning,
    #[serde(default)]
    signer_thumbprint: Option<String>,
}

/// A manifest that passed signature and policy checks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateManifest {
    pub version: AppVersion,
    pub installer_name: String,
    pub installer_size: u64,
    pub installer_sha256: [u8; 32],
    pub signing: UpdateSigning,
    /// Uppercase SHA-1 certificate thumbprint; present exactly when `signing` is Authenticode.
    pub signer_thumbprint: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UpdateManifestError {
    /// The build has no update key yet.
    NotConfigured,
    MalformedKey,
    TooLarge,
    MalformedSignature,
    BadSignature,
    Malformed,
    /// Not newer than the running app (also blocks rollback).
    NotNewer,
    /// The running app is older than the manifest's minimum version.
    BelowMinimumVersion,
    Expired,
    NotYetValid,
    InvalidInstaller,
    InvalidSigningPolicy,
}

impl fmt::Display for UpdateManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotConfigured => "this build has no update key",
            Self::MalformedKey => "the update key is malformed",
            Self::TooLarge => "the update manifest is too large",
            Self::MalformedSignature => "the update signature is malformed",
            Self::BadSignature => "the update signature does not match the trusted key",
            Self::Malformed => "the update manifest is malformed",
            Self::NotNewer => "the update is not newer than this version",
            Self::BelowMinimumVersion => "this version is too old to update directly",
            Self::Expired => "the update manifest has expired",
            Self::NotYetValid => "the update manifest is not valid yet",
            Self::InvalidInstaller => "the update installer description is invalid",
            Self::InvalidSigningPolicy => "the update signing policy is invalid",
        })
    }
}

impl std::error::Error for UpdateManifestError {}

/// The trusted update public key (separate from the rule-pack key).
#[derive(Clone, Debug)]
pub struct UpdateVerifier {
    key: VerifyingKey,
}

impl UpdateVerifier {
    /// Parses a key file: 64 hex characters (optionally one line ending), or
    /// the `unconfigured` marker, which yields `NotConfigured`.
    pub fn from_key_file(text: &str) -> Result<Self, UpdateManifestError> {
        let trimmed = text
            .strip_suffix("\r\n")
            .or_else(|| text.strip_suffix('\n'))
            .unwrap_or(text);
        if trimmed == UNCONFIGURED_UPDATE_KEY {
            return Err(UpdateManifestError::NotConfigured);
        }
        let bytes: [u8; 32] = (trimmed.len() == 64)
            .then(|| from_hex(trimmed))
            .flatten()
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or(UpdateManifestError::MalformedKey)?;
        let key =
            VerifyingKey::from_bytes(&bytes).map_err(|_| UpdateManifestError::MalformedKey)?;
        if key.is_weak() {
            return Err(UpdateManifestError::MalformedKey);
        }
        Ok(Self { key })
    }

    /// Verifies the detached signature over the exact manifest bytes, then
    /// parses and checks it against the running version and the clock.
    /// Nothing is parsed before the signature passes.
    pub fn verify(
        &self,
        manifest_bytes: &[u8],
        signature_file: &[u8],
        running: AppVersion,
        now_unix: u64,
    ) -> Result<UpdateManifest, UpdateManifestError> {
        if manifest_bytes.len() > MAX_UPDATE_MANIFEST_BYTES {
            return Err(UpdateManifestError::TooLarge);
        }
        let signature = parse_signature(signature_file)?;
        self.key
            .verify_strict(manifest_bytes, &signature)
            .map_err(|_| UpdateManifestError::BadSignature)?;
        let file: ManifestFile =
            serde_json::from_slice(manifest_bytes).map_err(|_| UpdateManifestError::Malformed)?;
        check(file, running, now_unix)
    }
}

fn parse_signature(file: &[u8]) -> Result<Signature, UpdateManifestError> {
    let trimmed = file
        .strip_suffix(b"\r\n")
        .or_else(|| file.strip_suffix(b"\n"))
        .unwrap_or(file);
    let bytes: [u8; 64] = (trimmed.len() == 128)
        .then(|| std::str::from_utf8(trimmed).ok())
        .flatten()
        .and_then(from_hex)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or(UpdateManifestError::MalformedSignature)?;
    Ok(Signature::from_bytes(&bytes))
}

fn is_hex(text: &str, len: usize, uppercase: bool) -> bool {
    text.len() == len
        && text.bytes().all(|b| {
            b.is_ascii_digit()
                || if uppercase {
                    (b'A'..=b'F').contains(&b)
                } else {
                    (b'a'..=b'f').contains(&b)
                }
        })
}

fn check(
    file: ManifestFile,
    running: AppVersion,
    now: u64,
) -> Result<UpdateManifest, UpdateManifestError> {
    if file.format != 1 {
        return Err(UpdateManifestError::Malformed);
    }
    let version = AppVersion::parse(&file.version).ok_or(UpdateManifestError::Malformed)?;
    let minimum = AppVersion::parse(&file.minimum_version).ok_or(UpdateManifestError::Malformed)?;
    if minimum > version {
        return Err(UpdateManifestError::Malformed);
    }
    if version <= running {
        return Err(UpdateManifestError::NotNewer);
    }
    if running < minimum {
        return Err(UpdateManifestError::BelowMinimumVersion);
    }
    if file.expires <= file.not_before
        || file.expires - file.not_before > MAX_UPDATE_VALIDITY_SECONDS
    {
        return Err(UpdateManifestError::Malformed);
    }
    if now.saturating_add(CLOCK_SKEW_SECONDS) < file.not_before {
        return Err(UpdateManifestError::NotYetValid);
    }
    if now >= file.expires {
        return Err(UpdateManifestError::Expired);
    }
    let installer = file.installer;
    if installer.name != version.installer_name()
        || installer.size == 0
        || installer.size > MAX_UPDATE_INSTALLER_BYTES
        || !is_hex(&installer.sha256, 64, false)
    {
        return Err(UpdateManifestError::InvalidInstaller);
    }
    let installer_sha256: [u8; 32] = from_hex(&installer.sha256)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or(UpdateManifestError::InvalidInstaller)?;
    match (file.signing, &file.signer_thumbprint) {
        (UpdateSigning::None, None) => {}
        (UpdateSigning::Authenticode, Some(thumbprint)) if is_hex(thumbprint, 40, true) => {}
        _ => return Err(UpdateManifestError::InvalidSigningPolicy),
    }
    Ok(UpdateManifest {
        version,
        installer_name: installer.name,
        installer_size: installer.size,
        installer_sha256,
        signing: file.signing,
        signer_thumbprint: file.signer_thumbprint,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_strict_release_versions() {
        assert_eq!(
            AppVersion::parse("1.20.3"),
            Some(AppVersion {
                major: 1,
                minor: 20,
                patch: 3
            })
        );
        for bad in [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "01.2.3",
            "1.2.3-beta",
            "1.2.x",
            " 1.2.3",
            "1..3",
            "+1.2.3",
            "99999999999.0.0",
        ] {
            assert_eq!(AppVersion::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn orders_numerically_and_names_the_installer() {
        let v = |s| AppVersion::parse(s).unwrap();
        assert!(v("0.10.0") > v("0.9.9"));
        assert!(v("1.0.0") > v("0.99.99"));
        assert_eq!(
            v("0.2.0").installer_name(),
            "Supa-Diska-Klinah_0.2.0_x64-setup.exe"
        );
    }
}
