//! 把 Prism Launcher / MultiMC 的实例读进来（文档 §5）。
//!
//! 它们的实例目录长这样：
//!
//! ```text
//! <实例>/
//!   instance.cfg      名字、内存上限、覆盖开关，`key=value` 一行一条
//!   mmc-pack.json     有序的组件表，这一份才是实例的定义
//!   .minecraft/       游戏文件（老 MultiMC 叫 minecraft/）
//!   patches/*.json    被改过的组件，标准组件不在这里
//!   jarmods/          1.6 之前那种要叠进 client jar 的模组
//! ```
//!
//! ## 两件必须先说清楚的事
//!
//! **标准组件没有版本描述。** `patches/` 下面只有用户改过的那些；`net.minecraft`
//! 和 `net.minecraftforge` 的描述在 Prism 自己的全局 `meta/` 缓存里，不在实例
//! 目录下。所以导入拿得到的是「游戏版本 + 加载器 + 加载器版本」这一组事实，
//! 描述由 Fern 自己装一遍——反正装出来的是同一个上游产物。
//!
//! **我们不往它的目录里写东西。** 版本描述和库都留在 Fern 这边
//! （`ExternalGame::shared_versions`），那个 `.minecraft` 只当游戏目录用：存档、
//! 配置、mods 还在原地，Prism 打开它照样能玩。jar mod 会**复制**一份到 Fern 的
//! 实例目录下——启动要用它，而它随时可能被那边删掉。
//!
//! ## 组件表怎么读
//!
//! `mmc-pack.json` 的每一项有一个 `uid`。认得出来的映射成层，认不出来的**如实
//! 报出来**：少认一个加载器而不吭声，用户得到的是一个「导入成功」但游戏里什么
//! 模组都没有的实例。

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::{
    Component, DataPaths, ExternalGame, InstanceId, InstanceProfile, Isolation, LoaderKind,
};

/// 一个 Prism 实例，读出来的样子。**只读，不改任何东西。**
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrismInstance {
    /// 那个实例目录。
    pub directory: PathBuf,
    pub name: String,
    pub game_version: String,
    /// 认得出来的层，按 `mmc-pack.json` 里的顺序。
    pub components: Vec<Component>,
    /// 认不出来的层，写成 `uid version`。
    pub unsupported: Vec<String>,
    /// 它自己设的内存上限，MB。没覆盖过就是 `None`。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_memory_mb: Option<u32>,
    /// `jarmods/` 里那些文件。
    pub jar_mods: Vec<PathBuf>,
    /// 已经导入过了。再导一次只会得到两个共用同一份存档的实例。
    pub imported: bool,
}

#[derive(Debug, Deserialize)]
struct Pack {
    #[serde(default)]
    components: Vec<PackComponent>,
}

#[derive(Debug, Deserialize)]
struct PackComponent {
    uid: String,
    #[serde(default)]
    version: String,
}

/// 这一层是什么。
enum Known {
    /// 游戏本体。
    Game,
    /// 我们装得了的加载器。
    Loader(LoaderKind),
    /// 认得出来，但它只是别的层的依赖，不必单独成层——我们自己装加载器时
    /// 会把它带上。
    Implied,
}

fn classify(uid: &str) -> Option<Known> {
    Some(match uid {
        "net.minecraft" => Known::Game,
        "net.minecraftforge" => Known::Loader(LoaderKind::Forge),
        "net.neoforged" => Known::Loader(LoaderKind::NeoForge),
        "net.fabricmc.fabric-loader" => Known::Loader(LoaderKind::Fabric),
        "org.quiltmc.quilt-loader" => Known::Loader(LoaderKind::Quilt),
        // 加载器的依赖。Prism 把它们单列成组件，我们装加载器时它们自己会来。
        "net.fabricmc.intermediary"
        | "org.quiltmc.hashed"
        | "net.minecraft.java"
        | "org.lwjgl"
        | "org.lwjgl3" => Known::Implied,
        _ => return None,
    })
}

/// 这个目录是不是一个 Prism / MultiMC 实例。
pub fn looks_like_one(directory: &Path) -> bool {
    directory.join("mmc-pack.json").is_file()
}

/// 读一遍，什么都不改。
pub fn read(paths: &DataPaths, directory: &Path) -> Result<PrismInstance> {
    let directory = directory
        .canonicalize()
        .unwrap_or_else(|_| directory.to_path_buf());
    let pack_path = directory.join("mmc-pack.json");
    let pack: Pack = serde_json::from_slice(
        &std::fs::read(&pack_path).with_context(|| format!("读取 {}", pack_path.display()))?,
    )
    .with_context(|| format!("解析 {}", pack_path.display()))?;

    let settings = read_cfg(&directory.join("instance.cfg"));
    let mut game_version = String::new();
    let mut components = Vec::new();
    let mut unsupported = Vec::new();
    for entry in &pack.components {
        match classify(&entry.uid) {
            Some(Known::Game) => game_version = entry.version.clone(),
            Some(Known::Loader(kind)) => components.push(Component {
                kind,
                version: entry.version.clone(),
                version_id: String::new(),
                jar_mods: Vec::new(),
            }),
            Some(Known::Implied) => {}
            None => unsupported.push(format!("{} {}", entry.uid, entry.version).trim().to_owned()),
        }
    }
    if game_version.is_empty() {
        return Err(anyhow!("{} 里没说游戏版本", pack_path.display()));
    }

    let name = settings
        .get("name")
        .filter(|name| !name.is_empty())
        .cloned()
        .or_else(|| {
            directory
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| game_version.clone());
    // 只有勾了「覆盖内存设置」时那个数字才算数——没勾的话它是上一次打开设置
    // 面板时留下的残值，照搬会把一个跟着全局走的实例钉死在某个数上。
    let max_memory_mb = settings
        .get("OverrideMemory")
        .is_some_and(|value| value == "true")
        .then(|| settings.get("MaxMemAlloc").and_then(|mb| mb.parse().ok()))
        .flatten();

    Ok(PrismInstance {
        imported: already_imported(paths, &game_directory(&directory)),
        jar_mods: jar_mods(&directory),
        directory,
        name,
        game_version,
        components,
        unsupported,
        max_memory_mb,
    })
}

/// 导进来。原目录一个字节都不动，只读它、复制 jar mod。
pub fn import(paths: &DataPaths, directory: &Path) -> Result<InstanceProfile> {
    let read = read(paths, directory)?;
    let game = game_directory(&read.directory);
    if !game.is_dir() {
        return Err(anyhow!("{} 里没有游戏目录", read.directory.display()));
    }
    if read.imported {
        return Err(anyhow!("{} 已经导入过了", read.name));
    }

    paths
        .ensure_exists()
        .context("create launcher data directories")?;
    let id = crate::instance::catalog::allocate_id(paths)?;
    let mut profile =
        InstanceProfile::vanilla(InstanceId::parse(&id)?, &read.name, &read.game_version);
    profile.components = read.components;
    profile.settings.max_memory_mb = read.max_memory_mb;
    profile.external = Some(ExternalGame {
        root: game,
        // Prism 的实例目录下就是 `.minecraft`，存档和 mods 都在它根下。
        isolation: Isolation::Shared,
        shared_libraries: true,
        // 版本描述留在 Fern 这边：那个目录里本来就没有 `versions/`，见模块
        // 开头「不往它的目录里写东西」。
        shared_versions: true,
    });

    let instance_root = paths.instance_root(&id);
    std::fs::create_dir_all(&instance_root).context("create instance directory")?;
    // jar mod 复制一份过来：启动要用它，而原处那一份随时可能被 Prism 删掉。
    if !read.jar_mods.is_empty() {
        let destination = instance_root.join("jarmods");
        std::fs::create_dir_all(&destination)?;
        let mut copied = Vec::new();
        for source in &read.jar_mods {
            let Some(name) = source.file_name() else {
                continue;
            };
            let target = destination.join(name);
            std::fs::copy(source, &target).with_context(|| format!("复制 {}", source.display()))?;
            copied.push(target);
        }
        // 挂在最外面那一层上：jar mod 改的是 client jar，而它要盖在加载器
        // 之上才有意义（那个年代的 Forge 自己就是一份 jar mod）。
        match profile.components.last_mut() {
            Some(component) => component.jar_mods = copied,
            None => profile.components.push(Component {
                kind: LoaderKind::Vanilla,
                version: read.game_version.clone(),
                version_id: String::new(),
                jar_mods: copied,
            }),
        }
    }

    crate::write_instance_profile(paths, &profile)?;
    // 接手时它长什么样，记一笔。晚一步做，这句话就已经不成立了。
    crate::instance::integrity::adopt(paths, profile.id.as_str());
    crate::read_instance(paths, &id)
}

/// 游戏文件在哪一层。Prism 用 `.minecraft`，老 MultiMC 用 `minecraft`。
fn game_directory(directory: &Path) -> PathBuf {
    let modern = directory.join(".minecraft");
    if modern.is_dir() {
        return modern;
    }
    let legacy = directory.join("minecraft");
    if legacy.is_dir() {
        return legacy;
    }
    modern
}

fn jar_mods(directory: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(directory.join("jarmods"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    extension.eq_ignore_ascii_case("jar") || extension.eq_ignore_ascii_case("zip")
                })
        })
        .collect();
    // 目录遍历的顺序不保证，而叠加的顺序有语义。按名字排，和 Prism 的默认
    // 排序一致。
    found.sort();
    found
}

/// `key=value` 一行一条，`#` 开头是注释。
fn read_cfg(path: &Path) -> HashMap<String, String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.starts_with('#') || line.is_empty() {
                return None;
            }
            let (key, value) = line.split_once('=')?;
            Some((key.trim().to_owned(), value.trim().to_owned()))
        })
        .collect()
}

fn already_imported(paths: &DataPaths, game: &Path) -> bool {
    crate::list_instances(paths)
        .unwrap_or_default()
        .iter()
        .filter_map(|profile| profile.external.as_ref())
        .any(|external| external.root == game)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("create");
        std::fs::write(path, text).expect("write");
    }

    /// 一个真实形状的 Prism 实例：Forge 1.7.10 加一份 jar mod。
    fn prism(root: &Path) -> PathBuf {
        let directory = root.join("instances/Old Pack");
        write(
            &directory.join("instance.cfg"),
            "InstanceType=OneSix\nname=旧整合包\nOverrideMemory=true\nMaxMemAlloc=4096\n",
        );
        write(
            &directory.join("mmc-pack.json"),
            r#"{
              "formatVersion": 1,
              "components": [
                { "uid": "net.minecraft", "version": "1.7.10" },
                { "uid": "net.minecraftforge", "version": "10.13.4.1614" },
                { "uid": "com.mumfrey.liteloader", "version": "1.7.10" }
              ]
            }"#,
        );
        std::fs::create_dir_all(directory.join(".minecraft/saves/World")).expect("saves");
        write(&directory.join(".minecraft/options.txt"), "fov:1.0\n");
        write(&directory.join("jarmods/optifine.jar"), "not really a jar");
        directory
    }

    #[test]
    fn a_prism_instance_reads_as_an_ordered_stack() {
        let root = std::env::temp_dir().join(format!("fern-prism-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = DataPaths::new(&root);
        let directory = prism(&root);

        assert!(looks_like_one(&directory));
        let read = read(&paths, &directory).expect("read the instance");
        assert_eq!(read.name, "旧整合包");
        assert_eq!(read.game_version, "1.7.10");
        assert_eq!(read.components.len(), 1);
        assert_eq!(read.components[0].kind, LoaderKind::Forge);
        assert_eq!(read.components[0].version, "10.13.4.1614");
        // 装不了的那一层要如实说，不能悄悄丢掉。
        assert_eq!(read.unsupported, vec!["com.mumfrey.liteloader 1.7.10"]);
        assert_eq!(read.max_memory_mb, Some(4096));
        assert_eq!(read.jar_mods.len(), 1);

        std::fs::remove_dir_all(root).expect("remove root");
    }

    /// 导进来之后：游戏目录还是它自己的，版本描述归 Fern，jar mod 复制了一份。
    #[test]
    fn importing_takes_the_game_directory_and_nothing_else() {
        let root = std::env::temp_dir().join(format!("fern-prism-import-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = DataPaths::new(root.join("fern"));
        let directory = prism(&root);

        let profile = import(&paths, &directory).expect("import");
        assert_eq!(profile.name, "旧整合包");
        assert_eq!(profile.game_version, "1.7.10");
        assert_eq!(profile.loader, LoaderKind::Forge);
        assert_eq!(profile.settings.max_memory_mb, Some(4096));

        let external = profile.external.as_ref().expect("外部实例");
        assert_eq!(external.root, directory.join(".minecraft"));
        assert!(external.shared_versions, "版本描述不该写进别人的目录");

        // jar mod 复制到了 Fern 这边，而且挂在最外面那一层上。
        let copied = &profile.components.last().expect("有层").jar_mods;
        assert_eq!(copied.len(), 1);
        assert!(copied[0].starts_with(paths.instance_root(profile.id.as_str())));
        assert!(copied[0].is_file());

        // 那个目录里除了原来就有的，一个新文件都不该多出来。
        let inside: Vec<String> = std::fs::read_dir(&directory)
            .expect("read")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(!inside.contains(&"versions".to_owned()));
        assert!(!inside.contains(&"libraries".to_owned()));

        // 导第二次只会得到两个共用同一份存档的实例。
        assert!(import(&paths, &directory).is_err());

        std::fs::remove_dir_all(root).expect("remove root");
    }
}
