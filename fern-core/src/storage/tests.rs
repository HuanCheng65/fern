//! 报数与瘦身的行为测试。全部建在真的文件系统上——这个模块的正确性就在
//! 「删了哪些文件、留了哪些」上。

use std::path::{Path, PathBuf};

use super::*;
use crate::storage::slim::{self, SlimContents};
use crate::{CoverSeed, InstanceId, InstanceProfile, LoaderKind, LoaderProfile};

fn scratch(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("fern-storage-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    root
}

/// 建一个只在磁盘上存在的实例。
fn instance(root: &Path, game_version: &str) -> DataPaths {
    let paths = DataPaths::new(root);
    let profile = InstanceProfile {
        cover: CoverSeed {
            identity: "moss".to_owned(),
            growth: 0,
        },
        ..InstanceProfile::vanilla(InstanceId::parse("moss").expect("id"), "苔", game_version)
    };
    fs::create_dir_all(paths.instance_root("moss")).expect("create instance");
    crate::write_instance_profile(&paths, &profile).expect("write profile");
    paths
}

fn put(root: &Path, relative: &str, body: &[u8]) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
    fs::write(path, body).expect("write");
}

fn write_version(paths: &DataPaths, id: &str, json: serde_json::Value) {
    let path = crate::launch::version::metadata_path(paths, id);
    fs::create_dir_all(path.parent().expect("parent")).expect("create version directory");
    fs::write(path, serde_json::to_vec(&json).expect("serialize")).expect("write version");
}

#[test]
fn the_report_covers_every_bucket_and_the_total_adds_up() {
    let root = scratch("report");
    let paths = instance(&root, "1.20.1");
    put(
        &root,
        "instances/moss/.minecraft/saves/家/level.dat",
        &[0; 64],
    );
    put(&root, "cache/manifest.json", &[0; 32]);
    put(&root, "logs/fern.log", &[0; 16]);
    put(&root, ".minecraft/libraries/a/b/c-1.0.jar", &[0; 128]);
    put(&root, ".minecraft/versions/1.20.1/1.20.1.json", &[0; 8]);
    put(&root, ".minecraft/assets/objects/ab/abcd", &[0; 4]);
    put(&root, "runtimes/java-runtime-gamma/bin/java", &[0; 256]);
    // 不属于任何分区的零散：数据根下的和共享目录下的各放一个。
    put(&root, "security/moss.jsonl", &[0; 10]);
    put(&root, ".minecraft/launcher_profiles.json", &[0; 6]);

    let report = report(&paths).expect("report");
    assert_eq!(report.cache, 32);
    assert_eq!(report.logs, 16);
    assert_eq!(report.libraries, 128);
    assert_eq!(report.versions, 8);
    assert_eq!(report.assets, 4);
    assert_eq!(report.runtimes, 256);
    // instances 里除了存档还有 instance.json，所以是「至少」。
    assert!(report.instances >= 64);
    // settings.json（write_instance_profile 不写它，但 security 和散文件要在）。
    assert!(report.other >= 16, "other = {}", report.other);
    let sum = report.instances
        + report.snapshots
        + report.versions
        + report.libraries
        + report.assets
        + report.runtimes
        + report.cache
        + report.logs
        + report.other;
    assert_eq!(report.total, sum);

    fs::remove_dir_all(root).expect("clean up");
}

#[test]
fn clearing_cache_and_logs_reports_what_was_freed() {
    let root = scratch("clear");
    let paths = DataPaths::new(&root);
    put(&root, "cache/a.json", &[0; 40]);
    put(&root, "cache/deep/b.json", &[0; 2]);
    put(&root, "logs/fern.log", &[0; 7]);

    assert_eq!(clear_cache(&paths).expect("clear cache"), 42);
    assert!(paths.cache.is_dir(), "缓存目录要重建，别处默认它存在");
    assert_eq!(tree_bytes(&paths.cache), 0);
    assert_eq!(clear_logs(&paths).expect("clear logs"), 7);

    fs::remove_dir_all(root).expect("clean up");
}

/// 一套所有文件都被引用的数据根：瘦身应当无事可做。这是活集算法的端到端
/// 校验——引用链上任何一环漏了，这里就会出现「可删」的误报。
#[test]
fn a_fully_referenced_setup_has_nothing_to_slim() {
    let root = scratch("live");
    let paths = instance(&root, "1.20.1");

    // 装上加载器，让链有两节。
    let mut profile = crate::read_instance(&paths, "moss").expect("read");
    profile.loader = LoaderKind::Fabric;
    profile.components.push(LoaderProfile {
        kind: LoaderKind::Fabric,
        version: "0.16.5".to_owned(),
        version_id: "fabric-loader-0.16.5-1.20.1".to_owned(),
    });

    crate::write_instance_profile(&paths, &profile).expect("write profile");

    write_version(
        &paths,
        "1.20.1",
        serde_json::json!({
            "id": "1.20.1",
            "libraries": [{
                "name": "com.mojang:brigadier:1.0.18",
                "downloads": { "artifact": {
                    "path": "com/mojang/brigadier/1.0.18/brigadier-1.0.18.jar",
                    "url": "https://x.invalid/a", "sha1": "aa", "size": 1
                } }
            }],
            "assetIndex": { "id": "17", "sha1": "aa", "size": 1, "url": "https://x.invalid/i" },
            "javaVersion": { "component": "java-runtime-gamma", "majorVersion": 17 }
        }),
    );
    write_version(
        &paths,
        "fabric-loader-0.16.5-1.20.1",
        serde_json::json!({
            "id": "fabric-loader-0.16.5-1.20.1",
            "inheritsFrom": "1.20.1",
            // 只有坐标没有 downloads：Fabric 的库就长这样，路径按 Maven 规则推。
            "libraries": [{ "name": "net.fabricmc:fabric-loader:0.16.5" }]
        }),
    );
    put(
        &root,
        ".minecraft/libraries/com/mojang/brigadier/1.0.18/brigadier-1.0.18.jar",
        b"jar",
    );
    put(
        &root,
        ".minecraft/libraries/net/fabricmc/fabric-loader/0.16.5/fabric-loader-0.16.5.jar",
        b"jar",
    );
    put(
        &root,
        ".minecraft/assets/indexes/17.json",
        br#"{"objects":{"icons/icon_16x16.png":{"hash":"badc0ffee","size":3}}}"#,
    );
    put(&root, ".minecraft/assets/objects/ba/badc0ffee", b"png");
    put(&root, "runtimes/java-runtime-gamma/bin/java", b"elf");

    let plan = slim::preview(&paths).expect("preview");
    assert!(plan.is_empty(), "全被引用却说可删：{plan:?}");

    fs::remove_dir_all(root).expect("clean up");
}

#[test]
fn slimming_removes_only_what_nothing_references() {
    let root = scratch("slim");
    let paths = instance(&root, "1.20.1");
    write_version(
        &paths,
        "1.20.1",
        serde_json::json!({
            "id": "1.20.1",
            "libraries": [{ "name": "com.mojang:brigadier:1.0.18" }],
            "assetIndex": { "id": "17", "sha1": "aa", "size": 1, "url": "https://x.invalid/i" },
            "javaVersion": { "component": "java-runtime-gamma", "majorVersion": 17 }
        }),
    );
    put(
        &root,
        ".minecraft/libraries/com/mojang/brigadier/1.0.18/brigadier-1.0.18.jar",
        b"jar",
    );
    // 活索引和死索引引用同一个对象——它必须活下来。
    put(
        &root,
        ".minecraft/assets/indexes/17.json",
        br#"{"objects":{"a":{"hash":"shared00","size":1}}}"#,
    );
    put(
        &root,
        ".minecraft/assets/indexes/legacy.json",
        br#"{"objects":{"a":{"hash":"shared00","size":1},"b":{"hash":"dead0000","size":1}}}"#,
    );
    put(&root, ".minecraft/assets/objects/sh/shared00", b"keep");
    put(&root, ".minecraft/assets/objects/de/dead0000", b"drop");
    put(&root, ".minecraft/assets/virtual/legacy/a.png", b"drop");
    put(&root, "runtimes/java-runtime-gamma/bin/java", b"elf");
    // 孤儿们。
    put(&root, ".minecraft/versions/1.8.9/1.8.9.jar", &[0; 32]);
    put(
        &root,
        ".minecraft/libraries/org/old/old/1.0/old-1.0.jar",
        &[0; 16],
    );
    put(&root, "runtimes/jre-legacy/bin/java", &[0; 8]);
    // 用户手动登记过的 Java 指着一份运行时：它没被任何版本要求，也得留。
    let mut settings = crate::data::settings::load(&paths);
    settings
        .java
        .extra_paths
        .push(paths.runtimes.join("jre-pinned").join("bin").join("java"));
    crate::save_settings(&paths, &settings).expect("save settings");
    put(&root, "runtimes/jre-pinned/bin/java", &[0; 8]);

    let plan = slim::preview(&paths).expect("preview");
    assert_eq!(plan.versions, vec!["1.8.9"]);
    assert_eq!(plan.versions_bytes, 32);
    assert_eq!(plan.runtimes, vec!["jre-legacy"]);
    assert_eq!(plan.libraries_files, 1);
    assert_eq!(plan.libraries_bytes, 16);
    // legacy.json、virtual/legacy、dead0000 三处，shared00 不算。
    assert_eq!(plan.assets_files, 3);

    let done = slim::apply(
        &paths,
        &SlimContents {
            versions: true,
            runtimes: true,
            libraries: true,
            assets: true,
        },
    )
    .expect("apply");
    assert_eq!(done.bytes(), plan.bytes());

    // 孤儿没了，被引用的都在。
    assert!(!paths.versions.join("1.8.9").exists());
    assert!(!paths.runtimes.join("jre-legacy").exists());
    assert!(paths.runtimes.join("jre-pinned").exists());
    assert!(!paths.libraries.join("org").exists(), "空壳目录也要扫掉");
    assert!(
        paths
            .libraries
            .join("com/mojang/brigadier/1.0.18/brigadier-1.0.18.jar")
            .exists()
    );
    assert!(paths.assets.join("objects/sh/shared00").exists());
    assert!(!paths.assets.join("objects/de/dead0000").exists());
    assert!(!paths.assets.join("indexes/legacy.json").exists());
    assert!(!paths.assets.join("virtual/legacy").exists());
    assert!(paths.assets.join("indexes/17.json").exists());

    // 再来一遍应当无事可做——瘦身要收敛。
    assert!(slim::preview(&paths).expect("preview again").is_empty());

    fs::remove_dir_all(root).expect("clean up");
}

#[test]
fn migrating_moves_the_root_and_leaves_a_note_that_resolve_follows() {
    let base = scratch("migrate");
    let old = base.join("old");
    let default = base.join("default");
    let paths = instance(&old, "1.20.1");
    put(&old, "cache/manifest.json", &[0; 8]);

    // 迁去别处：数据整棵过去，默认位置留字条。
    let target = base.join("drive/fern");
    let mut peak = 0u64;
    let landed = migrate_with_default(&paths, &target, &default, &mut |done, _| peak = done)
        .expect("migrate");
    assert_eq!(landed, target);
    assert!(!old.exists());
    assert!(target.join("cache/manifest.json").exists());
    assert!(target.join("instances/moss/instance.json").exists());
    let note = fs::read_to_string(default.join("data-root.txt")).expect("note");
    assert_eq!(note.trim(), target.display().to_string());

    // 迁回默认位置：只剩字条的目录算空，字条随之撤掉。
    let paths = DataPaths::new(&target);
    migrate_with_default(&paths, &default, &default, &mut |_, _| {}).expect("migrate back");
    assert!(default.join("cache/manifest.json").exists());
    assert!(!default.join("data-root.txt").exists());
    assert!(!target.exists());

    fs::remove_dir_all(base).expect("clean up");
}

#[test]
fn migration_refuses_what_would_lose_data() {
    let base = scratch("migrate-no");
    let old = base.join("old");
    let default = base.join("default");
    let paths = instance(&old, "1.20.1");

    // 相对路径、迁进自己、互相包含、非空目标，全都停下。
    let refuse = |destination: &Path| {
        migrate_with_default(&paths, destination, &default, &mut |_, _| {}).expect_err("应当拒绝")
    };
    refuse(Path::new("relative/fern"));
    refuse(&old.join("inside"));
    refuse(&base);
    let taken = base.join("taken");
    fs::create_dir_all(&taken).expect("create");
    fs::write(taken.join("keep.txt"), b"mine").expect("write");
    refuse(&taken);
    assert!(old.exists(), "拒绝之后原目录一动不动");

    fs::remove_dir_all(base).expect("clean up");
}

#[test]
fn a_picked_directory_lands_inside_when_it_is_not_empty() {
    let base = scratch("target");
    // 不存在、空、只剩字条：挑的就是目的地。
    assert_eq!(migration_target(&base.join("absent")), base.join("absent"));
    let empty = base.join("empty");
    fs::create_dir_all(&empty).expect("create");
    assert_eq!(migration_target(&empty), empty);
    let noted = base.join("noted");
    fs::create_dir_all(&noted).expect("create");
    fs::write(noted.join("data-root.txt"), b"/elsewhere").expect("write");
    assert_eq!(migration_target(&noted), noted);
    // 非空：落到里面的 Fern 子目录，谁也不会被清空。
    let busy = base.join("busy");
    fs::create_dir_all(&busy).expect("create");
    fs::write(busy.join("save.zip"), b"mine").expect("write");
    assert_eq!(migration_target(&busy), busy.join("Fern"));

    fs::remove_dir_all(base).expect("clean up");
}

#[test]
fn a_tree_copy_reports_progress_and_verifies_every_file() {
    let base = scratch("copy");
    let from = base.join("from");
    put(&from, "a.bin", &[1; 10]);
    put(&from, "deep/b.bin", &[2; 30]);

    let mut calls = Vec::new();
    copy_tree(&from, &base.join("to"), 40, &mut |done, total| {
        calls.push((done, total))
    })
    .expect("copy");
    assert_eq!(
        fs::read(base.join("to/a.bin")).expect("read a"),
        vec![1; 10]
    );
    assert_eq!(
        fs::read(base.join("to/deep/b.bin")).expect("read b"),
        vec![2; 30]
    );
    assert_eq!(calls.last(), Some(&(40, 40)));

    fs::remove_dir_all(base).expect("clean up");
}

/// 算不清就不删：一份存在却读不出来的版本 JSON 让整个瘦身停下。
#[test]
fn an_unreadable_version_json_stops_the_slim_cold() {
    let root = scratch("abort");
    let paths = instance(&root, "1.20.1");
    put(&root, ".minecraft/versions/1.20.1/1.20.1.json", b"not json");
    put(&root, ".minecraft/versions/1.8.9/1.8.9.jar", &[0; 32]);

    assert!(slim::preview(&paths).is_err());

    fs::remove_dir_all(root).expect("clean up");
}
