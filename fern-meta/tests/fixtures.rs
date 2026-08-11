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

/// 1.13 到 1.18.2 的官方元数据把一个原生库拆成坐标一字不差的两条：一条带
/// artifact 给 classpath，一条带 `natives` 表和 classifier。只按坐标去重会把
/// 后者当副本丢掉，natives 目录于是空着——游戏一路加载到开窗口才崩在
/// 「LWJGL Failed to load a library」，看不出和库清单有关。
///
/// 拿真的 1.16.5 钉住：Windows 上两条都要在，各自指向该指向的文件。
#[test]
fn split_native_entries_both_survive_on_windows() {
    let metadata: VersionMetadata =
        serde_json::from_str(include_str!("fixtures/version-1.16.5.json"))
            .expect("parse 1.16.5 metadata");
    let windows = RuleContext {
        os_name: "windows".to_owned(),
        os_arch: "x86_64".to_owned(),
        ..RuleContext::default()
    };

    let lwjgl: Vec<_> = metadata
        .effective_libraries(&windows)
        .into_iter()
        .filter(|library| library.name == "org.lwjgl:lwjgl:3.2.2")
        .filter_map(|library| library.file(&windows)?.path.as_deref())
        .collect();

    assert_eq!(
        lwjgl,
        vec![
            "org/lwjgl/lwjgl/3.2.2/lwjgl-3.2.2.jar",
            "org/lwjgl/lwjgl/3.2.2/lwjgl-3.2.2-natives-windows.jar",
        ]
    );

    // 一条都不能漏：这个版本的每个 LWJGL 模块都是这么拆的。
    let natives = metadata
        .effective_libraries(&windows)
        .into_iter()
        .filter(|library| library.natives.is_some())
        .count();
    assert_eq!(natives, 8);
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
