//! 快照：拍下一个实例现在的样子，以及把它拍回去。
//!
//! 设计与取舍在 [docs/fern-backup-design.md](../../../docs/fern-backup-design.md)。
//! 一句话的准绳写在那份文档最前面：**备份要么能真的恢复，要么就是一个假承诺。**
//! 这个模块里每一处「宁可报错」都是从那句话来的。
//!
//! ```text
//! backups/objects/…              内容，全局去重（store.rs）
//! backups/snapshots/<实例>/<时刻>.json.gz   清单（manifest.rs）
//! ```
//!
//! 三条不变量：
//!
//! - **游戏跑着的时候绝不拍。** region 文件正在写，拍到的是半个文件，而一张
//!   坏快照比没有快照更糟。
//! - **`(路径, 大小, 修改时刻)` 没变就复用上一份的哈希，不读内容。** 第一次
//!   拍 5 GB 的世界要读完，之后只读真正变过的那几个 region。这不是优化项，
//!   是「每次退出都拍」成不成立的分水岭（§4）。
//! - **恢复之前先拍一张。** 否则恢复本身就是一次不可逆操作。

pub(crate) mod export;
pub(crate) mod manifest;
pub(crate) mod schedule;
pub(crate) mod select;
pub(crate) mod store;

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::{
    DataPaths, InstanceProfile,
    backup::{
        manifest::{About, FileRecord, GameStamp, Manifest, ModRecord, Reason},
        select::Skipped,
        store::Store,
    },
    launch::running,
};

/// 刚建出来还没被任何清单引用的对象，不许回收。见 [`store::Store::sweep`]。
const ORPHAN_GRACE: Duration = Duration::from_secs(3600);

/// 连着改二十个模组不该拍二十张快照。这段时间内已经有一张「改模组之前」的，
/// 就沿用它——它记的正是这一串操作之前的状态。
const MOD_CHANGE_GRACE: u64 = 10 * 60;

/// 启动前那一张的间隔。兜住「上次退出时崩了没拍成」，不是每次启动都拍。
const LAUNCH_INTERVAL: u64 = 6 * 3600;

/// 拍完之后复查 mtime。变过的重拍一次，仍然在变就标记为可能不一致。
const RECHECK: usize = 1;

/// 界面看到的一张快照。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub id: String,
    pub instance: String,
    pub taken_at: u64,
    /// 文案 id 的后半段，`manual`、`before-mod-change` 之类。句子在界面里。
    pub reason: String,
    /// 触发它的那件事（`snapshot.about.<id>` 加参数）。reason 是类别，这个
    /// 才是身份——「装 Create 之前」。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub about: Option<About>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub files: usize,
    /// 备份的内容一共多大，按原文件算。不是它在仓库里实际占的——去重之后
    /// 各快照大小之和是个没有意义的数（§7）。
    pub bytes: u64,
    pub mods: usize,
    /// 这张快照里有哪些世界。恢复时要按它列出可选项。
    pub saves: Vec<String>,
    pub minecraft: String,
    pub loader: String,
    /// 拍摄时加载器的版本。「更新加载器之后世界打不开」正是快照要接住的
    /// 场景之一，这个数字是「回到哪一刻」的一半答案。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loader_version: Option<String>,
    /// 拍的时候文件还在变。恢复它之前用户有权知道这件事。
    pub inconsistent: bool,
    pub skipped: Vec<Skipped>,
}

/// 恢复哪一部分。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "name", rename_all = "camelCase")]
pub enum Scope {
    /// 整张快照。
    All,
    /// 一个世界。用户要的通常是这一个。
    Save(String),
    /// `config/` 加游戏目录根下那几个文件。
    Config,
    /// `mods/` 下的 jar。「改模组之后世界打不开」时最直接的一步。
    Mods,
}

/// 覆盖回去，还是另存一份。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "name", rename_all = "camelCase")]
pub enum Mode {
    /// 恢复到原来的位置。范围之内、快照里没有的文件会被删掉——留着它们
    /// 会得到一个半新半旧的世界，而那正是这次恢复要摆脱的东西。
    Replace,
    /// 恢复成一个新的世界，原来那个原封不动。名字由界面给（`家 (2026-08-07)`）。
    Copy(String),
}

/// 一次恢复的结果。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Restored {
    pub written: usize,
    pub bytes: u64,
    /// 范围之内、快照里没有的，删掉了几个。
    pub removed: usize,
    /// 内容在仓库里找不到或者校验不过的。
    ///
    /// 这些文件**没有被改动**，恢复的其余部分照常完成——半途停下只会留下一个
    /// 更说不清的状态（§9）。
    pub missing: Vec<Missing>,
    /// 恢复之前替用户拍的那一张。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safety: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Missing {
    pub path: String,
    /// 模组文件才有。拿它去模组站反查就能把 jar 重新下回来。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha1: Option<String>,
}

/// 磁盘占用。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    /// 对象仓库实际占多少磁盘。去重、压缩之后的真实数字。
    pub bytes: u64,
    /// 其中模组文件占多少。整合包用户最想知道的就是这一项。
    pub mods_bytes: u64,
    pub snapshots: usize,
    pub instances: Vec<InstanceUsage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceUsage {
    pub instance: String,
    pub snapshots: usize,
    /// 删掉这个实例的全部快照能收回多少。
    ///
    /// **只算没有别的实例引用的那些对象**——共享的部分删了也不会消失，把它
    /// 算进来就是给用户一个兑现不了的数。
    pub reclaimable: u64,
}

/// `backups/` 在哪。始终在 Fern 自己的数据根下，外部实例也一样——快照读的是
/// 别人的游戏目录，写的是我们自己的地盘。
pub fn root(paths: &DataPaths) -> PathBuf {
    paths.root.join("backups")
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// 这个实例的游戏目录在哪，以及它现在是什么版本。
fn situate(paths: &DataPaths, instance_id: &str) -> Result<(InstanceProfile, PathBuf)> {
    let profile = crate::read_instance(paths, instance_id)?;
    let scoped = crate::instance::paths_for(paths, &profile);
    let directory = scoped.game_directory(profile.id.as_str());
    Ok((profile, directory))
}

/// 界面上要说的是实例的名字，不是它的 id——id 是给磁盘看的。
fn display_name(paths: &DataPaths, instance_id: &str) -> String {
    crate::read_instance(paths, instance_id)
        .map(|profile| profile.name)
        .unwrap_or_else(|_| instance_id.to_owned())
}

fn stamp(profile: &InstanceProfile) -> GameStamp {
    GameStamp {
        minecraft: profile.game_version.clone(),
        loader: profile.loader,
        loader_version: profile
            .loader_component()
            .map(|loader| loader.version.clone()),
    }
}

/// 拍一张。
///
/// 游戏跑着的时候直接拒绝——这是硬规则，不是可以绕过的检查。
pub fn take(
    paths: &DataPaths,
    instance_id: &str,
    reason: Reason,
    label: Option<String>,
    about: Option<About>,
) -> Result<Snapshot> {
    let (profile, directory) = situate(paths, instance_id)?;
    if let Some(occupant) = running::occupant(&directory) {
        return Err(anyhow!(
            "{} 正在使用这个游戏目录，游戏运行时拍下的存档不完整",
            display_name(paths, &occupant)
        ));
    }

    let backups = root(paths);
    let store = Store::at(&backups);
    let (candidates, skipped) = select::scan(&directory);

    // 上一张的哈希能省掉几乎全部的读盘。见模块头第二条不变量。
    let previous = latest_manifest(&backups, instance_id);
    let mut known: HashMap<&str, &FileRecord> = HashMap::new();
    let mut hashed: HashMap<&str, &str> = HashMap::new();
    if let Some(manifest) = previous.as_ref() {
        known = manifest
            .files
            .iter()
            .map(|file| (file.path.as_str(), file))
            .collect();
        hashed = manifest
            .mods
            .iter()
            .map(|record| (record.file.as_str(), record.sha1.as_str()))
            .collect();
    }

    let mut files = Vec::with_capacity(candidates.len());
    let mut mods = Vec::new();
    for candidate in &candidates {
        let reused = known
            .get(candidate.relative.as_str())
            .filter(|record| record.size == candidate.size && record.mtime == candidate.mtime);

        let record = match reused {
            Some(record) => FileRecord {
                path: candidate.relative.clone(),
                size: candidate.size,
                mtime: candidate.mtime,
                chunks: record.chunks.clone(),
            },
            None => {
                let stored = store.put(&candidate.absolute)?;
                FileRecord {
                    path: candidate.relative.clone(),
                    size: stored.bytes,
                    mtime: candidate.mtime,
                    chunks: vec![stored.id],
                }
            }
        };

        if let Some(file) = candidate
            .relative
            .strip_prefix("mods/")
            .filter(|name| select::is_mod(&candidate.relative) && !name.contains('/'))
        {
            let sha1 = match reused.and(hashed.get(file)) {
                Some(sha1) => (*sha1).to_owned(),
                None => sha1_of(&candidate.absolute)?,
            };
            mods.push(ModRecord {
                file: file.to_owned(),
                sha1,
            });
        }
        files.push(record);
    }

    // 拍完复查：变过的重拍一次，仍然在变就照实标记，而不是假装它没事。
    let mut inconsistent = false;
    for _ in 0..=RECHECK {
        let moved = restamp(&mut files, &directory, &store)?;
        inconsistent = !moved.is_empty();
        if !inconsistent {
            break;
        }
    }

    let taken_at = now();
    let id = allot(&backups, instance_id, taken_at)?;
    let manifest = Manifest {
        version: manifest::FORMAT,
        instance: instance_id.to_owned(),
        game_directory: directory,
        taken_at,
        reason,
        about,
        label,
        game: stamp(&profile),
        mods,
        files,
        skipped,
        inconsistent,
    };
    manifest::write(&manifest::path(&backups, instance_id, &id)?, &manifest)?;
    Ok(describe(&id, &manifest))
}

/// 复查一遍 mtime，变过的重新入库。返回变过的那些路径。
fn restamp(files: &mut [FileRecord], directory: &Path, store: &Store) -> Result<Vec<String>> {
    let mut moved = Vec::new();
    for file in files.iter_mut() {
        let absolute = directory.join(&file.path);
        let Ok(metadata) = fs::metadata(&absolute) else {
            continue;
        };
        let mtime = metadata
            .modified()
            .ok()
            .and_then(|at| at.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |since| since.as_secs());
        if metadata.len() == file.size && mtime == file.mtime {
            continue;
        }
        let stored = store.put(&absolute)?;
        file.size = stored.bytes;
        file.mtime = mtime;
        file.chunks = vec![stored.id];
        moved.push(file.path.clone());
    }
    Ok(moved)
}

/// 同一秒里拍第二张也要有自己的名字。
fn allot(backups: &Path, instance: &str, taken_at: u64) -> Result<String> {
    let base = taken_at.to_string();
    for suffix in 0..1000 {
        let id = if suffix == 0 {
            base.clone()
        } else {
            format!("{base}-{suffix}")
        };
        if !manifest::path(backups, instance, &id)?.exists() {
            return Ok(id);
        }
    }
    Err(anyhow!("一秒之内拍下的快照过多"))
}

fn latest_manifest(backups: &Path, instance: &str) -> Option<Manifest> {
    manifest::ids(backups, instance)
        .into_iter()
        .find_map(|id| manifest::read(&manifest::path(backups, instance, &id).ok()?).ok())
}

fn describe(id: &str, manifest: &Manifest) -> Snapshot {
    let mut saves: Vec<String> = manifest
        .files
        .iter()
        .filter_map(|file| select::save_of(&file.path))
        .map(str::to_owned)
        .collect();
    saves.sort();
    saves.dedup();

    Snapshot {
        id: id.to_owned(),
        instance: manifest.instance.clone(),
        taken_at: manifest.taken_at,
        reason: manifest.reason.tag().to_owned(),
        about: manifest.about.clone(),
        label: manifest.label.clone(),
        files: manifest.files.len(),
        bytes: manifest.bytes(),
        mods: manifest.mods.len(),
        saves,
        minecraft: manifest.game.minecraft.clone(),
        loader: format!("{:?}", manifest.game.loader).to_lowercase(),
        loader_version: manifest.game.loader_version.clone(),
        inconsistent: manifest.inconsistent,
        skipped: manifest.skipped.clone(),
    }
}

/// 从那一刻到现在，这个实例变了什么。
///
/// 详情页靠它把「恢复」的后果说具体：「会删除此后新装的 3 个模组：sodium、
/// lithium、iris」比「之后新装的会被删除」多回答了一个「是哪几个」——而那
/// 正是决定敢不敢按下去时要知道的事。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diff {
    /// 拍摄之后新装的模组。恢复模组时它们会被删除。
    pub mods_added: Vec<String>,
    /// 拍摄之后移除的模组。恢复模组时它们会被带回。
    pub mods_removed: Vec<String>,
    /// 拍摄之后新建的世界。覆盖恢复整个实例时它们会被删除。
    pub saves_added: Vec<String>,
    /// 拍摄之后删除的世界。恢复会把它们带回来——这是零损失的那种恢复。
    pub saves_removed: Vec<String>,
    /// 两边都有、内容有出入的世界。
    pub saves_changed: Vec<String>,
    /// 有几个配置文件与快照不同。
    pub config_changed: usize,
}

impl Diff {
    /// 拍摄以来什么都没变。
    pub fn is_same(&self) -> bool {
        self.mods_added.is_empty()
            && self.mods_removed.is_empty()
            && self.saves_added.is_empty()
            && self.saves_removed.is_empty()
            && self.saves_changed.is_empty()
            && self.config_changed == 0
    }
}

/// 比较一张快照和现在的游戏目录。
///
/// 判据与拍摄时的复用判据相同：`(大小, 修改时刻)`。它可能把「改动后又改回
/// 原样」也算作有改动，但不会把真改过的说成没改——对一句要提醒丢失的话，
/// 错这个方向是安全的。
pub fn diff(paths: &DataPaths, instance_id: &str, snapshot: &str) -> Result<Diff> {
    let (_, directory) = situate(paths, instance_id)?;
    let backups = root(paths);
    let manifest = manifest::read(&manifest::path(&backups, instance_id, snapshot)?)?;
    let (candidates, _) = select::scan(&directory);

    let then: HashMap<&str, (u64, u64)> = manifest
        .files
        .iter()
        .map(|file| (file.path.as_str(), (file.size, file.mtime)))
        .collect();
    let now: HashMap<&str, (u64, u64)> = candidates
        .iter()
        .map(|candidate| {
            (
                candidate.relative.as_str(),
                (candidate.size, candidate.mtime),
            )
        })
        .collect();

    let mods = |side: &HashMap<&str, (u64, u64)>| -> HashSet<String> {
        side.keys()
            .filter(|path| select::is_mod(path))
            .filter_map(|path| path.strip_prefix("mods/"))
            .map(str::to_owned)
            .collect()
    };
    let worlds = |side: &HashMap<&str, (u64, u64)>| -> HashSet<String> {
        side.keys()
            .filter_map(|path| select::save_of(path))
            .map(str::to_owned)
            .collect()
    };
    let mods_then = mods(&then);
    let mods_now = mods(&now);
    let worlds_then = worlds(&then);
    let worlds_now = worlds(&now);

    // 两边都有的世界，逐文件比过才知道有没有变。
    let mut changed: HashSet<&str> = HashSet::new();
    let mut config_changed: HashSet<&str> = HashSet::new();
    for path in then.keys().chain(now.keys()) {
        if then.get(path) == now.get(path) {
            continue;
        }
        if let Some(world) = select::save_of(path) {
            changed.insert(world);
        } else if select::is_config(path) {
            config_changed.insert(path);
        }
    }

    let sorted = |set: HashSet<String>| -> Vec<String> {
        let mut names: Vec<String> = set.into_iter().collect();
        names.sort();
        names
    };
    Ok(Diff {
        mods_added: sorted(&mods_now - &mods_then),
        mods_removed: sorted(&mods_then - &mods_now),
        saves_added: sorted(&worlds_now - &worlds_then),
        saves_removed: sorted(&worlds_then - &worlds_now),
        saves_changed: sorted(
            changed
                .into_iter()
                .filter(|world| worlds_then.contains(*world) && worlds_now.contains(*world))
                .map(str::to_owned)
                .collect(),
        ),
        config_changed: config_changed.len(),
    })
}

/// 一张快照里的模组文件名单。
///
/// 详情页展开「模组」那一节时才要——不随列表下发：几十张快照各带几百个
/// 文件名，列表就白白重了一个数量级。
pub fn mod_files(paths: &DataPaths, instance_id: &str, snapshot: &str) -> Result<Vec<String>> {
    let backups = root(paths);
    let manifest = manifest::read(&manifest::path(&backups, instance_id, snapshot)?)?;
    Ok(manifest
        .mods
        .into_iter()
        .map(|record| record.file)
        .collect())
}

/// 这个实例有哪些快照，从新到旧。
pub fn list(paths: &DataPaths, instance_id: &str) -> Result<Vec<Snapshot>> {
    let backups = root(paths);
    let mut snapshots = Vec::new();
    for id in manifest::ids(&backups, instance_id) {
        let path = manifest::path(&backups, instance_id, &id)?;
        // 读不了的一份不该让整张列表消失——那正是用户要来找东西的时刻。
        if let Ok(manifest) = manifest::read(&path) {
            snapshots.push(describe(&id, &manifest));
        }
    }
    Ok(snapshots)
}

/// 删掉一张快照的清单。对象由 [`collect_garbage`] 回收。
pub fn remove(paths: &DataPaths, instance_id: &str, snapshot: &str) -> Result<()> {
    let path = manifest::path(&root(paths), instance_id, snapshot)?;
    fs::remove_file(&path).with_context(|| format!("删除 {}", path.display()))
}

/// 给一张快照贴标签。贴过标签的永久保留（§7）。
pub fn label(
    paths: &DataPaths,
    instance_id: &str,
    snapshot: &str,
    label: Option<String>,
) -> Result<Snapshot> {
    let backups = root(paths);
    let path = manifest::path(&backups, instance_id, snapshot)?;
    let mut manifest = manifest::read(&path)?;
    manifest.label = label.filter(|text| !text.trim().is_empty());
    manifest::write(&path, &manifest)?;
    Ok(describe(snapshot, &manifest))
}

/// 把一张快照恢复回去。
pub fn restore(
    paths: &DataPaths,
    instance_id: &str,
    snapshot: &str,
    scope: &Scope,
    mode: &Mode,
) -> Result<Restored> {
    let (_, directory) = situate(paths, instance_id)?;
    if let Some(occupant) = running::occupant(&directory) {
        return Err(anyhow!(
            "{} 正在使用这个游戏目录，请先退出游戏",
            display_name(paths, &occupant)
        ));
    }

    let backups = root(paths);
    let store = Store::at(&backups);
    let manifest = manifest::read(&manifest::path(&backups, instance_id, snapshot)?)?;

    if let Mode::Copy(name) = mode {
        if !matches!(scope, Scope::Save(_)) {
            return Err(anyhow!("只有恢复单个世界时才能另存一份"));
        }
        if !is_safe_name(name) {
            return Err(anyhow!("不能用作目录名：{name}"));
        }
        if directory.join("saves").join(name).exists() {
            return Err(anyhow!("已经有一个名为「{name}」的世界"));
        }
    }

    let wanted: Vec<&FileRecord> = manifest
        .files
        .iter()
        .filter(|file| covers(scope, &file.path))
        .collect();
    if wanted.is_empty() {
        return Err(anyhow!("这张快照里没有要恢复的内容"));
    }
    for file in &wanted {
        if !select::is_safe_relative(&file.path) {
            return Err(anyhow!("快照里有一条不安全的路径：{}", file.path));
        }
    }
    // 缺几个文件照常恢复并如实列出（§9），但一个都取不出来说明仓库丢了或者
    // 被清空了——那时候「恢复」只会删掉现有的东西再什么都写不回去。停在这里，
    // 什么都别动。
    if !wanted
        .iter()
        .any(|file| file.only_chunk().is_ok_and(|chunk| store.has(chunk)))
    {
        return Err(anyhow!("这张快照的内容已不在备份中，无法恢复"));
    }

    // 恢复本身也是一次不可逆操作，所以先替用户拍一张。拍不成就停下——
    // 没有退路的恢复不该开始。
    let safety = take(paths, instance_id, Reason::BeforeRestore, None, None)
        .context("恢复前的快照没有拍成，恢复未开始")?;

    let sha1s: HashMap<&str, &str> = manifest
        .mods
        .iter()
        .map(|record| (record.file.as_str(), record.sha1.as_str()))
        .collect();

    let mut restored = Restored {
        safety: Some(safety.id),
        ..Restored::default()
    };

    for file in &wanted {
        let relative = rewrite(&file.path, scope, mode);
        let destination = directory.join(&relative);
        let outcome = file
            .only_chunk()
            .and_then(|chunk| store.extract(chunk, &destination));
        match outcome {
            Ok(()) => {
                restored.written += 1;
                restored.bytes += file.size;
            }
            Err(_) => restored.missing.push(Missing {
                sha1: file
                    .path
                    .strip_prefix("mods/")
                    .and_then(|name| sha1s.get(name))
                    .map(|sha1| (*sha1).to_owned()),
                path: file.path.clone(),
            }),
        }
    }

    if matches!(mode, Mode::Replace) {
        let keep: HashSet<&str> = wanted.iter().map(|file| file.path.as_str()).collect();
        restored.removed = discard(&directory, scope, &keep);
    }

    Ok(restored)
}

/// 这条路径在这次恢复的范围里吗。
fn covers(scope: &Scope, relative: &str) -> bool {
    match scope {
        Scope::All => true,
        Scope::Save(name) => select::save_of(relative) == Some(name.as_str()),
        Scope::Config => select::is_config(relative),
        Scope::Mods => select::is_mod(relative),
    }
}

/// 另存一份时把 `saves/家/…` 改写成 `saves/家 (2026-08-07)/…`。
fn rewrite(relative: &str, scope: &Scope, mode: &Mode) -> String {
    match (scope, mode) {
        (Scope::Save(from), Mode::Copy(to)) => {
            match relative.strip_prefix(&format!("saves/{from}/")) {
                Some(rest) => format!("saves/{to}/{rest}"),
                None => relative.to_owned(),
            }
        }
        _ => relative.to_owned(),
    }
}

/// 范围之内、快照里没有的文件，删掉。
///
/// 不删的话「把这个世界回到昨天」会得到一个半新半旧的世界——今天新生成的
/// region 文件留在那里，而它们记录的地形和昨天的存档对不上。恢复之前那张
/// 安全快照就是为这一步存在的。
fn discard(directory: &Path, scope: &Scope, keep: &HashSet<&str>) -> usize {
    let (candidates, _) = select::scan(directory);
    candidates
        .into_iter()
        .filter(|candidate| covers(scope, &candidate.relative))
        .filter(|candidate| !keep.contains(candidate.relative.as_str()))
        .filter(|candidate| fs::remove_file(&candidate.absolute).is_ok())
        .count()
}

/// 世界名会变成目录名。
fn is_safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':', '\0'])
}

/// 磁盘占用。
pub fn usage(paths: &DataPaths) -> Result<Usage> {
    let backups = root(paths);
    let store = Store::at(&backups);

    // 一个对象被哪些实例引用、是不是模组文件。
    let mut owners: HashMap<String, HashSet<String>> = HashMap::new();
    let mut from_mods: HashSet<String> = HashSet::new();
    let mut counts: HashMap<String, usize> = HashMap::new();

    for (instance, id) in manifest::every(&backups) {
        *counts.entry(instance.clone()).or_default() += 1;
        let Ok(path) = manifest::path(&backups, &instance, &id) else {
            continue;
        };
        let Ok(manifest) = manifest::read(&path) else {
            continue;
        };
        for file in &manifest.files {
            for chunk in &file.chunks {
                owners
                    .entry(chunk.clone())
                    .or_default()
                    .insert(instance.clone());
                if select::is_mod(&file.path) {
                    from_mods.insert(chunk.clone());
                }
            }
        }
    }

    let mut usage = Usage {
        bytes: store.bytes(),
        snapshots: counts.values().sum(),
        ..Usage::default()
    };
    let mut reclaimable: HashMap<String, u64> = HashMap::new();
    for (object, holders) in &owners {
        let bytes = store.stored_bytes(object);
        if from_mods.contains(object) {
            usage.mods_bytes += bytes;
        }
        // 共享的对象删了也不会消失，不算进任何一个实例的可收回空间。
        if let [only] = holders.iter().collect::<Vec<_>>().as_slice() {
            *reclaimable.entry((*only).clone()).or_default() += bytes;
        }
    }

    usage.instances = counts
        .into_iter()
        .map(|(instance, snapshots)| InstanceUsage {
            reclaimable: reclaimable.get(&instance).copied().unwrap_or_default(),
            instance,
            snapshots,
        })
        .collect();
    usage
        .instances
        .sort_by_key(|instance| std::cmp::Reverse(instance.reclaimable));
    Ok(usage)
}

/// 收掉没人引用的对象。删快照之后跑一次。
pub fn collect_garbage(paths: &DataPaths) -> Result<store::Swept> {
    let backups = root(paths);
    Store::at(&backups).sweep(&manifest::live_objects(&backups), ORPHAN_GRACE)
}

/// 按保留策略剪掉一批，然后回收。返回删掉的快照 id。
pub fn prune(paths: &DataPaths, instance_id: &str) -> Result<Vec<String>> {
    let backups = root(paths);
    let existing: Vec<(String, u64, bool)> = manifest::ids(&backups, instance_id)
        .into_iter()
        .filter_map(|id| {
            let manifest =
                manifest::read(&manifest::path(&backups, instance_id, &id).ok()?).ok()?;
            let pinned = manifest.label.is_some() || manifest.reason == Reason::Manual;
            Some((id, manifest.taken_at, pinned))
        })
        .collect();

    let expired = schedule::expired(&existing, now());
    for id in &expired {
        remove(paths, instance_id, id)?;
    }
    if !expired.is_empty() {
        collect_garbage(paths)?;
    }
    Ok(expired)
}

// ——— 触发点 ———
//
// 这三个都不返回错误：备份失败是一条通知，绝不能挡住用户正在做的事。
// 「磁盘满 → 启动不了」是比没有备份严重得多的问题（§9）。

/// 改模组之前。装、删、启用禁用、更新都算。
///
/// 这是整个功能里最有价值的那一张：**用户以为自己只是装了个模组**，而这是一次
/// 会破坏存档的操作（§1）。所以不问，直接拍。
pub(crate) fn before_mod_change(paths: &DataPaths, instance_id: &str, about: Option<About>) {
    if recent(
        paths,
        instance_id,
        Some(Reason::BeforeModChange),
        MOD_CHANGE_GRACE,
    ) {
        // 十分钟内的第一张记的才是这串操作之前的状态，它的事件名也跟着
        // 第一张走——「装 Create 之前」顺手又装了两个前置，仍然是那一张。
        return;
    }
    quietly_about(paths, instance_id, Reason::BeforeModChange, about);
}

/// 游戏正常退出之后。此时文件没被占用，而且这一次的成果就在那里。
///
/// `minutes` 是这一场玩了多久——「游玩 3 小时之后」比「游戏结束之后」更像
/// 用户记忆里的那个锚点。
pub(crate) fn after_session(paths: &DataPaths, instance_id: &str, minutes: u64) {
    quietly_about(
        paths,
        instance_id,
        Reason::AfterSession,
        Some(About::new("session").with("minutes", minutes.to_string())),
    );
}

/// 这次启动之前该不该拍一张。
///
/// 和 [`quietly`] 分开，是为了让调用方能在真的要拍的时候先说一声——一次可能
/// 要十几秒的读盘，界面上不该是一段没有说明的停顿。
pub(crate) fn due_before_launch(paths: &DataPaths, instance_id: &str) -> bool {
    !recent(paths, instance_id, None, LAUNCH_INTERVAL)
}

pub(crate) fn quietly(paths: &DataPaths, instance_id: &str, reason: Reason) {
    quietly_about(paths, instance_id, reason, None);
}

fn quietly_about(paths: &DataPaths, instance_id: &str, reason: Reason, about: Option<About>) {
    if let Err(error) = take(paths, instance_id, reason, None, about) {
        let _ = paths.append_log(&format!(
            "snapshot {} for {instance_id} skipped: {error:#}",
            reason.tag()
        ));
        return;
    }
    // 新的一张落地了，顺手按保留策略剪掉过期的。放在这里而不是定时器上：
    // 快照只在这些事件时刻增长，清理跟着同一批时刻走就够了——此前这套策略
    // 写完测完却没有任何调用点，快照只增不减。手动拍的这里不清（它们本就
    // 永久保留，动手的时刻也轮不到我们顺手做别的）。
    if let Err(error) = prune(paths, instance_id) {
        let _ = paths.append_log(&format!("prune for {instance_id} skipped: {error:#}"));
    }
}

/// 一个实例被删掉之后，它的快照也没有存在的理由了。
///
/// 恢复需要实例本身（游戏目录、instance.json），实例没了这些清单只是孤儿：
/// 永久占盘，还让用量页列出一个已不存在的实例。对象仓库是跨实例共享的，
/// 删掉清单后回收一次，只有无人引用的内容会被清走。
///
/// 尽力而为：删实例这个动作本身已经完成，备份清不掉只该进日志，不该把
/// 「删除失败」报给一个实例其实已经删掉了的用户。
pub(crate) fn forget(paths: &DataPaths, instance_id: &str) {
    let backups = root(paths);
    let Ok(directory) = manifest::directory(&backups, instance_id) else {
        return;
    };
    if !directory.is_dir() {
        return;
    }
    if let Err(error) = std::fs::remove_dir_all(&directory) {
        let _ = paths.append_log(&format!(
            "snapshots of {instance_id} not removed: {error:#}"
        ));
        return;
    }
    if let Err(error) = collect_garbage(paths) {
        let _ = paths.append_log(&format!("backup gc after delete skipped: {error:#}"));
    }
}

/// 最近这些秒里已经拍过一张了吗。
fn recent(paths: &DataPaths, instance_id: &str, reason: Option<Reason>, within: u64) -> bool {
    let backups = root(paths);
    let floor = now().saturating_sub(within);
    manifest::ids(&backups, instance_id)
        .into_iter()
        .filter_map(|id| manifest::read(&manifest::path(&backups, instance_id, &id).ok()?).ok())
        .any(|manifest| {
            manifest.taken_at >= floor && reason.is_none_or(|wanted| manifest.reason == wanted)
        })
}

/// 一个文件的 sha1。模组站按它反查，所以它是对象仓库之外的第二条退路。
pub(crate) fn sha1_of(path: &Path) -> Result<String> {
    use sha1::{Digest, Sha1};
    use std::io::Read;

    let mut file = fs::File::open(path).with_context(|| format!("打开 {}", path.display()))?;
    let mut hasher = Sha1::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(40), |mut out, byte| {
            use std::fmt::Write;
            let _ = write!(out, "{byte:02x}");
            out
        }))
}

/// 全部拍摄原因。界面的文案表照着它检查有没有漏。
pub fn reasons() -> Vec<String> {
    Reason::ALL
        .iter()
        .map(|reason| reason.tag().to_owned())
        .collect()
}

#[cfg(test)]
mod tests;
