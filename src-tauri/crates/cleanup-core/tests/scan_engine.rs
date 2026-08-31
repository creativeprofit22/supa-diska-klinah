mod support;
use cleanup_core::*;
use std::{
    collections::HashMap,
    io::Cursor,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use support::{CounterEntropy, FixtureChange, FixtureFs};

fn catalog(rules: &str) -> RuleCatalog {
    let json = format!(r#"{{"schemaVersion":1,"rules":[{rules}]}}"#);
    load_catalog(Cursor::new(json), CatalogLimits::default()).unwrap()
}
fn direct(id: &str, binding: &str, targets: &str, excluded: &str) -> String {
    format!(
        r#"{{"id":"{id}","ruleVersion":1,"lifecycle":"verified","risk":"safe","provenance":{{"source":"test","verifiedAt":"2026-08-30"}},"defaultSelected":false,"scanner":"direct","roots":[{{"binding":"{binding}","suffix":""}}],"markers":{{}},"targets":[{targets}],"targetType":"directory","rootDepth":12,"minimumAgeSeconds":1,"excludedNames":[{excluded}],"excludedPaths":[]}}"#
    )
}
fn complete_policy(fs: &FixtureFs) -> ProtectionPolicy {
    let configured = PathBuf::from(r"C:\ConfiguredProtection");
    fs.directory(&configured);
    complete_policy_with(fs, vec![configured])
}
fn complete_policy_with(fs: &FixtureFs, configured: Vec<PathBuf>) -> ProtectionPolicy {
    let system = PathBuf::from(r"C:\Windows");
    let durable_user = PathBuf::from(r"C:\Users\Me\Documents");
    fs.directory(&system);
    fs.directory(&durable_user);
    ProtectionPolicy::compile(
        fs,
        ProtectionInputs::new(vec![system], vec![durable_user], configured).unwrap(),
    )
    .unwrap()
}
fn bindings(items: &[(&str, &str)]) -> HashMap<String, PathBuf> {
    items
        .iter()
        .map(|(key, value)| ((*key).into(), PathBuf::from(value)))
        .collect()
}
fn run(
    engine: &ScanEngine,
    catalog: &RuleCatalog,
    selected: &[&str],
    bindings: &HashMap<String, PathBuf>,
    policy: &ProtectionPolicy,
    limits: ScanLimits,
    entropy: &dyn Entropy,
) -> Result<ScanResult, ScanError> {
    engine.scan(ScanRequest {
        catalog,
        selected_rule_ids: &selected.iter().map(|id| (*id).into()).collect::<Vec<_>>(),
        root_bindings: bindings,
        protection: policy,
        limits,
        cancellation: CancellationToken::new(),
        entropy,
        progress: &|_| {},
    })
}

#[test]
fn protected_scan_roots_are_rejected() {
    let fs = Arc::new(FixtureFs::new());
    let root = PathBuf::from(r"C:\ProtectedScan");
    fs.directory(&root);
    let policy = complete_policy_with(fs.as_ref(), vec![root.clone()]);
    let rules = catalog(&direct("cache-rule", "root", "\"cache\"", ""));

    let error = run(
        &ScanEngine::new(fs),
        &rules,
        &["cache-rule"],
        &bindings(&[("root", r"C:\ProtectedScan")]),
        &policy,
        ScanLimits::default(),
        &CounterEntropy::default(),
    )
    .unwrap_err();

    assert!(matches!(error, ScanError::InvalidInput(_)));
}

#[test]
fn hostile_entries_never_become_preview_candidates() {
    let fs = Arc::new(FixtureFs::new());
    let root = PathBuf::from(r"C:\scan");
    fs.directory(&root);
    fs.directory(root.join("good"));
    fs.directory(root.join("good/cache"));
    fs.file(root.join("good/cache/data.bin"), 7);
    fs.link(root.join("cache"));
    fs.directory(root.join("denied"));
    fs.make_unreadable(root.join("denied"));
    fs.directory(root.join("skip"));
    fs.directory(root.join("skip/cache"));
    let long = root.join("x".repeat(280));
    fs.directory(&long);
    fs.directory(long.join("cache"));
    fs.directory(root.join("protected"));
    fs.directory(root.join("protected/cache"));
    fs.directory(root.join("loop-a"));
    fs.directory(root.join("loop-b"));
    fs.alias_identity(&root.join("loop-b"), &root.join("loop-a"));
    let policy = complete_policy_with(fs.as_ref(), vec![root.join("protected")]);
    let rules = catalog(&direct("cache-rule", "root", "\"cache\"", "\"skip\""));
    let result = run(
        &ScanEngine::new(fs),
        &rules,
        &["cache-rule"],
        &bindings(&[("root", r"C:\scan")]),
        &policy,
        ScanLimits::default(),
        &CounterEntropy::default(),
    )
    .unwrap();
    assert_eq!(
        result.snapshot.records().len(),
        2,
        "{:?}",
        result.diagnostics
    );
    assert!(
        result
            .snapshot
            .records()
            .iter()
            .all(|record| !record.display_path.contains("protected")
                && !record.display_path.contains("skip"))
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|item| item.reason == DiagnosticReason::LinkLike)
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|item| item.reason == DiagnosticReason::Unreadable)
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|item| item.reason == DiagnosticReason::Loop)
    );
}

#[test]
fn raced_children_are_diagnosed_individually_and_never_become_candidates() {
    let fs = Arc::new(FixtureFs::new());
    let root = PathBuf::from(r"C:\scan");
    fs.directory(&root);
    fs.directory(root.join("cache"));
    fs.directory(root.join("gone"));
    fs.directory(root.join("swap"));
    fs.disappear_on_enumeration(root.join("gone"));
    fs.replace_on_enumeration(root.join("swap"), EntryKind::LinkLike);
    let rules = catalog(&direct(
        "race-rule",
        "root",
        "\"cache\",\"gone\",\"swap\"",
        "",
    ));
    let policy = complete_policy(fs.as_ref());

    let result = run(
        &ScanEngine::new(fs),
        &rules,
        &["race-rule"],
        &bindings(&[("root", r"C:\scan")]),
        &policy,
        ScanLimits::default(),
        &CounterEntropy::default(),
    )
    .unwrap();

    assert_eq!(
        result.snapshot.records().len(),
        1,
        "{:?}",
        result.diagnostics
    );
    assert!(result.snapshot.records()[0].display_path.ends_with("cache"));
    assert!(result.diagnostics.iter().any(|item| {
        item.path.ends_with("gone") && item.reason == DiagnosticReason::Disappeared
    }));
    assert!(
        result.diagnostics.iter().any(|item| {
            item.path.ends_with("swap") && item.reason == DiagnosticReason::LinkLike
        })
    );
}

#[test]
fn changing_candidates_and_descendants_of_retained_directories_are_removed() {
    let fs = Arc::new(FixtureFs::new());
    let root = PathBuf::from(r"C:\scan");
    fs.directory(&root);
    for candidate in [
        "stable",
        "size",
        "modified",
        "identity",
        "type",
        "disappeared",
        "replacement",
        "appearance",
    ] {
        fs.directory(root.join(candidate));
    }
    fs.file(root.join("stable/file"), 5);
    for candidate in [
        "size",
        "modified",
        "identity",
        "type",
        "disappeared",
        "replacement",
    ] {
        fs.file(root.join(candidate).join("file"), 7);
    }
    fs.change_after(root.join("size/file"), 4, FixtureChange::Size);
    fs.change_after(root.join("modified/file"), 4, FixtureChange::Modified);
    fs.change_after(root.join("identity/file"), 4, FixtureChange::Identity);
    fs.change_after(root.join("type/file"), 4, FixtureChange::Type);
    fs.change_after(root.join("disappeared/file"), 4, FixtureChange::Disappear);
    fs.replace_on_enumeration(root.join("replacement/file"), EntryKind::File);
    fs.appear_after_read(
        root.join("appearance"),
        2,
        root.join("appearance/new-file"),
        EntryKind::File,
        11,
    );
    let rules = catalog(&direct(
        "a-rule",
        "root",
        "\"stable\",\"size\",\"modified\",\"identity\",\"type\",\"disappeared\",\"replacement\",\"appearance\"",
        "",
    ));
    let policy = complete_policy(fs.as_ref());
    let result = run(
        &ScanEngine::new(fs),
        &rules,
        &["a-rule"],
        &bindings(&[("root", r"C:\scan")]),
        &policy,
        ScanLimits::default(),
        &CounterEntropy::default(),
    )
    .unwrap();

    assert_eq!(result.snapshot.records().len(), 1, "{result:?}");
    assert!(
        result.snapshot.records()[0]
            .display_path
            .ends_with("stable")
    );
    assert_eq!(result.snapshot.records()[0].bytes, 5);
    for candidate in [
        "size",
        "modified",
        "identity",
        "type",
        "replacement",
        "appearance",
    ] {
        assert!(
            result.diagnostics.iter().any(|item| {
                item.path.ends_with(candidate) && item.reason == DiagnosticReason::Changed
            }),
            "missing Changed diagnostic for {candidate}: {:?}",
            result.diagnostics
        );
    }
    assert!(
        result.diagnostics.iter().any(|item| {
            item.path.ends_with("disappeared") && item.reason == DiagnosticReason::Disappeared
        }),
        "missing Disappeared diagnostic: {:?}",
        result.diagnostics
    );
}

#[test]
fn duplicate_ownership_order_and_identifiers_are_deterministic_and_opaque() {
    let fs = Arc::new(FixtureFs::new());
    fs.directory(r"C:\scan");
    fs.directory(r"C:\scan\cache");
    let rules = catalog(&format!(
        "{},{}",
        direct("z-rule", "root", "\"cache\"", ""),
        direct("a-rule", "root", "\"cache\"", "")
    ));
    let policy = complete_policy(fs.as_ref());
    let engine = ScanEngine::new(fs);
    let roots = bindings(&[("root", r"C:\scan")]);
    let first = run(
        &engine,
        &rules,
        &["z-rule", "a-rule"],
        &roots,
        &policy,
        ScanLimits::default(),
        &CounterEntropy::default(),
    )
    .unwrap();
    let second = run(
        &engine,
        &rules,
        &["z-rule", "a-rule"],
        &roots,
        &policy,
        ScanLimits::default(),
        &CounterEntropy::default(),
    )
    .unwrap();
    assert_eq!(first.snapshot.records()[0].rule_id, "a-rule");
    assert_eq!(
        first.snapshot.records()[0].display_path,
        second.snapshot.records()[0].display_path
    );
    for value in [first.snapshot.scan_id(), &first.snapshot.records()[0].id] {
        assert_eq!(value.len(), 32);
        assert!(!value.contains("cache") && !value.contains("rule"));
    }
    assert!(
        first
            .snapshot
            .resolve(&first.snapshot.records()[0].id)
            .is_some()
    );
}

#[test]
fn project_artifacts_snapshot_retains_complete_mutation_validation_proof() {
    let fs = Arc::new(FixtureFs::new());
    fs.directory(r"C:\work");
    fs.directory(r"C:\work\app");
    fs.file(r"C:\work\app\package.json", 1);
    fs.directory(r"C:\work\app\node_modules");
    fs.file(r"C:\work\app\node_modules\data", 9);
    let rule = r#"{"id":"projects","ruleVersion":3,"lifecycle":"stable","risk":"recoverable","provenance":{"source":"test","verifiedAt":"2026-08-30"},"defaultSelected":true,"artifact":{"ecosystem":"nodeJs","artifactType":"installedDependencies","recoverability":"rebuildable","rebuildConsequence":"networkDownloadRequired"},"scanner":"projectArtifacts","roots":[{"binding":"root","suffix":""}],"markers":{"all":["package.json"],"any":[]},"targets":["node_modules"],"targetType":"directory","rootDepth":4,"projectDepth":3,"targetDepth":2,"minimumAgeSeconds":1,"excludedNames":[],"excludedPaths":[]}"#;
    let rules = catalog(rule);
    let policy = complete_policy(fs.as_ref());

    let result = run(
        &ScanEngine::new(fs),
        &rules,
        &["projects"],
        &bindings(&[("root", r"C:\work")]),
        &policy,
        ScanLimits::default(),
        &CounterEntropy::default(),
    )
    .unwrap();
    let record = &result.snapshot.records()[0];
    assert_eq!(record.project_name.as_deref(), Some("app"));
    assert_eq!(record.project_path.as_deref(), Some(r"C:\work\app"));
    assert_eq!(record.bytes, 9);
    assert_eq!(record.risk, Some(Risk::Recoverable));
    assert_eq!(record.default_selected, Some(true));
    assert_eq!(
        record.artifact.unwrap().ecosystem,
        ArtifactEcosystem::NodeJs
    );
    assert_eq!(
        serde_json::to_value(record).unwrap()["artifact"],
        serde_json::json!({
            "ecosystem": "nodeJs",
            "artifactType": "installedDependencies",
            "recoverability": "rebuildable",
            "rebuildConsequence": "networkDownloadRequired"
        })
    );
    let proof = result.snapshot.resolve(&record.id).unwrap();

    assert_eq!(proof.scan_root, PathBuf::from(r"C:\work"));
    assert_eq!(proof.context_root, PathBuf::from(r"C:\work\app"));
    assert_eq!(proof.rule.id, "projects");
    assert_eq!(proof.rule.rule_version, 3);
    assert_eq!(proof.kind, EntryKind::Directory);
    assert_eq!(proof.logical_bytes, 9);
    assert!(proof.allocated_bytes >= proof.logical_bytes);
    assert!(
        proof
            .scanned_at
            .duration_since(std::time::UNIX_EPOCH)
            .is_ok()
    );
}

#[test]
fn final_validation_rejects_identity_changes_and_new_protection() {
    let fs = Arc::new(FixtureFs::new());
    fs.directory(r"C:\scan");
    fs.directory(r"C:\scan\cache");
    let rules = catalog(&direct("cache-rule", "root", "\"cache\"", ""));
    let policy = complete_policy(fs.as_ref());
    let result = run(
        &ScanEngine::new(fs.clone()),
        &rules,
        &["cache-rule"],
        &bindings(&[("root", r"C:\scan")]),
        &policy,
        ScanLimits::default(),
        &CounterEntropy::default(),
    )
    .unwrap();
    let proof = result
        .snapshot
        .resolve(&result.snapshot.records()[0].id)
        .unwrap();

    fs.make_active(r"C:\scan\cache");
    assert_eq!(
        revalidate_candidate(fs.as_ref(), proof, &policy, std::time::SystemTime::now()),
        Err(CandidateRejection::Active)
    );
    fs.make_inactive(Path::new(r"C:\scan\cache"));

    fs.change_after(r"C:\scan\cache", 1, FixtureChange::Identity);
    assert_eq!(
        revalidate_candidate(fs.as_ref(), proof, &policy, std::time::SystemTime::now()),
        Err(CandidateRejection::IdentityChanged)
    );

    let protected = complete_policy_with(fs.as_ref(), vec![PathBuf::from(r"C:\scan\cache")]);
    assert_eq!(
        revalidate_candidate(fs.as_ref(), proof, &protected, std::time::SystemTime::now()),
        Err(CandidateRejection::Protected)
    );
}

#[test]
fn progress_reports_discovery_measurement_and_finalization() {
    let fs = Arc::new(FixtureFs::new());
    fs.directory(r"C:\scan");
    fs.directory(r"C:\scan\cache");
    let rules = catalog(&direct("cache-rule", "root", "\"cache\"", ""));
    let policy = complete_policy(fs.as_ref());
    let roots = bindings(&[("root", r"C:\scan")]);
    let selected = vec!["cache-rule".into()];
    let events = Mutex::new(Vec::new());
    let sink = |event| events.lock().unwrap().push(event);
    ScanEngine::new(fs)
        .scan(ScanRequest {
            catalog: &rules,
            selected_rule_ids: &selected,
            root_bindings: &roots,
            protection: &policy,
            limits: ScanLimits::default(),
            cancellation: CancellationToken::new(),
            entropy: &CounterEntropy::default(),
            progress: &sink,
        })
        .unwrap();
    let events = events.into_inner().unwrap();
    assert_eq!(events.first().unwrap().phase, ScanPhase::Discovering);
    assert!(
        events
            .iter()
            .any(|event| event.phase == ScanPhase::Measuring)
    );
    assert_eq!(events.last().unwrap().phase, ScanPhase::Finalizing);
    assert!(
        events
            .windows(2)
            .all(|pair| pair[0].visited_entries <= pair[1].visited_entries)
    );
}

#[test]
fn project_artifacts_markers_age_and_exclusions_are_honored() {
    let fs = Arc::new(FixtureFs::new());
    fs.directory(r"C:\work");
    fs.directory(r"C:\work\app");
    fs.file(r"C:\work\app\package.json", 1);
    fs.directory(r"C:\work\app\node_modules");
    fs.file(r"C:\work\app\node_modules\x", 3);
    fs.directory(r"C:\work\unmarked");
    fs.directory(r"C:\work\unmarked\node_modules");
    let rule = r#"{"id":"projects","ruleVersion":1,"lifecycle":"stable","risk":"recoverable","provenance":{"source":"test","verifiedAt":"2026-08-30"},"defaultSelected":true,"artifact":{"ecosystem":"nodeJs","artifactType":"installedDependencies","recoverability":"rebuildable","rebuildConsequence":"networkDownloadRequired"},"scanner":"projectArtifacts","roots":[{"binding":"root","suffix":""}],"markers":{"all":["package.json"],"any":[]},"targets":["node_modules"],"targetType":"directory","rootDepth":4,"projectDepth":3,"targetDepth":2,"minimumAgeSeconds":1,"excludedNames":[],"excludedPaths":[]}"#;
    let rules = catalog(rule);
    let policy = complete_policy(fs.as_ref());
    let result = run(
        &ScanEngine::new(fs),
        &rules,
        &["projects"],
        &bindings(&[("root", r"C:\work")]),
        &policy,
        ScanLimits::default(),
        &CounterEntropy::default(),
    )
    .unwrap();
    assert_eq!(result.snapshot.records().len(), 1);
    assert!(
        result.snapshot.records()[0]
            .display_path
            .ends_with("node_modules")
    );
}

#[test]
fn nested_project_artifacts_are_ordered_without_duplicates() {
    let fs = Arc::new(FixtureFs::new());
    fs.directory(r"C:\work");
    fs.directory(r"C:\work\app");
    fs.file(r"C:\work\app\package.json", 1);
    fs.directory(r"C:\work\app\node_modules");
    fs.file(r"C:\work\app\node_modules\app.bin", 3);
    fs.directory(r"C:\work\app\packages");
    fs.directory(r"C:\work\app\packages\child");
    fs.file(r"C:\work\app\packages\child\package.json", 1);
    fs.directory(r"C:\work\app\packages\child\node_modules");
    fs.file(r"C:\work\app\packages\child\node_modules\child.bin", 5);
    let rule = r#"{"id":"projects","ruleVersion":1,"lifecycle":"stable","risk":"recoverable","provenance":{"source":"test","verifiedAt":"2026-08-30"},"defaultSelected":false,"artifact":{"ecosystem":"nodeJs","artifactType":"installedDependencies","recoverability":"rebuildable","rebuildConsequence":"networkDownloadRequired"},"scanner":"projectArtifacts","roots":[{"binding":"root","suffix":""}],"markers":{"all":["package.json"],"any":[]},"targets":["node_modules"],"targetType":"directory","rootDepth":4,"projectDepth":4,"targetDepth":0,"minimumAgeSeconds":1,"excludedNames":[],"excludedPaths":[]}"#;
    let rules = catalog(rule);
    let policy = complete_policy(fs.as_ref());

    let result = run(
        &ScanEngine::new(fs),
        &rules,
        &["projects"],
        &bindings(&[("root", r"C:\work")]),
        &policy,
        ScanLimits::default(),
        &CounterEntropy::default(),
    )
    .unwrap();
    let paths: Vec<_> = result
        .snapshot
        .records()
        .iter()
        .map(|record| record.display_path.as_str())
        .collect();
    assert_eq!(
        paths,
        vec![
            r"C:\work\app\node_modules",
            r"C:\work\app\packages\child\node_modules"
        ]
    );
}

#[test]
fn project_artifacts_matched_tree_is_measured_but_not_searched_for_projects() {
    let fs = Arc::new(FixtureFs::new());
    fs.directory(r"C:\work");
    fs.directory(r"C:\work\app");
    fs.file(r"C:\work\app\package.json", 1);
    fs.directory(r"C:\work\app\node_modules");
    fs.file(r"C:\work\app\node_modules\top.bin", 9);
    fs.directory(r"C:\work\app\node_modules\dependency");
    fs.file(r"C:\work\app\node_modules\dependency\package.json", 2);
    fs.directory(r"C:\work\app\node_modules\dependency\node_modules");
    fs.file(
        r"C:\work\app\node_modules\dependency\node_modules\nested.bin",
        5,
    );
    let rule = r#"{"id":"projects","ruleVersion":1,"lifecycle":"stable","risk":"recoverable","provenance":{"source":"test","verifiedAt":"2026-08-30"},"defaultSelected":false,"artifact":{"ecosystem":"nodeJs","artifactType":"installedDependencies","recoverability":"rebuildable","rebuildConsequence":"networkDownloadRequired"},"scanner":"projectArtifacts","roots":[{"binding":"root","suffix":""}],"markers":{"all":["package.json"],"any":[]},"targets":["node_modules"],"targetType":"directory","rootDepth":4,"projectDepth":4,"targetDepth":0,"minimumAgeSeconds":1,"excludedNames":[],"excludedPaths":[]}"#;
    let rules = catalog(rule);
    let policy = complete_policy(fs.as_ref());

    let result = run(
        &ScanEngine::new(fs),
        &rules,
        &["projects"],
        &bindings(&[("root", r"C:\work")]),
        &policy,
        ScanLimits::default(),
        &CounterEntropy::default(),
    )
    .unwrap();
    assert_eq!(result.snapshot.records().len(), 1);
    assert_eq!(result.snapshot.records()[0].bytes, 16);
    assert_eq!(
        result.snapshot.records()[0].project_name.as_deref(),
        Some("app")
    );
}

#[test]
fn project_artifacts_links_and_visit_limits_fail_closed() {
    let fs = Arc::new(FixtureFs::new());
    fs.directory(r"C:\work");
    fs.directory(r"C:\work\app");
    fs.file(r"C:\work\app\package.json", 1);
    fs.directory(r"C:\work\app\node_modules");
    fs.file(r"C:\work\app\node_modules\dependency.bin", 3);
    fs.link(r"C:\work\linked-project");
    let rule = r#"{"id":"projects","ruleVersion":1,"lifecycle":"stable","risk":"recoverable","provenance":{"source":"test","verifiedAt":"2026-08-30"},"defaultSelected":false,"artifact":{"ecosystem":"nodeJs","artifactType":"installedDependencies","recoverability":"rebuildable","rebuildConsequence":"networkDownloadRequired"},"scanner":"projectArtifacts","roots":[{"binding":"root","suffix":""}],"markers":{"all":["package.json"],"any":[]},"targets":["node_modules"],"targetType":"directory","rootDepth":4,"projectDepth":4,"targetDepth":0,"minimumAgeSeconds":1,"excludedNames":[],"excludedPaths":[]}"#;
    let rules = catalog(rule);
    let policy = complete_policy(fs.as_ref());

    let complete = run(
        &ScanEngine::new(fs.clone()),
        &rules,
        &["projects"],
        &bindings(&[("root", r"C:\work")]),
        &policy,
        ScanLimits::default(),
        &CounterEntropy::default(),
    )
    .unwrap();
    assert_eq!(complete.snapshot.records().len(), 1);

    let limited = run(
        &ScanEngine::new(fs),
        &rules,
        &["projects"],
        &bindings(&[("root", r"C:\work")]),
        &policy,
        ScanLimits {
            max_visited_entries: 1,
            ..ScanLimits::default()
        },
        &CounterEntropy::default(),
    )
    .unwrap();
    assert!(limited.snapshot.records().is_empty());
    assert!(
        limited
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.reason == DiagnosticReason::LimitReached)
    );
}

#[test]
fn worker_count_is_bounded() {
    let fs = Arc::new(FixtureFs::new());
    for name in ["one", "two", "three", "four"] {
        fs.directory(format!(r"C:\{name}"));
        fs.directory(format!(r"C:\{name}\cache"));
    }
    fs.delay_reads(5);
    let rules = catalog(
        &["one", "two", "three", "four"]
            .iter()
            .map(|name| direct(&format!("{name}-rule"), name, "\"cache\"", ""))
            .collect::<Vec<_>>()
            .join(","),
    );
    let selected = ["one-rule", "two-rule", "three-rule", "four-rule"];
    let roots = bindings(&[
        ("one", r"C:\one"),
        ("two", r"C:\two"),
        ("three", r"C:\three"),
        ("four", r"C:\four"),
    ]);
    let policy = complete_policy(fs.as_ref());
    let limits = ScanLimits {
        max_workers: 2,
        ..ScanLimits::default()
    };
    run(
        &ScanEngine::new(fs.clone()),
        &rules,
        &selected,
        &roots,
        &policy,
        limits,
        &CounterEntropy::default(),
    )
    .unwrap();
    assert!(fs.max_active() <= 2);
}

#[test]
fn cancellation_interrupts_directory_enumeration() {
    let fs = Arc::new(FixtureFs::new());
    fs.directory(r"C:\scan");
    for index in 0..100 {
        fs.directory(format!(r"C:\scan\item-{index}"));
    }
    let rules = catalog(&direct("cache-rule", "root", "\"cache\"", ""));
    let policy = complete_policy(fs.as_ref());
    let token = CancellationToken::new();
    fs.cancel_during_enumeration(3, token.clone());
    let result = ScanEngine::new(fs.clone()).scan(ScanRequest {
        catalog: &rules,
        selected_rule_ids: &["cache-rule".into()],
        root_bindings: &bindings(&[("root", r"C:\scan")]),
        protection: &policy,
        limits: ScanLimits::default(),
        cancellation: token,
        entropy: &CounterEntropy::default(),
        progress: &|_| {},
    });
    assert!(matches!(result, Err(ScanError::Cancelled)));
    assert_eq!(fs.enumerated_entries(), 3);
}

#[test]
fn discovery_and_measurement_stop_enumerating_at_entry_limits() {
    let discovery_fs = Arc::new(FixtureFs::new());
    discovery_fs.directory(r"C:\scan");
    for index in 0..100 {
        discovery_fs.directory(format!(r"C:\scan\item-{index}"));
    }
    let rules = catalog(&direct("cache-rule", "root", "\"cache\"", ""));
    let policy = complete_policy(discovery_fs.as_ref());
    let result = run(
        &ScanEngine::new(discovery_fs.clone()),
        &rules,
        &["cache-rule"],
        &bindings(&[("root", r"C:\scan")]),
        &policy,
        ScanLimits {
            max_visited_entries: 3,
            ..ScanLimits::default()
        },
        &CounterEntropy::default(),
    )
    .unwrap();
    assert!(result.snapshot.records().is_empty());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|item| item.reason == DiagnosticReason::LimitReached)
    );
    assert_eq!(discovery_fs.enumerated_entries(), 4);

    let measurement_fs = Arc::new(FixtureFs::new());
    measurement_fs.directory(r"C:\scan");
    measurement_fs.directory(r"C:\scan\cache");
    for index in 0..100 {
        measurement_fs.file(format!(r"C:\scan\cache\file-{index}"), 1);
    }
    let shallow = direct("cache-rule", "root", "\"cache\"", "")
        .replace("\"rootDepth\":12", "\"rootDepth\":0");
    let rules = catalog(&shallow);
    let policy = complete_policy(measurement_fs.as_ref());
    let result = run(
        &ScanEngine::new(measurement_fs.clone()),
        &rules,
        &["cache-rule"],
        &bindings(&[("root", r"C:\scan")]),
        &policy,
        ScanLimits {
            max_measurement_entries: 3,
            ..ScanLimits::default()
        },
        &CounterEntropy::default(),
    )
    .unwrap();
    assert!(result.snapshot.records().is_empty());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|item| item.reason == DiagnosticReason::LimitReached)
    );
    assert_eq!(measurement_fs.enumerated_entries(), 5);
}

#[test]
fn invalid_selected_rules_bindings_and_limits_fail_before_traversal() {
    let fs = Arc::new(FixtureFs::new());
    fs.directory(r"C:\scan");
    let rules = catalog(&direct("cache-rule", "root", "\"cache\"", ""));
    let policy = complete_policy(fs.as_ref());
    let engine = ScanEngine::new(fs.clone());
    assert!(matches!(
        run(
            &engine,
            &rules,
            &["missing"],
            &HashMap::new(),
            &policy,
            ScanLimits::default(),
            &CounterEntropy::default()
        ),
        Err(ScanError::InvalidInput(_))
    ));
    let two_roots = catalog(&direct("cache-rule", "root", "\"cache\"", "").replace(
        "\"roots\":[{\"binding\":\"root\",\"suffix\":\"\"}]",
        "\"roots\":[{\"binding\":\"root\",\"suffix\":\"\"},{\"binding\":\"missing\",\"suffix\":\"\"}]",
    ));
    assert!(matches!(
        run(
            &engine,
            &two_roots,
            &["cache-rule"],
            &bindings(&[("root", r"C:\scan")]),
            &policy,
            ScanLimits::default(),
            &CounterEntropy::default()
        ),
        Err(ScanError::InvalidInput(_))
    ));
    assert_eq!(fs.enumerated_entries(), 0);
    let limits = ScanLimits {
        max_workers: 0,
        ..ScanLimits::default()
    };
    assert!(matches!(
        run(
            &engine,
            &rules,
            &["cache-rule"],
            &bindings(&[("root", "relative")]),
            &policy,
            limits,
            &CounterEntropy::default()
        ),
        Err(ScanError::InvalidInput(_))
    ));
    assert!(!Path::new("relative").is_absolute());
}
