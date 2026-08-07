//! 补给站：Modrinth。
//!
//! 先接 Modrinth 而不是 CurseForge：它的 API 公开、无需鉴权、协议清楚。
//! CurseForge 要申请 API key 并把密钥分发进客户端，那是另一码事。
//!
//! 这一层只负责「去哪里找、下哪个文件」。文件落进 `mods/` 之后就归 `mods`
//! 模块管——补给是获取，管理是状态，两件事。

use anyhow::{Context, Result, anyhow};
use fern_download::{DownloadClient, DownloadEvent, DownloadTask};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use tokio::sync::mpsc::UnboundedSender;

use crate::{DataPaths, LoaderKind};

const API: &str = "https://api.modrinth.com/v2";

/// Modrinth 要求调用方表明身份，并据此做速率限制（每分钟 300 次）。
/// 匿名调用会被降级甚至拒绝。
const USER_AGENT: &str = concat!(
    "HuanCheng65/fern/",
    env!("CARGO_PKG_VERSION"),
    " (github.com/HuanCheng65/fern)"
);

/// 一次装依赖最多跟这么深。真实的依赖链两三层就到头，留点余量，主要是防
/// 上游数据里出现环让我们一直转下去。
const MAX_DEPTH: usize = 6;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub author: String,
    pub downloads: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
    pub categories: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub hits: Vec<SearchHit>,
    pub total: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectVersion {
    pub id: String,
    pub name: String,
    pub version_number: String,
    /// `release` / `beta` / `alpha`。界面默认只推正式版。
    pub version_type: String,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub downloads: u64,
    pub date_published: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
}

// ——— 线上的 JSON 形状，只取用得上的字段 ———

#[derive(Debug, Deserialize)]
struct RawSearch {
    hits: Vec<RawHit>,
    total_hits: u64,
}

#[derive(Debug, Deserialize)]
struct RawHit {
    project_id: String,
    slug: String,
    title: String,
    description: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    downloads: u64,
    #[serde(default)]
    icon_url: Option<String>,
    #[serde(default)]
    categories: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RawVersion {
    id: String,
    project_id: String,
    name: String,
    version_number: String,
    version_type: String,
    #[serde(default)]
    game_versions: Vec<String>,
    #[serde(default)]
    loaders: Vec<String>,
    #[serde(default)]
    downloads: u64,
    #[serde(default)]
    date_published: String,
    #[serde(default)]
    files: Vec<RawFile>,
    #[serde(default)]
    dependencies: Vec<RawDependency>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawFile {
    url: String,
    filename: String,
    #[serde(default)]
    primary: bool,
    #[serde(default)]
    size: u64,
    hashes: RawHashes,
}

#[derive(Debug, Clone, Deserialize)]
struct RawHashes {
    #[serde(default)]
    sha1: String,
}

#[derive(Debug, Deserialize)]
struct RawDependency {
    #[serde(default)]
    version_id: Option<String>,
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    dependency_type: String,
}

impl RawVersion {
    /// 一个版本可能挂多个文件（源码包、附带资源包）。`primary` 才是要装的
    /// 那一个；上游偶尔不标，退回第一个。
    fn primary_file(&self) -> Option<&RawFile> {
        self.files
            .iter()
            .find(|file| file.primary)
            .or_else(|| self.files.first())
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("valid modrinth client")
}

/// 每个请求试几次。装一个带依赖的模组要发 N+1 次请求，任何一次抖动都会让
/// 整个安装失败——而 TLS 握手在不稳的网络上说断就断。
const ATTEMPTS: u32 = 3;

async fn get<T: serde::de::DeserializeOwned>(url: &str) -> Result<T> {
    let mut last_error = None;
    for attempt in 0..ATTEMPTS {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(300u64 << attempt)).await;
        }
        match attempt_get(url).await {
            Ok(bytes) => return serde_json::from_slice(&bytes).context("解析 Modrinth 的响应"),
            // 服务端明确的拒绝重试没有意义，直接抛出去。
            Err(Refusal::Final(error)) => return Err(error),
            Err(Refusal::Retryable(error)) => last_error = Some(error),
        }
    }
    Err(last_error
        .unwrap_or_else(|| anyhow!("请求 {url} 失败"))
        .context(format!("请求 {url}")))
}

enum Refusal {
    /// 再试也是这个结果。
    Final(anyhow::Error),
    /// 网络层面的抖动，值得再来一次。
    Retryable(anyhow::Error),
}

async fn attempt_get(url: &str) -> std::result::Result<Vec<u8>, Refusal> {
    let response = match client().get(url).send().await {
        Ok(response) => response,
        Err(error) => return Err(Refusal::Retryable(error.into())),
    };
    let status = response.status();
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(Refusal::Final(anyhow!("请求过于频繁，稍后再试")));
    }
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err(Refusal::Final(anyhow!("Modrinth 上没有这个条目")));
    }
    if status.is_client_error() {
        return Err(Refusal::Final(anyhow!("Modrinth 返回 HTTP {status}")));
    }
    if !status.is_success() {
        return Err(Refusal::Retryable(anyhow!("Modrinth 返回 HTTP {status}")));
    }
    match response.bytes().await {
        Ok(bytes) => Ok(bytes.to_vec()),
        Err(error) => Err(Refusal::Retryable(error.into())),
    }
}

/// 这个加载器在 Modrinth 上对应哪些标签。
///
/// Quilt 能直接加载 Fabric 模组，而大多数作者只标 fabric——只按 quilt 过滤
/// 会得到一个几乎空的列表。反过来 Forge 和 NeoForge 不能混，别合并。
fn loader_tags(loader: LoaderKind) -> Vec<&'static str> {
    match loader {
        LoaderKind::Fabric => vec!["fabric"],
        LoaderKind::Quilt => vec!["quilt", "fabric"],
        LoaderKind::NeoForge => vec!["neoforge"],
        LoaderKind::Forge => vec!["forge"],
        LoaderKind::Vanilla => Vec::new(),
    }
}

fn json_array(values: &[&str]) -> String {
    let quoted: Vec<String> = values.iter().map(|value| format!("\"{value}\"")).collect();
    format!("[{}]", quoted.join(","))
}

/// 搜索模组。
///
/// 按当前实例的游戏版本和加载器过滤——用户是在为某个实例找东西，列出装不上的
/// 结果只会浪费他一次点击。
pub async fn search(
    query: &str,
    game_version: &str,
    loader: LoaderKind,
    offset: u32,
    limit: u32,
) -> Result<SearchResult> {
    let mut facets = vec![json_array(&["project_type:mod"])];
    if !game_version.is_empty() {
        facets.push(json_array(&[&format!("versions:{game_version}")]));
    }
    let tags = loader_tags(loader);
    if !tags.is_empty() {
        // 同一个 facet 数组里的条目是「或」的关系，正好用来表达 quilt 或 fabric。
        let any: Vec<String> = tags.iter().map(|tag| format!("categories:{tag}")).collect();
        facets.push(json_array(
            &any.iter().map(String::as_str).collect::<Vec<_>>(),
        ));
    }
    let url = format!(
        "{API}/search?query={}&offset={offset}&limit={limit}&facets={}",
        urlencoding(query),
        urlencoding(&format!("[{}]", facets.join(",")))
    );

    let raw: RawSearch = get(&url).await?;
    Ok(SearchResult {
        total: raw.total_hits,
        hits: raw
            .hits
            .into_iter()
            .map(|hit| SearchHit {
                project_id: hit.project_id,
                slug: hit.slug,
                title: hit.title,
                description: hit.description,
                author: hit.author,
                downloads: hit.downloads,
                icon_url: hit.icon_url.filter(|url| !url.is_empty()),
                categories: hit.categories,
            })
            .collect(),
    })
}

/// 一个项目在这个版本和加载器下有哪些可选版本，新的在前。
pub async fn versions(
    project: &str,
    game_version: &str,
    loader: LoaderKind,
) -> Result<Vec<ProjectVersion>> {
    let raw = raw_versions(project, game_version, loader).await?;
    Ok(raw.into_iter().map(describe).collect())
}

async fn raw_versions(
    project: &str,
    game_version: &str,
    loader: LoaderKind,
) -> Result<Vec<RawVersion>> {
    let mut url = format!("{API}/project/{project}/version");
    let mut query = Vec::new();
    if !game_version.is_empty() {
        query.push(format!(
            "game_versions={}",
            urlencoding(&json_array(&[game_version]))
        ));
    }
    let tags = loader_tags(loader);
    if !tags.is_empty() {
        query.push(format!("loaders={}", urlencoding(&json_array(&tags))));
    }
    if !query.is_empty() {
        url.push('?');
        url.push_str(&query.join("&"));
    }
    get(&url).await
}

fn describe(raw: RawVersion) -> ProjectVersion {
    let file_name = raw.primary_file().map(|file| file.filename.clone());
    ProjectVersion {
        id: raw.id,
        name: raw.name,
        version_number: raw.version_number,
        version_type: raw.version_type,
        game_versions: raw.game_versions,
        loaders: raw.loaders,
        downloads: raw.downloads,
        date_published: raw.date_published,
        file_name,
    }
}

/// 装一个版本，连同它的必需依赖。
///
/// 依赖多半只给 `project_id`，得自己去挑一个兼容版本；少数会指定
/// `version_id`，那就照它说的来。`optional` 的不装——那是作者的建议，不是
/// 要求，替用户决定装什么不是我们的事。
pub async fn install(
    paths: &DataPaths,
    instance_id: &str,
    version_id: &str,
    events: &UnboundedSender<DownloadEvent>,
) -> Result<Vec<String>> {
    let profile = crate::list_instances(paths)?
        .into_iter()
        .find(|profile| profile.id.as_str() == instance_id)
        .ok_or_else(|| anyhow!("实例 {instance_id} 不存在"))?;
    let game_version = profile.game_version.as_str();
    let loader = profile.loader;

    let mods_directory = paths.game_directory(instance_id).join("mods");
    tokio::fs::create_dir_all(&mods_directory).await?;

    let mut seen_projects = HashSet::new();
    let mut pending = vec![(version_id.to_owned(), 0usize)];
    let mut tasks = Vec::new();
    let mut installed = Vec::new();

    while let Some((id, depth)) = pending.pop() {
        if depth > MAX_DEPTH {
            continue;
        }
        let version: RawVersion = get(&format!("{API}/version/{id}")).await?;
        if !seen_projects.insert(version.project_id.clone()) {
            continue;
        }

        let Some(file) = version.primary_file() else {
            return Err(anyhow!("{} 这个版本没有可下载的文件", version.name));
        };
        // Modrinth 每个文件都给 sha1，所以这里是有校验的下载。
        tasks.push(DownloadTask::new(
            fern_download::safe_join(&mods_directory, std::path::Path::new(&file.filename))?,
            &file.url,
            &file.hashes.sha1,
            file.size,
        )?);
        installed.push(file.filename.clone());

        for dependency in &version.dependencies {
            if dependency.dependency_type != "required" {
                continue;
            }
            if let Some(exact) = dependency.version_id.as_ref().filter(|id| !id.is_empty()) {
                pending.push((exact.clone(), depth + 1));
                continue;
            }
            let Some(project) = dependency.project_id.as_ref() else {
                continue;
            };
            if seen_projects.contains(project) {
                continue;
            }
            // 依赖只给了项目，兼容的版本要自己挑：优先正式版，没有就用最新的。
            let candidates = raw_versions(project, game_version, loader).await?;
            let chosen = candidates
                .iter()
                .find(|version| version.version_type == "release")
                .or_else(|| candidates.first());
            match chosen {
                Some(version) => pending.push((version.id.clone(), depth + 1)),
                None => {
                    return Err(anyhow!(
                        "依赖的项目 {project} 没有适用于 {game_version} 的版本"
                    ));
                }
            }
        }
    }

    let _ = events.send(DownloadEvent::Status {
        message: if installed.len() > 1 {
            format!("下载 {} 个文件（含依赖）", installed.len())
        } else {
            "下载模组".to_owned()
        },
    });
    let downloader = DownloadClient::new(crate::settings::source_order(), 8);
    downloader.download_all(tasks, events).await?;
    Ok(installed)
}

/// 只转义查询串里真正会出事的那几个字符。
///
/// 不引一个 URL 编码库：这里拼的都是我们自己构造的 facet 和用户输入的搜索词，
/// 需要处理的字符是可枚举的。
fn urlencoding(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char)
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quilt_also_looks_for_fabric_mods() {
        // Quilt 能直接加载 Fabric 模组，而大多数作者只标 fabric——只按 quilt
        // 过滤会得到一个几乎空的列表。
        assert_eq!(loader_tags(LoaderKind::Quilt), vec!["quilt", "fabric"]);
        // Forge 和 NeoForge 不能混。
        assert_eq!(loader_tags(LoaderKind::NeoForge), vec!["neoforge"]);
        assert_eq!(loader_tags(LoaderKind::Forge), vec!["forge"]);
        assert!(loader_tags(LoaderKind::Vanilla).is_empty());
    }

    #[test]
    fn queries_are_escaped_before_they_reach_the_url() {
        // 中文搜索词和 facet 里的方括号、引号都必须编码。
        assert_eq!(urlencoding("sodium"), "sodium");
        assert_eq!(urlencoding("[\"a\"]"), "%5B%22a%22%5D");
        assert_eq!(urlencoding("光影"), "%E5%85%89%E5%BD%B1");
        assert_eq!(urlencoding("a b&c=d"), "a%20b%26c%3Dd");
    }

    #[test]
    fn the_primary_file_wins_over_the_extras() {
        // 一个版本可能挂源码包和附带资源包，装错了就是装了个源码。
        let raw: RawVersion = serde_json::from_str(
            r#"{"id":"v","project_id":"p","name":"n","version_number":"1",
                "version_type":"release",
                "files":[
                  {"url":"https://x/sources.jar","filename":"sources.jar","primary":false,
                   "size":1,"hashes":{"sha1":"aa"}},
                  {"url":"https://x/mod.jar","filename":"mod.jar","primary":true,
                   "size":2,"hashes":{"sha1":"bb"}}]}"#,
        )
        .expect("parse");
        assert_eq!(raw.primary_file().unwrap().filename, "mod.jar");

        // 上游偶尔一个都不标 primary，那就用第一个，不能什么都不装。
        let unmarked: RawVersion = serde_json::from_str(
            r#"{"id":"v","project_id":"p","name":"n","version_number":"1",
                "version_type":"release",
                "files":[{"url":"https://x/a.jar","filename":"a.jar","primary":false,
                          "size":1,"hashes":{"sha1":"aa"}}]}"#,
        )
        .expect("parse");
        assert_eq!(unmarked.primary_file().unwrap().filename, "a.jar");
    }

    #[test]
    fn only_required_dependencies_are_installed() {
        // optional 是作者的建议不是要求；incompatible 装了会坏事。
        let raw: RawVersion = serde_json::from_str(
            r#"{"id":"v","project_id":"p","name":"n","version_number":"1",
                "version_type":"release","files":[],
                "dependencies":[
                  {"project_id":"a","dependency_type":"required"},
                  {"project_id":"b","dependency_type":"optional"},
                  {"project_id":"c","dependency_type":"incompatible"},
                  {"project_id":"d","dependency_type":"embedded"}]}"#,
        )
        .expect("parse");
        let required: Vec<_> = raw
            .dependencies
            .iter()
            .filter(|dependency| dependency.dependency_type == "required")
            .filter_map(|dependency| dependency.project_id.clone())
            .collect();
        assert_eq!(required, vec!["a"]);
    }

    #[test]
    fn the_user_agent_identifies_this_launcher() {
        // Modrinth 按 User-Agent 做速率限制，匿名调用会被降级甚至拒绝。
        assert!(USER_AGENT.contains("fern"));
        assert!(USER_AGENT.contains("github.com"));
    }
}
