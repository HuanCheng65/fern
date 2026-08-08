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

    let snapshot =
        take(&paths, "moss", Reason::Manual, Some("第一张".to_owned())).expect("take snapshot");
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

    let first = take(&paths, "moss", Reason::Manual, None).expect("first");
    let original = chunk_of(&paths, &first.id, "r.0.0.mca");

    fs::write(&region, [2u8; 4096]).expect("rewrite");
    set_mtime(&region, 1_700_000_000);
    let second = take(&paths, "moss", Reason::Manual, None).expect("second");
    assert_eq!(chunk_of(&paths, &second.id, "r.0.0.mca"), original);

    // mtime 一变就必须重新读，于是换一个 id。
    set_mtime(&region, 1_700_000_600);
    let third = take(&paths, "moss", Reason::Manual, None).expect("third");
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

    let snapshot = take(&paths, "moss", Reason::Manual, None).expect("take");

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
    let snapshot = take(&paths, "moss", Reason::Manual, None).expect("take");
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
    let snapshot = take(&paths, "moss", Reason::Manual, None).expect("take");

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
    let first = take(&paths, "moss", Reason::Manual, None).expect("first");

    fs::remove_file(paths.game_directory("moss").join("config/only-first.toml")).expect("remove");
    take(&paths, "moss", Reason::Manual, None).expect("second");

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
    take(&paths, "moss", Reason::Manual, None).expect("take");
    take(&paths, "moss", Reason::Manual, None).expect("take again");

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
    assert!(take(&paths, "moss", Reason::Manual, None).is_err());
    drop(guard);

    assert!(take(&paths, "moss", Reason::Manual, None).is_ok());
    fs::remove_dir_all(root).expect("clean up");
}

#[test]
fn repeated_mod_changes_share_one_snapshot() {
    // 连着改二十个模组不该拍二十张快照。
    let root = scratch("trigger");
    let paths = instance(&root);
    put(&paths, "mods/a.jar", b"one");

    before_mod_change(&paths, "moss");
    before_mod_change(&paths, "moss");
    before_mod_change(&paths, "moss");
    assert_eq!(list(&paths, "moss").expect("list").len(), 1);

    fs::remove_dir_all(root).expect("clean up");
}
