use cleanup_core::{
    ArtifactEcosystem, ArtifactType, CatalogError, CatalogLimits, Lifecycle, RebuildConsequence,
    Recoverability, ScannerKind, load_catalog,
};
use std::io::Cursor;

fn valid() -> String {
    r#"{
      "schemaVersion": 1,
      "rules": [{
        "id": "node-modules", "ruleVersion": 1, "lifecycle": "verified", "risk": "recoverable",
        "provenance": {"source": "maintainer", "verifiedAt": "2026-08-30"}, "defaultSelected": true,
        "artifact": {"ecosystem": "nodeJs", "artifactType": "installedDependencies", "recoverability": "rebuildable", "rebuildConsequence": "networkDownloadRequired"},
        "scanner": "projectArtifacts", "roots": [{"binding": "profile", "suffix": "source"}],
        "markers": {"all": ["package.json"], "any": []}, "targets": ["node_modules"], "targetType": "directory",
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
    assert_eq!(artifact.recoverability, Recoverability::Rebuildable);
    assert_eq!(
        artifact.rebuild_consequence,
        RebuildConsequence::NetworkDownloadRequired
    );
    assert_eq!(
        serde_json::to_value(artifact).unwrap(),
        serde_json::json!({
            "ecosystem": "nodeJs",
            "artifactType": "installedDependencies",
            "recoverability": "rebuildable",
            "rebuildConsequence": "networkDownloadRequired"
        })
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
        "        \"artifact\": {\"ecosystem\": \"nodeJs\", \"artifactType\": \"installedDependencies\", \"recoverability\": \"rebuildable\", \"rebuildConsequence\": \"networkDownloadRequired\"},\n",
        "",
    );
    assert!(matches!(
        load_catalog(Cursor::new(missing_artifact), CatalogLimits::default()),
        Err(CatalogError::Invalid(_))
    ));

    let direct_artifact = valid()
        .replace("\"projectArtifacts\"", "\"direct\"")
        .replace(
            "\"markers\": {\"all\": [\"package.json\"], \"any\": []}",
            "\"markers\": {}",
        )
        .replace(", \"projectDepth\": 3, \"targetDepth\": 2", "");
    assert!(matches!(
        load_catalog(Cursor::new(direct_artifact), CatalogLimits::default()),
        Err(CatalogError::Invalid(_))
    ));

    let direct_target_depth = valid()
        .replace(
            "        \"artifact\": {\"ecosystem\": \"nodeJs\", \"artifactType\": \"installedDependencies\", \"recoverability\": \"rebuildable\", \"rebuildConsequence\": \"networkDownloadRequired\"},\n",
            "",
        )
        .replace("\"projectArtifacts\"", "\"direct\"")
        .replace(
            "\"markers\": {\"all\": [\"package.json\"], \"any\": []}",
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
}
