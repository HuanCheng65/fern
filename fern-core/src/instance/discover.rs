//! 「我电脑上已经有游戏了，怎么让 Fern 用它」——这一个问题的入口。
//!
//! 用户不知道也不该知道我们内部分了几种来源。他手上只有一个目录，可能是：
//!
//! ```text
//! 官方启动器那一系   有 versions/，一堆版本各自成实例（HMCL / PCL2 / 官方）
//! Prism / MultiMC    有 mmc-pack.json，一个目录就是一个实例
//! 上面两者的上一层   .minecraft/ 的父目录，或者 Prism 的 instances/
//! ```
//!
//! 所以只给**一个**入口，选完目录我们自己认。认不出来时要说清我们在找什么，
//! 而不是丢一句「没有可用的版本」——那句话既不告诉他选错了目录，也不告诉他
//! 我们读不懂。

use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::{DataPaths, instance::prism::PrismInstance};

/// 那个目录里是什么。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Discovery {
    /// 官方启动器那一系的游戏目录，里面一堆版本。
    GameDirectory(super::external::ExternalScan),
    /// 一个或多个 Prism / MultiMC 实例。
    #[serde(rename_all = "camelCase")]
    PrismInstances {
        /// 真正读的那个目录：用户选了 `instances/` 的上一层时，这里是解析
        /// 之后的结果。
        root: PathBuf,
        instances: Vec<PrismInstance>,
    },
    /// 认不出来。
    #[serde(rename_all = "camelCase")]
    Unrecognised {
        /// 我们看的那个目录。
        looked_at: PathBuf,
    },
}

/// 一层目录里最多认这么多个 Prism 实例。
///
/// 不是性能考虑——每个实例只读两个小文件。是防一个选错的目录（比如整个家目录）
/// 把几千个子目录都翻一遍。
const MOST_INSTANCES: usize = 500;

/// 看一眼那个目录里有什么。**什么都不改。**
pub fn inspect(paths: &DataPaths, directory: &Path) -> Result<Discovery> {
    let directory = directory
        .canonicalize()
        .map_err(|_| anyhow!("{} 打不开", directory.display()))?;
    if !directory.is_dir() {
        return Err(anyhow!("{} 不是一个目录", directory.display()));
    }

    // 一、它自己就是一个 Prism 实例。
    if super::prism::looks_like_one(&directory) {
        let instance = super::prism::read(paths, &directory)?;
        return Ok(Discovery::PrismInstances {
            root: directory
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| directory.clone()),
            instances: vec![instance],
        });
    }

    // 二、它是一堆 Prism 实例的容器——`instances/` 本身，或者 Prism 的根
    // （那时候实例在 `instances/` 下面）。
    for candidate in [directory.clone(), directory.join("instances")] {
        let found = prism_children(paths, &candidate);
        if !found.is_empty() {
            return Ok(Discovery::PrismInstances {
                root: candidate,
                instances: found,
            });
        }
    }

    // 三、官方启动器那一系。它自己那套定位（选中上一层时往里走）在 external
    // 里，照用。
    if let Ok(scan) = super::external::scan(paths, &directory) {
        return Ok(Discovery::GameDirectory(scan));
    }

    Ok(Discovery::Unrecognised {
        looked_at: directory,
    })
}

/// 这个目录下面那些是 Prism 实例的子目录。按名字排，目录遍历的顺序不保证。
fn prism_children(paths: &DataPaths, directory: &Path) -> Vec<PrismInstance> {
    let mut children: Vec<PathBuf> = std::fs::read_dir(directory)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| super::prism::looks_like_one(path))
        .take(MOST_INSTANCES)
        .collect();
    children.sort();
    children
        .iter()
        // 读不懂的那一个跳过，不要让它拖垮整份清单——一个坏掉的
        // mmc-pack.json 不该让旁边十个好的都导不进来。
        .filter_map(|path| super::prism::read(paths, path).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("create");
        std::fs::write(path, text).expect("write");
    }

    fn prism_instance(directory: &Path, name: &str, version: &str) {
        write(&directory.join("instance.cfg"), &format!("name={name}\n"));
        write(
            &directory.join("mmc-pack.json"),
            &format!(
                r#"{{"formatVersion":1,"components":[{{"uid":"net.minecraft","version":"{version}"}}]}}"#
            ),
        );
        std::fs::create_dir_all(directory.join(".minecraft")).expect("game directory");
    }

    #[test]
    fn a_prism_instances_folder_lists_all_of_them() {
        let root = std::env::temp_dir().join(format!("fern-discover-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = DataPaths::new(root.join("fern"));
        let prism = root.join("PrismLauncher");
        prism_instance(&prism.join("instances/Alpha"), "阿尔法", "1.21.1");
        prism_instance(&prism.join("instances/Beta"), "贝塔", "1.12.2");
        // 一份坏掉的不该拖垮旁边好的那些。
        write(&prism.join("instances/Broken/mmc-pack.json"), "{ 不是 json");

        // 选中 instances/ 本身。
        for chosen in [prism.join("instances"), prism.clone()] {
            match inspect(&paths, &chosen).expect("inspect") {
                Discovery::PrismInstances { instances, .. } => {
                    let names: Vec<&str> = instances.iter().map(|one| one.name.as_str()).collect();
                    assert_eq!(names, vec!["阿尔法", "贝塔"], "选中 {}", chosen.display());
                }
                other => panic!("{} 该认成 Prism，认成了 {other:?}", chosen.display()),
            }
        }

        // 选中其中一个实例本身。
        match inspect(&paths, &prism.join("instances/Alpha")).expect("inspect") {
            Discovery::PrismInstances { instances, .. } => {
                assert_eq!(instances.len(), 1);
                assert_eq!(instances[0].game_version, "1.21.1");
            }
            other => panic!("认成了 {other:?}"),
        }

        std::fs::remove_dir_all(root).expect("remove root");
    }

    /// 官方那一系仍然走原来那条路，而且不能被 Prism 那两步抢走。
    #[test]
    fn a_dot_minecraft_is_still_recognised() {
        let root = std::env::temp_dir().join(format!("fern-discover-mc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = DataPaths::new(root.join("fern"));
        let game = root.join(".minecraft");
        write(
            &game.join("versions/1.21.1/1.21.1.json"),
            r#"{"id":"1.21.1","mainClass":"net.minecraft.client.main.Main"}"#,
        );

        match inspect(&paths, &game).expect("inspect") {
            Discovery::GameDirectory(scan) => {
                assert_eq!(scan.versions.len(), 1);
                assert_eq!(scan.versions[0].id, "1.21.1");
            }
            other => panic!("认成了 {other:?}"),
        }

        std::fs::remove_dir_all(root).expect("remove root");
    }

    /// 认不出来时要说清看的是哪个目录，而不是含糊地说没有版本。
    #[test]
    fn an_unrelated_folder_says_so() {
        let root = std::env::temp_dir().join(format!("fern-discover-no-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = DataPaths::new(root.join("fern"));
        std::fs::create_dir_all(root.join("文档")).expect("create");

        match inspect(&paths, &root.join("文档")).expect("inspect") {
            Discovery::Unrecognised { looked_at } => {
                assert!(looked_at.ends_with("文档"));
            }
            other => panic!("认成了 {other:?}"),
        }
        // 根本不存在的目录是另一回事，那是一个错误。
        assert!(inspect(&paths, &root.join("不存在")).is_err());

        std::fs::remove_dir_all(root).expect("remove root");
    }
}
