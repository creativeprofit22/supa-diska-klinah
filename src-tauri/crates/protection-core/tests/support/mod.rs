#![allow(dead_code)]

use ed25519_dalek::{Signer, SigningKey};
use protection_core::{PackVerifier, to_hex};

/// Deterministic test keys. Never used for release packs.
pub fn signing_key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

pub fn verifier_for(key: &SigningKey) -> PackVerifier {
    PackVerifier::from_key_file(&to_hex(key.verifying_key().as_bytes())).unwrap()
}

pub fn sign(key: &SigningKey, bytes: &[u8]) -> Vec<u8> {
    to_hex(&key.sign(bytes).to_bytes()).into_bytes()
}

pub const EICAR: &[u8] = br"X5O!P%@AP[4\PZX54(P^)7CC)7}$EICAR-STANDARD-ANTIVIRUS-TEST-FILE!$H+H*";

pub const EICAR_SHA256: &str = "275a021bbfb6489e54d471899f7db9d1663fc695ec2fe2a2c4538aabf651fd0f";

pub fn pack_json(sequence: u64, rules: &str) -> String {
    format!(
        r#"{{"format":1,"sequence":{sequence},"created":"2026-09-23T00:00:00Z","minAppVersion":"0.1.0","description":"test pack","rules":[{rules}]}}"#
    )
}

pub fn baseline_rules() -> String {
    format!(
        r#"{{"id":"eicar.sha256","name":"EICAR test file","severity":"low","provenance":"https://www.eicar.org/download-anti-malware-testfile/","match":{{"kind":"sha256","sha256":"{EICAR_SHA256}"}}}},
{{"id":"eicar.bytes","name":"EICAR test signature","severity":"low","provenance":"EICAR standard test string","match":{{"kind":"bytes","pattern":"{}","offsetMin":0,"offsetMax":0,"maxFileSize":128}}}},
{{"id":"name.test","name":"Test name rule","severity":"medium","provenance":"test","match":{{"kind":"fileName","name":"Evil-Test.EXE"}}}}"#,
        to_hex(&EICAR[..16])
    )
}
