//! 把一个已经存在的 `.minecraft` 接进来。
//!
//! 大多数人用启动器的方式是把它和 `.minecraft` 放在一起，那个目录里已经有
//! 版本、有存档、有几百个 Mod。要求这样的用户先导出再导入，等于要求他放弃
//! 已有的一切，而这一步是不必要的：记下那个目录在哪、按哪种布局摆放，剩下的
//! 照常工作。
//!
//! 这一层只做两件事：**看**（`scan`）和**记**（`attach`）。不移动、不复制、
//! 不删除任何游戏文件——那些文件不归我们所有，这是整个模块的底线。
//!
//! 判断布局是这里最要紧的一件事。第三方启动器分裂出两种约定：
//!
//! ```text
//! 共用      .minecraft/saves        所有版本共享存档与 mods（官方启动器）
//! 版本隔离  .minecraft/versions/<id>/saves   每个版本一套（HMCL、PCL2 的默认）
//! ```
//!
//! **判断错了的后果是存档看起来消失了**——游戏会在另一个目录里新建一份空的。
//! 所以要真的去看目录里有什么，不能默认一种，也不能问用户（他多半不知道自己
//! 上一个启动器是怎么设的）。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::{
    DataPaths, InstanceId, InstanceProfile, LoaderKind, LoaderProfile,
    data::{ExternalGame, Isolation},
    launch::version::is_safe_id,
};

/// 目录里扫到的一个版本。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalVersion {
    /// `versions/` 下面那个目录名，也是版本描述的 id。
    pub id: String,
    /// 它最终继承到的原版版本。装了加载器时和 `id` 不同。
    pub game_version: String,
    pub loader: LoaderKind,
    /// 加载器自己的版本号，认得出来才有。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loader_version: Option<String>,
    /// 这个版本按哪种布局摆放。同一个目录里的不同版本可以不一样。
    pub isolation: Isolation,
    /// 已经有实例指向它了。再添加一次只会得到两个共用同一份存档的实例。
    pub attached: bool,
    /// 这个版本目录下有几个存档、几个 Mod。用来说「这不是一个空目录」。
    pub saves: u32,
    pub mods: u32,
}

/// 扫描的结果。
///
/// 除了扫到什么，还要说清楚**没扫到的是什么**。一个装了几十个版本的目录里
/// 总有几个目录是别的东西——残留、备份、启动器自己的文件夹。上一版把它们
/// 一律静默跳过，于是全部被跳过时界面上只剩一句「没有可用的版本」，用户和
/// 我们都无从判断是目录选错了、还是我们读不懂。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalScan {
    /// 真正读的那个目录。用户选了它的上一层时，这里是解析之后的结果。
    pub root: PathBuf,
    pub versions: Vec<ExternalVersion>,
    pub skipped: Vec<SkippedVersion>,
}

/// `versions/` 下面一个没能成为版本的目录，以及原因。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedVersion {
    pub name: String,
    pub reason: String,
}

/// 看一眼那个目录里有什么。不改任何东西。
pub fn scan(paths: &DataPaths, root: &Path) -> Result<ExternalScan> {
    let root = locate(root)?;
    let versions = root.join("versions");

    let claimed = claimed_versions(paths, &root);
    let mut found = Vec::new();
    let mut skipped = Vec::new();
    for entry in std::fs::read_dir(&versions).context("读取 versions 目录")? {
        let entry = entry.context("读取 versions 里的条目")?;
        if !entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
            continue;
        }
        let id = entry.file_name().to_string_lossy().into_owned();
        let mut skip = |reason: String| {
            skipped.push(SkippedVersion {
                name: id.clone(),
                reason,
            })
        };

        // 目录名会被拼进路径——它来自别人的磁盘，照样过关口。
        if !is_safe_id(&id) {
            skip("目录名无法作为路径使用".to_owned());
            continue;
        }
        let json = entry.path().join(format!("{id}.json"));
        if !json.is_file() {
            // 有目录没描述的多半是删了一半的残留，不是一个能启动的版本。
            skip(format!("缺少 {id}.json"));
            continue;
        }
        let Some(described) = describe(&json, &id) else {
            skip(format!("{id}.json 无法解析"));
            continue;
        };
        let isolation = detect_isolation(&root, &id);
        let game = game_directory(&root, &id, isolation);
        found.push(ExternalVersion {
            attached: claimed.contains(&id),
            saves: count_worlds(&game.join("saves")),
            mods: count_mods(&game.join("mods")),
            isolation,
            ..described
        });
    }
    found.sort_by(|left, right| left.id.cmp(&right.id));
    skipped.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(ExternalScan {
        root,
        versions: found,
        skipped,
    })
}

/// 把其中一个版本添加为实例。
///
/// 只写入一份指向该目录的实例描述，不复制任何文件。
pub fn attach(
    paths: &DataPaths,
    root: &Path,
    version_id: &str,
    shared_libraries: bool,
) -> Result<InstanceProfile> {
    let root = locate(root)?;
    if !is_safe_id(version_id) {
        return Err(anyhow!("版本 id 不可用作目录名：{version_id}"));
    }
    let json = root
        .join("versions")
        .join(version_id)
        .join(format!("{version_id}.json"));
    let described =
        describe(&json, version_id).ok_or_else(|| anyhow!("读不懂 {}", json.display()))?;

    if claimed_versions(paths, &root).contains(&version_id.to_owned()) {
        return Err(anyhow!("{version_id} 已经添加过了"));
    }

    paths
        .ensure_exists()
        .context("create launcher data directories")?;
    let id = crate::instance::catalog::allocate_id(paths)?;
    let mut profile = InstanceProfile::vanilla(
        InstanceId::parse(&id)?,
        display_name(version_id, &described),
        &described.game_version,
    );
    profile.components.extend(
        described
            .loader
            .ne(&LoaderKind::Vanilla)
            .then(|| LoaderProfile {
                kind: described.loader,
                version: described.loader_version.clone().unwrap_or_default(),
                version_id: version_id.to_owned(),
            }),
    );
    profile = profile.normalized();
    profile.external = Some(ExternalGame {
        root: root.clone(),
        isolation: detect_isolation(&root, version_id),
        shared_libraries,
    });

    // 实例目录里只有一份描述，没有 `.minecraft`：游戏文件在那个外部目录里。
    std::fs::create_dir_all(paths.instance_root(&id)).context("create instance directory")?;
    crate::write_instance_profile(paths, &profile)?;

    // 清点一次这个目录里现在有什么。那些文件从哪来我们不知道——那段历史发生
    // 在 Fern 之前——但从这一刻起的每一次变化都有了参照物。
    //
    // 要读一遍全部 jar 才算得出哈希，几百个模组是几秒钟。它不能放在别处：晚
    // 一步做，「接手时它长这样」这句话就已经不成立了。
    crate::instance::integrity::adopt(paths, profile.id.as_str());
    Ok(profile)
}

/// 这个目录里哪些版本已经有实例指向它们。
fn claimed_versions(paths: &DataPaths, root: &Path) -> Vec<String> {
    crate::list_instances(paths)
        .unwrap_or_default()
        .into_iter()
        .filter(|profile| {
            profile
                .external
                .as_ref()
                .is_some_and(|external| external.root == root)
        })
        .map(|profile| crate::effective_version_id(&profile))
        .collect()
}

/// 从版本描述里读出「它是什么」。
///
/// 不走完整的 `version::resolve`：那要把整条继承链读出来合并，而这里只要几个
/// 字段，而且要能在一个我们还没接管的目录上跑——那里的父版本可能压根不存在。
fn describe(json: &Path, id: &str) -> Option<ExternalVersion> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Raw {
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        inherits_from: Option<String>,
        #[serde(default)]
        main_class: Option<String>,
        #[serde(default)]
        libraries: Vec<RawLibrary>,
    }

    #[derive(Deserialize)]
    struct RawLibrary {
        #[serde(default)]
        name: Option<String>,
    }

    let raw: Raw = serde_json::from_slice(&std::fs::read(json).ok()?).ok()?;
    let names: Vec<String> = raw
        .libraries
        .iter()
        .filter_map(|library| library.name.clone())
        .collect();
    let (loader, loader_version) = detect_loader(&names, raw.main_class.as_deref(), id);

    Some(ExternalVersion {
        game_version: detect_game_version(&raw.inherits_from, &names, json, raw.id, id),
        id: id.to_owned(),
        loader,
        loader_version,
        isolation: Isolation::default(),
        attached: false,
        saves: 0,
        mods: 0,
    })
}

/// 这个版本对应的原版版本号是哪一个。
///
/// 有 `inheritsFrom` 就是它，这是最省事的一条，也是我们自己装加载器时写的那条。
/// 麻烦的是**没有继承关系的那一份**：不少启动器（以及整合包作者）把原版和加载器
/// 合并成一整份 JSON 写进 `versions/<名字>/`，里面的 `id` 也一并改成了那个名字。
/// 那时候照着 `id` 读，得到的是「Simply Craftmine」这样一个实例名——它会被当成
/// 游戏版本传给 Java 版本判断、模组搜索、崩溃归因，每一处都会得出错的结论。
///
/// 所以按可信度往下找：
///
/// 1. `inheritsFrom`——启动器明确写下的。
/// 2. client jar 里的 `version.json`——**游戏自己写的**，1.14 起每个 jar 都带。
///    目录名和 JSON 里的 id 都是启动器写的、用户能改的，只有这一份不是。
/// 3. 库坐标——`net.fabricmc:intermediary:25w14craftmine` 里的版本号就是游戏
///    版本，Quilt、Forge、NeoForge 各有一条同样性质的坐标。
/// 4. 实在认不出来，才退回 JSON 里的 id 或目录名。
fn detect_game_version(
    inherits_from: &Option<String>,
    libraries: &[String],
    json: &Path,
    declared_id: Option<String>,
    directory_id: &str,
) -> String {
    if let Some(parent) = inherits_from.clone().filter(|it| !it.is_empty()) {
        return parent;
    }
    if let Some(version) = json
        .parent()
        .map(|directory| directory.join(format!("{directory_id}.jar")))
        .and_then(|jar| version_inside_jar(&jar))
    {
        return version;
    }
    if let Some(version) = version_from_libraries(libraries) {
        return version;
    }
    declared_id.unwrap_or_else(|| directory_id.to_owned())
}

/// client jar 里那份 `version.json` 说自己是哪个版本。
fn version_inside_jar(jar: &Path) -> Option<String> {
    #[derive(Deserialize)]
    struct Stamp {
        id: String,
    }

    let file = std::fs::File::open(jar).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    let entry = archive.by_name("version.json").ok()?;
    let stamp: Stamp = serde_json::from_reader(entry).ok()?;
    plausible_version(stamp.id)
}

/// 加载器的库坐标里带着的游戏版本。
///
/// 认的是坐标而不是目录名：`net.fabricmc:intermediary:<版本>` 是 Fabric 自己
/// 写进去的，用户改不到，也不会因为整合包换了个名字而变。
fn version_from_libraries(libraries: &[String]) -> Option<String> {
    // 越靠前越可信：前两条的版本段**就是**游戏版本，后面几条要切掉尾巴。
    const EXACT: [&str; 2] = ["net.fabricmc:intermediary:", "org.quiltmc:hashed:"];
    // `net.minecraft:client:1.20.1-20230612.114412`、
    // `net.minecraftforge:forge:1.20.1-47.2.0`、
    // `net.neoforged:neoform:1.21.1-20240808.144430`——横杠前面那段是游戏版本。
    const BEFORE_DASH: [&str; 3] = [
        "net.minecraft:client:",
        "net.neoforged:neoform:",
        "net.minecraftforge:forge:",
    ];
    // Yarn 的版本形如 `1.21.1+build.3`。
    const BEFORE_PLUS: [&str; 1] = ["net.fabricmc:yarn:"];

    let segment = |marker: &str, cut: Option<char>| {
        libraries.iter().find_map(|name| {
            let rest = name.strip_prefix(marker)?;
            let version = rest.split(':').next()?;
            let version = match cut {
                Some(separator) => version.split(separator).next()?,
                None => version,
            };
            plausible_version(version.to_owned())
        })
    };

    EXACT
        .iter()
        .find_map(|marker| segment(marker, None))
        .or_else(|| {
            BEFORE_DASH
                .iter()
                .find_map(|marker| segment(marker, Some('-')))
        })
        .or_else(|| {
            BEFORE_PLUS
                .iter()
                .find_map(|marker| segment(marker, Some('+')))
        })
}

/// 认出来的东西还要能当版本号用：它会被拼进路径，也会被送去搜模组。
fn plausible_version(version: String) -> Option<String> {
    let version = version.trim().to_owned();
    (!version.is_empty() && version.len() <= 64 && is_safe_id(&version)).then_some(version)
}

/// 从库坐标认加载器。
///
/// 认库而不是认目录名：`1.20.1-forge-47.2.0` 这种名字是启动器起的，用户可以
/// 随手改成「我的整合包」，而 `net.minecraftforge:forge:` 这条坐标是 Forge
/// 自己写进去的。
fn detect_loader(
    libraries: &[String],
    main_class: Option<&str>,
    id: &str,
) -> (LoaderKind, Option<String>) {
    const MARKERS: [(&str, LoaderKind); 5] = [
        ("net.neoforged:neoforge:", LoaderKind::NeoForge),
        ("net.minecraftforge:forge:", LoaderKind::Forge),
        ("org.quiltmc:quilt-loader:", LoaderKind::Quilt),
        ("net.fabricmc:fabric-loader:", LoaderKind::Fabric),
        ("net.neoforged.fancymodloader:", LoaderKind::NeoForge),
    ];
    for name in libraries {
        for (marker, kind) in MARKERS {
            if let Some(rest) = name.strip_prefix(marker) {
                let version = rest.split(':').next().filter(|it| !it.is_empty());
                return (kind, version.map(str::to_owned));
            }
        }
    }
    // 库里认不出来时退回主类：老版本 Forge 的坐标形状不一样，但主类是稳定的。
    match main_class {
        Some(class) if class.contains("fml") || class.contains("forge") => {
            (LoaderKind::Forge, None)
        }
        Some(class) if class.contains("knot") || class.contains("fabric") => {
            (LoaderKind::Fabric, None)
        }
        // 最后才看名字。它是最不可靠的一条，所以排在最后。
        _ if id.to_ascii_lowercase().contains("optifine") => (LoaderKind::Vanilla, None),
        _ => (LoaderKind::Vanilla, None),
    }
}

/// 这个版本按哪种布局摆放。
///
/// 判据是**哪一边真的有东西**：版本目录下有 saves/mods/config 就是版本隔离，
/// 否则按共用算。这比问用户可靠——他多半不知道自己上一个启动器是怎么设的，
/// 而这件事看一眼目录就知道。
fn detect_isolation(root: &Path, version_id: &str) -> Isolation {
    let per_version = root.join("versions").join(version_id);
    let occupied = ["saves", "mods", "config", "resourcepacks", "options.txt"]
        .iter()
        .any(|name| per_version.join(name).exists());
    if occupied {
        Isolation::PerVersion
    } else {
        Isolation::Shared
    }
}

fn game_directory(root: &Path, version_id: &str, isolation: Isolation) -> PathBuf {
    match isolation {
        Isolation::Shared => root.to_path_buf(),
        Isolation::PerVersion => root.join("versions").join(version_id),
    }
}

/// 有几个世界。认 `level.dat` 而不是数目录——`saves/` 下面常有备份、截图
/// 和别的工具留下的东西，把它们算成存档会让这一行说出一个假的数。
fn count_worlds(directory: &Path) -> u32 {
    std::fs::read_dir(directory)
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.path().join("level.dat").is_file())
                .count() as u32
        })
        .unwrap_or(0)
}

/// 有几个启用的模组。`.disabled` 的不算：加载器不读它们。
fn count_mods(directory: &Path) -> u32 {
    std::fs::read_dir(directory)
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .to_ascii_lowercase()
                        .ends_with(".jar")
                })
                .count() as u32
        })
        .unwrap_or(0)
}

/// 添加时的实例名称。版本 id 通常已经足够，原版的加一句来源。
fn display_name(version_id: &str, described: &ExternalVersion) -> String {
    if described.loader == LoaderKind::Vanilla && version_id == described.game_version {
        format!("{version_id}（现有目录）")
    } else {
        version_id.to_owned()
    }
}

/// 用户选的目录，解析成真正要读的那一个。
///
/// 选高一层是最常见的一件事：目录选择器打开时看到的正是**包含** `.minecraft`
/// 的那个文件夹，点一下确定就选中了它。里面正好有一个 `.minecraft` 时不必让
/// 用户重来一次。
fn locate(root: &Path) -> Result<PathBuf> {
    let root = normalise(root)?;
    if root.join("versions").is_dir() {
        return Ok(root);
    }
    let nested = root.join(".minecraft");
    if nested.join("versions").is_dir() {
        return normalise(&nested);
    }
    Err(anyhow!(
        "{} 里没有 versions 目录，它不像一个游戏目录",
        root.display()
    ))
}

/// 绝对化，并挡住不存在的路径。
///
/// 存进实例描述的必须是绝对路径：相对路径会随工作目录漂移，而启动器的工作
/// 目录在不同的启动方式下并不一样。
fn normalise(root: &Path) -> Result<PathBuf> {
    if !root.is_dir() {
        return Err(anyhow!("{} 不是一个目录", root.display()));
    }
    let real = std::fs::canonicalize(root).with_context(|| format!("解析 {}", root.display()))?;
    Ok(plain(real))
}

/// 去掉 Windows 上 `canonicalize` 加的 `\\?\` 前缀。
///
/// 这个路径要显示在界面上、写进实例描述，还要作为工作目录交给 Java。带着
/// 前缀的形式很多程序处理不了，而它只有在路径超过 260 个字符时才是必需的。
fn plain(path: PathBuf) -> PathBuf {
    if !cfg!(windows) {
        return path;
    }
    match path.to_str().and_then(without_verbatim) {
        Some(text) => PathBuf::from(text),
        None => path,
    }
}

fn without_verbatim(text: &str) -> Option<String> {
    let rest = text.strip_prefix(r"\\?\")?;
    // 网络路径去掉前缀之后要把两条反斜杠补回来，否则指向的就不是同一个地方了。
    Some(match rest.strip_prefix(r"UNC\") {
        Some(share) => format!(r"\\{share}"),
        None => rest.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("fern-external-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        path
    }

    fn write_version(root: &Path, id: &str, json: serde_json::Value) {
        let directory = root.join("versions").join(id);
        std::fs::create_dir_all(&directory).expect("create version directory");
        std::fs::write(
            directory.join(format!("{id}.json")),
            serde_json::to_vec(&json).expect("serialize"),
        )
        .expect("write version json");
    }

    #[test]
    fn a_vanilla_directory_scans_into_one_version() {
        let root = temporary("vanilla");
        write_version(&root, "1.21.1", serde_json::json!({"id": "1.21.1"}));
        let paths = DataPaths::new(root.join("fern-data"));

        let found = scan(&paths, &root).expect("scan").versions;
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "1.21.1");
        assert_eq!(found[0].game_version, "1.21.1");
        assert_eq!(found[0].loader, LoaderKind::Vanilla);
        assert_eq!(found[0].isolation, Isolation::Shared);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_loader_is_recognised_from_its_library_coordinates() {
        let root = temporary("loader");
        write_version(
            &root,
            "my-pack",
            serde_json::json!({
                "id": "my-pack",
                "inheritsFrom": "1.20.1",
                "libraries": [
                    {"name": "net.minecraftforge:forge:1.20.1-47.2.0:universal"},
                    {"name": "org.ow2.asm:asm:9.5"}
                ]
            }),
        );
        let paths = DataPaths::new(root.join("fern-data"));

        let found = scan(&paths, &root).expect("scan").versions;
        assert_eq!(found[0].loader, LoaderKind::Forge);
        assert_eq!(found[0].loader_version.as_deref(), Some("1.20.1-47.2.0"));
        // 目录名是用户可以随手改的，版本要从 inheritsFrom 读。
        assert_eq!(found[0].game_version, "1.20.1");
        std::fs::remove_dir_all(&root).ok();
    }

    /// 一份合并好的 JSON 里没有 `inheritsFrom`，`id` 也被改成了整合包的名字。
    /// 照着它读，「Simply Craftmine」就成了游戏版本——Java 版本判断、模组搜索、
    /// 崩溃归因全都会照着这个不存在的版本号去做。
    #[test]
    fn a_flat_version_json_still_yields_the_real_game_version() {
        let root = temporary("flat");
        write_version(
            &root,
            "Simply Craftmine",
            serde_json::json!({
                "id": "Simply Craftmine",
                "mainClass": "net.fabricmc.loader.impl.launch.knot.KnotClient",
                "libraries": [
                    {"name": "org.ow2.asm:asm:9.9"},
                    {"name": "net.fabricmc:intermediary:25w14craftmine"},
                    {"name": "net.fabricmc:fabric-loader:0.18.4"}
                ]
            }),
        );
        let paths = DataPaths::new(root.join("fern-data"));

        let found = scan(&paths, &root).expect("scan").versions;
        assert_eq!(found[0].id, "Simply Craftmine");
        assert_eq!(found[0].game_version, "25w14craftmine");
        assert_eq!(found[0].loader, LoaderKind::Fabric);
        assert_eq!(found[0].loader_version.as_deref(), Some("0.18.4"));
        std::fs::remove_dir_all(&root).ok();
    }

    /// 游戏自己在 client jar 里写了版本号。它比目录名、比 JSON 里的 id 都可信
    /// ——后两者是启动器写的，而用户可以改名。
    #[test]
    fn the_client_jar_says_which_version_it_is() {
        use std::io::Write;

        let root = temporary("stamped");
        write_version(
            &root,
            "我的整合包",
            serde_json::json!({"id": "我的整合包", "libraries": []}),
        );
        let jar = root.join("versions/我的整合包/我的整合包.jar");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&jar).expect("create jar"));
        writer
            .start_file("version.json", zip::write::SimpleFileOptions::default())
            .expect("start entry");
        writer
            .write_all(br#"{"id":"1.20.1","name":"1.20.1","world_version":3465}"#)
            .expect("write entry");
        writer.finish().expect("finish jar");
        let paths = DataPaths::new(root.join("fern-data"));

        let found = scan(&paths, &root).expect("scan").versions;
        assert_eq!(found[0].game_version, "1.20.1");
        std::fs::remove_dir_all(&root).ok();
    }

    /// 认不出来的时候要老老实实退回原来那个名字，不能瞎猜一个。
    #[test]
    fn an_unrecognisable_version_keeps_its_own_name() {
        let root = temporary("unknown");
        write_version(
            &root,
            "谜之版本",
            serde_json::json!({"id": "谜之版本", "libraries": [{"name": "org.ow2.asm:asm:9.6"}]}),
        );
        let paths = DataPaths::new(root.join("fern-data"));

        let found = scan(&paths, &root).expect("scan").versions;
        assert_eq!(found[0].game_version, "谜之版本");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn version_isolation_is_detected_from_what_is_actually_there() {
        let root = temporary("isolation");
        write_version(&root, "1.21.1", serde_json::json!({"id": "1.21.1"}));
        write_version(&root, "packed", serde_json::json!({"id": "packed"}));
        // 共用布局的存档在根下。
        std::fs::create_dir_all(root.join("saves/world")).expect("create shared saves");
        std::fs::write(root.join("saves/world/level.dat"), b"x").expect("write level.dat");
        // 版本隔离的那一个自己带着 saves。
        std::fs::create_dir_all(root.join("versions/packed/saves/other")).expect("create saves");
        std::fs::write(root.join("versions/packed/saves/other/level.dat"), b"x")
            .expect("write level.dat");
        // saves 下面的备份不是存档，不该被算进去。
        std::fs::create_dir_all(root.join("saves/backups")).expect("create backups");
        let paths = DataPaths::new(root.join("fern-data"));

        let found = scan(&paths, &root).expect("scan").versions;
        let by_id = |id: &str| {
            found
                .iter()
                .find(|item| item.id == id)
                .expect("version")
                .clone()
        };
        assert_eq!(by_id("1.21.1").isolation, Isolation::Shared);
        assert_eq!(by_id("packed").isolation, Isolation::PerVersion);
        // 存档数量按各自的游戏目录算。
        assert_eq!(by_id("1.21.1").saves, 1);
        assert_eq!(by_id("packed").saves, 1);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn attaching_copies_nothing_and_claims_the_version_once() {
        let root = temporary("attach");
        write_version(&root, "1.21.1", serde_json::json!({"id": "1.21.1"}));
        std::fs::create_dir_all(root.join("saves/world")).expect("create saves");
        let paths = DataPaths::new(root.join("fern-data"));

        let profile = attach(&paths, &root, "1.21.1", true).expect("attach");
        let external = profile.external.as_ref().expect("external");
        assert_eq!(external.isolation, Isolation::Shared);
        // 实例目录里不该长出一个 .minecraft：游戏文件在别人那边。
        assert!(
            !paths
                .instance_root(profile.id.as_str())
                .join(".minecraft")
                .exists()
        );
        // 那边的存档一个都没动。
        assert!(root.join("saves/world").is_dir());

        // 添加过的版本再扫一次会被标出来，而且不能添加第二次。
        let found = scan(&paths, &root).expect("rescan").versions;
        assert!(found[0].attached);
        assert!(attach(&paths, &root, "1.21.1", true).is_err());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_directory_without_versions_is_refused_with_a_reason() {
        let root = temporary("empty");
        std::fs::create_dir_all(&root).expect("create root");
        let paths = DataPaths::new(root.join("fern-data"));
        let error = scan(&paths, &root).expect_err("not a game directory");
        assert!(format!("{error}").contains("versions"));
        std::fs::remove_dir_all(&root).ok();
    }

    /// 版本名是人起的，不归我们管。
    ///
    /// 这是「选了 `.minecraft` 却一个版本都扫不出来」的原因：早先套用的是我们
    /// 自己下载版本时那条只认 ASCII 的规则，于是带空格的、中文的全被丢掉，
    /// 界面上只剩一句「没有可用的版本」。
    #[test]
    fn versions_named_by_a_human_are_not_thrown_away() {
        let root = temporary("named");
        for id in ["1.20.1-Fabric 0.16.9", "空岛生存", "1.21.1"] {
            write_version(&root, id, serde_json::json!({"id": id}));
        }
        // 只有目录、没有描述的那些仍然不算版本，但要说得出为什么。
        std::fs::create_dir_all(root.join("versions/半个残留")).expect("create leftovers");
        let paths = DataPaths::new(root.join("fern-data"));

        let scanned = scan(&paths, &root).expect("scan");
        let mut names: Vec<&str> = scanned
            .versions
            .iter()
            .map(|item| item.id.as_str())
            .collect();
        names.sort_unstable();
        assert_eq!(names, ["1.20.1-Fabric 0.16.9", "1.21.1", "空岛生存"]);
        assert_eq!(scanned.skipped.len(), 1);
        assert_eq!(scanned.skipped[0].name, "半个残留");
        assert!(scanned.skipped[0].reason.contains("半个残留.json"));

        // 名字照样过关口：能跳出这个目录的一律不放行。
        assert!(!is_safe_id("../escape"));
        assert!(!is_safe_id("a/b"));
        assert!(!is_safe_id(".."));
        assert!(is_safe_id("1.20.1-Fabric 0.16.9"));
        std::fs::remove_dir_all(&root).ok();
    }

    /// 目录选择器打开时看到的是**包含** `.minecraft` 的那个文件夹。
    #[test]
    fn choosing_the_folder_that_contains_minecraft_still_works() {
        let root = temporary("parent");
        let game = root.join(".minecraft");
        write_version(&game, "1.21.1", serde_json::json!({"id": "1.21.1"}));
        let paths = DataPaths::new(root.join("fern-data"));

        let scanned = scan(&paths, &root).expect("scan");
        assert_eq!(
            scanned.root,
            std::fs::canonicalize(&game).expect("real path")
        );
        assert_eq!(scanned.versions.len(), 1);
        // 添加时同样解析，两边指向的是同一个目录。
        let profile = attach(&paths, &root, "1.21.1", true).expect("attach");
        assert_eq!(profile.external.expect("external").root, scanned.root);
        std::fs::remove_dir_all(&root).ok();
    }

    /// 字段名要和界面上那一份 TypeScript 对得上。这条链路编译期没有任何
    /// 检查——名字错了只会安静地渲染出一片 undefined。
    #[test]
    fn the_scan_reaches_the_interface_in_the_shape_it_expects() {
        let root = temporary("shape");
        write_version(&root, "1.21.1", serde_json::json!({"id": "1.21.1"}));
        std::fs::create_dir_all(root.join("versions/残留")).expect("create leftovers");
        let paths = DataPaths::new(root.join("fern-data"));

        let json = serde_json::to_value(scan(&paths, &root).expect("scan")).expect("serialize");
        let version = &json["versions"][0];
        for key in [
            "id",
            "gameVersion",
            "loader",
            "isolation",
            "attached",
            "saves",
            "mods",
        ] {
            assert!(!version[key].is_null(), "versions[0].{key} 应当存在");
        }
        assert!(!json["root"].is_null());
        assert_eq!(json["skipped"][0]["name"], "残留");
        assert!(json["skipped"][0]["reason"].is_string());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_windows_verbatim_prefix_is_stripped() {
        assert_eq!(
            without_verbatim(r"\\?\C:\Games\.minecraft").as_deref(),
            Some(r"C:\Games\.minecraft")
        );
        assert_eq!(
            without_verbatim(r"\\?\UNC\nas\games\.minecraft").as_deref(),
            Some(r"\\nas\games\.minecraft")
        );
        assert_eq!(without_verbatim(r"C:\Games\.minecraft"), None);
    }

    #[test]
    fn version_ids_from_someone_elses_disk_still_pass_the_gate() {
        // 目录名会被拼进路径。它来自别人的磁盘，和从网上拿到的字符串一样
        // 不可信。
        let root = temporary("unsafe");
        std::fs::create_dir_all(root.join("versions")).expect("create versions");
        let paths = DataPaths::new(root.join("fern-data"));
        assert!(attach(&paths, &root, "../escape", true).is_err());
        std::fs::remove_dir_all(&root).ok();
    }
}
