use super::*;
use crate::protection::test_support::*;

fn open(root: &Path) -> RulesStore {
    let (pack, sig) = signed_pack(1, "baseline");
    RulesStore::open_with(root.to_path_buf(), test_verifier(), pack, sig).unwrap()
}

#[test]
fn embedded_baseline_verifies_with_the_compiled_key() {
    let root = temp_dir("rules-embedded");
    let store = RulesStore::open(root.clone()).unwrap();
    assert_eq!(store.status().source, RulesSource::EmbeddedBaseline);
    assert_eq!(store.status().sequence, 1);
    assert!(store.status().recovery_note.is_none());
    let mut matcher = store.active().matcher();
    matcher.update(&eicar());
    assert_eq!(matcher.finish().hits.len(), 2);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn install_then_restart_uses_installed_pack_and_retains_previous() {
    let root = temp_dir("rules-install");
    let mut store = open(&root);
    let (p2, s2) = signed_pack(2, "two");
    assert_eq!(store.install_unchecked(&p2, &s2).unwrap().sequence, 2);
    let (p3, s3) = signed_pack(3, "three");
    let status = store.install_unchecked(&p3, &s3).unwrap();
    assert_eq!((status.sequence, status.previous_sequence), (3, Some(2)));

    let reopened = open(&root);
    assert_eq!(reopened.status().source, RulesSource::Installed);
    assert_eq!(reopened.status().sequence, 3);
    assert_eq!(reopened.status().previous_sequence, Some(2));
    // Only current and previous are kept.
    let mut dirs: Vec<_> = fs::read_dir(root.join("packs"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    dirs.sort();
    assert_eq!(dirs, ["2", "3"]);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn rollback_is_refused_and_restore_previous_is_explicit() {
    let root = temp_dir("rules-rollback");
    let mut store = open(&root);
    let (p2, s2) = signed_pack(2, "two");
    let (p3, s3) = signed_pack(3, "three");
    store.install_unchecked(&p2, &s2).unwrap();
    store.install_unchecked(&p3, &s3).unwrap();
    assert_eq!(
        store.install_unchecked(&p2, &s2).unwrap_err(),
        RulesError::Pack(PackError::Rollback {
            candidate: 2,
            floor: 3
        })
    );
    assert_eq!(
        store.install_unchecked(&p3, &s3).unwrap_err(),
        RulesError::Pack(PackError::Rollback {
            candidate: 3,
            floor: 3
        })
    );
    let status = store.restore_previous().unwrap();
    assert_eq!((status.sequence, status.previous_sequence), (2, None));
    assert_eq!(
        store.restore_previous().unwrap_err(),
        RulesError::Pack(PackError::NoPrevious)
    );
    assert_eq!(open(&root).status().sequence, 2);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn baseline_sequence_is_the_floor() {
    let root = temp_dir("rules-floor");
    let mut store = open(&root);
    let (p1, s1) = signed_pack(1, "same as baseline");
    assert!(matches!(
        store.install_unchecked(&p1, &s1),
        Err(RulesError::Pack(PackError::Rollback { .. }))
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn malformed_or_foreign_packs_never_touch_disk() {
    let root = temp_dir("rules-malformed");
    let mut store = open(&root);
    let (p2, s2) = signed_pack(2, "two");
    let mut bad = s2.clone();
    bad[0] = if bad[0] == b'a' { b'b' } else { b'a' };
    assert!(matches!(
        store.install_unchecked(&p2, &bad),
        Err(RulesError::Pack(PackError::BadSignature))
    ));
    assert!(matches!(
        store.install_unchecked(&p2, b""),
        Err(RulesError::Pack(PackError::MalformedSignature))
    ));
    let (foreign, foreign_sig) = signed_pack_with(99, 2, "foreign");
    assert!(matches!(
        store.install_unchecked(&foreign, &foreign_sig),
        Err(RulesError::Pack(PackError::BadSignature))
    ));
    assert!(fs::read_dir(root.join("packs")).unwrap().next().is_none());
    assert!(!root.join("current.json").exists());
    assert_eq!(store.status().sequence, 1);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn interruption_at_every_step_keeps_a_working_pack() {
    for step in [
        InstallStep::Stage,
        InstallStep::Rename,
        InstallStep::Pointer,
        InstallStep::Prune,
    ] {
        let root = temp_dir("rules-interrupt");
        let mut store = open(&root);
        let (p2, s2) = signed_pack(2, "two");
        store.install_unchecked(&p2, &s2).unwrap();
        let (p3, s3) = signed_pack(3, "three");
        store.fail_next(step);
        let result = store.install_unchecked(&p3, &s3);
        let expected = if step == InstallStep::Prune { 3 } else { 2 };
        assert_eq!(result.is_ok(), step == InstallStep::Prune, "{step:?}");
        assert_eq!(
            store.status().sequence,
            expected,
            "{step:?}: in-memory state"
        );

        // Simulate a crash that left a staging directory behind.
        fs::create_dir(root.join("staging-deadbeef")).unwrap();
        fs::write(root.join("staging-deadbeef").join("pack.json"), b"partial").unwrap();

        let reopened = open(&root);
        assert_eq!(
            reopened.status().sequence,
            expected,
            "{step:?}: after restart"
        );
        assert_eq!(reopened.status().source, RulesSource::Installed);
        assert!(
            !root.join("staging-deadbeef").exists(),
            "{step:?}: staging cleaned"
        );
        drop(reopened);

        // A retry after the interruption succeeds (or is correctly refused as
        // already installed when the pointer was written).
        let mut retry = open(&root);
        let result = retry.install_unchecked(&p3, &s3);
        if expected == 2 {
            assert_eq!(result.unwrap().sequence, 3, "{step:?}: retry");
        } else {
            assert!(result.is_err());
        }
        let _ = fs::remove_dir_all(root);
    }
}

#[test]
fn corrupt_current_falls_back_to_previous_then_baseline() {
    let root = temp_dir("rules-corrupt");
    let mut store = open(&root);
    let (p2, s2) = signed_pack(2, "two");
    let (p3, s3) = signed_pack(3, "three");
    store.install_unchecked(&p2, &s2).unwrap();
    store.install_unchecked(&p3, &s3).unwrap();

    fs::write(root.join("packs/3/pack.json"), b"{\"tampered\":true}").unwrap();
    let reopened = open(&root);
    assert_eq!(reopened.status().source, RulesSource::PreviousFallback);
    assert_eq!(reopened.status().sequence, 2);
    assert!(
        reopened
            .status()
            .recovery_note
            .as_deref()
            .unwrap()
            .contains("previous pack 2")
    );

    fs::remove_file(root.join("packs/2/pack.sig")).unwrap();
    let reopened = open(&root);
    assert_eq!(reopened.status().source, RulesSource::EmbeddedBaseline);
    assert!(reopened.status().recovery_note.is_some());

    fs::write(root.join("current.json"), b"not json").unwrap();
    let reopened = open(&root);
    assert_eq!(reopened.status().source, RulesSource::EmbeddedBaseline);
    assert!(
        reopened
            .status()
            .recovery_note
            .as_deref()
            .unwrap()
            .contains("unreadable")
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_pack_renamed_to_another_sequence_is_rejected() {
    let root = temp_dir("rules-renamed");
    let mut store = open(&root);
    let (p2, s2) = signed_pack(2, "two");
    store.install_unchecked(&p2, &s2).unwrap();
    fs::rename(root.join("packs/2"), root.join("packs/7")).unwrap();
    fs::write(
        root.join("current.json"),
        br#"{"current":7,"previous":null}"#,
    )
    .unwrap();
    assert_eq!(open(&root).status().source, RulesSource::EmbeddedBaseline);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_junction_planted_as_a_pack_directory_is_not_followed() {
    let root = temp_dir("rules-junction");
    let outside = temp_dir("rules-junction-target");
    let (p2, s2) = signed_pack(2, "two");
    fs::write(outside.join("pack.json"), &p2).unwrap();
    fs::write(outside.join("pack.sig"), &s2).unwrap();
    let store = open(&root);
    drop(store);
    junction::create(&outside, root.join("packs").join("2")).unwrap();
    fs::write(
        root.join("current.json"),
        br#"{"current":2,"previous":null}"#,
    )
    .unwrap();
    let reopened = open(&root);
    assert_eq!(reopened.status().source, RulesSource::EmbeddedBaseline);
    assert!(
        outside.join("pack.json").exists(),
        "junction target untouched"
    );
    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(outside);
}
