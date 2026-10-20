//! 这个实例是从哪个整合包装出来的。
//!
//! 装的那一刻什么都知道——哪一家、哪个项目、哪个版本、铺下来的每个文件是什么
//! ——过后就再也问不回来了：`mods/` 里的文件会被更新、被删、被用户自己加，
//! 配置会被改。所以在装的时候记一份，以后要判断「有没有新版」「哪些文件是包
//! 带来的、哪些是用户改过的」，都从这里出发。
//!
//! ```text
//! <数据目录>/instances/<实例 id>/pack.json
//! ```
//!
//! 放在实例目录而不是游戏目录：游戏目录是模组能读写的地方，也会被导出带走；
//! 也不并进 `instance.json`，那一份被很多入口整份重写，多一块不归它们管的数据
//! 迟早被谁覆盖掉。
//!
//! 这一层只记录，不做任何判断。

use std::{collections::BTreeMap, path::PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::{DataPaths, supply::Source};

/// 整合包在上游的身份。项目和版本 id 都是字符串：CurseForge 的整数也这么存。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackRef {
    pub source: Source,
    pub project_id: String,
    pub version_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackRecord {
    /// 哪一家的格式。上游身份认不出来时也知道这一点。
    pub source: Source,
    /// 上游的项目和版本。从本地文件导入、而且反查不到时为空。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream: Option<PackRef>,
    /// 整合包自己写的名字和版本号。
    pub name: String,
    pub version: String,
    /// Unix 秒。
    pub installed_at: u64,
    /// 清单里装进来的文件：相对游戏目录的路径 → sha1。
    pub files: BTreeMap<String, String>,
    /// overrides 铺下来的文件：相对游戏目录的路径 → 铺下去那一刻的 sha1。
    ///
    /// 以后要分清「包里的配置」和「用户改过的配置」，比的就是这个。
    pub overrides: BTreeMap<String, String>,
}

fn path(paths: &DataPaths, instance_id: &str) -> PathBuf {
    paths.instance_root(instance_id).join("pack.json")
}

/// 记下来。失败只是少了一份记录，不该让一次装好了的整合包算作失败——调用方
/// 自己决定怎么处理返回的错误。
pub fn write(paths: &DataPaths, instance_id: &str, record: &PackRecord) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(record)?;
    std::fs::write(path(paths, instance_id), bytes)?;
    Ok(())
}

/// 没有记录（不是整合包装的、或者是记录这件事出现之前装的）就是 `None`。
pub fn read(paths: &DataPaths, instance_id: &str) -> Option<PackRecord> {
    let bytes = std::fs::read(path(paths, instance_id)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// 复制实例时一起带过去：副本的文件是原样拷的，它仍然是那个整合包。
pub fn inherit(paths: &DataPaths, from: &str, to: &str) {
    let source = path(paths, from);
    if source.is_file() {
        let _ = std::fs::copy(source, path(paths, to));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_survives_a_round_trip_and_reads_in_the_shape_the_interface_expects() {
        let root = std::env::temp_dir().join(format!("fern-pack-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = DataPaths::new(root.clone());
        std::fs::create_dir_all(paths.instance_root("a")).expect("mkdir");

        let record = PackRecord {
            source: Source::CurseForge,
            upstream: Some(PackRef {
                source: Source::CurseForge,
                project_id: "715572".to_owned(),
                version_id: "5000000".to_owned(),
            }),
            name: "All the Mods 9".to_owned(),
            version: "0.2.60".to_owned(),
            installed_at: 1,
            files: BTreeMap::from([("mods/a.jar".to_owned(), "aa".to_owned())]),
            overrides: BTreeMap::from([("config/a.toml".to_owned(), "bb".to_owned())]),
        };
        write(&paths, "a", &record).expect("write");
        assert_eq!(read(&paths, "a"), Some(record));
        assert_eq!(read(&paths, "missing"), None);

        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path(&paths, "a")).expect("read")).expect("json");
        assert_eq!(json["source"], "curseforge");
        assert_eq!(json["upstream"]["projectId"], "715572");
        assert_eq!(json["installedAt"], 1);

        std::fs::remove_dir_all(root).expect("clean");
    }
}
