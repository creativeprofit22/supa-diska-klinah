use cleanup_core::{
    Activity, ArtifactEcosystem, ArtifactType, CatalogError, CatalogLimits, Confidence, Lifecycle,
    RebuildConsequence, Recoverability, ScannerKind, load_catalog,
};
use std::io::Cursor;

fn valid() -> String {
    r#"{
      "schemaVersion": 1,
      "rules": [{
        "id": "node-modules", "ruleVersion": 1, "lifecycle": "verified", "risk": "recoverable",
        "provenance": {"source": "maintainer", "verifiedAt": "2026-08-30"}, "defaultSelected": true,
        "artifact": {"ecosystem": "nodeJs", "artifactType": "installedDependencies", "confidence": "high", "recoverability": "rebuildable", "rebuildConsequence": "networkDownloadRequired"},
        "scanner": "projectArtifacts", "roots": [{"binding": "profile", "suffix": "source"}],
        "markers": {"all": ["package.json"], "any": [], "anySuffix": [".csproj"]}, "targets": ["node_modules"], "targetPrefixes": ["cmake-build-"], "targetSuffixes": [".egg-info"], "targetType": "directory",
        "rootDepth": 4, "projectDepth": 3, "targetDepth": 2, "minimumAgeSeconds": 3600,
        "excludedNames": ["keep"], "excludedPaths": ["important/cache"]
      }]
    }"#.into()
}

#[test]
fn rules_documented_fixture_is_valid() {
    let catalog = load_catalog(
        Cursor::new(include_bytes!("fixtures/catalog-v1.json")),
        CatalogLimits::default(),
    )
    .unwrap();
    assert_eq!(catalog.rules().len(), 2);
}

#[test]
fn rules_load_a_complete_v1_rule() {
    let catalog = load_catalog(Cursor::new(valid()), CatalogLimits::default()).unwrap();
    let rule = &catalog.rules()[0];
    assert_eq!(rule.lifecycle, Lifecycle::Verified);
    assert_eq!(rule.scanner, ScannerKind::ProjectArtifacts);
    assert!(rule.default_selected);
    let artifact = rule.artifact.unwrap();
    assert_eq!(artifact.ecosystem, ArtifactEcosystem::NodeJs);
    assert_eq!(artifact.artifact_type, ArtifactType::InstalledDependencies);
    assert_eq!(artifact.confidence, Confidence::High);
    assert_eq!(artifact.recoverability, Recoverability::Rebuildable);
    assert_eq!(rule.markers.any_suffix, [".csproj"]);
    assert_eq!(rule.target_prefixes, ["cmake-build-"]);
    assert_eq!(rule.target_suffixes, [".egg-info"]);
    assert_eq!(
        artifact.rebuild_consequence,
        RebuildConsequence::NetworkDownloadRequired
    );
    assert_eq!(
        serde_json::to_value(artifact).unwrap(),
        serde_json::json!({
            "ecosystem": "nodeJs",
            "artifactType": "installedDependencies",
            "confidence": "high",
            "recoverability": "rebuildable",
            "rebuildConsequence": "networkDownloadRequired"
        })
    );
}

#[test]
fn project_artifact_enums_serialize_as_closed_camel_case_values() {
    assert_eq!(
        serde_json::to_value([
            ArtifactEcosystem::Rust,
            ArtifactEcosystem::NextJs,
            ArtifactEcosystem::Angular,
            ArtifactEcosystem::Nuxt,
            ArtifactEcosystem::Vite,
            ArtifactEcosystem::SvelteKit,
            ArtifactEcosystem::Astro,
            ArtifactEcosystem::Python,
            ArtifactEcosystem::DotNet,
            ArtifactEcosystem::Gradle,
            ArtifactEcosystem::Maven,
            ArtifactEcosystem::Cmake,
            ArtifactEcosystem::Unity,
            ArtifactEcosystem::Unreal,
            ArtifactEcosystem::Godot,
        ])
        .unwrap(),
        serde_json::json!([
            "rust",
            "nextJs",
            "angular",
            "nuxt",
            "vite",
            "svelteKit",
            "astro",
            "python",
            "dotNet",
            "gradle",
            "maven",
            "cmake",
            "unity",
            "unreal",
            "godot"
        ])
    );
    assert_eq!(
        serde_json::to_value([
            ArtifactType::BuildOutput,
            ArtifactType::CompilerCache,
            ArtifactType::FrameworkCache,
            ArtifactType::VirtualEnvironment,
            ArtifactType::TestCache,
            ArtifactType::GeneratedIntermediate,
            ArtifactType::ImportedAssetCache,
        ])
        .unwrap(),
        serde_json::json!([
            "buildOutput",
            "compilerCache",
            "frameworkCache",
            "virtualEnvironment",
            "testCache",
            "generatedIntermediate",
            "importedAssetCache"
        ])
    );
    assert_eq!(
        serde_json::to_value([Confidence::High, Confidence::Medium]).unwrap(),
        serde_json::json!(["high", "medium"])
    );
    assert_eq!(
        serde_json::to_value([Activity::Idle, Activity::InUse]).unwrap(),
        serde_json::json!(["idle", "inUse"])
    );
    assert_eq!(
        serde_json::to_value([
            RebuildConsequence::LocalRebuild,
            RebuildConsequence::ToolchainRequired,
            RebuildConsequence::ExpensiveReimport,
        ])
        .unwrap(),
        serde_json::json!(["localRebuild", "toolchainRequired", "expensiveReimport"])
    );
}

#[test]
fn rules_reject_oversized_unknown_and_malformed_catalogs() {
    let limits = CatalogLimits {
        max_bytes: 8,
        ..CatalogLimits::default()
    };
    assert!(matches!(
        load_catalog(Cursor::new(valid()), limits),
        Err(CatalogError::TooLarge)
    ));
    let unknown = valid().replace(
        "\"schemaVersion\": 1,",
        "\"schemaVersion\": 1, \"surprise\": true,",
    );
    assert!(matches!(
        load_catalog(Cursor::new(unknown), CatalogLimits::default()),
        Err(CatalogError::Json(_))
    ));
    assert!(matches!(
        load_catalog(Cursor::new("{"), CatalogLimits::default()),
        Err(CatalogError::Json(_))
    ));
}

#[test]
fn rules_reject_unsafe_defaults_duplicates_traversal_and_contradictions() {
    let unsafe_default = valid().replace("\"risk\": \"recoverable\"", "\"risk\": \"highImpact\"");
    assert!(matches!(
        load_catalog(Cursor::new(unsafe_default), CatalogLimits::default()),
        Err(CatalogError::Invalid(_))
    ));
    let duplicate = valid().replace("}]\n    }", "}, {\"id\":\"node-modules\",\"ruleVersion\":1,\"lifecycle\":\"stable\",\"risk\":\"safe\",\"provenance\":{\"source\":\"x\",\"verifiedAt\":\"x\"},\"defaultSelected\":false,\"scanner\":\"direct\",\"roots\":[{\"binding\":\"p\",\"suffix\":\"\"}],\"markers\":{},\"targets\":[\"x\"],\"targetType\":\"either\",\"rootDepth\":1}]\n    }");
    assert!(matches!(
        load_catalog(Cursor::new(duplicate), CatalogLimits::default()),
        Err(CatalogError::Invalid(_))
    ));
    let traversal = valid().replace("\"suffix\": \"source\"", "\"suffix\": \"../source\"");
    assert!(matches!(
        load_catalog(Cursor::new(traversal), CatalogLimits::default()),
        Err(CatalogError::Invalid(_))
    ));
    let direct_markers = valid().replace("\"projectArtifacts\"", "\"direct\"");
    assert!(matches!(
        load_catalog(Cursor::new(direct_markers), CatalogLimits::default()),
        Err(CatalogError::Invalid(_))
    ));

    let missing_artifact = valid().replace(
        "        \"artifact\": {\"ecosystem\": \"nodeJs\", \"artifactType\": \"installedDependencies\", \"confidence\": \"high\", \"recoverability\": \"rebuildable\", \"rebuildConsequence\": \"networkDownloadRequired\"},\n",
        "",
    );
    assert!(matches!(
        load_catalog(Cursor::new(missing_artifact), CatalogLimits::default()),
        Err(CatalogError::Invalid(_))
    ));

    let direct_artifact = valid()
        .replace("\"projectArtifacts\"", "\"direct\"")
        .replace(
            "\"markers\": {\"all\": [\"package.json\"], \"any\": [], \"anySuffix\": [\".csproj\"]}",
            "\"markers\": {}",
        )
        .replace(", \"projectDepth\": 3, \"targetDepth\": 2", "");
    assert!(matches!(
        load_catalog(Cursor::new(direct_artifact), CatalogLimits::default()),
        Err(CatalogError::Invalid(_))
    ));

    let direct_target_depth = valid()
        .replace(
            "        \"artifact\": {\"ecosystem\": \"nodeJs\", \"artifactType\": \"installedDependencies\", \"confidence\": \"high\", \"recoverability\": \"rebuildable\", \"rebuildConsequence\": \"networkDownloadRequired\"},\n",
            "",
        )
        .replace("\"projectArtifacts\"", "\"direct\"")
        .replace(
            "\"markers\": {\"all\": [\"package.json\"], \"any\": [], \"anySuffix\": [\".csproj\"]}",
            "\"markers\": {}",
        )
        .replace(", \"projectDepth\": 3", "");
    assert!(matches!(
        load_catalog(Cursor::new(direct_target_depth), CatalogLimits::default()),
        Err(CatalogError::Invalid(_))
    ));

    let unknown_ecosystem = valid().replace("\"nodeJs\"", "\"unknown\"");
    assert!(matches!(
        load_catalog(Cursor::new(unknown_ecosystem), CatalogLimits::default()),
        Err(CatalogError::Json(_))
    ));

    for invalid_literal in ["", "*", "?", "../cache", "cache/name", "cache\\name"] {
        let invalid = valid().replace("\"cmake-build-\"", &format!("\"{invalid_literal}\""));
        assert!(matches!(
            load_catalog(Cursor::new(invalid), CatalogLimits::default()),
            Err(CatalogError::Invalid(_))
        ));
    }
    let no_markers = valid().replace(
        "\"markers\": {\"all\": [\"package.json\"], \"any\": [], \"anySuffix\": [\".csproj\"]}",
        "\"markers\": {}",
    );
    assert!(matches!(
        load_catalog(Cursor::new(no_markers), CatalogLimits::default()),
        Err(CatalogError::Invalid(_))
    ));
    let no_targets = valid()
        .replace("\"targets\": [\"node_modules\"]", "\"targets\": []")
        .replace(
            "\"targetPrefixes\": [\"cmake-build-\"]",
            "\"targetPrefixes\": []",
        )
        .replace(
            "\"targetSuffixes\": [\".egg-info\"]",
            "\"targetSuffixes\": []",
        );
    assert!(matches!(
        load_catalog(Cursor::new(no_targets), CatalogLimits::default()),
        Err(CatalogError::Invalid(_))
    ));
}
