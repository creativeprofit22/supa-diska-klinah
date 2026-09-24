#![allow(dead_code)]

use std::path::PathBuf;

use ed25519_dalek::{Signer, SigningKey};
use protection_core::{PackVerifier, to_hex};

pub(crate) const TEST_SEED: u8 = 42;

pub(crate) fn temp_dir(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "sdk-protection-{label}-{}-{}",
        std::process::id(),
        getrandom::u64().unwrap()
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}

pub(crate) fn test_verifier() -> PackVerifier {
    let key = SigningKey::from_bytes(&[TEST_SEED; 32]);
    PackVerifier::from_key_file(&to_hex(key.verifying_key().as_bytes())).unwrap()
}

/// The EICAR test file, assembled at runtime so the source does not contain it.
pub(crate) fn eicar() -> Vec<u8> {
    let mut bytes = b"X5O!P%@AP[4\\PZX54(P^)7CC)7}$EICAR-STANDARD-".to_vec();
    bytes.extend_from_slice(b"ANTIVIRUS-TEST-FILE!$H+H*");
    assert_eq!(bytes.len(), 68);
    bytes
}

pub(crate) fn signed_pack_with(seed: u8, sequence: u64, description: &str) -> (Vec<u8>, Vec<u8>) {
    let json = format!(
        r#"{{"format":1,"sequence":{sequence},"created":"2026-09-23T00:00:00Z","minAppVersion":"0.1.0","description":"{description}","rules":[{{"id":"eicar.sha256","name":"EICAR","severity":"low","provenance":"test","match":{{"kind":"sha256","sha256":"275a021bbfb6489e54d471899f7db9d1663fc695ec2fe2a2c4538aabf651fd0f"}}}},{{"id":"test.bytes","name":"Test marker","severity":"medium","provenance":"test","match":{{"kind":"bytes","pattern":"{}","offsetMin":0,"offsetMax":64,"maxFileSize":null}}}}]}}"#,
        to_hex(b"SDK-TEST-MARKER")
    );
    let key = SigningKey::from_bytes(&[seed; 32]);
    let sig = to_hex(&key.sign(json.as_bytes()).to_bytes()).into_bytes();
    (json.into_bytes(), sig)
}

pub(crate) fn signed_pack(sequence: u64, description: &str) -> (Vec<u8>, Vec<u8>) {
    signed_pack_with(TEST_SEED, sequence, description)
}
