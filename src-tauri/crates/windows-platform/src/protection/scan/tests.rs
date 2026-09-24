//! Files on disk use a harmless marker from the test pack instead of EICAR, so
//! the installed antivirus does not remove fixtures mid-test. EICAR matching
//! is covered in memory by the rules-store and protection-core tests.

use super::*;
use crate::protection::test_support::*;
use protection_core::{MatchMethod, VerifiedPack};

fn pack() -> CompiledPack {
    let (json, sig) = signed_pack(2, "scan");
    let verified: VerifiedPack = test_verifier().verify(&json, &sig, "0.1.0").unwrap();
    CompiledPack::compile(&verified)
}

fn scanner<'a>(
    pack: &'a CompiledPack,
    signers: &'a SignerCache,
    known: &'a KnownLocations,
    allowlist: &'a HashSet<String>,
    limits: ScanLimits,
) -> Scanner<'a> {
    Scanner {
        pack,
        signers,
        known,
        allowlist,
        limits,
        control: Arc::new(ScanControl::default()),
    }
}

fn kinds(outcome: &ScanOutcome) -> Vec<(String, &'static str)> {
    outcome
        .findings
        .iter()
        .map(|f| {
            let name = f.path.file_name().unwrap().to_string_lossy().into_owned();
            let kind = match f.evidence {
                Evidence::Deterministic { .. } => "deterministic",
                Evidence::Heuristic { .. } => "heuristic",
                Evidence::Unavailable { .. } => "unavailable",
                Evidence::External { .. } => "external",
            };
            (name, kind)
        })
        .collect()
}

#[test]
fn detects_marker_deterministically_and_heuristics_separately() {
    let dir = temp_dir("scan-basic");
    fs::write(dir.join("marked.bin"), b"....SDK-TEST-MARKER....").unwrap();
    fs::write(dir.join("plain.txt"), b"nothing to see").unwrap();
    fs::write(dir.join("invoice.pdf.exe"), b"not a real program").unwrap();
    fs::create_dir(dir.join("nested")).unwrap();
    fs::write(dir.join("nested").join("image.png"), b"MZ\x90\x00").unwrap();
    let (pack, signers, known, allow) = (
        pack(),
        SignerCache::default(),
        KnownLocations::default(),
        HashSet::new(),
    );
    let outcome = scanner(&pack, &signers, &known, &allow, ScanLimits::FOLDER)
        .run(&[ScanTarget { path: dir.clone() }]);
    let found = kinds(&outcome);
    assert!(found.contains(&("marked.bin".into(), "deterministic")));
    assert!(found.contains(&("invoice.pdf.exe".into(), "heuristic")));
    assert!(found.contains(&("image.png".into(), "heuristic")));
    assert!(!found.iter().any(|(name, _)| name == "plain.txt"));
    assert_eq!(outcome.summary.files_scanned, 4);
    assert_eq!(outcome.summary.deterministic, 1);
    let marked = outcome
        .findings
        .iter()
        .find(|f| f.evidence.is_deterministic())
        .unwrap();
    assert!(matches!(
        marked.evidence,
        Evidence::Deterministic {
            method: MatchMethod::Bytes,
            pack_sequence: 2,
            ..
        }
    ));
    assert_eq!(marked.sha256.as_deref().map(str::len), Some(64));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn junctions_are_never_followed() {
    let dir = temp_dir("scan-junction");
    let outside = temp_dir("scan-junction-outside");
    fs::write(outside.join("marked.bin"), b"SDK-TEST-MARKER").unwrap();
    junction::create(&outside, dir.join("link")).unwrap();
    let (pack, signers, known, allow) = (
        pack(),
        SignerCache::default(),
        KnownLocations::default(),
        HashSet::new(),
    );
    let outcome = scanner(&pack, &signers, &known, &allow, ScanLimits::FOLDER)
        .run(&[ScanTarget { path: dir.clone() }]);
    assert_eq!(outcome.summary.files_scanned, 0);
    assert_eq!(outcome.summary.reparse_points_skipped, 1);
    assert!(outcome.findings.is_empty());
    let _ = fs::remove_dir_all(dir);
    let _ = fs::remove_dir_all(outside);
}

#[test]
fn oversized_locked_and_cancelled_files_are_unavailable_not_clean() {
    let dir = temp_dir("scan-unavailable");
    fs::write(dir.join("big.bin"), vec![b'x'; 4096]).unwrap();
    let locked_path = dir.join("locked.bin");
    fs::write(&locked_path, b"SDK-TEST-MARKER").unwrap();
    let _lock = OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&locked_path)
        .unwrap();
    let limits = ScanLimits {
        max_file_bytes: 1024,
        ..ScanLimits::FOLDER
    };
    let (pack, signers, known, allow) = (
        pack(),
        SignerCache::default(),
        KnownLocations::default(),
        HashSet::new(),
    );
    let outcome =
        scanner(&pack, &signers, &known, &allow, limits).run(&[ScanTarget { path: dir.clone() }]);
    let reasons: Vec<_> = outcome
        .findings
        .iter()
        .filter_map(|f| match f.evidence {
            Evidence::Unavailable { reason } => Some((
                f.path.file_name().unwrap().to_string_lossy().into_owned(),
                reason,
            )),
            _ => None,
        })
        .collect();
    assert!(reasons.contains(&("big.bin".into(), UnavailableReason::TooLarge)));
    assert!(reasons.contains(&("locked.bin".into(), UnavailableReason::InUse)));
    assert_eq!(outcome.summary.deterministic, 0);

    let cancelled = scanner(&pack, &signers, &known, &allow, ScanLimits::FOLDER);
    cancelled.control.cancelled.store(true, Ordering::Relaxed);
    let outcome = cancelled.run(&[ScanTarget { path: dir.clone() }]);
    assert!(outcome.summary.cancelled);
    drop(_lock);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn allowlist_suppresses_heuristics_but_never_deterministic_matches() {
    let dir = temp_dir("scan-allowlist");
    let body = b"MZ SDK-TEST-MARKER";
    fs::write(dir.join("both.png"), body).unwrap();
    let (pack, signers, known) = (pack(), SignerCache::default(), KnownLocations::default());
    let first = scanner(&pack, &signers, &known, &HashSet::new(), ScanLimits::FOLDER)
        .run(&[ScanTarget { path: dir.clone() }]);
    assert_eq!(
        (first.summary.deterministic, first.summary.heuristic),
        (1, 1)
    );
    let digest = first.findings[0].sha256.clone().unwrap();
    let allow = HashSet::from([digest]);
    let second = scanner(&pack, &signers, &known, &allow, ScanLimits::FOLDER)
        .run(&[ScanTarget { path: dir.clone() }]);
    assert_eq!(
        (
            second.summary.deterministic,
            second.summary.heuristic,
            second.summary.allowlisted
        ),
        (1, 0, 1)
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn file_and_depth_limits_truncate_honestly() {
    let dir = temp_dir("scan-limits");
    for index in 0..5 {
        fs::write(dir.join(format!("{index}.txt")), b"x").unwrap();
    }
    let limits = ScanLimits {
        max_files: 2,
        ..ScanLimits::FOLDER
    };
    let (pack, signers, known, allow) = (
        pack(),
        SignerCache::default(),
        KnownLocations::default(),
        HashSet::new(),
    );
    let outcome =
        scanner(&pack, &signers, &known, &allow, limits).run(&[ScanTarget { path: dir.clone() }]);
    assert!(outcome.summary.truncated);
    assert_eq!(outcome.summary.files_scanned, 2);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn quick_targets_include_existing_folders_and_unique_images() {
    let dir = temp_dir("scan-quick");
    let known = KnownLocations {
        temp: vec![dir.clone(), dir.join("missing")],
        ..KnownLocations::default()
    };
    let exe = std::env::current_exe().unwrap();
    let targets = quick_targets(&known, [exe.clone(), exe.clone()]);
    assert_eq!(targets.len(), 2);
    let _ = fs::remove_dir_all(dir);
}
