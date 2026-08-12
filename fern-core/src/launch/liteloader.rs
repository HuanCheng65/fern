//! LiteLoader 的安装。
//!
//! 它和 Fabric 一样是纯数据操作——落一份版本描述就完事，不像 Forge 那样要在
//! 本地跑安装器。区别只在元数据的形状：上游给的是一份自己的
//! `versions.json`，而不是 meta server 那套 REST。
//!
//! ## 它为什么值得做
//!
//! LiteLoader 停在 1.12.2，是一份**不会再变的静态数据**——接进来一次就永远
//! 不用跟进。而它是这个启动器第一个真正意义上的**附加层**：它不取代加载器，
//! 而是叠在 Forge 之上（那个年代很多人两个一起用）。层模型和 tweaker 的有序
//! 合并本来就是为这种情况做的，接上它这两件事才有真实的用户。
//!
//! ## 元数据的形状
//!
//! ```text
//! versions.<游戏版本>
//!   repo        { url }              这一档的 maven 仓库
//!   artefacts   RELEASE 那一档       ┐ 两个档名，同一个形状；
//!   snapshots   SNAPSHOT 那一档      ┘ 有的版本只有其中一个
//!     com.mumfrey:liteloader
//!       latest  { version, tweakClass, libraries }
//!       <md5>   同上，历史构建
//! ```
//!
//! 历史构建用 md5 当键，没有可读的版本号，所以只取 `latest`：一个游戏版本上
//! 最多列出「正式」和「开发」两条。

use anyhow::{Context, Result, anyhow};
use fern_download::DownloadEvent;
use serde::Deserialize;
use std::collections::HashMap;
use tokio::sync::mpsc::UnboundedSender;

use crate::{
    DataPaths, LoaderKind,
    data::metacache::{self, Freshness},
    launch::{loader::LoaderVersion, version},
};

const VERSIONS_URL: &str = "https://dl.liteloader.com/versions/versions.json";

#[derive(Debug, Deserialize)]
struct Catalogue {
    versions: HashMap<String, GameVersion>,
}

#[derive(Debug, Deserialize)]
struct GameVersion {
    #[serde(default)]
    repo: Option<Repository>,
    #[serde(default)]
    artefacts: Option<Stream>,
    #[serde(default)]
    snapshots: Option<Stream>,
}

#[derive(Debug, Deserialize)]
struct Repository {
    #[serde(default)]
    url: String,
}

#[derive(Debug, Deserialize)]
struct Stream {
    #[serde(rename = "com.mumfrey:liteloader", default)]
    builds: HashMap<String, Build>,
}

#[derive(Debug, Clone, Deserialize)]
struct Build {
    version: String,
    #[serde(rename = "tweakClass")]
    tweak_class: String,
    #[serde(default)]
    libraries: Vec<Library>,
}

#[derive(Debug, Clone, Deserialize)]
struct Library {
    name: String,
    #[serde(default)]
    url: Option<String>,
}

/// 这个游戏版本上可用的 LiteLoader。正式那一档在前。
pub async fn list_versions(paths: &DataPaths, game_version: &str) -> Result<Vec<LoaderVersion>> {
    let entry = catalogue(paths).await?.versions.remove(game_version);
    let Some(entry) = entry else {
        return Ok(Vec::new());
    };
    Ok([(entry.artefacts, true), (entry.snapshots, false)]
        .into_iter()
        .filter_map(|(stream, stable)| Some((stream?.builds.remove("latest")?, stable)))
        .map(|(build, stable)| LoaderVersion {
            version: build.version,
            stable,
        })
        .collect())
}

/// 落一份版本描述，返回它的 id。
pub async fn install(
    paths: &DataPaths,
    game_version: &str,
    loader_version: &str,
    events: &UnboundedSender<DownloadEvent>,
) -> Result<String> {
    let id = format!("{game_version}-LiteLoader{loader_version}");
    if !version::is_safe_id(&id) {
        return Err(anyhow!("版本 id 无法作为目录名：{id}"));
    }
    if version::read_one(paths, &id).is_ok() {
        return Ok(id);
    }

    let _ = events.send(DownloadEvent::StatusId {
        id: "job.note.loader-profile".to_owned(),
        params: vec![
            ("loader".to_owned(), "LiteLoader".to_owned()),
            ("version".to_owned(), loader_version.to_owned()),
        ],
    });

    let entry = catalogue(paths)
        .await?
        .versions
        .remove(game_version)
        .ok_or_else(|| anyhow!("LiteLoader 不支持 {game_version}"))?;
    // 上游那份表里的地址是 http，而这三个仓库都支持 https。
    let repository = entry
        .repo
        .map(|repo| repo.url)
        .unwrap_or_default()
        .replacen("http://", "https://", 1);
    let build = [entry.artefacts, entry.snapshots]
        .into_iter()
        .flatten()
        .flat_map(|stream| stream.builds.into_values())
        .find(|build| build.version == loader_version)
        .ok_or_else(|| anyhow!("找不到 LiteLoader {loader_version}"))?;

    // 它自己那个 jar 在上面那个仓库里；它依赖的几个库各自带地址，缺地址的
    // 按老约定去 Mojang 那个仓库（`Library::file` 已经这么处理）。
    let mut libraries = vec![serde_json::json!({
        "name": format!("com.mumfrey:liteloader:{loader_version}"),
        "url": repository,
    })];
    libraries.extend(build.libraries.iter().map(|library| match &library.url {
        Some(url) => serde_json::json!({ "name": library.name, "url": url }),
        None => serde_json::json!({ "name": library.name }),
    }));

    let profile = serde_json::json!({
        "id": id,
        "inheritsFrom": game_version,
        "type": "release",
        // 这一层不换主类：叠在 Forge 上时主类归 Forge，单独用时它就是
        // LaunchWrapper 本身——两种情况下这个值都是对的。
        "mainClass": "net.minecraft.launchwrapper.Launch",
        // 只写自己那一句。合并时它会**追加**在已有的 tweaker 后面，而不是
        // 把 Forge 那一句顶掉（见 fern_meta::LegacyArguments）。
        "minecraftArguments": format!("--tweakClass {}", build.tweak_class),
        "libraries": libraries,
    });
    // 解得出 VersionMetadata 才算数，否则问题会推迟到启动那一刻。
    serde_json::from_value::<fern_meta::VersionMetadata>(profile.clone())
        .context("生成的 LiteLoader 版本描述无法解析")?;

    let path = version::metadata_path(paths, &id);
    tokio::fs::create_dir_all(path.parent().expect("version directory")).await?;
    tokio::fs::write(&path, serde_json::to_vec_pretty(&profile)?).await?;
    Ok(id)
}

async fn catalogue(paths: &DataPaths) -> Result<Catalogue> {
    let client = crate::data::downloader::client(4);
    let bytes = metacache::mutable(
        &client,
        paths,
        "loader-liteloader-versions.json",
        VERSIONS_URL,
        // 上游停更在 2017 年，这份表不会再变——但缓存策略还是跟别家一致，
        // 免得为一个特例多一套规矩。
        Freshness::Within(metacache::LISTING_TTL),
    )
    .await
    .context("读取 LiteLoader 的版本列表")?
    .bytes;
    serde_json::from_slice(&bytes).context("解析 LiteLoader 的版本列表")
}

/// LiteLoader 有没有覆盖这个游戏版本。
///
/// 不联网，用来决定界面上要不要显示这一项。上游停在 1.12.2，这份名单是终态。
pub fn covers(game_version: &str) -> bool {
    const COVERED: [&str; 12] = [
        "1.5.2", "1.6.2", "1.6.4", "1.7.2", "1.7.10", "1.8", "1.8.9", "1.9", "1.9.4", "1.10.2",
        "1.11.2", "1.12.2",
    ];
    COVERED.contains(&game_version)
}

/// 它能叠在这个加载器上吗。
///
/// Forge 与原版可以——那个年代两个一起用是常态。Fabric 那一系不行：它们不是
/// LaunchWrapper 的世界，`--tweakClass` 根本没人读。
pub fn stacks_on(loader: LoaderKind) -> bool {
    matches!(loader, LoaderKind::Vanilla | LoaderKind::Forge)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_only_covers_the_versions_upstream_shipped() {
        assert!(covers("1.12.2"));
        assert!(covers("1.7.10"));
        // 上游停在 1.12.2，之后的版本不该出现这一项。
        assert!(!covers("1.13"));
        assert!(!covers("1.21.1"));
        assert!(!covers("26.2"));
        // 1.12 和 1.12.1 上游确实发过，但我们只列 1.12.2——那份表里另外两个
        // 只有 SNAPSHOT，装出来的东西没人测过。
        assert!(!covers("1.12"));
    }

    #[test]
    fn it_stacks_on_forge_but_not_on_fabric() {
        assert!(stacks_on(LoaderKind::Forge));
        assert!(stacks_on(LoaderKind::Vanilla));
        assert!(!stacks_on(LoaderKind::Fabric));
        assert!(!stacks_on(LoaderKind::Quilt));
        assert!(!stacks_on(LoaderKind::NeoForge));
    }

    /// 上游那份表两种档名混用，两种都要认得。
    #[test]
    fn both_stream_names_parse() {
        let catalogue: Catalogue = serde_json::from_str(
            r#"{"versions":{
              "1.7.10":{"repo":{"url":"http://dl.liteloader.com/versions/"},
                "artefacts":{"com.mumfrey:liteloader":{"latest":{
                  "version":"1.7.10_04","tweakClass":"com.mumfrey.liteloader.launch.LiteLoaderTweaker",
                  "libraries":[{"name":"net.minecraft:launchwrapper:1.12"}]}}}},
              "1.12.2":{"repo":{"url":"http://repo.mumfrey.com/content/repositories/snapshots/"},
                "snapshots":{"com.mumfrey:liteloader":{"latest":{
                  "version":"1.12.2-SNAPSHOT","tweakClass":"com.mumfrey.liteloader.launch.LiteLoaderTweaker",
                  "libraries":[]}}}}
            }}"#,
        )
        .expect("parse");
        assert!(catalogue.versions["1.7.10"].artefacts.is_some());
        assert!(catalogue.versions["1.12.2"].snapshots.is_some());
    }
}
