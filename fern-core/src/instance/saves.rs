//! 实例里的存档。
//!
//! 只读。删存档这种事交给文件管理器——启动器误删一个世界是不可挽回的，
//! 而「打开存档目录」已经能满足所有真实需求。
//!
//! 显示的是**目录名**，不是 `level.dat` 里的 `LevelName`。两个原因：目录名
//! 是这个世界在磁盘上的真实身份，而 `LevelName` 允许重名——两个都叫「新的
//! 世界」的存档在列表里长得一模一样，反而认不出来；而且读它要解 gzip 再走
//! 一遍 NBT，为一个多数时候和目录名相同的字符串引入一整套解析不划算。

use std::{
    fs, io,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::DataPaths;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveEntry {
    /// 目录名，同时是显示名。
    pub name: String,
    /// 整个存档目录占多少字节。
    pub bytes: u64,
    /// `level.dat` 的修改时刻，Unix 秒——也就是这个世界上次被保存的时候。
    pub modified: Option<u64>,
}

fn seconds(time: io::Result<SystemTime>) -> Option<u64> {
    time.ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|since| since.as_secs())
}

/// 目录占用。软链接不跟——跟过去会把别处的东西算进这个存档的体积里。
fn tree_bytes(path: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.metadata() {
            Ok(metadata) if metadata.is_dir() => tree_bytes(&entry.path()),
            Ok(metadata) if metadata.is_file() => metadata.len(),
            _ => 0,
        })
        .sum()
}

/// 只要名字，不算体积。
///
/// 命令面板要的是「这台机器上有哪些世界」，而 `tree_bytes` 会把每个世界的
/// 几万个区块文件都 stat 一遍——那是详情页里「这个存档占多大」才需要付的
/// 代价。二十个实例各走一遍的话，一次搜索要等好几秒。
pub fn names(paths: &DataPaths, instance_id: &str) -> Vec<String> {
    let root = crate::instance::paths_by_id(paths, instance_id)
        .game_directory(instance_id)
        .join("saves");
    let Ok(entries) = fs::read_dir(&root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.path().join("level.dat").is_file())
        .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
        .collect()
}

/// 列出一个实例的存档，最近保存的排在前面。
///
/// `saves` 目录不存在是正常的——还没进过游戏的实例没有存档，这不是错误。
pub fn list(paths: &DataPaths, instance_id: &str) -> Result<Vec<SaveEntry>> {
    let root = crate::instance::paths_by_id(paths, instance_id)
        .game_directory(instance_id)
        .join("saves");
    let entries = match fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).context("read saves directory"),
    };

    let mut saves = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        // 有 level.dat 才是一个世界。saves 下面还会有别的东西（备份、
        // 截图工具的残留），列出来只会让人以为存档坏了。
        if !path.join("level.dat").is_file() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        saves.push(SaveEntry {
            name,
            bytes: tree_bytes(&path),
            modified: seconds(
                fs::metadata(path.join("level.dat")).and_then(|meta| meta.modified()),
            ),
        });
    }

    // 最近保存的在前，没有时间戳的沉底。
    saves.sort_by(|left, right| {
        right
            .modified
            .cmp(&left.modified)
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(saves)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn temp_root(tag: &str) -> std::path::PathBuf {
        env::temp_dir().join(format!("fern-saves-{tag}-{}", std::process::id()))
    }

    #[test]
    fn missing_saves_directory_is_not_an_error() {
        let root = temp_root("absent");
        let paths = DataPaths::new(&root);
        assert_eq!(list(&paths, "moss").expect("list saves"), Vec::new());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn lists_only_directories_holding_a_level_dat() {
        let root = temp_root("filter");
        let paths = DataPaths::new(&root);
        let saves = paths.game_directory("moss").join("saves");

        fs::create_dir_all(saves.join("world/region")).expect("create world");
        fs::write(saves.join("world/level.dat"), b"nbt").expect("level.dat");
        fs::write(saves.join("world/region/r.0.0.mca"), vec![0u8; 512]).expect("region");
        // 备份目录里没有 level.dat，不该出现在列表里。
        fs::create_dir_all(saves.join("backup")).expect("create backup");
        fs::write(saves.join("backup/readme.txt"), b"x").expect("readme");

        let listed = list(&paths, "moss").expect("list saves");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "world");
        // 3 字节的 level.dat 加 512 字节的区块。
        assert_eq!(listed[0].bytes, 515);

        fs::remove_dir_all(root).expect("remove test root");
    }
}
