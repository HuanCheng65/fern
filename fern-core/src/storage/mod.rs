//! 存储：Fern 占了多少磁盘，其中多少拿得回来。
//!
//! 这一层回答的问题只有一个：**「占用在哪、哪些删了不心疼」**。磁盘上的东西
//! 按处置方式分三档，档与档绝不混在一个按钮里：
//!
//! 1. **用户的数据**（实例、快照）——不可再生，只报数，不清理。删实例、删
//!    快照各有各的入口，那里有各自的后果说明。
//! 2. **可再生的缓存**（`cache/`、日志）——清掉的唯一后果是下次重新获取，
//!    见 [`clear_cache`] / [`clear_logs`]。
//! 3. **可重新下载的共享文件**（版本、库、资源、Java 运行时）——删错了最坏
//!    也就是一次重新下载，但删不删要按引用关系算，见 [`slim`]。
//!
//! 数字的准绳沿用快照用量页立下的那条：**报出来的数必须兑现得了**。各分区
//! 加起来等于总数（对不上的账会让人怀疑所有数字），外部实例的游戏目录不在
//! 我们的地盘上，不算进来。

pub mod slim;

use std::{fs, path::Path};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::{DataPaths, launch::running};

/// 数据根下各分区的占用。全部是磁盘上的真实字节数。
///
/// 只做文件系统遍历，不探测 Java、不解压清单——报告要能在打开设置的那一刻
/// 就开始出数。每个实例多大是单独一条命令（[`instance_bytes`]），界面逐个
/// 拉，几十 GB 的实例不挡住整张报告。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageReport {
    /// 各分区之和。
    pub total: u64,
    /// `instances/` 整棵。外部实例只算落在这里的描述文件，游戏本体不在
    /// 我们的数据根下。
    pub instances: u64,
    /// `backups/`。快照在仓库里实际占的磁盘，去重压缩之后的数。
    pub snapshots: u64,
    pub versions: u64,
    pub libraries: u64,
    pub assets: u64,
    /// `runtimes/`，Fern 下载的 Java。
    pub runtimes: u64,
    /// 元数据缓存。整棵树都可以随时重建。
    pub cache: u64,
    pub logs: u64,
    /// 不属于上面任何一档的零散（设置、来源记录……）。单独归一档是为了让
    /// 各分区加起来正好是总数。
    pub other: u64,
}

/// 目录占用的字节数。
///
/// 软链接不跟：`DirEntry::metadata` 拿到的是链接本身，跟过去会把别处的东西
/// 算进这里的体积。这是全 crate 唯一的一份实现——存档详情、Java 运行时和
/// 这份报告量的必须是同一种「大小」。
pub(crate) fn tree_bytes(root: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![root.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata.is_dir() {
                stack.push(entry.path());
            } else if metadata.is_file() {
                total += metadata.len();
            }
        }
    }
    total
}

/// 量一遍数据根。
pub fn report(paths: &DataPaths) -> Result<StorageReport> {
    let mut report = StorageReport {
        instances: tree_bytes(&paths.instances),
        snapshots: tree_bytes(&crate::backup::root(paths)),
        versions: tree_bytes(&paths.versions),
        libraries: tree_bytes(&paths.libraries),
        assets: tree_bytes(&paths.assets),
        runtimes: tree_bytes(&paths.runtimes),
        cache: tree_bytes(&paths.cache),
        logs: tree_bytes(&paths.logs),
        ..StorageReport::default()
    };

    // 数据根和共享游戏目录的顶层里，不属于任何已知分区的都归「其他」。
    // 挨个加而不是「总量减已知」：两遍遍历之间文件在变，减出负数没法解释。
    let shared = paths.shared_game_root();
    let claimed_root = [
        paths.instances.clone(),
        crate::backup::root(paths),
        paths.runtimes.clone(),
        paths.cache.clone(),
        paths.logs.clone(),
        shared.clone(),
    ];
    let claimed_shared = [&paths.versions, &paths.libraries, &paths.assets];
    for (directory, claimed) in [
        (&paths.root, &claimed_root[..]),
        (&shared, &claimed_shared.map(Clone::clone)[..]),
    ] {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if claimed.contains(&path) {
                continue;
            }
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata.is_dir() {
                report.other += tree_bytes(&path);
            } else if metadata.is_file() {
                report.other += metadata.len();
            }
        }
    }

    report.total = report.instances
        + report.snapshots
        + report.versions
        + report.libraries
        + report.assets
        + report.runtimes
        + report.cache
        + report.logs
        + report.other;
    Ok(report)
}

/// 一个实例占多大。界面逐个拉，不塞进整张报告里等最慢的那一个。
pub fn instance_bytes(paths: &DataPaths, instance_id: &str) -> Result<u64> {
    let id = crate::InstanceId::parse(instance_id).map_err(|error| anyhow!("{error}"))?;
    Ok(tree_bytes(&paths.instance_root(id.as_str())))
}

/// 清空元数据缓存。返回省下的字节数。
///
/// `cache/` 是数据根下唯一整棵可以随时重建的树（见 `data/mod.rs`），清掉的
/// 后果只有下次需要时重新获取一遍。
pub fn clear_cache(paths: &DataPaths) -> Result<u64> {
    clear_directory(&paths.cache)
}

/// 清空日志。返回省下的字节数。
///
/// 游戏跑着的时候它的日志正在写，先拒绝——删一个正在写的文件在有的系统上
/// 会失败，在另一些系统上会让日志无声地断掉。
pub fn clear_logs(paths: &DataPaths) -> Result<u64> {
    if let Some(name) = occupant_name(paths) {
        return Err(anyhow!("{name} 正在运行，日志正在写入，请先退出游戏"));
    }
    clear_directory(&paths.logs)
}

/// 这个数据根下有游戏在跑的话，它叫什么。界面上要说的是名字，不是 id。
///
/// 按目录逐个问（与快照那边同一套判定），而不是查全局的运行清单——别的
/// 数据根里跑着的游戏用不着我们的文件，不该挡这里的清理。
pub(crate) fn occupant_name(paths: &DataPaths) -> Option<String> {
    for profile in crate::list_instances(paths).ok()? {
        let scoped = crate::instance::paths_for(paths, &profile);
        let directory = scoped.game_directory(profile.id.as_str());
        if let Some(occupant) = running::occupant(&directory) {
            return Some(
                crate::read_instance(paths, &occupant)
                    .map(|profile| profile.name)
                    .unwrap_or(occupant),
            );
        }
    }
    None
}

fn clear_directory(directory: &Path) -> Result<u64> {
    let freed = tree_bytes(directory);
    if directory.is_dir() {
        fs::remove_dir_all(directory).with_context(|| format!("清空 {}", directory.display()))?;
    }
    fs::create_dir_all(directory).with_context(|| format!("重建 {}", directory.display()))?;
    Ok(freed)
}

#[cfg(test)]
mod tests;
