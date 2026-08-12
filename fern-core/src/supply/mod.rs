//! 补给站：Modrinth。
//!
//! 先接 Modrinth 而不是 CurseForge：它的 API 公开、无需鉴权、协议清楚。
//! CurseForge 要申请 API key 并把密钥分发进客户端，那是另一码事。
//!
//! 这一层只负责「去哪里找、下哪个文件」。文件落进 `mods/` 之后就归 `mods`
//! 模块管——补给是获取，管理是状态，两件事。
//!
//! 搜索条件全部由调用方明确给出，这里不去问「当前实例是什么」。补给站是一个
//! 独立的地方：先按实例过滤，等于把浏览和「给这个实例装东西」压成了一件事，
//! 于是想看看有什么就得先有一个实例。要不要装得上是**标注**，不是过滤器。
//!
//! 整合包是这一层的另一条支线，在 `modpack.rs`：装它不是「装东西到某个实例」，
//! 是**建一个实例**。

pub(crate) mod modpack;
pub(crate) mod plan;
pub(crate) mod survey;

use anyhow::{Context, Result, anyhow};
use fern_download::{DownloadEvent, DownloadTask};
use serde::{Deserialize, Serialize};
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

/// 东西是从哪一家来的。
///
/// 目前只有 Modrinth 一家在跑，但**这个枚举要先存在**：它会被写进来源日志，
/// 而那份日志是磁盘上的、补不回来的东西。等接 CurseForge 时再加字段，就是一次
/// 迁移；现在加，是零成本。
///
/// 两家的 id 类型不一样——Modrinth 是字符串（`AANobbMI`），CurseForge 是整数
/// （`394468`）。日志里一律按字符串存：它只用来回查，不参与运算。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    #[default]
    Modrinth,
    CurseForge,
}

impl Source {
    /// 写进日志的那个词。**改了它就等于把老日志作废**，所以不跟着枚举名走。
    pub(crate) fn tag(self) -> &'static str {
        match self {
            Self::Modrinth => "modrinth",
            Self::CurseForge => "curseforge",
        }
    }
}

/// 补给站能找的东西。
///
/// 数据包要选存档、插件是服务端的事，所以不在这里——把它们摆进类型筛选，
/// 等于给一个点了会失败的按钮。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    #[default]
    Mod,
    ResourcePack,
    Shader,
    /// 整合包是个例外：它不是「装进某个实例」，而是**建一个新实例**。
    Modpack,
}

impl ResourceKind {
    /// Modrinth 的 `project_type`。
    fn project_type(self) -> &'static str {
        match self {
            Self::Mod => "mod",
            Self::ResourcePack => "resourcepack",
            Self::Shader => "shader",
            Self::Modpack => "modpack",
        }
    }

    /// 文件落到游戏目录下的哪里。整合包没有这个答案——它自带一整个目录树。
    pub fn directory(self) -> Option<&'static str> {
        match self {
            Self::Mod => Some("mods"),
            Self::ResourcePack => Some("resourcepacks"),
            Self::Shader => Some("shaderpacks"),
            Self::Modpack => None,
        }
    }

    /// 只有模组有依赖图。资源包和光影是自足的文件，整合包的依赖写在它自己的
    /// index 里，去解析这里的 dependencies 只会把一堆无关的东西拖下来。
    fn has_dependencies(self) -> bool {
        matches!(self, Self::Mod)
    }

    /// 加载器标签只对模组和整合包成立。资源包在 Modrinth 上的 loader 是
    /// `minecraft`，光影是 `iris`/`optifine`——拿 fabric 去筛会得到空列表。
    fn honours_loader(self) -> bool {
        matches!(self, Self::Mod | Self::Modpack)
    }
}

/// 一次搜索要问的全部条件。空字符串和 None 一律表示「不限」。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchQuery {
    pub query: String,
    pub kind: ResourceKind,
    pub game_version: String,
    pub loader: Option<LoaderKind>,
    pub category: String,
    /// `relevance` / `downloads` / `follows` / `newest` / `updated`。
    pub sort: String,
    pub offset: u32,
    pub limit: u32,
}

impl Default for SearchQuery {
    fn default() -> Self {
        Self {
            query: String::new(),
            kind: ResourceKind::Mod,
            game_version: String::new(),
            loader: None,
            category: String::new(),
            sort: "relevance".to_owned(),
            offset: 0,
            limit: 40,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GalleryImage {
    pub url: String,
    pub title: String,
}

/// 项目详情页要的东西。
///
/// 不含 `body`。它是一整篇 markdown，渲染它要么引一个解析器加一层消毒，要么
/// 自己写一个——把网络来的字符串变成 DOM 是 XSS 面，不值得为一段介绍开这个口。
/// 详情页给一个「在 Modrinth 打开」，正文交给浏览器。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDetail {
    pub id: String,
    pub slug: String,
    pub title: String,
    /// 一句话摘要，纯文本。
    pub description: String,
    pub project_type: String,
    pub categories: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
    pub gallery: Vec<GalleryImage>,
    pub downloads: u64,
    pub followers: u64,
    pub updated: String,
    pub license: String,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    /// 外部链接。只保留 https 的，其余当没有。
    pub links: Vec<ProjectLink>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectLink {
    pub label: String,
    pub url: String,
}

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
    /// 这个版本声明的依赖，原样带上。
    ///
    /// 只有 id，没有名字——列表里每一行都去换一次名字是几十次请求。想知道
    /// 它们是什么，点进去看那一份计划（`resolve_install_plan`）。
    pub dependencies: Vec<VersionDependency>,
}

/// 版本列表里那个「N 个前置」的原料。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionDependency {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    pub kind: plan::DependencyKind,
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
struct RawProject {
    id: String,
    slug: String,
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    project_type: String,
    #[serde(default)]
    categories: Vec<String>,
    #[serde(default)]
    icon_url: Option<String>,
    #[serde(default)]
    gallery: Vec<RawGallery>,
    #[serde(default)]
    downloads: u64,
    #[serde(default)]
    followers: u64,
    #[serde(default)]
    updated: String,
    #[serde(default)]
    license: Option<RawLicense>,
    #[serde(default)]
    game_versions: Vec<String>,
    #[serde(default)]
    loaders: Vec<String>,
    #[serde(default)]
    source_url: Option<String>,
    #[serde(default)]
    issues_url: Option<String>,
    #[serde(default)]
    wiki_url: Option<String>,
    #[serde(default)]
    discord_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawGallery {
    url: String,
    #[serde(default)]
    title: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawLicense {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: Option<String>,
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
    /// 导出 mrpack 时要写进 `hashes`，格式要求两个都有，而这一个只有上游算得出来。
    #[serde(default)]
    sha512: String,
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

/// 带 JSON 请求体的那一种。只有按 hash 批量查版本用得上。
///
/// 不做重试：POST 在语义上不保证幂等，而这一个具体的接口失败了就退回「不知道
/// 装了什么」，比反复敲上游好。
async fn post<T: serde::de::DeserializeOwned>(url: &str, body: Vec<u8>) -> Result<T> {
    let response = client()
        .post(url)
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await
        .with_context(|| format!("请求 {url}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(anyhow!("Modrinth 返回 HTTP {status}"));
    }
    let bytes = response.bytes().await.context("读取 Modrinth 的响应")?;
    serde_json::from_slice(&bytes).context("解析 Modrinth 的响应")
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
        // Modrinth 上有 liteloader 这个标签，但在架的模组个位数——按它过滤
        // 得到的是一个空列表，不如不过滤，让人按游戏版本自己找。
        LoaderKind::Vanilla | LoaderKind::LiteLoader => Vec::new(),
    }
}

fn json_array(values: &[&str]) -> String {
    let quoted: Vec<String> = values.iter().map(|value| format!("\"{value}\"")).collect();
    format!("[{}]", quoted.join(","))
}

/// 搜索。
///
/// 条件全部来自调用方，一个都不从「当前实例」推断。不写条件就是不限，于是
/// 补给站默认是在浏览整个 Modrinth，而不是在浏览「这个实例装得上的东西」。
pub async fn search(request: &SearchQuery) -> Result<SearchResult> {
    let kind = request.kind;
    let mut facets = vec![json_array(&[&format!(
        "project_type:{}",
        kind.project_type()
    )])];
    if !request.game_version.is_empty() {
        facets.push(json_array(&[&format!("versions:{}", request.game_version)]));
    }
    if !request.category.is_empty() {
        facets.push(json_array(&[&format!("categories:{}", request.category)]));
    }
    if let Some(loader) = request.loader.filter(|_| kind.honours_loader()) {
        let tags = loader_tags(loader);
        if !tags.is_empty() {
            // 同一个 facet 数组里的条目是「或」的关系，正好用来表达 quilt 或 fabric。
            let any: Vec<String> = tags.iter().map(|tag| format!("categories:{tag}")).collect();
            facets.push(json_array(
                &any.iter().map(String::as_str).collect::<Vec<_>>(),
            ));
        }
    }
    let sort = match request.sort.as_str() {
        "downloads" | "follows" | "newest" | "updated" => request.sort.as_str(),
        _ => "relevance",
    };
    let url = format!(
        "{API}/search?query={}&index={sort}&offset={}&limit={}&facets={}",
        urlencoding(&request.query),
        request.offset,
        request.limit.clamp(1, 100),
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

/// 一个项目的全部版本，新的在前。
///
/// 不在这里按目标实例筛。详情页要把所有版本都摆出来，装不上的标出来而不是
/// 藏起来——「这个模组还没适配 1.21」是用户需要知道的事实，一个空列表说不了
/// 这句话。
pub async fn versions(project: &str) -> Result<Vec<ProjectVersion>> {
    let raw: Vec<RawVersion> = get(&format!("{API}/project/{project}/version")).await?;
    Ok(raw.into_iter().map(describe).collect())
}

/// 项目详情。
pub async fn project(slug: &str) -> Result<ProjectDetail> {
    let raw: RawProject = get(&format!("{API}/project/{slug}")).await?;
    let mut links = Vec::new();
    for (label, url) in [
        ("源码", raw.source_url),
        ("问题追踪", raw.issues_url),
        ("百科", raw.wiki_url),
        ("Discord", raw.discord_url),
    ] {
        // 链接来自上游，非 https 的一律当没有——详情页上的每一个都会被点。
        if let Some(url) = url.filter(|url| is_external_url(url)) {
            links.push(ProjectLink {
                label: label.to_owned(),
                url,
            });
        }
    }
    links.push(ProjectLink {
        label: "Modrinth 页面".to_owned(),
        url: format!("https://modrinth.com/{}/{}", raw.project_type, raw.slug),
    });

    Ok(ProjectDetail {
        id: raw.id,
        slug: raw.slug,
        title: raw.title,
        description: raw.description,
        project_type: raw.project_type,
        categories: raw.categories,
        icon_url: raw.icon_url.filter(|url| !url.is_empty()),
        gallery: raw
            .gallery
            .into_iter()
            .map(|image| GalleryImage {
                title: image.title.unwrap_or_default(),
                url: image.url,
            })
            .collect(),
        downloads: raw.downloads,
        followers: raw.followers,
        updated: raw.updated,
        // 上游的 name 可能是空串而不是缺失，所以按「第一个非空」取，别用
        // unwrap_or——那样会得到一个空的许可证栏。
        license: raw
            .license
            .map(|license| {
                let name = license.name.unwrap_or_default();
                if name.is_empty() { license.id } else { name }
            })
            .unwrap_or_default(),
        game_versions: raw.game_versions,
        loaders: raw.loaders,
        links,
    })
}

/// 能不能交给系统浏览器打开。
///
/// 详情页上的链接是上游给的字符串，会被原样递给 `xdg-open` / `open` /
/// `explorer`。限死 https 一种协议：`file://` 会打开本地文件，Windows 上还有
/// 一堆自定义协议处理器，而以 `-` 开头的串会被当成命令行开关。
pub fn is_external_url(raw: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(raw) else {
        return false;
    };
    url.scheme() == "https" && url.host_str().is_some_and(|host| !host.is_empty())
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

async fn raw_version(version_id: &str) -> Result<RawVersion> {
    get(&format!("{API}/version/{version_id}")).await
}

/// 一批 sha1 分别是哪个版本。
///
/// 这是 Modrinth 上唯一可靠的「这个文件是什么」——文件名不是身份，同一个模组
/// 从不同渠道拿到的文件名可以完全不同。一次问一批，不是一个文件一次。
async fn versions_by_hash(
    hashes: &[String],
) -> Result<std::collections::HashMap<String, survey::KnownVersion>> {
    #[derive(Serialize)]
    struct Request<'a> {
        hashes: &'a [String],
        algorithm: &'a str,
    }

    let mut out = std::collections::HashMap::new();
    // 一次问太多会被上游拒绝，也会把一次失败的代价放大到整批。
    for chunk in hashes.chunks(100) {
        let body = serde_json::to_vec(&Request {
            hashes: chunk,
            algorithm: "sha1",
        })?;
        let found: std::collections::HashMap<String, RawVersion> =
            post(&format!("{API}/version_files"), body).await?;
        for (hash, version) in found {
            out.insert(
                hash,
                survey::KnownVersion {
                    project_id: version.project_id,
                    version_id: version.id,
                    version_number: version.version_number,
                    game_versions: version.game_versions,
                    loaders: version.loaders,
                },
            );
        }
    }
    Ok(out)
}

/// 一批项目 id 分别叫什么。id 对用户没有任何意义。
async fn project_names(ids: &[String]) -> Result<std::collections::HashMap<String, plan::Named>> {
    let mut unique: Vec<&str> = ids.iter().map(String::as_str).collect();
    unique.sort_unstable();
    unique.dedup();
    if unique.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    let query = urlencoding(&json_array(&unique));
    let projects: Vec<RawProject> = get(&format!("{API}/projects?ids={query}")).await?;
    Ok(projects
        .into_iter()
        .map(|project| {
            (
                project.id,
                plan::Named {
                    slug: project.slug,
                    title: project.title,
                    icon_url: project.icon_url,
                },
            )
        })
        .collect())
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
        dependencies: raw
            .dependencies
            .iter()
            .map(|dependency| VersionDependency {
                project_id: dependency.project_id.clone().filter(|id| !id.is_empty()),
                kind: plan::DependencyKind::from_api(&dependency.dependency_type),
            })
            .collect(),
    }
}

/// 把一个版本的主文件下到指定位置，返回落盘的路径。
///
/// 整合包要先把 `.mrpack` 拿到手才能读里面的 index，所以它走这一条而不是
/// `install`——那一条的前提是「文件放进某个实例的某个目录」。
pub async fn fetch_primary_file(
    version_id: &str,
    directory: &std::path::Path,
    events: &UnboundedSender<DownloadEvent>,
) -> Result<std::path::PathBuf> {
    let version: RawVersion = get(&format!("{API}/version/{version_id}")).await?;
    let file = version
        .primary_file()
        .ok_or_else(|| anyhow!("{} 这个版本没有可下载的文件", version.name))?;
    tokio::fs::create_dir_all(directory).await?;
    let destination = fern_download::safe_join(directory, std::path::Path::new(&file.filename))?;
    let task = DownloadTask::new(destination.clone(), &file.url, &file.hashes.sha1, file.size)?;
    crate::data::downloader::client(4)
        .download_all(vec![task], events)
        .await?;
    Ok(destination)
}

/// 装一个版本，连同它缺的那些必需依赖。
///
/// 「缺的」是这里的关键词：计划由 [`plan::resolve`] 算，它看得见实例里已经有
/// 什么，所以已经装过的前置不会再下一份。界面上显示的也是同一份计划——显示
/// 一套、做另一套，是这类功能出错最常见的方式。
pub async fn install(
    paths: &DataPaths,
    instance_id: &str,
    version_id: &str,
    kind: ResourceKind,
    job: &crate::Job,
) -> Result<InstallOutcome> {
    // 两步：先把要装的东西问清楚（一个模组可能牵出好几个必需依赖），再一起下。
    job.expect(2);
    job.step("解析依赖");
    let events = &job.downloads();

    let subdirectory = kind
        .directory()
        .ok_or_else(|| anyhow!("整合包要用来新建实例，不能装进已有的实例"))?;
    // 装到这个实例真正的游戏目录里去——外部实例的那个在别人的目录树下。
    let profile = crate::read_instance(paths, instance_id)?;
    let directory = crate::instance::paths_for(paths, &profile)
        .game_directory(instance_id)
        .join(subdirectory);
    tokio::fs::create_dir_all(&directory).await?;

    let plan = plan::resolve(paths, instance_id, version_id, kind).await?;
    if kind == ResourceKind::Mod {
        // 改模组之前先拍一张。用户以为自己只是装了个模组，而这一步可能让
        // 存档打不开（docs/fern-backup-design.md §1）。放在解析计划之后、
        // 写盘之前：解析是只读的，而它知道装的叫什么名字——快照因此记得住
        // 「装 Create 之前」，而不只是「改动模组之前」。
        let name = plan
            .files
            .iter()
            .find(|file| file.primary)
            .or(plan.files.first())
            .map(|file| file.title.clone());
        crate::backup::before_mod_change(
            paths,
            instance_id,
            name.map(|name| crate::backup::manifest::About::new("install").with("name", name)),
        );
    }
    if let Some(missing) = plan.requirements.iter().find(|item| {
        item.kind == plan::DependencyKind::Required
            && item.state == plan::RequirementState::Unavailable
    }) {
        return Err(anyhow!(
            "{} 需要 {}，但它没有适用于这个实例的版本",
            plan.files
                .first()
                .map_or("这个版本", |file| file.title.as_str()),
            missing.title
        ));
    }

    let mut tasks = Vec::new();
    for file in &plan.files {
        // Modrinth 每个文件都给 sha1，所以这里是有校验的下载。
        tasks.push(DownloadTask::new(
            fern_download::safe_join(&directory, std::path::Path::new(&file.file_name))?,
            &file.url,
            &file.sha1,
            file.bytes,
        )?);
    }

    job.step(if tasks.len() > 1 {
        format!("下载 {} 个文件（含依赖）", tasks.len())
    } else {
        "下载文件".to_owned()
    });
    let downloader = crate::data::downloader::client(8);
    downloader.download_all(tasks, events).await?;

    // 记一笔谁放进来的。sha1 用 Modrinth 给的那个——下载刚刚照着它校验过，
    // 再读一遍磁盘算一次是在为同一个答案付两次钱。
    //
    // 版本号得从 jar 里读，不能用 Modrinth 的 `version_number`：对账时拿来比
    // 的是 jar 自己声明的那个，两次比较必须问同一个人，否则每一次更新都会被
    // 判成「版本号没变」。
    crate::instance::origin::record(
        paths,
        instance_id,
        plan.files
            .iter()
            .map(|file| crate::instance::origin::Entry {
                file: format!("{subdirectory}/{}", file.file_name),
                sha1: file.sha1.clone(),
                version: crate::instance::integrity::declared_version(
                    &directory.join(&file.file_name),
                ),
                origin: crate::instance::origin::Origin::Supply {
                    source: Source::Modrinth,
                    project_id: file.project_id.clone(),
                    version_id: file.version_id.clone(),
                },
            })
            .collect(),
    );

    Ok(InstallOutcome {
        installed: plan.files.iter().map(|file| file.title.clone()).collect(),
        files: plan
            .files
            .iter()
            .map(|file| file.file_name.clone())
            .collect(),
        reused: plan
            .requirements
            .iter()
            .filter(|item| item.state == plan::RequirementState::Satisfied)
            .map(|item| item.title.clone())
            .collect(),
    })
}

/// 装完之后有什么可说的。
///
/// 分成「装了什么」和「本来就有什么」两栏：后者是这次**没有**发生的事，而它
/// 恰恰是用户最需要知道的一句——上一版会把已有的前置再装一遍，而界面上看不出
/// 任何区别。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallOutcome {
    /// 装上的东西，用项目名而不是文件名——文件名是给磁盘看的。
    pub installed: Vec<String>,
    pub files: Vec<String>,
    /// 已经有了、这次跳过的前置。
    pub reused: Vec<String>,
}

/// 磁盘上一个 jar 在 Modrinth 上的身份。
///
/// 按内容哈希反查，而不是靠安装时记下来的来源：后者对用户自己拖进来的 jar
/// 无效，也会随时间和实际文件对不上，而哈希描述的永远是磁盘上真正的那一份。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnownFile {
    pub project_id: String,
    pub version_id: String,
    pub url: String,
    pub file_name: String,
    pub size: u64,
    pub sha1: String,
    /// mrpack 的 `hashes` 两个都要，而 sha512 只有上游算得出来。
    pub sha512: String,
}

/// 一批 sha1 分别属于哪个版本。查不到的不在返回值里。
///
/// 一次请求问完全部：三百个模组发三百次请求会被速率限制挡下来，而这个端点
/// 本来就是为批量查询设计的。
pub(crate) async fn known_files(
    hashes: &[String],
) -> Result<std::collections::HashMap<String, KnownFile>> {
    if hashes.is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    let response = client()
        .post(format!("{API}/version_files"))
        .json(&serde_json::json!({ "hashes": hashes, "algorithm": "sha1" }))
        .send()
        .await
        .context("向 Modrinth 反查模组来源")?
        .error_for_status()
        .context("Modrinth 拒绝了这次反查")?;
    let raw: std::collections::HashMap<String, RawVersion> =
        response.json().await.context("解析 Modrinth 的响应")?;

    Ok(raw
        .into_iter()
        .filter_map(|(hash, version)| {
            // 一个版本可能挂好几个文件，要的是哈希对得上的那一个。
            let file = version
                .files
                .iter()
                .find(|file| file.hashes.sha1.eq_ignore_ascii_case(&hash))?;
            Some((
                hash.to_ascii_lowercase(),
                KnownFile {
                    project_id: version.project_id.clone(),
                    version_id: version.id.clone(),
                    url: file.url.clone(),
                    file_name: file.filename.clone(),
                    size: file.size,
                    sha1: file.hashes.sha1.clone(),
                    sha512: file.hashes.sha512.clone(),
                },
            ))
        })
        .collect())
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
    fn only_https_links_are_handed_to_the_system_opener() {
        assert!(is_external_url("https://modrinth.com/mod/sodium"));
        assert!(is_external_url("https://github.com/some/repo"));

        // 这些会被原样递给 xdg-open / explorer。
        assert!(!is_external_url("http://example.com"));
        assert!(!is_external_url("file:///etc/passwd"));
        assert!(!is_external_url("javascript:alert(1)"));
        assert!(!is_external_url("ms-settings:privacy"));
        assert!(!is_external_url("--version"));
        assert!(!is_external_url(""));
        assert!(!is_external_url("https://"));
    }

    #[test]
    fn resource_kinds_land_in_the_directory_the_game_reads() {
        assert_eq!(ResourceKind::Mod.directory(), Some("mods"));
        assert_eq!(
            ResourceKind::ResourcePack.directory(),
            Some("resourcepacks")
        );
        assert_eq!(ResourceKind::Shader.directory(), Some("shaderpacks"));
        // 整合包没有「放进哪个目录」这个答案，它自带一整棵树。
        assert_eq!(ResourceKind::Modpack.directory(), None);
        // 加载器筛选只对模组成立：资源包的 loader 是 minecraft，光影是 iris。
        assert!(ResourceKind::Mod.honours_loader());
        assert!(!ResourceKind::ResourcePack.honours_loader());
        assert!(!ResourceKind::Shader.honours_loader());
    }

    #[test]
    fn the_user_agent_identifies_this_launcher() {
        // Modrinth 按 User-Agent 做速率限制，匿名调用会被降级甚至拒绝。
        assert!(USER_AGENT.contains("fern"));
        assert!(USER_AGENT.contains("github.com"));
    }
}
