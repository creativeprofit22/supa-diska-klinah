mod support;

use protection_core::{
    AppVersion, UNCONFIGURED_UPDATE_KEY, UpdateManifestError, UpdateSigning, UpdateVerifier, to_hex,
};
use support::{sign, signing_key};

const NOW: u64 = 1_790_000_000;
const DAY: u64 = 24 * 60 * 60;
const SHA: &str = "a3f1c2d4e5b60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90";
const THUMB: &str = "0123456789ABCDEF0123456789ABCDEF01234567";

fn v(text: &str) -> AppVersion {
    AppVersion::parse(text).unwrap()
}

fn verifier(seed: u8) -> UpdateVerifier {
    UpdateVerifier::from_key_file(&to_hex(signing_key(seed).verifying_key().as_bytes())).unwrap()
}

struct Manifest {
    version: &'static str,
    minimum: &'static str,
    name: String,
    size: u64,
    sha: &'static str,
    not_before: u64,
    expires: u64,
    signing: &'static str,
    thumbprint: Option<&'static str>,
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            version: "0.2.0",
            minimum: "0.1.0",
            name: "Supa-Diska-Klinah_0.2.0_x64-setup.exe".into(),
            size: 12_345_678,
            sha: SHA,
            not_before: NOW - DAY,
            expires: NOW + 30 * DAY,
            signing: "none",
            thumbprint: None,
        }
    }
}

impl Manifest {
    fn json(&self) -> String {
        let thumb = self
            .thumbprint
            .map(|t| format!(r#","signerThumbprint":"{t}""#))
            .unwrap_or_default();
        format!(
            r#"{{"format":1,"version":"{}","minimumVersion":"{}","installer":{{"name":"{}","size":{},"sha256":"{}"}},"notBefore":{},"expires":{},"signing":"{}"{thumb}}}"#,
            self.version,
            self.minimum,
            self.name,
            self.size,
            self.sha,
            self.not_before,
            self.expires,
            self.signing
        )
    }
}

fn verify(manifest: &Manifest) -> Result<protection_core::UpdateManifest, UpdateManifestError> {
    let bytes = manifest.json().into_bytes();
    verifier(7).verify(&bytes, &sign(&signing_key(7), &bytes), v("0.1.0"), NOW)
}

#[test]
fn accepts_a_valid_unsigned_release_manifest() {
    let manifest = verify(&Manifest::default()).unwrap();
    assert_eq!(manifest.version, v("0.2.0"));
    assert_eq!(
        manifest.installer_name,
        "Supa-Diska-Klinah_0.2.0_x64-setup.exe"
    );
    assert_eq!(manifest.installer_size, 12_345_678);
    assert_eq!(to_hex(&manifest.installer_sha256), SHA);
    assert_eq!(manifest.signing, UpdateSigning::None);
    assert_eq!(manifest.signer_thumbprint, None);
}

#[test]
fn accepts_an_authenticode_manifest_with_a_thumbprint() {
    let manifest = verify(&Manifest {
        signing: "authenticode",
        thumbprint: Some(THUMB),
        ..Manifest::default()
    })
    .unwrap();
    assert_eq!(manifest.signing, UpdateSigning::Authenticode);
    assert_eq!(manifest.signer_thumbprint.as_deref(), Some(THUMB));
}

#[test]
fn rejects_tampering_and_foreign_keys_before_parsing() {
    let bytes = Manifest::default().json().into_bytes();
    let signature = sign(&signing_key(7), &bytes);
    let mut tampered = bytes.clone();
    let index = tampered.iter().position(|&b| b == b'5').unwrap();
    tampered[index] = b'6';
    assert_eq!(
        verifier(7).verify(&tampered, &signature, v("0.1.0"), NOW),
        Err(UpdateManifestError::BadSignature)
    );
    assert_eq!(
        verifier(8).verify(&bytes, &signature, v("0.1.0"), NOW),
        Err(UpdateManifestError::BadSignature)
    );
    // A validly signed non-JSON payload is still refused.
    let junk = b"not json".to_vec();
    assert_eq!(
        verifier(7).verify(&junk, &sign(&signing_key(7), &junk), v("0.1.0"), NOW),
        Err(UpdateManifestError::Malformed)
    );
    for bad in [&b""[..], b"zz", &[b'a'; 127], &[b'g'; 128]] {
        assert_eq!(
            verifier(7).verify(&bytes, bad, v("0.1.0"), NOW),
            Err(UpdateManifestError::MalformedSignature)
        );
    }
    let huge = vec![b' '; 16 * 1024 + 1];
    assert_eq!(
        verifier(7).verify(&huge, &signature, v("0.1.0"), NOW),
        Err(UpdateManifestError::TooLarge)
    );
}

#[test]
fn rejects_rollback_and_same_version() {
    let bytes = Manifest::default().json().into_bytes();
    let signature = sign(&signing_key(7), &bytes);
    for running in ["0.2.0", "0.3.0", "1.0.0"] {
        assert_eq!(
            verifier(7).verify(&bytes, &signature, v(running), NOW),
            Err(UpdateManifestError::NotNewer),
            "{running}"
        );
    }
    let old = verify(&Manifest {
        minimum: "0.1.5",
        ..Manifest::default()
    });
    assert_eq!(old, Err(UpdateManifestError::BelowMinimumVersion));
}

#[test]
fn enforces_the_validity_window() {
    assert_eq!(
        verify(&Manifest {
            not_before: NOW - 31 * DAY,
            expires: NOW - 1,
            ..Manifest::default()
        }),
        Err(UpdateManifestError::Expired)
    );
    assert_eq!(
        verify(&Manifest {
            not_before: NOW + DAY,
            expires: NOW + 2 * DAY,
            ..Manifest::default()
        }),
        Err(UpdateManifestError::NotYetValid)
    );
    // Small clock skew is tolerated.
    assert!(
        verify(&Manifest {
            not_before: NOW + 60,
            ..Manifest::default()
        })
        .is_ok()
    );
    // Validity longer than 90 days is refused, bounding replay.
    assert_eq!(
        verify(&Manifest {
            expires: NOW + 120 * DAY,
            ..Manifest::default()
        }),
        Err(UpdateManifestError::Malformed)
    );
}

#[test]
fn rejects_wrong_installer_descriptions() {
    for manifest in [
        Manifest {
            name: "Supa-Diska-Klinah_0.3.0_x64-setup.exe".into(),
            ..Manifest::default()
        },
        Manifest {
            name: "../evil.exe".into(),
            ..Manifest::default()
        },
        Manifest {
            size: 0,
            ..Manifest::default()
        },
        Manifest {
            size: 512 * 1024 * 1024 + 1,
            ..Manifest::default()
        },
        Manifest {
            sha: "A3F1C2D4E5B60718293A4B5C6D7E8F90A1B2C3D4E5F60718293A4B5C6D7E8F90",
            ..Manifest::default()
        },
        Manifest {
            sha: "abc",
            ..Manifest::default()
        },
    ] {
        assert_eq!(
            verify(&manifest),
            Err(UpdateManifestError::InvalidInstaller),
            "{}",
            manifest.json()
        );
    }
}

#[test]
fn signing_policy_must_be_consistent() {
    for (signing, thumbprint) in [
        ("none", Some(THUMB)),
        ("authenticode", None),
        (
            "authenticode",
            Some("0123456789abcdef0123456789abcdef01234567"),
        ),
        ("authenticode", Some("0123")),
    ] {
        assert_eq!(
            verify(&Manifest {
                signing,
                thumbprint,
                ..Manifest::default()
            }),
            Err(UpdateManifestError::InvalidSigningPolicy),
            "{signing} {thumbprint:?}"
        );
    }
    assert_eq!(
        verify(&Manifest {
            signing: "optional",
            ..Manifest::default()
        }),
        Err(UpdateManifestError::Malformed)
    );
}

#[test]
fn rejects_unknown_fields_and_formats() {
    let json = Manifest::default()
        .json()
        .replace(r#""format":1"#, r#""format":2"#);
    let bytes = json.into_bytes();
    assert_eq!(
        verifier(7).verify(&bytes, &sign(&signing_key(7), &bytes), v("0.1.0"), NOW),
        Err(UpdateManifestError::Malformed)
    );
    let json = Manifest::default()
        .json()
        .replace(r#""format":1"#, r#""format":1,"url":"https://evil""#);
    let bytes = json.into_bytes();
    assert_eq!(
        verifier(7).verify(&bytes, &sign(&signing_key(7), &bytes), v("0.1.0"), NOW),
        Err(UpdateManifestError::Malformed)
    );
}

#[test]
fn key_files_are_strict() {
    assert_eq!(
        UpdateVerifier::from_key_file(UNCONFIGURED_UPDATE_KEY).err(),
        Some(UpdateManifestError::NotConfigured)
    );
    assert_eq!(
        UpdateVerifier::from_key_file("unconfigured\n").err(),
        Some(UpdateManifestError::NotConfigured)
    );
    let good = to_hex(signing_key(7).verifying_key().as_bytes());
    assert!(UpdateVerifier::from_key_file(&format!("{good}\r\n")).is_ok());
    for bad in [
        String::new(),
        good[..62].to_owned(),
        format!("{good}\n\n"),
        "0".repeat(64),
    ] {
        assert_eq!(
            UpdateVerifier::from_key_file(&bad).err(),
            Some(UpdateManifestError::MalformedKey),
            "{bad:?}"
        );
    }
}

/// The release script (scripts/sign-update-manifest.mjs) and this verifier
/// must agree byte for byte. The fixture was produced by the Node script with
/// the deterministic test key (seed [7; 32]); never a release key.
#[test]
fn accepts_a_manifest_signed_by_the_release_script() {
    assert_eq!(
        to_hex(signing_key(7).verifying_key().as_bytes()),
        "ea4a6c63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c"
    );
    let manifest = verifier(7)
        .verify(
            include_bytes!("fixtures/node-signed-update.json"),
            include_bytes!("fixtures/node-signed-update.json.sig"),
            v("0.1.0"),
            NOW,
        )
        .unwrap();
    assert_eq!(manifest.version, v("0.2.0"));
    assert_eq!(manifest.signing, UpdateSigning::None);
}
