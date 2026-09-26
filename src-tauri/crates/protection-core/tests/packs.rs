mod support;

use protection_core::{
    MAX_PACK_BYTES, PackError, PackVerifier, check_install_sequence, check_restore_previous,
};
use support::*;

fn verify(verifier: &PackVerifier, json: &str, sig: &[u8]) -> Result<u64, PackError> {
    verifier
        .verify(json.as_bytes(), sig, "0.1.0")
        .map(|p| p.sequence())
}

fn signed(json: &str) -> (PackVerifier, Vec<u8>) {
    let key = signing_key(7);
    (verifier_for(&key), sign(&key, json.as_bytes()))
}

#[test]
fn accepts_a_correctly_signed_pack() {
    let json = pack_json(3, &baseline_rules());
    let (verifier, sig) = signed(&json);
    assert_eq!(verify(&verifier, &json, &sig), Ok(3));
    let mut with_newline = sig.clone();
    with_newline.extend_from_slice(b"\r\n");
    assert_eq!(verify(&verifier, &json, &with_newline), Ok(3));
}

#[test]
fn rejects_missing_or_malformed_signature() {
    let json = pack_json(3, &baseline_rules());
    let (verifier, sig) = signed(&json);
    assert_eq!(
        verify(&verifier, &json, b""),
        Err(PackError::MalformedSignature)
    );
    assert_eq!(
        verify(&verifier, &json, &sig[..127]),
        Err(PackError::MalformedSignature)
    );
    let mut not_hex = sig.clone();
    not_hex[0] = b'z';
    assert_eq!(
        verify(&verifier, &json, &not_hex),
        Err(PackError::MalformedSignature)
    );
    let mut extra = sig.clone();
    extra.extend_from_slice(b"\n\n");
    assert_eq!(
        verify(&verifier, &json, &extra),
        Err(PackError::MalformedSignature)
    );
}

#[test]
fn rejects_bad_signature_and_tampered_bytes() {
    let json = pack_json(3, &baseline_rules());
    let (verifier, sig) = signed(&json);
    let mut flipped = sig.clone();
    flipped[10] = if flipped[10] == b'0' { b'1' } else { b'0' };
    assert_eq!(
        verify(&verifier, &json, &flipped),
        Err(PackError::BadSignature)
    );
    let tampered = json.replace("\"severity\":\"low\"", "\"severity\":\"high\"");
    assert_eq!(
        verify(&verifier, &tampered, &sig),
        Err(PackError::BadSignature)
    );
    // Reformatting valid JSON also breaks the signature: exact bytes are signed.
    assert_eq!(
        verify(&verifier, &format!("{json} "), &sig),
        Err(PackError::BadSignature)
    );
}

#[test]
fn rejects_pack_signed_by_another_key() {
    let json = pack_json(3, &baseline_rules());
    let other = signing_key(9);
    let verifier = verifier_for(&signing_key(7));
    assert_eq!(
        verify(&verifier, &json, &sign(&other, json.as_bytes())),
        Err(PackError::BadSignature)
    );
}

#[test]
fn rejects_malformed_keys() {
    assert_eq!(
        PackVerifier::from_key_file("abc").unwrap_err(),
        PackError::MalformedKey
    );
    assert_eq!(
        PackVerifier::from_key_file(&"00".repeat(32)).unwrap_err(),
        PackError::MalformedKey
    );
    assert_eq!(
        PackVerifier::from_key_file(&"zz".repeat(32)).unwrap_err(),
        PackError::MalformedKey
    );
}

#[test]
fn rejects_truncated_empty_and_oversized_packs() {
    let json = pack_json(3, &baseline_rules());
    let truncated = &json[..json.len() / 2];
    let (verifier, sig) = signed(truncated);
    assert!(matches!(
        verify(&verifier, truncated, &sig),
        Err(PackError::Malformed(_))
    ));
    assert_eq!(
        verifier.verify(b"", &sig, "0.1.0").unwrap_err(),
        PackError::Empty
    );
    let huge = vec![b' '; MAX_PACK_BYTES + 1];
    assert_eq!(
        verifier.verify(&huge, &sig, "0.1.0").unwrap_err(),
        PackError::TooLarge
    );
}

#[test]
fn rejects_duplicate_ids_unknown_fields_and_bad_format() {
    let rule = format!(
        r#"{{"id":"dup","name":"a","severity":"low","provenance":"p","match":{{"kind":"sha256","sha256":"{EICAR_SHA256}"}}}}"#
    );
    let dup = pack_json(3, &format!("{rule},{rule}"));
    let (verifier, sig) = signed(&dup);
    assert_eq!(
        verify(&verifier, &dup, &sig),
        Err(PackError::DuplicateRuleId("dup".into()))
    );

    let unknown_top = pack_json(3, &rule).replacen("{", r#"{"extra":1,"#, 1);
    let (verifier, sig) = signed(&unknown_top);
    assert!(matches!(
        verify(&verifier, &unknown_top, &sig),
        Err(PackError::Malformed(_))
    ));

    let unknown_match = pack_json(
        3,
        &rule.replace(r#""kind":"sha256","#, r#""kind":"sha256","yara":"x","#),
    );
    let (verifier, sig) = signed(&unknown_match);
    assert!(matches!(
        verify(&verifier, &unknown_match, &sig),
        Err(PackError::Malformed(_))
    ));

    let unknown_kind = pack_json(3, &rule.replace(r#""kind":"sha256""#, r#""kind":"yara""#));
    let (verifier, sig) = signed(&unknown_kind);
    assert!(matches!(
        verify(&verifier, &unknown_kind, &sig),
        Err(PackError::Malformed(_))
    ));

    let format2 = pack_json(3, &rule).replace(r#""format":1"#, r#""format":2"#);
    let (verifier, sig) = signed(&format2);
    assert_eq!(
        verify(&verifier, &format2, &sig),
        Err(PackError::UnsupportedFormat(2))
    );
}

#[test]
fn rejects_out_of_bounds_rules() {
    let cases = [
        r#"{"id":"BAD ID","name":"a","severity":"low","provenance":"p","match":{"kind":"fileName","name":"x.exe"}}"#.to_string(),
        r#"{"id":"a","name":"a","severity":"low","provenance":"p","match":{"kind":"sha256","sha256":"ABC"}}"#.to_string(),
        r#"{"id":"a","name":"a","severity":"low","provenance":"p","match":{"kind":"bytes","pattern":"0102","offsetMin":0,"offsetMax":0,"maxFileSize":null}}"#.to_string(),
        r#"{"id":"a","name":"a","severity":"low","provenance":"p","match":{"kind":"bytes","pattern":"01020304","offsetMin":5,"offsetMax":1,"maxFileSize":null}}"#.to_string(),
        r#"{"id":"a","name":"a","severity":"low","provenance":"p","match":{"kind":"bytes","pattern":"01020304","offsetMin":0,"offsetMax":99999999,"maxFileSize":null}}"#.to_string(),
        r#"{"id":"a","name":"a","severity":"low","provenance":"p","match":{"kind":"fileName","name":"..\\x.exe"}}"#.to_string(),
        r#"{"id":"a","name":"","severity":"low","provenance":"p","match":{"kind":"fileName","name":"x.exe"}}"#.to_string(),
    ];
    for rule in cases {
        let json = pack_json(3, &rule);
        let (verifier, sig) = signed(&json);
        assert!(
            matches!(verify(&verifier, &json, &sig), Err(PackError::Invalid(_))),
            "{rule}"
        );
    }
    let empty = pack_json(3, "");
    let (verifier, sig) = signed(&empty);
    assert!(matches!(
        verify(&verifier, &empty, &sig),
        Err(PackError::Invalid(_))
    ));
    let zero = pack_json(0, &baseline_rules());
    let (verifier, sig) = signed(&zero);
    assert!(matches!(
        verify(&verifier, &zero, &sig),
        Err(PackError::Invalid(_))
    ));
}

#[test]
fn rejects_packs_for_newer_apps() {
    let json = pack_json(3, &baseline_rules()).replace("\"0.1.0\"", "\"9.0.0\"");
    let (verifier, sig) = signed(&json);
    assert_eq!(
        verify(&verifier, &json, &sig),
        Err(PackError::RequiresNewerApp("9.0.0".into()))
    );
}

#[test]
fn sequence_rollback_is_refused_except_explicit_previous_restore() {
    assert!(check_install_sequence(5, 4).is_ok());
    assert_eq!(
        check_install_sequence(4, 4),
        Err(PackError::Rollback {
            candidate: 4,
            floor: 4
        })
    );
    assert_eq!(
        check_install_sequence(2, 4),
        Err(PackError::Rollback {
            candidate: 2,
            floor: 4
        })
    );
    assert_eq!(check_restore_previous(Some(3)), Ok(3));
    assert_eq!(check_restore_previous(None), Err(PackError::NoPrevious));
}
