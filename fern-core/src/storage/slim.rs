//! 瘦身：删掉没有实例引用的共享文件。
//!
//! `versions/`、`libraries/`、`assets/`、`runtimes/` 只增不减地陪着用户换版本、
//! 换加载器，几年攒下来常常比所有实例加起来还大。这里的每一样东西都**可以
//! 重新下载**——所以敢删；但删不删要按引用关系算——所以先算活集。
//!
//! 活集从每个实例的描述出发：实例要启动的版本沿 `inheritsFrom` 链跟到根，
//! 链上每份 JSON 引用的库、资源索引、Java 组件都算活。这份 JSON 与补全、
//! 启动读的是同一份（AGENTS.md：必须是同一份），所以「启动要用的」和「瘦身
//! 要留的」不会漂移。
//!
//! 两条硬规则：
//!
//! - **算不清就不删。** 链上的 JSON 还没下载，说明它不引用任何共享文件，
//!   跳过；存在却读不了，整个瘦身中止——一份读不出来的清单背后可能是一个
//!   还活着的实例，宁可一个字节都不省。
//! - **预检与执行同一套判定，执行时现算。** 界面上那份预检可能已经过时，
//!   拿旧计划删新磁盘等于闭着眼删。

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::{DataPaths, launch::version, storage::tree_bytes};

/// 继承链最多跟这么深。与 `launch::version` 的上限同义：防一份写坏的 JSON
/// 让我们转圈。
const MAX_DEPTH: usize = 8;

/// 预检的结果，也是执行的回执：哪些东西没有实例引用，删了能省多少。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlimPlan {
    /// 没有实例使用的版本目录。
    pub versions: Vec<String>,
    pub versions_bytes: u64,
    /// 没有实例需要的 Java 运行时（`runtimes/` 下的目录名）。
    pub runtimes: Vec<String>,
    pub runtimes_bytes: u64,
    /// 没有版本引用的库文件数。名单没有意义——几百条 Maven 路径谁也不看。
    pub libraries_files: usize,
    pub libraries_bytes: u64,
    /// 没有索引引用的资源对象数。
    pub assets_files: usize,
    pub assets_bytes: u64,
}

impl SlimPlan {
    /// 一共能省多少。
    pub fn bytes(&self) -> u64 {
        self.versions_bytes + self.runtimes_bytes + self.libraries_bytes + self.assets_bytes
    }

    /// 没什么可省的。
    pub fn is_empty(&self) -> bool {
        self.versions.is_empty()
            && self.runtimes.is_empty()
            && self.libraries_files == 0
            && self.assets_files == 0
    }
}

/// 这次执行哪几类。预检列出四类各自的分量，用户按类勾选。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlimContents {
    pub versions: bool,
    pub runtimes: bool,
    pub libraries: bool,
    pub assets: bool,
}

/// 活着的引用集合。不在这里面的才可以删。
#[derive(Debug, Default)]
struct LiveSet {
    /// 版本目录名。
    versions: HashSet<String>,
    /// Java 组件名（`runtimes/` 下的目录名）。
    components: HashSet<String>,
    /// 相对 `libraries/` 的路径，`/` 分隔。
    libraries: HashSet<String>,
    /// 资源索引 id。
    indexes: HashSet<String>,
    /// 资源对象哈希。
    objects: HashSet<String>,
}

/// 资源索引文件里我们关心的部分。
#[derive(Deserialize)]
struct AssetIndexFile {
    #[serde(default)]
    objects: HashMap<String, AssetObject>,
}

#[derive(Deserialize)]
struct AssetObject {
    hash: String,
}

/// 预检：能省什么、省多少。只读，不动任何文件。
pub fn preview(paths: &DataPaths) -> Result<SlimPlan> {
    walk(paths, &live_set(paths)?, None)
}

/// 执行。按 [`SlimContents`] 勾选的类别删，返回实际删掉的那些。
///
/// 游戏跑着的时候拒绝：正在运行的 JVM 打开着这些 jar，删一半会得到一个
/// 说不清状态的游戏。
pub fn apply(paths: &DataPaths, contents: &SlimContents) -> Result<SlimPlan> {
    if let Some(name) = crate::storage::occupant_name(paths) {
        return Err(anyhow!("{name} 正在运行，请先退出游戏再瘦身"));
    }
    walk(paths, &live_set(paths)?, Some(contents))
}

/// 把所有实例引用的东西收齐。
fn live_set(paths: &DataPaths) -> Result<LiveSet> {
    let mut live = LiveSet::default();

    for profile in crate::list_instances(paths)? {
        // 用户给这个实例钉住的 Java 若在 runtimes/ 里，那份运行时就是活的，
        // 无论有没有版本 JSON 还认它。
        note_component(&mut live.components, paths, profile.settings.java_path.as_deref());

        let scoped = crate::instance::paths_for(paths, &profile);
        // 外部实例的版本目录是别人的，但 shared_libraries 开着时库和资源用的
        // 是我们的——它引用什么就得留什么。
        let shares_versions = scoped.versions == paths.versions;
        let shares_libraries = scoped.libraries == paths.libraries;
        let shares_assets = scoped.assets == paths.assets;
        if !(shares_versions || shares_libraries || shares_assets) {
            continue;
        }

        // 实例记着的版本无条件算活：刚建好还没补全的实例，版本目录里可能
        // 只有半份东西，删掉它等于把正要开始的补全拆台。
        if shares_versions {
            live.versions.insert(profile.game_version.clone());
            live.versions.insert(version::effective_id(&profile));
        }

        let mut current = version::effective_id(&profile);
        for _ in 0..MAX_DEPTH {
            // 还没下载不算错——一份不存在的 JSON 不引用任何东西。存在却
            // 读不了是另一回事：算不清就不删。
            if !version::metadata_path(&scoped, &current).exists() {
                break;
            }
            let metadata = version::read_one(&scoped, &current).with_context(|| {
                format!("读不出「{}」引用的版本描述，瘦身没有开始", profile.name)
            })?;

            if shares_versions {
                live.versions.insert(current.clone());
            }
            if shares_libraries {
                for library in &metadata.libraries {
                    // 全部载荷都算活，不按本机的 rules 挑：这块磁盘可能装在
                    // 便携盘里换电脑用，别的系统要的 natives 删了就要重下。
                    if let Some(path) = fern_meta::maven_path(&library.name) {
                        live.libraries.insert(path);
                    }
                    if let Some(downloads) = &library.downloads {
                        let classifiers = downloads.classifiers.iter().flat_map(|map| map.values());
                        for info in downloads.artifact.iter().chain(classifiers) {
                            if let Some(path) = &info.path {
                                live.libraries.insert(path.clone());
                            }
                        }
                    }
                }
            }
            if shares_assets && let Some(index) = &metadata.asset_index {
                live.indexes.insert(index.id.clone());
            }
            if let Some(java) = &metadata.java_version {
                live.components.insert(java.component.clone());
            }

            match metadata.inherits_from {
                Some(parent) if !parent.is_empty() && parent != current => current = parent,
                _ => break,
            }
        }
    }

    // 全局登记过的 Java 路径同样作数。
    let settings = crate::data::settings::load(paths);
    for path in &settings.java.extra_paths {
        note_component(&mut live.components, paths, Some(path));
    }

    // 活索引引用的对象。索引读不了就中止：它背后是一整代资源。
    for id in live.indexes.clone() {
        let path = paths.assets.join("indexes").join(format!("{id}.json"));
        if !path.exists() {
            continue;
        }
        let bytes = fs::read(&path).with_context(|| format!("读取 {}", path.display()))?;
        let index: AssetIndexFile =
            serde_json::from_slice(&bytes).with_context(|| format!("解析 {}", path.display()))?;
        live.objects
            .extend(index.objects.into_values().map(|object| object.hash));
    }

    Ok(live)
}

/// 这个 Java 路径若指向 `runtimes/<组件>`，把组件记为活。
fn note_component(components: &mut HashSet<String>, paths: &DataPaths, java: Option<&Path>) {
    let Some(rest) = java.and_then(|path| path.strip_prefix(&paths.runtimes).ok()) else {
        return;
    };
    if let Some(std::path::Component::Normal(name)) = rest.components().next()
        && let Some(name) = name.to_str()
    {
        components.insert(name.to_owned());
    }
}

/// 预检与执行共用的一次遍历。`delete` 为 `None` 时只数不删。
fn walk(paths: &DataPaths, live: &LiveSet, delete: Option<&SlimContents>) -> Result<SlimPlan> {
    let mut plan = SlimPlan::default();

    // 版本目录。
    for entry in children(&paths.versions) {
        let Some(name) = entry.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !entry.is_dir() || live.versions.contains(name) {
            continue;
        }
        plan.versions_bytes += tree_bytes(&entry);
        plan.versions.push(name.to_owned());
        if delete.is_some_and(|contents| contents.versions) {
            fs::remove_dir_all(&entry).with_context(|| format!("删除 {}", entry.display()))?;
        }
    }
    plan.versions.sort();

    // Java 运行时。
    for entry in children(&paths.runtimes) {
        let Some(name) = entry.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !entry.is_dir() || live.components.contains(name) {
            continue;
        }
        plan.runtimes_bytes += tree_bytes(&entry);
        plan.runtimes.push(name.to_owned());
        if delete.is_some_and(|contents| contents.runtimes) {
            crate::java::runtime::remove(paths, &entry)?;
        }
    }
    plan.runtimes.sort();

    // 库：不被任何活版本引用的文件。
    let deleting = delete.is_some_and(|contents| contents.libraries);
    for (file, relative) in files_under(&paths.libraries) {
        if live.libraries.contains(&relative) {
            continue;
        }
        plan.libraries_files += 1;
        plan.libraries_bytes += fs::metadata(&file).map_or(0, |metadata| metadata.len());
        if deleting {
            fs::remove_file(&file).with_context(|| format!("删除 {}", file.display()))?;
        }
    }
    if deleting {
        sweep_empty(&paths.libraries);
    }

    // 资源：死索引本身、死索引专属的 virtual 树、没有活索引引用的对象。
    let deleting = delete.is_some_and(|contents| contents.assets);
    for entry in children(&paths.assets.join("indexes")) {
        let Some(id) = entry
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".json"))
        else {
            continue;
        };
        if live.indexes.contains(id) {
            continue;
        }
        plan.assets_files += 1;
        plan.assets_bytes += fs::metadata(&entry).map_or(0, |metadata| metadata.len());
        if deleting {
            fs::remove_file(&entry).with_context(|| format!("删除 {}", entry.display()))?;
        }
    }
    for entry in children(&paths.assets.join("virtual")) {
        let Some(id) = entry.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !entry.is_dir() || live.indexes.contains(id) {
            continue;
        }
        plan.assets_files += 1;
        plan.assets_bytes += tree_bytes(&entry);
        if deleting {
            fs::remove_dir_all(&entry).with_context(|| format!("删除 {}", entry.display()))?;
        }
    }
    for (file, _) in files_under(&paths.assets.join("objects")) {
        let Some(hash) = file.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if live.objects.contains(hash) {
            continue;
        }
        plan.assets_files += 1;
        plan.assets_bytes += fs::metadata(&file).map_or(0, |metadata| metadata.len());
        if deleting {
            fs::remove_file(&file).with_context(|| format!("删除 {}", file.display()))?;
        }
    }
    if deleting {
        sweep_empty(&paths.assets.join("objects"));
    }

    Ok(plan)
}

/// 一个目录的直接子项。目录不存在就是空——还没下载过任何东西的数据根。
fn children(directory: &Path) -> Vec<PathBuf> {
    fs::read_dir(directory)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .collect()
}

/// 一棵树里的全部文件，带相对根的 `/` 分隔路径。
fn files_under(root: &Path) -> Vec<(PathBuf, String)> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            let path = entry.path();
            if metadata.is_dir() {
                stack.push(path);
            } else if metadata.is_file() {
                let relative = path.strip_prefix(root).ok().map(|relative| {
                    relative
                        .components()
                        .filter_map(|part| part.as_os_str().to_str())
                        .collect::<Vec<_>>()
                        .join("/")
                });
                if let Some(relative) = relative {
                    files.push((path, relative));
                }
            }
        }
    }
    files
}

/// 删空目录，自底向上。删文件留下的空壳没有信息量，还让人以为里面有东西。
fn sweep_empty(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            sweep_empty(&path);
            // 非空时失败，这正是想要的判断，不必先数一遍。
            let _ = fs::remove_dir(&path);
        }
    }
}
