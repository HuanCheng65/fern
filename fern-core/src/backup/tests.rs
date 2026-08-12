//! 拍摄与恢复的行为测试。
//!
//! 都建在临时目录上，走的是真的文件系统——这个模块的正确性几乎全在「文件到底
//! 有没有被改动」上，用假的文件系统验不出什么。

use super::*;
use crate::{CoverSeed, InstanceId, InstanceProfile, backup::select::SkipReason};

fn scratch(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("fern-backup-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    root
}

/// 建一个只在磁盘上存在的实例，不联网、不补全。
fn instance(root: &Path) -> DataPaths {
    let paths = DataPaths::new(root);
    let profile = InstanceProfile {
        cover: CoverSeed {
            identity: "moss".to_owned(),
            growth: 0,
        },
        ..InstanceProfile::vanilla(InstanceId::parse("moss").expect("id"), "苔", "1.20.1")
    };
    fs::create_dir_all(paths.instance_root("moss")).expect("create instance");
    crate::write_instance_profile(&paths, &profile).expect("write profile");
    paths
}

fn put(paths: &DataPaths, relative: &str, body: &[u8]) -> PathBuf {
    let path = paths.game_directory("moss").join(relative);
    fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
    fs::write(&path, body).expect("write");
    path
}

fn read(paths: &DataPaths, relative: &str) -> Option<Vec<u8>> {
    fs::read(paths.game_directory("moss").join(relative)).ok()
}

#[test]
fn a_snapshot_records_what_matters_and_says_what_it_skipped() {
    let root = scratch("take");
    let paths = instance(&root);
    put(&paths, "saves/家/level.dat", b"a world");
    put(&paths, "saves/家/region/r.0.0.mca", &[3u8; 128]);
    put(&paths, "config/create.toml", b"answer = 42");
    put(&paths, "mods/create.jar", b"not really a jar");
    put(&paths, "options.txt", b"fov:70");
    put(&paths, "logs/latest.log", b"noise");
    put(&paths, "journeymap/tiles/x", b"cache");

    let snapshot = take(
        &paths,
        "moss",
        Reason::Manual,
        Some("第一张".to_owned()),
        None,
    )
    .expect("take snapshot");
    assert_eq!(snapshot.files, 5);
    assert_eq!(snapshot.mods, 1);
    assert_eq!(snapshot.saves, vec!["家".to_owned()]);
    assert_eq!(snapshot.label.as_deref(), Some("第一张"));
    assert!(!snapshot.inconsistent);

    // 日志一次性，journeymap 只是没被选中——两者对用户的意义不一样，所以
    // 分开说。
    let skipped: Vec<_> = snapshot
        .skipped
        .iter()
        .map(|it| (it.path.as_str(), it.reason))
        .collect();
    assert!(skipped.contains(&("logs/", SkipReason::Transient)));
    assert!(skipped.contains(&("journeymap/", SkipReason::NotSelected)));

    assert_eq!(list(&paths, "moss").expect("list").len(), 1);
    fs::remove_dir_all(root).expect("clean up");
}

/// 这张快照里某个文件引用的对象 id。
fn chunk_of(paths: &DataPaths, snapshot: &str, ends_with: &str) -> String {
    let backups = super::root(paths);
    let manifest =
        manifest::read(&manifest::path(&backups, "moss", snapshot).expect("path")).expect("read");
    manifest
        .files
        .iter()
        .find(|file| file.path.ends_with(ends_with))
        .expect("record")
        .chunks[0]
        .clone()
}

fn set_mtime(path: &Path, seconds: u64) {
    let file = fs::File::options().write(true).open(path).expect("open");
    let at = std::time::UNIX_EPOCH + Duration::from_secs(seconds);
    file.set_times(fs::FileTimes::new().set_modified(at))
        .expect("set mtime");
}

#[test]
fn unchanged_files_are_not_read_again() {
    // 这一条不是优化，是「每次退出都拍」成不成立的分水岭（§4）。
    //
    // 「有没有再读一遍」没法直接观察，所以反过来验：把内容换掉但保持大小和
    // mtime 不变，如果第二张快照仍然引用原来的对象，就说明它确实没有读。
    // 这同时也把 mtime 的理论缺陷摆在明面上——对 Minecraft 的存档不成立，
    // 但「完整校验」那个入口存在的理由就是它。
    let root = scratch("reuse");
    let paths = instance(&root);
    let region = put(&paths, "saves/家/region/r.0.0.mca", &[1u8; 4096]);
    set_mtime(&region, 1_700_000_000);

    let first = take(&paths, "moss", Reason::Manual, None, None).expect("first");
    let original = chunk_of(&paths, &first.id, "r.0.0.mca");

    fs::write(&region, [2u8; 4096]).expect("rewrite");
    set_mtime(&region, 1_700_000_000);
    let second = take(&paths, "moss", Reason::Manual, None, None).expect("second");
    assert_eq!(chunk_of(&paths, &second.id, "r.0.0.mca"), original);

    // mtime 一变就必须重新读，于是换一个 id。
    set_mtime(&region, 1_700_000_600);
    let third = take(&paths, "moss", Reason::Manual, None, None).expect("third");
    assert_ne!(chunk_of(&paths, &third.id, "r.0.0.mca"), original);

    fs::remove_dir_all(root).expect("clean up");
}

#[test]
fn restoring_one_world_leaves_the_rest_alone() {
    let root = scratch("restore-save");
    let paths = instance(&root);
    put(&paths, "saves/家/level.dat", b"yesterday");
    put(&paths, "saves/别处/level.dat", b"untouched");
    put(&paths, "config/create.toml", b"old config");

    let snapshot = take(&paths, "moss", Reason::Manual, None, None).expect("take");

    put(&paths, "saves/家/level.dat", b"today");
    put(&paths, "saves/家/region/new.mca", b"generated today");
    put(&paths, "config/create.toml", b"new config");

    let restored = restore(
        &paths,
        "moss",
        &snapshot.id,
        &Scope::Save("家".to_owned()),
        &Mode::Replace,
    )
    .expect("restore");

    assert_eq!(restored.written, 1);
    assert!(restored.missing.is_empty());
    // 恢复本身也是不可逆操作，所以先替用户拍了一张。
    assert!(restored.safety.is_some());

    assert_eq!(
        read(&paths, "saves/家/level.dat").as_deref(),
        Some(&b"yesterday"[..])
    );
    // 快照之后新生成的 region 留在那里就会得到一个半新半旧的世界。
    assert_eq!(restored.removed, 1);
    assert!(read(&paths, "saves/家/region/new.mca").is_none());
    // 范围之外的东西一个字节都不许动。
    assert_eq!(
        read(&paths, "config/create.toml").as_deref(),
        Some(&b"new config"[..])
    );
    assert_eq!(
        read(&paths, "saves/别处/level.dat").as_deref(),
        Some(&b"untouched"[..])
    );

    fs::remove_dir_all(root).expect("clean up");
}

#[test]
fn a_world_can_be_restored_beside_the_current_one() {
    let root = scratch("restore-copy");
    let paths = instance(&root);
    put(&paths, "saves/家/level.dat", b"yesterday");
    let snapshot = take(&paths, "moss", Reason::Manual, None, None).expect("take");
    put(&paths, "saves/家/level.dat", b"today");

    let restored = restore(
        &paths,
        "moss",
        &snapshot.id,
        &Scope::Save("家".to_owned()),
        &Mode::Copy("家 (昨天)".to_owned()),
    )
    .expect("restore");
    assert_eq!(restored.written, 1);
    assert_eq!(restored.removed, 0);
    assert_eq!(
        read(&paths, "saves/家 (昨天)/level.dat").as_deref(),
        Some(&b"yesterday"[..])
    );
    // 「我就想看看昨天的基地长什么样」——原来那个原封不动。
    assert_eq!(
        read(&paths, "saves/家/level.dat").as_deref(),
        Some(&b"today"[..])
    );

    // 名字撞了要说清楚，不能悄悄覆盖。
    assert!(
        restore(
            &paths,
            "moss",
            &snapshot.id,
            &Scope::Save("家".to_owned()),
            &Mode::Copy("家 (昨天)".to_owned()),
        )
        .is_err()
    );
    // 目录名不安全的一律拒绝。
    assert!(
        restore(
            &paths,
            "moss",
            &snapshot.id,
            &Scope::Save("家".to_owned()),
            &Mode::Copy("../../逃".to_owned()),
        )
        .is_err()
    );

    fs::remove_dir_all(root).expect("clean up");
}

#[test]
fn a_missing_object_is_reported_and_the_rest_still_lands() {
    // 半途停下只会留下一个更说不清的状态（§9）。
    let root = scratch("missing");
    let paths = instance(&root);
    put(&paths, "config/a.toml", b"first");
    put(&paths, "config/b.toml", b"second");
    let snapshot = take(&paths, "moss", Reason::Manual, None, None).expect("take");

    let backups = super::root(&paths);
    let manifest = manifest::read(&manifest::path(&backups, "moss", &snapshot.id).expect("path"))
        .expect("read");
    let victim = manifest
        .files
        .iter()
        .find(|file| file.path.ends_with("a.toml"))
        .expect("record");
    // 把仓库里那一份挖掉，模拟仓库损坏。
    let store = store::Store::at(&backups);
    assert!(store.has(&victim.chunks[0]));
    for bucket in fs::read_dir(backups.join("objects"))
        .expect("objects")
        .flatten()
    {
        for object in fs::read_dir(bucket.path()).into_iter().flatten().flatten() {
            let name = object.file_name().to_string_lossy().into_owned();
            if victim.chunks[0].ends_with(name.trim_end_matches(".z")) {
                fs::remove_file(object.path()).expect("remove object");
            }
        }
    }

    put(&paths, "config/a.toml", b"changed");
    put(&paths, "config/b.toml", b"changed");
    let restored =
        restore(&paths, "moss", &snapshot.id, &Scope::Config, &Mode::Replace).expect("restore");

    assert_eq!(restored.missing.len(), 1);
    assert!(restored.missing[0].path.ends_with("a.toml"));
    // 缺的那一个原封不动，别的照常恢复。
    assert_eq!(
        read(&paths, "config/a.toml").as_deref(),
        Some(&b"changed"[..])
    );
    assert_eq!(
        read(&paths, "config/b.toml").as_deref(),
        Some(&b"second"[..])
    );

    fs::remove_dir_all(root).expect("clean up");
}

#[test]
fn deleting_a_snapshot_frees_only_what_nothing_else_uses() {
    let root = scratch("gc");
    let paths = instance(&root);
    put(&paths, "config/shared.toml", b"in both snapshots");
    put(&paths, "config/only-first.toml", b"about to be deleted");
    let first = take(&paths, "moss", Reason::Manual, None, None).expect("first");

    fs::remove_file(paths.game_directory("moss").join("config/only-first.toml")).expect("remove");
    take(&paths, "moss", Reason::Manual, None, None).expect("second");

    remove(&paths, "moss", &first.id).expect("remove snapshot");
    // 宽限期挡着新对象，所以这一次什么都收不掉——这是对的：正在写入的那一批
    // 还没有清单引用它们。
    let swept = collect_garbage(&paths).expect("gc");
    assert_eq!(swept.objects, 0);

    let backups = super::root(&paths);
    let live = manifest::live_objects(&backups);
    let swept = store::Store::at(&backups)
        .sweep(&live, Duration::ZERO)
        .expect("sweep");
    assert_eq!(swept.objects, 1);
    // 两张快照共用的那一份还在。
    assert_eq!(list(&paths, "moss").expect("list").len(), 1);

    fs::remove_dir_all(root).expect("clean up");
}

#[test]
fn usage_counts_shared_content_once() {
    let root = scratch("usage");
    let paths = instance(&root);
    put(&paths, "mods/create.jar", &[9u8; 2048]);
    put(&paths, "config/a.toml", b"x");
    take(&paths, "moss", Reason::Manual, None, None).expect("take");
    take(&paths, "moss", Reason::Manual, None, None).expect("take again");

    let usage = usage(&paths).expect("usage");
    assert_eq!(usage.snapshots, 2);
    assert_eq!(usage.instances.len(), 1);
    assert_eq!(usage.instances[0].instance, "moss");
    // 一个 jar 只存一次，两张快照引用同一份——去重之后各快照大小之和是个
    // 没有意义的数（§7）。
    assert!(usage.mods_bytes > 0);
    assert!(usage.bytes >= usage.mods_bytes);
    assert_eq!(usage.instances[0].reclaimable, usage.bytes);

    fs::remove_dir_all(root).expect("clean up");
}

#[test]
fn taking_a_snapshot_while_the_game_runs_is_refused() {
    // 硬规则：region 正在写，拍到的是半个文件，而一张坏快照比没有快照更糟。
    let root = scratch("running");
    let paths = instance(&root);
    put(&paths, "config/a.toml", b"x");
    let directory = paths.game_directory("moss");

    let guard = running::testing::occupy("moss", &directory);
    assert!(take(&paths, "moss", Reason::Manual, None, None).is_err());
    drop(guard);

    assert!(take(&paths, "moss", Reason::Manual, None, None).is_ok());
    fs::remove_dir_all(root).expect("clean up");
}

#[test]
fn repeated_mod_changes_share_one_snapshot() {
    // 连着改二十个模组不该拍二十张快照。
    let root = scratch("trigger");
    let paths = instance(&root);
    put(&paths, "mods/a.jar", b"one");

    before_mod_change(&paths, "moss", None);
    before_mod_change(&paths, "moss", None);
    before_mod_change(&paths, "moss", None);
    assert_eq!(list(&paths, "moss").expect("list").len(), 1);

    fs::remove_dir_all(root).expect("clean up");
}

/// 界面和后端之间那份约定。
///
/// 这几个类型是**跨语言**的：Rust 这边改一个字段名，TypeScript 那边不会有任何
/// 编译错误，只会在运行时得到 `undefined`。`fern-ui/src-tauri` 在多数开发机上
/// 编译不了（要 WebKitGTK），所以这一条测试是这份约定唯一的守卫。
#[test]
fn the_interface_sees_the_field_names_it_expects() {
    let scope = |value: &str| serde_json::from_str::<Scope>(value).expect(value);
    assert_eq!(scope(r#"{"kind":"all"}"#), Scope::All);
    assert_eq!(scope(r#"{"kind":"config"}"#), Scope::Config);
    assert_eq!(scope(r#"{"kind":"mods"}"#), Scope::Mods);
    assert_eq!(
        scope(r#"{"kind":"save","name":"家"}"#),
        Scope::Save("家".to_owned())
    );

    let mode = |value: &str| serde_json::from_str::<Mode>(value).expect(value);
    assert_eq!(mode(r#"{"kind":"replace"}"#), Mode::Replace);
    assert_eq!(
        mode(r#"{"kind":"copy","name":"家 (2026-08-07)"}"#),
        Mode::Copy("家 (2026-08-07)".to_owned())
    );

    let snapshot = Snapshot {
        id: "1786152000".to_owned(),
        instance: "moss".to_owned(),
        taken_at: 1_786_152_000,
        reason: Reason::BeforeModChange.tag().to_owned(),
        about: Some(manifest::About::new("install").with("name", "Create")),
        label: None,
        files: 3,
        bytes: 4096,
        mods: 1,
        saves: vec!["家".to_owned()],
        minecraft: "1.20.1".to_owned(),
        loader: "neoforge".to_owned(),
        loader_version: Some("21.1.77".to_owned()),
        inconsistent: false,
        skipped: vec![Skipped {
            path: "logs/".to_owned(),
            reason: SkipReason::Transient,
        }],
    };
    let json = serde_json::to_value(&snapshot).expect("serialize");
    // `label` 是 None 时不出现，界面上那一行因此走 `?? 默认` 而不是显示 null。
    assert!(json.get("label").is_none());
    assert_eq!(json["takenAt"], 1_786_152_000_u64);
    assert_eq!(json["reason"], "before-mod-change");
    assert_eq!(json["loaderVersion"], "21.1.77");
    // 事件上下文的形状：界面按 snapshot.about.<id> 查文案表。
    assert_eq!(json["about"]["id"], "install");
    assert_eq!(json["about"]["params"]["name"], "Create");
    assert_eq!(json["skipped"][0]["reason"], "transient");

    let restored = serde_json::to_value(Restored {
        written: 2,
        bytes: 10,
        removed: 1,
        missing: vec![Missing {
            path: "mods/create.jar".to_owned(),
            sha1: Some("abc".to_owned()),
        }],
        safety: Some("1786152000".to_owned()),
    })
    .expect("serialize");
    assert_eq!(restored["missing"][0]["path"], "mods/create.jar");
    assert_eq!(restored["safety"], "1786152000");

    let usage = serde_json::to_value(Usage {
        bytes: 100,
        mods_bytes: 40,
        snapshots: 2,
        instances: vec![InstanceUsage {
            instance: "moss".to_owned(),
            snapshots: 2,
            reclaimable: 60,
        }],
    })
    .expect("serialize");
    assert_eq!(usage["modsBytes"], 40);
    assert_eq!(usage["instances"][0]["reclaimable"], 60);

    let contents: export::Contents = serde_json::from_str(
        r#"{"saves":["家"],"mods":false,"config":true,"resourcepacks":true,"shaderpacks":false,"schematics":false,"screenshots":false}"#,
    )
    .expect("contents");
    assert_eq!(contents.saves, vec!["家".to_owned()]);
    assert!(!contents.mods && contents.config);
    let exported = serde_json::to_value(export::Exported {
        path: PathBuf::from("/tmp/a.mrpack"),
        bytes: 5,
        files: 2,
        linked: Some(1),
    })
    .expect("serialize");
    assert_eq!(exported["linked"], 1);

    let diff = serde_json::to_value(Diff {
        mods_added: vec!["sodium.jar".to_owned()],
        config_changed: 2,
        ..Diff::default()
    })
    .expect("serialize");
    assert_eq!(diff["modsAdded"][0], "sodium.jar");
    assert_eq!(diff["configChanged"], 2);
    assert_eq!(diff["savesChanged"], serde_json::json!([]));
}

/// 差异要指名道姓——「会删除此后新装的 3 个」的那个「哪 3 个」从这里来。
#[test]
fn the_diff_names_what_changed_since_the_snapshot() {
    let root = scratch("diff");
    let paths = instance(&root);
    put(&paths, "saves/家/level.dat", b"a world");
    put(&paths, "saves/矿场/level.dat", b"a mine");
    put(&paths, "mods/create.jar", b"jar");
    put(&paths, "mods/flywheel.jar", b"jar too");
    put(&paths, "config/create.toml", b"answer = 42");

    let snapshot = take(&paths, "moss", Reason::Manual, None, None).expect("take");
    assert!(diff(&paths, "moss", &snapshot.id).expect("diff").is_same());

    // 此后：装一个模组、删一个，改一个世界、删一个、建一个，动一个配置。
    // 改动都伴随大小变化——同一秒内 mtime 分不出先后，大小分得出。
    put(&paths, "mods/sodium.jar", b"new jar");
    fs::remove_file(paths.game_directory("moss").join("mods/flywheel.jar")).expect("remove mod");
    put(&paths, "saves/家/level.dat", b"a bigger world");
    fs::remove_dir_all(paths.game_directory("moss").join("saves/矿场")).expect("remove world");
    put(&paths, "saves/新家/level.dat", b"fresh");
    put(&paths, "config/create.toml", b"answer = 43!");

    let changes = diff(&paths, "moss", &snapshot.id).expect("diff");
    assert_eq!(changes.mods_added, vec!["sodium.jar"]);
    assert_eq!(changes.mods_removed, vec!["flywheel.jar"]);
    assert_eq!(changes.saves_added, vec!["新家"]);
    assert_eq!(changes.saves_removed, vec!["矿场"]);
    assert_eq!(changes.saves_changed, vec!["家"]);
    assert_eq!(changes.config_changed, 1);
    assert!(!changes.is_same());
    fs::remove_dir_all(root).expect("clean up");
}

/// 保留策略要真的被执行。此前 `schedule` 写完测完却没有调用点，快照只增不减。
#[test]
fn an_automatic_snapshot_prunes_the_expired_ones_behind_it() {
    let root = scratch("prune-hook");
    let paths = instance(&root);
    put(&paths, "saves/家/level.dat", b"a world");

    // 两张旧快照，倒填到三十多天前的同一个月里：按月每桶留一张，旧的那张
    // 该被剪掉。
    let backups = super::root(&paths);
    let mut backdated = Vec::new();
    for (offset, body) in [(40u64, b"day one"), (39, b"day two")] {
        put(&paths, "saves/家/level.dat", body);
        let snapshot = take(&paths, "moss", Reason::AfterSession, None, None).expect("take");
        let path = manifest::path(&backups, "moss", &snapshot.id).expect("path");
        let mut manifest = manifest::read(&path).expect("read");
        manifest.taken_at = now() - offset * 86_400;
        manifest::write(&path, &manifest).expect("backdate");
        // 文件名就是 id，也要跟着时间走，否则新旧排序对不上。
        let renamed =
            manifest::path(&backups, "moss", &manifest.taken_at.to_string()).expect("renamed path");
        fs::rename(&path, &renamed).expect("rename");
        backdated.push(manifest.taken_at.to_string());
    }

    // 事件驱动的那一张：拍完顺手清理。
    put(&paths, "saves/家/level.dat", b"today");
    quietly(&paths, "moss", Reason::AfterSession);

    let remaining: Vec<String> = manifest::ids(&backups, "moss");
    assert!(
        !remaining.contains(&backdated[0]),
        "旧的那张该被剪掉：{remaining:?}"
    );
    assert!(
        remaining.contains(&backdated[1]),
        "每个月桶最新的那张要留下：{remaining:?}"
    );
    fs::remove_dir_all(root).expect("clean up");
}

/// 删掉实例要连它的快照一起清，孤儿快照没有恢复对象，只是占盘。
#[test]
fn deleting_an_instance_forgets_its_snapshots() {
    let root = scratch("forget");
    let paths = instance(&root);
    put(&paths, "saves/家/level.dat", b"a world");
    take(
        &paths,
        "moss",
        Reason::Manual,
        Some("留念".to_owned()),
        None,
    )
    .expect("take");
    assert_eq!(usage(&paths).expect("usage").instances.len(), 1);

    crate::delete_instance(&paths, "moss").expect("delete instance");

    assert!(list(&paths, "moss").expect("list").is_empty());
    let after = usage(&paths).expect("usage");
    assert!(
        after.instances.is_empty(),
        "用量页不该再列出已删除的实例：{:?}",
        after.instances
    );
    fs::remove_dir_all(root).expect("clean up");
}

/// 上限是一把和保留策略不同的尺子：那把量时间，这把量磁盘。
///
/// 走真的文件系统而不只是 `schedule::over_limit` 的单测——这个函数的活是把
/// 磁盘上的事实（仓库多大、每张引用了哪些对象、每个对象多大）喂给那套算法，
/// 而喂错了单测一个都发现不了。
#[test]
fn the_size_limit_cuts_from_the_oldest_and_spares_the_manual_one() {
    let root = scratch("limit");
    let paths = instance(&root);

    // 每一张都带一份只属于它自己的内容，这样每剪一张都真的能腾出空间。
    put(&paths, "saves/家/level.dat", &[1u8; 64 * 1024]);
    let manual = take(&paths, "moss", Reason::Manual, None, None).expect("manual");
    put(&paths, "saves/家/level.dat", &[2u8; 64 * 1024]);
    let older = take(&paths, "moss", Reason::BeforeLaunch, None, None).expect("older");
    put(&paths, "saves/家/level.dat", &[3u8; 64 * 1024]);
    let newer = take(&paths, "moss", Reason::BeforeLaunch, None, None).expect("newer");

    let before = usage(&paths).expect("usage").bytes;
    assert!(before > 0, "仓库应该有内容");

    // 上限设成当前的一半：够不着的时候能剪的都剪，手动那张不动。
    let removed = enforce_limit(&paths, before / 2).expect("enforce");
    assert!(!removed.is_empty(), "超了上限却一张都没剪");
    assert!(
        !removed.contains(&manual.id),
        "手动拍的那张永远不剪，哪怕仍然超着"
    );
    // 从最旧的开始：`older` 比 `newer` 先走。
    assert_eq!(removed.first(), Some(&older.id));

    let left = list(&paths, "moss").expect("list");
    assert!(left.iter().any(|snapshot| snapshot.id == manual.id));
    assert!(
        left.iter().any(|snapshot| snapshot.id == newer.id)
            || removed.contains(&newer.id),
        "剩下的要么还在，要么被记在删除清单里"
    );

    // 没超的时候一张都不动，而且不必把每份清单都读一遍。
    assert!(
        enforce_limit(&paths, u64::MAX)
            .expect("enforce")
            .is_empty()
    );

    fs::remove_dir_all(root).expect("clean up");
}
