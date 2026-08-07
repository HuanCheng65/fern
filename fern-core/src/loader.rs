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

use crate::{DataPaths, LoaderKind, settings::source_order, version};

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
        LoaderKind::NeoForge | LoaderKind::Forge => Err(anyhow!(
            "{kind:?} 的安装需要在本地执行 processors，还没有实现"
        )),
    }
}

/// 这个游戏版本上可用的加载器版本，新的在前。
pub async fn list_versions(kind: LoaderKind, game_version: &str) -> Result<Vec<LoaderVersion>> {
    let url = format!("{}/versions/loader/{game_version}", meta_root(kind)?);
    let client = DownloadClient::new(source_order(), 4);
    let bytes = client
        .fetch(&url)
        .await
        .with_context(|| format!("读取 {kind:?} 的版本列表"))?;
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
pub async fn latest_version(kind: LoaderKind, game_version: &str) -> Result<String> {
    let versions = list_versions(kind, game_version).await?;
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
    let expected_id = version_id(kind, game_version, loader_version);
    if version::read_one(paths, &expected_id).is_ok() {
        return Ok(expected_id);
    }

    let _ = events.send(DownloadEvent::Status {
        message: format!("安装 {} {loader_version}", display_name(kind)),
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
        .ok_or_else(|| anyhow!("加载器 profile 没有 id"))?
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
            "这份 profile 继承的是 {inherits:?}，不是 {game_version}"
        ));
    }
    // 解得出 VersionMetadata 才算数：写进去一份读不动的 JSON，问题会推迟到
    // 启动那一刻才爆出来。
    serde_json::from_slice::<fern_meta::VersionMetadata>(&bytes)
        .context("加载器 profile 不是一份能用的版本描述")?;

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
    [LoaderKind::Vanilla, LoaderKind::Fabric, LoaderKind::Quilt]
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
    fn only_the_loaders_we_can_actually_install_are_offered() {
        let kinds: Vec<_> = installable()
            .into_iter()
            .map(|option| option.kind)
            .collect();
        assert!(kinds.contains(&LoaderKind::Fabric));
        assert!(kinds.contains(&LoaderKind::Quilt));
        // NeoForge 的安装要在本地跑 processors，还没做——列出来就是骗人。
        assert!(!kinds.contains(&LoaderKind::NeoForge));
        assert!(!kinds.contains(&LoaderKind::Forge));
        for kind in kinds {
            assert!(
                kind == LoaderKind::Vanilla || meta_root(kind).is_ok(),
                "{kind:?} 被列为可安装，却没有 meta server"
            );
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
    fn unsupported_loaders_say_why_instead_of_failing_obscurely() {
        let error = meta_root(LoaderKind::NeoForge).unwrap_err().to_string();
        assert!(
            error.contains("processors"),
            "错误信息该说清缺的是什么：{error}"
        );
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
}
