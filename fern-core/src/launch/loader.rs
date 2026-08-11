//! Mod 加载器的安装（文档 §2.5，第一阶段）。
//!
//! Fabric 和 Quilt 是纯数据操作：向 meta server 要一份 profile JSON，那份 JSON
//! 用 `inheritsFrom` 指回原版，只写自己改动的部分（mainClass、几个库、几条
//! 参数）。我们把它落盘，剩下的事——合并、补全、拼 classpath——已经有的那套
//! 全都能直接用。
//!
//! NeoForge 和旧 Forge 不在这里：它们的安装期要真的跑起若干个 Java 进程做
//! deobf 和 patch，那是另一个量级的工作，等这套框架稳了再说。

use anyhow::{Context, Result, anyhow};
use fern_download::{DownloadClient, DownloadEvent};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedSender;

use crate::{
    DataPaths, LoaderKind,
    data::{
        metacache::{self, Freshness},
        settings::source_order,
    },
    launch::version,
};

/// 版本列表在缓存目录里的名字。
///
/// Fabric / Quilt 的列表是按游戏版本查的，NeoForge / Forge 那两个 Maven 列表
/// 是全量的——一份缓存能服务所有游戏版本，所以名字里不带版本号。
fn listing_slug(kind: LoaderKind, game_version: &str) -> String {
    match kind {
        LoaderKind::NeoForge => "loader-neoforge-versions.json".to_owned(),
        LoaderKind::Forge => "loader-forge-maven-metadata.xml".to_owned(),
        other => format!(
            "loader-{}-{game_version}.json",
            display_name(other).to_lowercase()
        ),
    }
}

/// 版本列表，走缓存。
async fn listing(
    paths: &DataPaths,
    client: &DownloadClient,
    kind: LoaderKind,
    game_version: &str,
    url: &str,
) -> Result<Vec<u8>> {
    let cached = metacache::mutable(
        client,
        paths,
        &listing_slug(kind, game_version),
        url,
        Freshness::Within(metacache::LISTING_TTL),
    )
    .await
    .with_context(|| format!("读取 {} 的版本列表", display_name(kind)))?;
    Ok(cached.bytes)
}

/// 一个可选的加载器版本。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoaderVersion {
    pub version: String,
    /// 上游标记为稳定版。界面默认只给这些，尝鲜的要自己展开。
    pub stable: bool,
}

/// meta server 的列表条目。只取我们用得上的那一层。
#[derive(Debug, Deserialize)]
struct LoaderEntry {
    loader: LoaderDescriptor,
}

#[derive(Debug, Deserialize)]
struct LoaderDescriptor {
    version: String,
    #[serde(default)]
    stable: bool,
}

/// 这个加载器的 meta server 根地址。
///
/// 两家的 API 形状一致（v2/v3 的差别只在路径），所以下面的代码不必分叉。
fn meta_root(kind: LoaderKind) -> Result<&'static str> {
    match kind {
        LoaderKind::Fabric => Ok("https://meta.fabricmc.net/v2"),
        LoaderKind::Quilt => Ok("https://meta.quiltmc.org/v3"),
        LoaderKind::Vanilla => Err(anyhow!("原版不需要安装加载器")),
        // 这两家没有 meta server，走 installer，见 forge 模块。
        LoaderKind::NeoForge | LoaderKind::Forge => Err(anyhow!("{kind:?} 不使用 meta server")),
    }
}

/// NeoForge / Forge 的版本从 Maven 仓库列出来，没有 meta server。
async fn list_maven_versions(
    paths: &DataPaths,
    kind: LoaderKind,
    game_version: &str,
) -> Result<Vec<LoaderVersion>> {
    let client = DownloadClient::new(source_order(), 4);
    match kind {
        LoaderKind::NeoForge => {
            #[derive(Deserialize)]
            struct Listing {
                versions: Vec<String>,
            }
            let bytes = listing(
                paths,
                &client,
                kind,
                game_version,
                "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge",
            )
            .await?;
            let listing: Listing =
                serde_json::from_slice(&bytes).context("解析 NeoForge 的版本列表")?;
            // NeoForge 的版本号前两段对应游戏版本：1.21.1 → 21.1.x。
            let prefix = neoforge_prefix(game_version)
                .ok_or_else(|| anyhow!("NeoForge 不支持 {game_version}"))?;
            Ok(listing
                .versions
                .into_iter()
                .filter(|version| version.starts_with(&prefix))
                .rev()
                .map(|version| {
                    let stable = !version.contains("beta") && !version.contains("alpha");
                    LoaderVersion { version, stable }
                })
                .collect())
        }
        LoaderKind::Forge => {
            // Forge 只有 maven-metadata.xml。条目形如 `1.12.2-14.23.5.2859`，
            // 不引 XML 解析器，按标签切出来就够。
            let bytes = listing(
                paths,
                &client,
                kind,
                game_version,
                "https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml",
            )
            .await?;
            let text = String::from_utf8_lossy(&bytes);
            let wanted = format!("{game_version}-");
            let mut versions: Vec<LoaderVersion> = text
                .split("<version>")
                .skip(1)
                .filter_map(|chunk| chunk.split("</version>").next())
                .filter_map(|entry| entry.strip_prefix(&wanted))
                .map(|version| LoaderVersion {
                    version: version.to_owned(),
                    // Forge 不标稳定性；同一游戏版本的都当稳定。
                    stable: true,
                })
                .collect();
            // 自己排，别信文件顺序。maven-metadata 大体是按发布时间来的，但
            // 1.7.2 那一段是倒着的——照单反转，列表第一条（也就是新建实例的
            // 默认值）会变成 2014 年的第一个构建。
            versions.sort_by(|left, right| {
                forge_ordinal(&right.version).cmp(&forge_ordinal(&left.version))
            });
            Ok(versions)
        }
        other => Err(anyhow!("{other:?} 的版本不从 Maven 列")),
    }
}

/// Forge 版本号排序用的键。
///
/// 形如 `10.12.2.1161-mc172`：几段数字，偶尔跟一个后缀。按段比数字，比不动
/// 的（后缀）留到最后按字典序——`1161-mc172` 比 `1161` 新。
fn forge_ordinal(version: &str) -> (Vec<u64>, &str) {
    let numeric = version
        .split('.')
        .map(|segment| {
            let digits = segment
                .split(|character: char| !character.is_ascii_digit())
                .next()
                .unwrap_or_default();
            digits.parse::<u64>().unwrap_or_default()
        })
        .collect();
    (numeric, version)
}

/// `1.21.1` → `21.1.`，`1.21` → `21.0.`
fn neoforge_prefix(game_version: &str) -> Option<String> {
    let (major, minor, patch) = fern_meta::release_ordinal(game_version)?;
    if major != 1 {
        return None;
    }
    Some(format!("{minor}.{patch}."))
}

/// 这个游戏版本上可用的加载器版本，新的在前。
pub async fn list_versions(
    paths: &DataPaths,
    kind: LoaderKind,
    game_version: &str,
) -> Result<Vec<LoaderVersion>> {
    if matches!(kind, LoaderKind::NeoForge | LoaderKind::Forge) {
        return list_maven_versions(paths, kind, game_version).await;
    }
    let url = format!("{}/versions/loader/{game_version}", meta_root(kind)?);
    let client = DownloadClient::new(source_order(), 4);
    let bytes = listing(paths, &client, kind, game_version, &url).await?;
    let entries: Vec<LoaderEntry> =
        serde_json::from_slice(&bytes).with_context(|| format!("解析 {kind:?} 的版本列表"))?;
    Ok(entries
        .into_iter()
        .map(|entry| LoaderVersion {
            version: entry.loader.version,
            stable: entry.loader.stable,
        })
        .collect())
}

/// 最新的稳定版；一个稳定的都没有就退回最新的那个。
pub async fn latest_version(
    paths: &DataPaths,
    kind: LoaderKind,
    game_version: &str,
) -> Result<String> {
    let versions = list_versions(paths, kind, game_version).await?;
    versions
        .iter()
        .find(|version| version.stable)
        .or_else(|| versions.first())
        .map(|version| version.version.clone())
        .ok_or_else(|| anyhow!("{kind:?} 没有支持 {game_version} 的版本"))
}

/// 把 profile JSON 落盘，返回它的版本 id。
///
/// 已经在磁盘上、而且读得出来，就不再请求——补全是幂等的，重跑一遍不该每次
/// 都打一次 meta server。
pub async fn install(
    paths: &DataPaths,
    kind: LoaderKind,
    game_version: &str,
    loader_version: &str,
    events: &UnboundedSender<DownloadEvent>,
) -> Result<String> {
    // NeoForge 和 Forge 要在本地跑安装器，是完全不同的一条路。
    if matches!(kind, LoaderKind::NeoForge | LoaderKind::Forge) {
        return crate::launch::forge::install(paths, kind, game_version, loader_version, events)
            .await;
    }
    let expected_id = version_id(kind, game_version, loader_version);
    if version::read_one(paths, &expected_id).is_ok() {
        return Ok(expected_id);
    }

    let _ = events.send(DownloadEvent::StatusId {
        id: "job.note.loader-profile".to_owned(),
        params: vec![
            ("loader".to_owned(), display_name(kind).to_owned()),
            ("version".to_owned(), loader_version.to_owned()),
        ],
    });

    let url = format!(
        "{}/versions/loader/{game_version}/{loader_version}/profile/json",
        meta_root(kind)?
    );
    let client = DownloadClient::new(source_order(), 4);
    let bytes = client
        .fetch(&url)
        .await
        .with_context(|| format!("读取 {} {loader_version} 的 profile", display_name(kind)))?;

    let profile: serde_json::Value =
        serde_json::from_slice(&bytes).context("解析加载器 profile")?;
    let id = profile
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow!("加载器 profile 缺少 id"))?
        .to_owned();
    // id 会直接变成目录名，来自网络的字符串不能原样拿去拼路径。
    if !version::is_safe_id(&id) {
        return Err(anyhow!("加载器 profile 的 id 不能作为目录名：{id}"));
    }

    let inherits = profile
        .get("inheritsFrom")
        .and_then(serde_json::Value::as_str);
    if inherits != Some(game_version) {
        return Err(anyhow!(
            "该 profile 继承自 {inherits:?}，而非 {game_version}"
        ));
    }
    // 解得出 VersionMetadata 才算数：写进去一份读不动的 JSON，问题会推迟到
    // 启动那一刻才爆出来。
    serde_json::from_slice::<fern_meta::VersionMetadata>(&bytes)
        .context("加载器 profile 不是有效的版本描述")?;

    let path = version::metadata_path(paths, &id);
    tokio::fs::create_dir_all(path.parent().expect("version directory")).await?;
    let temporary = path.with_extension("json.part");
    tokio::fs::write(&temporary, &bytes).await?;
    tokio::fs::rename(&temporary, &path).await?;
    Ok(id)
}

/// 加载器生成的版本 id 的命名规则。
///
/// 只用来判断「装过没有」，真正的 id 以 profile 里写的为准——万一上游改了
/// 命名，最坏也只是多请求一次。
fn version_id(kind: LoaderKind, game_version: &str, loader_version: &str) -> String {
    match kind {
        LoaderKind::Quilt => format!("quilt-loader-{loader_version}-{game_version}"),
        _ => format!("fabric-loader-{loader_version}-{game_version}"),
    }
}

pub fn display_name(kind: LoaderKind) -> &'static str {
    match kind {
        LoaderKind::Vanilla => "原版",
        LoaderKind::Fabric => "Fabric",
        LoaderKind::Quilt => "Quilt",
        LoaderKind::NeoForge => "NeoForge",
        LoaderKind::Forge => "Forge",
    }
}

/// 创建面板上的一个选项。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoaderOption {
    pub kind: LoaderKind,
    pub label: String,
}

/// 现在装得上的加载器。界面按这个决定给出哪些选项——列出一个装不上的，
/// 等于让用户走到一半才被拦住。名字也从这里给：能装什么和它叫什么是同一件
/// 事，分在两处，加一种加载器就要改两个地方。
pub fn installable() -> Vec<LoaderOption> {
    [
        LoaderKind::Vanilla,
        LoaderKind::Fabric,
        LoaderKind::Quilt,
        LoaderKind::NeoForge,
        LoaderKind::Forge,
    ]
    .into_iter()
    .map(|kind| LoaderOption {
        kind,
        label: display_name(kind).to_owned(),
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_offered_loader_has_a_way_to_be_installed() {
        // 列一个装不上的选项，等于让用户走到一半才被拦住。所以每一个被列出来
        // 的加载器，都必须要么有 meta server，要么走 installer。
        let kinds: Vec<_> = installable()
            .into_iter()
            .map(|option| option.kind)
            .collect();
        for kind in [
            LoaderKind::Fabric,
            LoaderKind::Quilt,
            LoaderKind::NeoForge,
            LoaderKind::Forge,
        ] {
            assert!(kinds.contains(&kind), "{kind:?} 装得上却没有被列出来");
        }
        for kind in kinds {
            let reachable = match kind {
                LoaderKind::Vanilla => true,
                LoaderKind::NeoForge | LoaderKind::Forge => {
                    crate::launch::forge::installer_url(kind, "1.21.1", "1.0").is_ok()
                }
                other => meta_root(other).is_ok(),
            };
            assert!(reachable, "{kind:?} 被列为可安装，却没有安装途径");
        }
    }

    #[test]
    fn every_option_carries_a_name_and_a_machine_readable_kind() {
        for option in installable() {
            assert!(!option.label.is_empty());
            // 界面把 kind 原样发回来，序列化出来必须是个字符串。
            let value = serde_json::to_value(option.kind).expect("serialize kind");
            assert!(value.as_str().is_some_and(|kind| !kind.is_empty()));
        }
    }

    #[test]
    fn the_two_installer_based_loaders_do_not_go_through_a_meta_server() {
        // 它们的版本从 Maven 列、profile 从 installer jar 里掏。走错路会得到
        // 一个 404，而不是一句能看懂的话。
        for kind in [LoaderKind::NeoForge, LoaderKind::Forge] {
            let error = meta_root(kind).unwrap_err().to_string();
            assert!(error.contains("meta server"), "{error}");
        }
        assert!(meta_root(LoaderKind::Vanilla).is_err());
    }

    #[test]
    fn version_ids_follow_each_loaders_convention() {
        assert_eq!(
            version_id(LoaderKind::Fabric, "1.21.1", "0.16.5"),
            "fabric-loader-0.16.5-1.21.1"
        );
        assert_eq!(
            version_id(LoaderKind::Quilt, "1.21.1", "0.26.0"),
            "quilt-loader-0.26.0-1.21.1"
        );
        // 这些 id 会变成目录名，必须过得了版本 id 那一关。
        for id in [
            version_id(LoaderKind::Fabric, "1.21.1", "0.16.5"),
            version_id(LoaderKind::Quilt, "1.21.1", "0.26.0"),
        ] {
            assert!(version::is_safe_id(&id), "{id}");
        }
    }

    /// Forge 的 maven-metadata 不是可靠的顺序：1.12.2 那一段最新的在最后，
    /// 1.7.2 那一段最新的在最前。照单反转，1.7.2 的默认值会变成 2014 年的第
    /// 一个构建。
    #[test]
    fn forge_versions_are_ordered_by_number_not_by_file_order() {
        let mut versions = vec![
            "10.12.0.967",
            "10.12.2.1147",
            "10.12.2.1161-mc172",
            "10.12.2.1154-mc172",
            "10.12.1.1090",
        ];
        versions.sort_by(|left, right| forge_ordinal(right).cmp(&forge_ordinal(left)));
        assert_eq!(
            versions,
            vec![
                "10.12.2.1161-mc172",
                "10.12.2.1154-mc172",
                "10.12.2.1147",
                "10.12.1.1090",
                "10.12.0.967",
            ]
        );
    }
}
