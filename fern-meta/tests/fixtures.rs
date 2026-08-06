use fern_meta::{RuleContext, VersionManifest, VersionMetadata};

#[test]
fn official_version_manifest_fixture_is_readable() {
    let raw = include_str!("fixtures/version_manifest_v2.json");
    let manifest: VersionManifest = serde_json::from_str(raw).expect("parse official manifest");

    assert!(
        manifest
            .versions
            .iter()
            .any(|version| version.id == "1.21.1")
    );
    assert!(
        manifest
            .versions
            .iter()
            .any(|version| version.id == "1.12.2")
    );
}

#[test]
fn modern_and_legacy_version_fixtures_keep_required_protocol_fields() {
    let modern: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/version-1.21.1.json"))
            .expect("parse modern version fixture");
    let legacy: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/version-1.12.2.json"))
            .expect("parse legacy version fixture");

    assert!(modern["arguments"]["game"].is_array());
    assert_eq!(modern["javaVersion"]["majorVersion"], 21);
    assert!(legacy["minecraftArguments"].is_string());
    assert!(legacy.get("arguments").is_none());
}

#[test]
fn official_version_fixtures_parse_into_metadata_models() {
    let modern: VersionMetadata =
        serde_json::from_str(include_str!("fixtures/version-1.21.1.json"))
            .expect("parse modern metadata");
    let legacy: VersionMetadata =
        serde_json::from_str(include_str!("fixtures/version-1.12.2.json"))
            .expect("parse legacy metadata");

    assert_eq!(modern.id, "1.21.1");
    assert_eq!(
        modern.java_version.expect("java requirement").major_version,
        21
    );
    assert!(modern.arguments.is_some());
    assert_eq!(legacy.id, "1.12.2");
    assert!(legacy.minecraft_arguments.is_some());
    assert_eq!(
        legacy.resolved_arguments(&RuleContext::linux_x64()).0.len(),
        5
    );
}
