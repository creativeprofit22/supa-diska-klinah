use super::*;
use crate::protection::test_support::temp_dir;

const BODY: &[u8] = b"MZ pretend executable SDK-TEST-MARKER";

fn not_protected(_: &Path) -> bool {
    false
}

fn digest(bytes: &[u8]) -> String {
    to_hex(&Sha256::digest(bytes))
}

struct Fixture {
    base: PathBuf,
    scan_root: PathBuf,
    quarantine: Quarantine,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let base = fs::canonicalize(temp_dir(label)).unwrap();
        let scan_root = base.join("scanned");
        fs::create_dir(&scan_root).unwrap();
        let quarantine = Quarantine::open(base.join("quarantine")).unwrap();
        Self {
            base,
            scan_root,
            quarantine,
        }
    }

    fn file(&self, name: &str, body: &[u8]) -> PathBuf {
        let path = self.scan_root.join(name);
        fs::write(&path, body).unwrap();
        path
    }

    fn quarantine(&self, path: &Path) -> Result<QuarantineEntry, QuarantineError> {
        self.quarantine.quarantine(
            &QuarantineRequest {
                source: path,
                scan_root: &self.scan_root,
                expected_sha256: Some(&digest(BODY)),
                finding: "test finding",
            },
            &not_protected,
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

#[test]
fn quarantine_neuters_payload_and_restore_round_trips() {
    let fx = Fixture::new("q-roundtrip");
    let source = fx.file("tool.exe", BODY);
    let entry = fx.quarantine(&source).unwrap();
    assert!(!source.exists(), "source removed");
    let payload = fs::read(fx.quarantine.root.join(&entry.id).join("payload.bin")).unwrap();
    assert!(payload.starts_with(MAGIC));
    assert!(
        !payload.windows(15).any(|w| w == b"SDK-TEST-MARKER"),
        "payload is neutered"
    );
    assert_eq!(fx.quarantine.list().len(), 1);

    let restored = fx.quarantine.restore(&entry.id, &not_protected).unwrap();
    assert_eq!(Path::new(&restored), source);
    assert_eq!(fs::read(&source).unwrap(), BODY);
    assert!(fx.quarantine.list().is_empty());
}

#[test]
fn restore_after_restart_uses_only_native_paths() {
    let fx = Fixture::new("q-restart");
    let source = fx.file("tool.exe", BODY);
    let entry = fx.quarantine(&source).unwrap();
    let reopened = Quarantine::open(fx.base.join("quarantine")).unwrap();
    assert_eq!(reopened.list()[0].id, entry.id);
    reopened.restore(&entry.id, &not_protected).unwrap();
    assert_eq!(fs::read(&source).unwrap(), BODY);
}

#[test]
fn restore_collision_never_overwrites() {
    let fx = Fixture::new("q-collision");
    let source = fx.file("tool.exe", BODY);
    let entry = fx.quarantine(&source).unwrap();
    fs::write(&source, b"the user's new file").unwrap();
    assert_eq!(
        fx.quarantine.restore(&entry.id, &not_protected),
        Err(QuarantineError::Collision)
    );
    assert_eq!(fs::read(&source).unwrap(), b"the user's new file");
    assert_eq!(
        fx.quarantine.list().len(),
        1,
        "entry kept for a later retry"
    );
}

#[test]
fn tampered_payload_is_detected_before_anything_is_written() {
    let fx = Fixture::new("q-tamper");
    let source = fx.file("tool.exe", BODY);
    let entry = fx.quarantine(&source).unwrap();
    let payload = fx.quarantine.root.join(&entry.id).join("payload.bin");
    let mut bytes = fs::read(&payload).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    fs::write(&payload, bytes).unwrap();
    assert_eq!(
        fx.quarantine.restore(&entry.id, &not_protected),
        Err(QuarantineError::HashMismatch)
    );
    assert!(!source.exists());
}

#[test]
fn ids_cannot_traverse() {
    let fx = Fixture::new("q-ids");
    for id in [
        "..",
        "../../etc",
        "ABCDEF0123456789ABCDEF0123456789",
        "a".repeat(31).as_str(),
        "C:\\Windows",
        "",
    ] {
        assert_eq!(
            fx.quarantine.restore(id, &not_protected),
            Err(QuarantineError::InvalidId),
            "{id}"
        );
        assert_eq!(
            fx.quarantine.delete(id),
            Err(QuarantineError::InvalidId),
            "{id}"
        );
    }
    assert_eq!(
        fx.quarantine.delete(&"a".repeat(32)),
        Err(QuarantineError::NotFound)
    );
}

#[test]
fn record_paths_are_validated_and_never_used_for_storage() {
    let fx = Fixture::new("q-record");
    let source = fx.file("tool.exe", BODY);
    let entry = fx.quarantine(&source).unwrap();
    let record_path = fx.quarantine.root.join(&entry.id).join("record.json");
    let mut record: serde_json::Value =
        serde_json::from_slice(&fs::read(&record_path).unwrap()).unwrap();
    record["originalPath"] = "..\\..\\evil.exe".into();
    fs::write(&record_path, serde_json::to_vec(&record).unwrap()).unwrap();
    assert_eq!(
        fx.quarantine.restore(&entry.id, &not_protected),
        Err(QuarantineError::Damaged)
    );
    assert!(fx.quarantine.list()[0].damaged);
    record["originalPath"] = source.to_string_lossy().into_owned().into();
    record["id"] = "b".repeat(32).into();
    fs::write(&record_path, serde_json::to_vec(&record).unwrap()).unwrap();
    assert_eq!(
        fx.quarantine.restore(&entry.id, &not_protected),
        Err(QuarantineError::Damaged)
    );
    // Delete still works on a damaged entry, and only inside the root.
    fx.quarantine.delete(&entry.id).unwrap();
    assert!(fx.quarantine.list().is_empty());
}

#[test]
fn junction_inside_quarantine_root_is_refused_and_target_untouched() {
    let fx = Fixture::new("q-junction");
    let outside = fx.base.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("keep.txt"), b"keep").unwrap();
    let id = "c".repeat(32);
    junction::create(&outside, fx.quarantine.root.join(&id)).unwrap();
    assert_eq!(
        fx.quarantine.delete(&id),
        Err(QuarantineError::ReparsePoint)
    );
    assert_eq!(
        fx.quarantine.restore(&id, &not_protected),
        Err(QuarantineError::ReparsePoint)
    );
    fx.quarantine.reconcile();
    assert!(outside.join("keep.txt").exists());
    assert!(
        !fx.quarantine.root.join(&id).exists(),
        "link removed, target kept"
    );
}

#[test]
fn reparse_protected_outside_and_changed_sources_are_refused() {
    let fx = Fixture::new("q-refuse");
    let real = fx.base.join("real");
    fs::create_dir(&real).unwrap();
    fs::write(real.join("tool.exe"), BODY).unwrap();
    junction::create(&real, fx.scan_root.join("link")).unwrap();
    assert_eq!(
        fx.quarantine(&fx.scan_root.join("link").join("tool.exe"))
            .unwrap_err(),
        QuarantineError::ReparsePoint
    );
    assert!(real.join("tool.exe").exists());

    let outside = fx.base.join("elsewhere.exe");
    fs::write(&outside, BODY).unwrap();
    assert_eq!(
        fx.quarantine(&outside).unwrap_err(),
        QuarantineError::OutsideRoot
    );

    let protected = fx.file("protected.exe", BODY);
    let result = fx.quarantine.quarantine(
        &QuarantineRequest {
            source: &protected,
            scan_root: &fx.scan_root,
            expected_sha256: None,
            finding: "x",
        },
        &|_| true,
    );
    assert_eq!(result.unwrap_err(), QuarantineError::Protected);

    let changed = fx.file("changed.exe", b"different content");
    assert_eq!(
        fx.quarantine(&changed).unwrap_err(),
        QuarantineError::Changed
    );
    assert!(changed.exists());
    assert!(fx.quarantine.list().is_empty(), "no partial entries left");

    assert_eq!(
        fx.quarantine(Path::new("relative\\tool.exe")).unwrap_err(),
        QuarantineError::InvalidPath
    );
}

#[test]
fn in_use_source_is_refused() {
    let fx = Fixture::new("q-inuse");
    let source = fx.file("tool.exe", BODY);
    let _writer = OpenOptions::new().write(true).open(&source).unwrap();
    assert_eq!(fx.quarantine(&source).unwrap_err(), QuarantineError::InUse);
    assert!(fx.quarantine.list().is_empty());
}

#[test]
fn interrupted_quarantine_completes_on_restart_without_losing_data() {
    let mut fx = Fixture::new("q-interrupt");
    let source = fx.file("tool.exe", BODY);
    fx.quarantine.fail_before_source_delete = true;
    assert!(fx.quarantine(&source).is_err());
    assert!(source.exists(), "source intact after the simulated crash");
    fx.quarantine = Quarantine::open(fx.base.join("quarantine")).unwrap();
    assert!(!source.exists(), "confirmed quarantine finished on restart");
    let entries = fx.quarantine.list();
    assert_eq!(entries.len(), 1);
    fx.quarantine
        .restore(&entries[0].id, &not_protected)
        .unwrap();
    assert_eq!(fs::read(&source).unwrap(), BODY);
}

#[test]
fn interrupted_quarantine_with_replaced_source_keeps_the_new_file() {
    let mut fx = Fixture::new("q-interrupt-replaced");
    let source = fx.file("tool.exe", BODY);
    fx.quarantine.fail_before_source_delete = true;
    assert!(fx.quarantine(&source).is_err());
    fs::remove_file(&source).unwrap();
    fs::write(&source, b"replacement").unwrap();
    fx.quarantine = Quarantine::open(fx.base.join("quarantine")).unwrap();
    assert_eq!(fs::read(&source).unwrap(), b"replacement");
}

#[test]
fn interrupted_restore_is_reconciled_without_touching_user_files() {
    let fx = Fixture::new("q-restore-interrupt");
    let source = fx.file("tool.exe", BODY);
    let entry = fx.quarantine(&source).unwrap();
    let mut record = fx.quarantine.read_record(&entry.id).unwrap();
    record.state = EntryState::Restoring;
    fx.quarantine.write_record(&record).unwrap();
    fs::write(&source, b"partial or user file").unwrap();
    let reopened = Quarantine::open(fx.base.join("quarantine")).unwrap();
    assert_eq!(fs::read(&source).unwrap(), b"partial or user file");
    assert_eq!(reopened.list().len(), 1);

    // A restore that finished writing before the crash is recognised.
    let mut record = reopened.read_record(&entry.id).unwrap();
    record.state = EntryState::Restoring;
    reopened.write_record(&record).unwrap();
    fs::write(&source, BODY).unwrap();
    let reopened = Quarantine::open(fx.base.join("quarantine")).unwrap();
    assert!(reopened.list().is_empty());
}

#[test]
fn restore_refuses_reparse_parent_and_protected_target() {
    let fx = Fixture::new("q-restore-guard");
    let source = fx.file("tool.exe", BODY);
    let entry = fx.quarantine(&source).unwrap();
    assert_eq!(
        fx.quarantine.restore(&entry.id, &|_| true),
        Err(QuarantineError::Protected)
    );
    fs::remove_dir(&fx.scan_root).unwrap();
    let elsewhere = fx.base.join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    junction::create(&elsewhere, &fx.scan_root).unwrap();
    assert_eq!(
        fx.quarantine.restore(&entry.id, &not_protected),
        Err(QuarantineError::ReparsePoint)
    );
    assert!(!elsewhere.join("tool.exe").exists());
}
