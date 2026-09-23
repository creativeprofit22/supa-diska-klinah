use std::sync::Mutex;

use cleanup_core::system_change::ContractError;

use super::*;
use crate::security::system_changes::HelperChange;

struct MemoryHosts(Mutex<Result<Vec<u8>, HostsError>>);

impl MemoryHosts {
    fn new(bytes: &[u8]) -> Self {
        Self(Mutex::new(Ok(bytes.to_vec())))
    }
    fn failing(error: HostsError) -> Self {
        Self(Mutex::new(Err(error)))
    }
}

impl HostsReader for MemoryHosts {
    fn read(&self) -> Result<Vec<u8>, HostsError> {
        self.0.lock().unwrap().clone()
    }
}

#[derive(Default)]
struct RecordingWriter(Mutex<Vec<(String, Vec<u8>)>>);

impl HostsWriter for RecordingWriter {
    fn replace(&self, expected: &Sha256Digest, contents: &[u8]) -> Result<(), HostsError> {
        self.0
            .lock()
            .unwrap()
            .push((expected.as_str().to_owned(), contents.to_vec()));
        Ok(())
    }
}

const LF: &[u8] =
    b"# Copyright\n\n127.0.0.1 localhost\n10.0.0.5 intranet.local # dev box\nnot-an-ip foo\n";
const CRLF: &[u8] =
    b"# Copyright\r\n\r\n127.0.0.1 localhost\r\n10.0.0.5 intranet.local # dev box\r\nnot-an-ip foo\r\n";
const NO_FINAL: &[u8] =
    b"# Copyright\n\n127.0.0.1 localhost\n10.0.0.5 intranet.local # dev box\nnot-an-ip foo";

fn kinds(bytes: &[u8]) -> Vec<HostsLineKind> {
    let document = HostsDocument::parse(bytes);
    (0..document.line_count())
        .map(|index| document.classify(index).unwrap().kind())
        .collect()
}

fn op(line: u32, action: HostsLineAction) -> HostsLineOp {
    HostsLineOp { line, action }
}

fn edit_change(ops: Vec<HostsLineOp>) -> SystemChange {
    SystemChange::EditHosts { line_ops: ops }
}

fn temp_root() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "sdk-hosts-{}",
        crate::system_change::random_id().unwrap()
    ));
    fs::create_dir_all(root.join("etc")).unwrap();
    root
}

#[test]
fn parses_lf_crlf_and_missing_final_newline_identically() {
    use HostsLineKind::*;
    let expected = vec![Comment, Blank, Mapping, Mapping, Invalid];
    for fixture in [LF, CRLF, NO_FINAL] {
        assert_eq!(kinds(fixture), expected);
    }
    let document = HostsDocument::parse(CRLF);
    assert_eq!(
        document.classify(3),
        Some(HostsLine::Mapping {
            ip: "10.0.0.5".parse().unwrap(),
            hostnames: vec!["intranet.local".into()],
        })
    );
    assert_eq!(kinds(b""), Vec::<HostsLineKind>::new());
    assert_eq!(kinds(b"\n"), vec![Blank]);
    assert_eq!(
        kinds(b"#sdk-disabled# 1.2.3.4 a.com\r\n#sdk-disabled# junk\n  ::1 localhost\n"),
        vec![AppDisabled, Comment, Mapping]
    );
    assert_eq!(kinds(b"\xEF\xBB\xBF127.0.0.1 x.test\n"), vec![Mapping]);
    assert_eq!(
        kinds(b"1.2.3.4\n1.2.3.4 bad/host\n"),
        vec![Invalid, Invalid]
    );
}

#[test]
fn round_trips_byte_exact() {
    for fixture in [
        LF,
        CRLF,
        NO_FINAL,
        b"" as &[u8],
        b"\n",
        b"\r\n\r\n",
        b"\xEF\xBB\xBF# bom\r\n\xff\xfe odd \x80 bytes\n\n\n",
        b"mixed\r\nendings\nlast\r",
    ] {
        assert_eq!(HostsDocument::parse(fixture).to_bytes(), fixture);
    }
}

#[test]
fn disable_then_restore_returns_original_bytes() {
    let original: &[u8] =
        b"\xEF\xBB\xBF127.0.0.1 a.test\r\n# c\r\n  10.1.1.1   b.test\tc.test  # note\r\n\xe9 comment\r\n0.0.0.0 d.test";
    let ops = vec![
        op(0, HostsLineAction::Disable),
        op(2, HostsLineAction::Disable),
        op(4, HostsLineAction::Disable),
    ];
    let disabled = edit(original, &ops).unwrap();
    assert_eq!(
        disabled,
        b"\xEF\xBB\xBF#sdk-disabled# 127.0.0.1 a.test\r\n# c\r\n#sdk-disabled#   10.1.1.1   b.test\tc.test  # note\r\n\xe9 comment\r\n#sdk-disabled# 0.0.0.0 d.test"
    );
    assert_eq!(
        kinds(&disabled)[..3],
        [
            HostsLineKind::AppDisabled,
            HostsLineKind::Comment,
            HostsLineKind::AppDisabled
        ]
    );
    let inverse = edit_change(ops)
        .inverse(&PriorState::Hosts {
            sha256: sha256_digest(original).unwrap(),
        })
        .unwrap()
        .unwrap();
    let SystemChange::EditHosts { line_ops: restore } = inverse else {
        panic!("wrong inverse");
    };
    assert!(
        restore
            .iter()
            .all(|op| op.action == HostsLineAction::Restore)
    );
    assert_eq!(edit(&disabled, &restore).unwrap(), original);
}

#[test]
fn findings_flag_redirects_blocking_custom_and_invalid() {
    let bytes = b"127.0.0.1 localhost\n\
        6.6.6.6 www.google.com notgoogle.com\n\
        0.0.0.0 download.windowsupdate.com ads.example\n\
        ::1 kaspersky.com\n\
        127.0.0.1 google.com\n\
        #sdk-disabled# 6.6.6.6 paypal.com\n\
        # 6.6.6.6 paypal.com\n\
        garbage\n";
    let found: Vec<_> = findings(&HostsDocument::parse(bytes))
        .into_iter()
        .map(|finding| (finding.line, finding.severity, finding.kind))
        .collect();
    assert_eq!(
        found,
        vec![
            (1, FindingSeverity::High, FindingKind::Redirect),
            (1, FindingSeverity::Low, FindingKind::CustomMapping),
            (2, FindingSeverity::High, FindingKind::SecurityBlocked),
            (3, FindingSeverity::High, FindingKind::SecurityBlocked),
            (7, FindingSeverity::Low, FindingKind::Invalid),
        ]
    );
    assert!(matches_domain(
        "login.microsoftonline.com",
        SENSITIVE_DOMAINS
    ));
    assert!(matches_domain("a.b.eset.com.", SENSITIVE_DOMAINS));
    assert!(!matches_domain("reset.com", SENSITIVE_DOMAINS));
}

#[test]
fn report_is_bounded_and_serializes_camel_case() {
    let mut bytes = Vec::new();
    for index in 0..(MAX_REPORT_LINES + 5) {
        bytes.extend_from_slice(format!("# {index}\r\n").as_bytes());
    }
    bytes.extend_from_slice(format!("# {}\n", "x".repeat(2000)).as_bytes());
    let report = build_report("C:\\hosts".into(), &bytes);
    assert_eq!(report.lines.len(), MAX_REPORT_LINES);
    assert!(report.lines_truncated);
    assert_eq!(report.total_lines as usize, MAX_REPORT_LINES + 6);
    assert_eq!(report.lines[1].text, "# 1");
    assert_eq!(report.sha256, sha256_hex(&bytes));
    let json = serde_json::to_value(&report).unwrap();
    assert!(json.get("sizeBytes").is_some());
    assert_eq!(json["lines"][0]["kind"], "comment");

    let long = build_report(String::new(), format!("# {}", "y".repeat(2000)).as_bytes());
    assert_eq!(long.lines[0].text.chars().count(), MAX_TEXT_CHARS);
}

#[test]
fn adapter_observes_describes_and_detects_satisfaction() {
    let adapter = HostsAdapter::with_reader(MemoryHosts::new(CRLF));
    let disable = edit_change(vec![op(3, HostsLineAction::Disable)]);
    let prior = adapter.observe(&disable).unwrap();
    assert_eq!(
        prior,
        PriorState::Hosts {
            sha256: sha256_digest(CRLF).unwrap()
        }
    );
    let impact = adapter.describe(&disable).unwrap();
    assert!(impact.effect.contains("comment out 1 hosts mapping"));
    assert_eq!(
        disable.privilege(),
        cleanup_core::system_change::Privilege::Helper
    );
    assert!(disable.is_satisfied_by(&prior).is_none());
    assert!(!adapter.is_satisfied(&disable, &prior));

    let edited = edit(CRLF, &[op(3, HostsLineAction::Disable)]).unwrap();
    let after = HostsAdapter::with_reader(MemoryHosts::new(&edited));
    let now = after.observe(&disable).unwrap();
    assert!(after.is_satisfied(&disable, &now));
    // A stale hash never counts as satisfied.
    assert!(!after.is_satisfied(&disable, &prior));
}

#[test]
fn invalid_ops_fail_closed_and_duplicates_are_rejected() {
    let adapter = HostsAdapter::with_reader(MemoryHosts::new(LF));
    let not_present = AdapterError::Unsupported(UnsupportedReason::NotPresent);
    for bad in [
        op(0, HostsLineAction::Disable),  // comment
        op(1, HostsLineAction::Disable),  // blank
        op(4, HostsLineAction::Disable),  // invalid
        op(5, HostsLineAction::Disable),  // out of range
        op(0, HostsLineAction::Restore),  // comment, not ours
        op(99, HostsLineAction::Restore), // out of range
    ] {
        let change = edit_change(vec![bad]);
        assert_eq!(adapter.observe(&change), Err(not_present));
        assert_eq!(adapter.describe(&change).map(|_| ()), Err(not_present));
        assert_eq!(edit(LF, &[bad]), Err(not_present));
    }
    let duplicate = edit_change(vec![
        op(2, HostsLineAction::Disable),
        op(2, HostsLineAction::Restore),
    ]);
    assert_eq!(duplicate.validate(), Err(ContractError::DuplicateLine));
    assert_eq!(adapter.observe(&duplicate), Err(AdapterError::Failed));
    assert_eq!(
        adapter.observe(&SystemChange::SetHibernation { enabled: false }),
        Err(AdapterError::Failed)
    );
}

#[test]
fn reader_failures_map_to_adapter_errors() {
    let change = edit_change(vec![op(0, HostsLineAction::Disable)]);
    for (error, expected) in [
        (HostsError::Denied, AdapterError::Denied),
        (
            HostsError::SystemDirectory,
            AdapterError::Unsupported(UnsupportedReason::ApiUnavailable),
        ),
        (
            HostsError::NotFound,
            AdapterError::Unsupported(UnsupportedReason::NotPresent),
        ),
        (HostsError::TooLarge, AdapterError::Failed),
    ] {
        let adapter = HostsAdapter::with_reader(MemoryHosts::failing(error));
        assert_eq!(adapter.observe(&change), Err(expected));
    }
    assert_eq!(
        io_error(io::Error::from_raw_os_error(ERROR_ACCESS_DENIED as i32)),
        HostsError::Denied
    );
}

#[test]
fn apply_ops_writes_only_pending_edits_with_the_read_hash() {
    let reader = MemoryHosts::new(LF);
    let writer = RecordingWriter::default();
    apply_ops(&reader, &writer, &[op(2, HostsLineAction::Disable)]).unwrap();
    let writes = writer.0.lock().unwrap().clone();
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].0, sha256_hex(LF));
    assert!(
        writes[0]
            .1
            .starts_with(b"# Copyright\n\n#sdk-disabled# 127.0.0.1")
    );

    // Already in the target state: no write.
    let done = MemoryHosts::new(&writes[0].1);
    let writer = RecordingWriter::default();
    apply_ops(&done, &writer, &[op(2, HostsLineAction::Disable)]).unwrap();
    assert!(writer.0.lock().unwrap().is_empty());
}

#[test]
fn writer_backs_up_prunes_and_replaces_atomically() {
    let root = temp_root();
    let hosts = root.join("etc").join("hosts");
    let backups = root.join("backups");
    fs::create_dir_all(&backups).unwrap();
    for secs in 1..=25u64 {
        fs::write(backups.join(backup_name(secs, "0123abcd")), b"old").unwrap();
    }
    fs::write(backups.join("keep-me.txt"), b"unrelated").unwrap();
    fs::write(&hosts, CRLF).unwrap();
    let store = HostsStore::at(hosts.clone(), backups.clone());

    apply_ops(&store, &store, &[op(3, HostsLineAction::Disable)]).unwrap();
    let written = fs::read(&hosts).unwrap();
    assert_eq!(
        written,
        edit(CRLF, &[op(3, HostsLineAction::Disable)]).unwrap()
    );

    let mut names: Vec<String> = fs::read_dir(&backups)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert!(names.contains(&"keep-me.txt".to_owned()));
    let kept: Vec<u64> = names.iter().filter_map(|n| parse_backup_name(n)).collect();
    assert_eq!(kept.len(), KEEP_BACKUPS);
    assert!(!kept.contains(&6));
    assert!(kept.contains(&7));
    let fresh = names
        .iter()
        .find(|name| name.ends_with(&format!("-{}.bak", &sha256_hex(CRLF)[..8])))
        .unwrap();
    assert_eq!(fs::read(backups.join(fresh)).unwrap(), CRLF);
    let leftovers = fs::read_dir(root.join("etc"))
        .unwrap()
        .filter(|entry| entry.as_ref().unwrap().path() != hosts)
        .count();
    assert_eq!(leftovers, 0);

    // Restore goes back to the original bytes.
    apply_ops(&store, &store, &[op(3, HostsLineAction::Restore)]).unwrap();
    assert_eq!(fs::read(&hosts).unwrap(), CRLF);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn hash_mismatch_refuses_to_write() {
    let root = temp_root();
    let hosts = root.join("etc").join("hosts");
    let backups = root.join("backups");
    fs::write(&hosts, LF).unwrap();
    let store = HostsStore::at(hosts.clone(), backups.clone());
    let stale = sha256_digest(b"something else").unwrap();
    assert_eq!(store.replace(&stale, b"new"), Err(HostsError::StateChanged));
    assert_eq!(fs::read(&hosts).unwrap(), LF);
    assert!(!backups.exists());
    assert_eq!(fs::read_dir(root.join("etc")).unwrap().count(), 1);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn oversized_file_is_refused() {
    let root = temp_root();
    let hosts = root.join("etc").join("hosts");
    fs::write(&hosts, vec![b'#'; MAX_HOSTS_BYTES + 1]).unwrap();
    let store = HostsStore::at(hosts, root.join("backups"));
    assert_eq!(store.read(), Err(HostsError::TooLarge));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn elevated_rejects_other_variants() {
    let other = HelperChange::SetHibernation { enabled: false };
    assert_eq!(elevated::observe(&other), Err(AdapterError::Failed));
    assert_eq!(elevated::apply(&other), Err(AdapterError::Failed));
}

#[test]
fn live_read_only_report_on_real_hosts_file() {
    let report = hosts_report().unwrap();
    assert!(
        report
            .path
            .to_ascii_lowercase()
            .ends_with("\\drivers\\etc\\hosts")
    );
    assert_eq!(report.sha256.len(), 64);
    assert!(report.lines.len() <= MAX_REPORT_LINES);
    // Observing through the elevated entry point is read-only as well.
    let adapter = HostsAdapter::new();
    let change = edit_change(vec![op(u32::MAX, HostsLineAction::Disable)]);
    assert_eq!(
        adapter.observe(&change),
        Err(AdapterError::Unsupported(UnsupportedReason::NotPresent))
    );
}
