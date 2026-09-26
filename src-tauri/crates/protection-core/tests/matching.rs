mod support;

use protection_core::{
    CompiledPack, Evidence, FileFacts, HEURISTIC_CATALOG, LocationClass, MatchMethod, ProcessFacts,
    SignerStatus, UnavailableReason, evaluate_file, evaluate_process,
};
use support::*;

fn compiled(rules: &str) -> CompiledPack {
    let json = pack_json(4, rules);
    let key = signing_key(1);
    let verified = verifier_for(&key)
        .verify(json.as_bytes(), &sign(&key, json.as_bytes()), "0.1.0")
        .unwrap();
    CompiledPack::compile(&verified)
}

fn scan(pack: &CompiledPack, data: &[u8], chunk: usize) -> protection_core::FileVerdict {
    let mut matcher = pack.matcher();
    for part in data.chunks(chunk.max(1)) {
        matcher.update(part);
    }
    matcher.finish()
}

#[test]
fn eicar_matches_by_hash_and_bytes_across_any_chunking() {
    let pack = compiled(&baseline_rules());
    for chunk in [1, 3, 16, 4096] {
        let verdict = scan(&pack, EICAR, chunk);
        assert_eq!(verdict.sha256, EICAR_SHA256);
        let methods: Vec<_> = verdict.hits.iter().map(|hit| hit.method).collect();
        assert!(methods.contains(&MatchMethod::Sha256), "chunk {chunk}");
        assert!(methods.contains(&MatchMethod::Bytes), "chunk {chunk}");
        assert!(
            verdict
                .hits
                .iter()
                .all(|hit| hit.evidence(4).is_deterministic())
        );
    }
}

#[test]
fn byte_pattern_respects_offset_and_size_limits() {
    let pack = compiled(&baseline_rules());
    let mut shifted = b"x".to_vec();
    shifted.extend_from_slice(EICAR);
    assert!(
        scan(&pack, &shifted, 7).hits.is_empty(),
        "offset 1 is outside offsetMax 0"
    );

    let mut padded = EICAR.to_vec();
    padded.extend(std::iter::repeat_n(b' ', 200));
    let verdict = scan(&pack, &padded, 64);
    assert!(verdict.hits.is_empty(), "file larger than maxFileSize 128");
}

#[test]
fn clean_bytes_have_no_hits_and_file_names_are_heuristic() {
    let pack = compiled(&baseline_rules());
    assert!(scan(&pack, b"hello world", 4).hits.is_empty());
    let hits = pack.match_name("evil-test.exe");
    assert_eq!(hits.len(), 1);
    assert!(matches!(hits[0].evidence(4), Evidence::Heuristic { .. }));
    assert!(pack.match_name("other.exe").is_empty());
}

#[test]
fn heuristic_catalog_has_unique_ids_and_notes() {
    let mut ids: Vec<_> = HEURISTIC_CATALOG.iter().map(|h| h.id).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), HEURISTIC_CATALOG.len());
    assert!(
        HEURISTIC_CATALOG
            .iter()
            .all(|h| !h.false_positive_note.is_empty())
    );
}

fn ids(evidence: &[Evidence]) -> Vec<String> {
    evidence
        .iter()
        .filter_map(|e| match e {
            Evidence::Heuristic { heuristic_id, .. } => Some(heuristic_id.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn system_name_outside_windows_is_downgraded_by_microsoft_signature() {
    let unsigned = SignerStatus::Unsigned;
    let facts = FileFacts {
        file_name: "svchost.exe",
        location: LocationClass::Temp,
        header: b"MZ",
        signer: &unsigned,
    };
    let result = evaluate_file(&facts);
    assert!(result.iter().any(|e| matches!(e, Evidence::Heuristic { heuristic_id, severity: protection_core::Severity::High, .. } if heuristic_id.starts_with("H001"))));

    let ms = SignerStatus::Valid {
        subject: "Microsoft Windows".into(),
        microsoft: true,
    };
    let facts = FileFacts {
        signer: &ms,
        ..facts
    };
    let result = evaluate_file(&facts);
    assert!(result.iter().any(|e| matches!(e, Evidence::Heuristic { heuristic_id, severity: protection_core::Severity::Low, .. } if heuristic_id.starts_with("H001"))));

    let facts = FileFacts {
        location: LocationClass::SystemRoot,
        ..facts
    };
    assert!(
        !ids(&evaluate_file(&facts))
            .iter()
            .any(|id| id.starts_with("H001"))
    );
}

#[test]
fn name_tricks_and_mz_mismatch_are_flagged() {
    let na = SignerStatus::NotApplicable;
    let double = FileFacts {
        file_name: "invoice.pdf.exe",
        location: LocationClass::Other,
        header: b"",
        signer: &na,
    };
    assert!(
        ids(&evaluate_file(&double))
            .iter()
            .any(|id| id.starts_with("H002"))
    );
    let bidi = FileFacts {
        file_name: "photo\u{202E}gpj.exe",
        ..double.clone()
    };
    assert!(
        ids(&evaluate_file(&bidi))
            .iter()
            .any(|id| id.starts_with("H003"))
    );
    let mz = FileFacts {
        file_name: "notes.txt",
        header: b"MZ\x90\x00",
        ..double.clone()
    };
    assert!(
        ids(&evaluate_file(&mz))
            .iter()
            .any(|id| id.starts_with("H004"))
    );
    let plain = FileFacts {
        file_name: "notes.txt",
        header: b"hello",
        ..double
    };
    assert!(evaluate_file(&plain).is_empty());
}

#[test]
fn unsigned_risky_executable_and_unavailable_signer() {
    let unsigned = SignerStatus::Unsigned;
    let facts = FileFacts {
        file_name: "tool.exe",
        location: LocationClass::Downloads,
        header: b"MZ",
        signer: &unsigned,
    };
    assert!(
        ids(&evaluate_file(&facts))
            .iter()
            .any(|id| id.starts_with("H005"))
    );
    let unavailable = SignerStatus::Unavailable;
    let facts = FileFacts {
        signer: &unavailable,
        ..facts
    };
    assert!(evaluate_file(&facts).contains(&Evidence::Unavailable {
        reason: UnavailableReason::SignerNotCheckable
    }));
    let valid = SignerStatus::Valid {
        subject: "Vendor".into(),
        microsoft: false,
    };
    let facts = FileFacts {
        signer: &valid,
        ..facts
    };
    assert!(evaluate_file(&facts).is_empty());
}

#[test]
fn process_heuristics() {
    let unsigned = SignerStatus::Unsigned;
    let deleted = ProcessFacts {
        image_name: "app.exe",
        image_location: LocationClass::Other,
        image_deleted: true,
        signer: &unsigned,
    };
    assert!(
        ids(&evaluate_process(&deleted))
            .iter()
            .any(|id| id.starts_with("H006"))
    );
    let roaming = ProcessFacts {
        image_deleted: false,
        image_location: LocationClass::RoamingAppData,
        ..deleted.clone()
    };
    assert!(
        ids(&evaluate_process(&roaming))
            .iter()
            .any(|id| id.starts_with("H006"))
    );
    let fake = ProcessFacts {
        image_name: "lsass.exe",
        image_location: LocationClass::Temp,
        ..roaming.clone()
    };
    assert!(
        ids(&evaluate_process(&fake))
            .iter()
            .any(|id| id.starts_with("H001"))
    );
    let installed = ProcessFacts {
        image_location: LocationClass::ProgramFiles,
        ..roaming
    };
    assert!(evaluate_process(&installed).is_empty());
}

#[test]
fn evidence_serializes_with_kind_tags() {
    let json = serde_json::to_string(&Evidence::Unavailable {
        reason: UnavailableReason::InUse,
    })
    .unwrap();
    assert_eq!(json, r#"{"kind":"unavailable","reason":"inUse"}"#);
    let json = serde_json::to_value(Evidence::External {
        provider: "AMSI".into(),
        observed_at: "t".into(),
        detail: "d".into(),
    })
    .unwrap();
    assert_eq!(json["kind"], "external");
    assert_eq!(json["observedAt"], "t");
}
