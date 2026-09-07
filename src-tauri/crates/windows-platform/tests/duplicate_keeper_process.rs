//! External PowerShell driver is the only process owner. This executable never
//! launches processes and accepts only the driver's marked disposable temp root.
#![cfg(windows)]
use cleanup_core::{
    CancellationToken, FileSystem,
    storage::{ObservedEntry, RootAuthorization},
};
use std::{
    fs,
    path::Path,
    time::{Duration, Instant, UNIX_EPOCH},
};
use windows_platform::{
    WindowsFileSystem,
    storage::duplicates::{hash, verify},
};
fn wait(path: &Path) {
    let end = Instant::now() + Duration::from_secs(30);
    while !path.exists() {
        assert!(Instant::now() < end, "driver timeout");
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
#[ignore = "requires scripts/test-duplicate-keeper.ps1 external race driver"]
fn keeper_is_pinned_through_entire_group() {
    let supplied = std::path::PathBuf::from(
        std::env::var_os("DUPLICATE_KEEPER_FIXTURE").expect("external fixture driver"),
    );
    let temp = fs::canonicalize(std::env::temp_dir()).unwrap();
    let fixture = fs::canonicalize(supplied).unwrap();
    assert_eq!(fixture.parent(), Some(temp.as_path()));
    assert!(
        fixture
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("duplicate-keeper-fixture-")
    );
    assert_eq!(
        fs::read(fixture.join("fixture-marker")).unwrap(),
        b"disposable-duplicate-test-v1"
    );
    let directory = fixture.join("group");
    fs::create_dir(&directory).unwrap();
    for name in ["keeper", "one", "two"] {
        fs::write(directory.join(name), vec![42; 200_000]).unwrap();
    }
    let root = RootAuthorization {
        snapshot_id: "a".repeat(32),
        root_id: "b".repeat(32),
        canonical_path: directory.clone(),
        identity: WindowsFileSystem
            .metadata_no_follow(&directory)
            .unwrap()
            .identity
            .unwrap(),
    };
    let entry = |name: &str| {
        let path = directory.join(name);
        let m = WindowsFileSystem.metadata_no_follow(&path).unwrap();
        ObservedEntry {
            canonical_path: path,
            identity: m.identity.unwrap(),
            kind: m.kind,
            logical_bytes: m.size,
            allocated_bytes: Some(m.size),
            modified_unix_nanos: m
                .modified
                .unwrap()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64,
        }
    };
    let keeper = WindowsFileSystem
        .guard_entry(&root, &entry("keeper"), true)
        .unwrap();
    let one = WindowsFileSystem
        .guard_duplicate_member(&root, &entry("one"))
        .unwrap();
    let two = WindowsFileSystem
        .guard_duplicate_member(&root, &entry("two"))
        .unwrap();
    let cancel = CancellationToken::default();
    let digest = hash(&keeper, u64::MAX, &cancel).unwrap();
    verify(&keeper, &one, &digest, &cancel).unwrap();
    verify(&keeper, &two, &digest, &cancel).unwrap();
    fs::write(fixture.join("ready"), b"ready").unwrap();
    wait(&fixture.join("probe-one"));
    one.remove().unwrap();
    fs::write(fixture.join("between"), b"between").unwrap();
    wait(&fixture.join("probe-two"));
    two.remove().unwrap();
    fs::write(fixture.join("completed-held"), b"held").unwrap();
    wait(&fixture.join("probe-three"));
    assert_eq!(hash(&keeper, u64::MAX, &cancel).unwrap(), digest);
    drop(keeper);
    fs::write(fixture.join("released"), b"released").unwrap();
    wait(&fixture.join("probe-released"));
}
