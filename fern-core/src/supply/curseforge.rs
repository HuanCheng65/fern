//! CurseForge 的那一半：URL 怎么拼、JSON 长什么样。
//!
//! 和 Modrinth 有三处根本的不同，都在这个文件里消化掉，出了这个文件就看不见：
//!
//! - **要密钥。** 每个请求带 `x-api-key`。密钥在构建时注入（`option_env!`），
//!   只出现在 Rust 侧的请求头里。它能被从二进制里提取出来，这一点不必假装；
//!   真正的风险是被滥用后吊销和限流，所以基址留成一个常量，以后换成自己的
//!   代理不用改协议。
//! - **id 是整数。** 项目（mod）和版本（file）都是。出了这里一律是字符串。
//! - **没有「版本号」。** 一个 file 只有 `displayName` 和 `fileName`；
//!   `gameVersions` 里把游戏版本和加载器名字混在一起放（`["1.20.1", "Forge"]`），
//!   要自己拆。
//!
//! 常量都以 <https://docs.curseforge.com/rest-api/> 为准。

use std::collections::HashMap;

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

use super::{
    Dependency, GalleryImage, ProjectDetail, ProjectLink, ProjectVersion, Release, ReleaseFile,
    ResourceKind, SearchHit, SearchQuery, SearchResult, Source, VersionDependency, http::Upstream,
    is_external_url, plan, survey::KnownVersion, urlencoding,
};
use crate::LoaderKind;

const API: &str = "https://api.curseforge.com/v1";

/// 构建时注入的密钥。本地开发放在 `~/.cargo/config.toml` 的 `[env]` 里，
/// CI 从仓库密钥 `CURSEFORGE_API_KEY` 来。
const KEY: Option<&str> = option_env!("FERN_CURSEFORGE_API_KEY");

/// Minecraft 在 CurseForge 上的 gameId。
const MINECRAFT: u32 = 432;

/// 一页最多 50 条，`index + pageSize` 不能超过 10000。
const PAGE_LIMIT: u32 = 50;
const INDEX_LIMIT: u32 = 10_000;

/// 一个项目最多列这么多个版本。热门模组有上千个文件，全拉下来是二十多次请求，
/// 而翻到第两百个版本的人几乎不存在。
const VERSION_LIMIT: u32 = 200;

pub(super) fn available() -> bool {
    KEY.is_some_and(|key| !key.trim().is_empty())
}

fn upstream() -> Result<Upstream> {
    match KEY.map(str::trim).filter(|key| !key.is_empty()) {
        Some(key) => Ok(Upstream {
            name: "CurseForge",
            key: Some(key),
        }),
        None => Err(anyhow!("此版本未接入 CurseForge")),
    }
}

async fn get<T: serde::de::DeserializeOwned>(url: &str) -> Result<T> {
    let wrapped: Data<T> = upstream()?.get(url).await?;
    Ok(wrapped.data)
}

async fn post<T: serde::de::DeserializeOwned>(url: &str, body: &impl Serialize) -> Result<T> {
    let wrapped: Data<T> = upstream()?.post(url, body).await?;
    Ok(wrapped.data)
}

/// 资源种类在 CurseForge 上叫 class。
fn class_id(kind: ResourceKind) -> u32 {
    match kind {
        ResourceKind::Mod => 6,
        ResourceKind::ResourcePack => 12,
        ResourceKind::Shader => 6552,
        ResourceKind::Modpack => 4471,
    }
}

/// 反过来：详情页要知道一个项目是什么，才知道装到哪个目录去。名字和
/// Modrinth 的 `project_type` 一致，界面只认这一套。
fn project_type(class_id: u32) -> &'static str {
    match class_id {
        12 => "resourcepack",
        6552 => "shader",
        4471 => "modpack",
        _ => "mod",
    }
}

/// `ModLoaderType` 枚举。
fn loader_type(loader: LoaderKind) -> Option<u32> {
    match loader {
        LoaderKind::Forge => Some(1),
        LoaderKind::LiteLoader => Some(3),
        LoaderKind::Fabric => Some(4),
        LoaderKind::Quilt => Some(5),
        LoaderKind::NeoForge => Some(6),
        LoaderKind::Vanilla => None,
    }
}

/// `ModLoaderType` 的反面，翻成两家共用的小写标签（见 [`super::loader_tags`]）。
fn loader_tag(value: u32) -> Option<&'static str> {
    match value {
        1 => Some("forge"),
        3 => Some("liteloader"),
        4 => Some("fabric"),
        5 => Some("quilt"),
        6 => Some("neoforge"),
        _ => None,
    }
}

/// 一个文件的 `gameVersions` 拆成「游戏版本」和「加载器」两栏。
///
/// 上游把它们放在同一个数组里：`["1.20.1", "Forge", "NeoForge", "Client"]`。
/// 加载器按名字认；游戏版本是以数字开头的那些——剩下的（`Client`、`Server`、
/// `Java 17`、光影包上的 `Iris`）两边都不是，丢掉。
fn split_game_versions(values: &[String]) -> (Vec<String>, Vec<String>) {
    let mut games = Vec::new();
    let mut loaders = Vec::new();
    for value in values {
        let lower = value.to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            "forge" | "neoforge" | "fabric" | "quilt" | "liteloader"
        ) {
            loaders.push(lower);
        } else if value.starts_with(|c: char| c.is_ascii_digit()) {
            games.push(value.clone());
        }
    }
    (games, loaders)
}

// ——— 线上的 JSON 形状，只取用得上的字段 ———

#[derive(Debug, Deserialize)]
struct Data<T> {
    data: T,
}

#[derive(Debug, Deserialize)]
struct Paged<T> {
    data: Vec<T>,
    pagination: Pagination,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pagination {
    #[serde(default)]
    total_count: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawMod {
    id: u64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    slug: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    class_id: u32,
    #[serde(default)]
    download_count: u64,
    #[serde(default)]
    thumbs_up_count: u64,
    #[serde(default)]
    date_modified: String,
    #[serde(default)]
    links: RawLinks,
    #[serde(default)]
    logo: Option<RawAsset>,
    #[serde(default)]
    screenshots: Vec<RawAsset>,
    #[serde(default)]
    authors: Vec<RawAuthor>,
    #[serde(default)]
    categories: Vec<RawCategory>,
    #[serde(default)]
    latest_files_indexes: Vec<RawFileIndex>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawLinks {
    #[serde(default)]
    website_url: Option<String>,
    #[serde(default)]
    wiki_url: Option<String>,
    #[serde(default)]
    issues_url: Option<String>,
    #[serde(default)]
    source_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawAsset {
    #[serde(default)]
    title: String,
    #[serde(default)]
    thumbnail_url: String,
    #[serde(default)]
    url: String,
}

#[derive(Debug, Deserialize)]
struct RawAuthor {
    #[serde(default)]
    name: String,
}

#[derive(Debug, Deserialize)]
struct RawCategory {
    #[serde(default)]
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawFileIndex {
    #[serde(default)]
    game_version: String,
    #[serde(default)]
    file_id: u64,
    #[serde(default)]
    release_type: u32,
    #[serde(default)]
    mod_loader: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawMatches {
    #[serde(default)]
    exact_matches: Vec<RawMatch>,
}

#[derive(Debug, Deserialize)]
struct RawMatch {
    file: RawFile,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawFile {
    id: u64,
    mod_id: u64,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    file_name: String,
    /// 1 正式、2 测试、3 内测。
    #[serde(default)]
    release_type: u32,
    #[serde(default)]
    hashes: Vec<RawHash>,
    #[serde(default)]
    file_date: String,
    #[serde(default)]
    file_length: u64,
    #[serde(default)]
    download_count: u64,
    /// 作者不允许第三方分发时为 null。
    #[serde(default)]
    download_url: Option<String>,
    #[serde(default)]
    game_versions: Vec<String>,
    #[serde(default)]
    dependencies: Vec<RawDependency>,
    #[serde(default)]
    is_server_pack: bool,
    #[serde(default)]
    file_fingerprint: u64,
}

#[derive(Debug, Deserialize)]
struct RawHash {
    value: String,
    /// 1 是 sha1，2 是 md5。
    algo: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawDependency {
    mod_id: u64,
    relation_type: u32,
}

impl RawMod {
    fn icon(&self) -> Option<String> {
        self.logo
            .as_ref()
            .map(|logo| logo.thumbnail_url.clone())
            .filter(|url| is_external_url(url))
    }
}

impl RawFile {
    fn sha1(&self) -> Option<String> {
        self.hashes
            .iter()
            .find(|hash| hash.algo == 1 && !hash.value.is_empty())
            .map(|hash| hash.value.to_ascii_lowercase())
    }

    fn version_type(&self) -> &'static str {
        match self.release_type {
            2 => "beta",
            3 => "alpha",
            _ => "release",
        }
    }

    /// 只认 https 的下载地址。它会被原样交给下载器。
    fn url(&self) -> Option<String> {
        self.download_url.clone().filter(|url| is_external_url(url))
    }
}

/// `FileRelationType`。
///
/// 「工具」是作者推荐配套的东西，不装也能跑，当作可选；「内含」是打包在里面的，
/// 和 Modrinth 的 embedded 一个意思。
fn dependency_kind(relation: u32) -> Option<plan::DependencyKind> {
    match relation {
        1 | 6 => Some(plan::DependencyKind::Embedded),
        2 | 4 => Some(plan::DependencyKind::Optional),
        3 => Some(plan::DependencyKind::Required),
        5 => Some(plan::DependencyKind::Incompatible),
        _ => None,
    }
}

fn dependencies(raw: &[RawDependency]) -> Vec<(String, plan::DependencyKind)> {
    raw.iter()
        .filter_map(|dependency| {
            Some((
                dependency.mod_id.to_string(),
                dependency_kind(dependency.relation_type)?,
            ))
        })
        .collect()
}

/// 排序。CurseForge 的 `ModsSearchSortField`：
/// 1 推荐、2 人气、3 最近更新、6 下载量、11 发布日期、12 评分。
///
/// 「相关度」没有同名的字段，实测（2026-10）挑的：
///
/// - 不传排序时，搜 `jei` 第一条是个不相干的小模组，不能用。
/// - 「推荐、降序」对模组最准：jei、create、waystones、applied energistics
///   第一条都是本体。但对整合包，它会把推广位顶上来——搜 `all the mods 9`
///   第一条是个宝可梦服务器包。
/// - 「人气、降序」对整合包最准（ATM 系列、Better Adventures+ 在前），对模组
///   则会把 Xaero's Minimap 排到 Create 前面。
///
/// 所以整合包按人气，其余按推荐。没有搜索词时相关度无从谈起，按人气排。
/// 「关注数」也没有，评分是最近的那个。
fn sort_field(sort: &str, query: &str, kind: ResourceKind) -> u32 {
    match sort {
        "downloads" => 6,
        "updated" => 3,
        "newest" => 11,
        "follows" => 12,
        _ if query.trim().is_empty() || kind == ResourceKind::Modpack => 2,
        _ => 1,
    }
}

pub(super) async fn search(request: &SearchQuery) -> Result<SearchResult> {
    let kind = request.kind;
    let limit = request.limit.clamp(1, PAGE_LIMIT);
    // 上游不允许翻过第一万条。到头就说到头了，别让它报一个 400。
    if request.offset + limit > INDEX_LIMIT {
        return Ok(SearchResult {
            hits: Vec::new(),
            total: u64::from(INDEX_LIMIT),
        });
    }
    let mut url = format!(
        "{API}/mods/search?gameId={MINECRAFT}&classId={}&index={}&pageSize={limit}",
        class_id(kind),
        request.offset,
    );
    if !request.query.trim().is_empty() {
        url.push_str(&format!(
            "&searchFilter={}",
            urlencoding(request.query.trim())
        ));
    }
    if !request.game_version.is_empty() {
        url.push_str(&format!(
            "&gameVersion={}",
            urlencoding(&request.game_version)
        ));
    }
    if let Some(loader) = request.loader.filter(|_| kind.honours_loader()) {
        // Quilt 能加载 Fabric 模组，而多数作者只标 Fabric——和 Modrinth 那边同一条规则。
        let types: Vec<u32> = match loader {
            LoaderKind::Quilt => vec![5, 4],
            other => loader_type(other).into_iter().collect(),
        };
        if types.len() == 1 {
            url.push_str(&format!("&modLoaderType={}", types[0]));
        } else if !types.is_empty() {
            let names: Vec<String> = types
                .iter()
                .filter_map(|value| loader_name(*value))
                .map(|name| format!("\"{name}\""))
                .collect();
            url.push_str(&format!(
                "&modLoaderTypes={}",
                urlencoding(&format!("[{}]", names.join(",")))
            ));
        }
    }
    // 不带 sortOrder 时上游按升序排，「人气」就成了最冷门的在前。
    url.push_str(&format!(
        "&sortField={}&sortOrder=desc",
        sort_field(&request.sort, &request.query, kind)
    ));

    let raw: Paged<RawMod> = upstream()?.get(&url).await?;
    Ok(SearchResult {
        total: raw.pagination.total_count.min(u64::from(INDEX_LIMIT)),
        hits: raw
            .data
            .into_iter()
            .map(|item| SearchHit {
                project_id: item.id.to_string(),
                icon_url: item.icon(),
                slug: item.slug,
                title: item.name.trim().to_owned(),
                description: item.summary,
                author: item
                    .authors
                    .into_iter()
                    .next()
                    .map(|author| author.name)
                    .unwrap_or_default(),
                downloads: item.download_count,
                categories: item
                    .categories
                    .into_iter()
                    .map(|category| category.name)
                    .collect(),
                source: Source::CurseForge,
            })
            .collect(),
    })
}

/// `modLoaderTypes` 要的是枚举名而不是数字。
fn loader_name(value: u32) -> Option<&'static str> {
    match value {
        1 => Some("Forge"),
        3 => Some("LiteLoader"),
        4 => Some("Fabric"),
        5 => Some("Quilt"),
        6 => Some("NeoForge"),
        _ => None,
    }
}

fn mod_id(project: &str) -> Result<u64> {
    project
        .parse()
        .map_err(|_| anyhow!("CurseForge 项目 id 应为数字：{project}"))
}

fn file_id(version: &str) -> Result<u64> {
    version
        .parse()
        .map_err(|_| anyhow!("CurseForge 文件 id 应为数字：{version}"))
}

pub(super) async fn project(id: &str) -> Result<ProjectDetail> {
    let raw: RawMod = get(&format!("{API}/mods/{}", mod_id(id)?)).await?;
    let icon_url = raw.icon();

    let mut links = Vec::new();
    for (label, url) in [
        ("源码", raw.links.source_url.clone()),
        ("问题追踪", raw.links.issues_url.clone()),
        ("百科", raw.links.wiki_url.clone()),
    ] {
        if let Some(url) = url.filter(|url| is_external_url(url)) {
            links.push(ProjectLink {
                label: label.to_owned(),
                url,
            });
        }
    }
    if let Some(url) = raw
        .links
        .website_url
        .clone()
        .filter(|url| is_external_url(url))
    {
        links.push(ProjectLink {
            label: "CurseForge 页面".to_owned(),
            url,
        });
    }

    // 项目上没有「支持哪些版本」这一栏，从各版本最新文件的索引里收。
    let mut game_versions: Vec<String> = Vec::new();
    let mut loaders: Vec<String> = Vec::new();
    for index in &raw.latest_files_indexes {
        if !index.game_version.is_empty() && !game_versions.contains(&index.game_version) {
            game_versions.push(index.game_version.clone());
        }
        if let Some(tag) = index.mod_loader.and_then(loader_tag)
            && !loaders.iter().any(|known| known == tag)
        {
            loaders.push(tag.to_owned());
        }
    }

    Ok(ProjectDetail {
        id: raw.id.to_string(),
        slug: raw.slug,
        title: raw.name.trim().to_owned(),
        description: raw.summary,
        project_type: project_type(raw.class_id).to_owned(),
        categories: raw
            .categories
            .into_iter()
            .map(|category| category.name)
            .collect(),
        icon_url,
        gallery: raw
            .screenshots
            .into_iter()
            .filter(|image| is_external_url(&image.url))
            .map(|image| GalleryImage {
                url: image.url,
                title: image.title,
            })
            .collect(),
        downloads: raw.download_count,
        followers: raw.thumbs_up_count,
        updated: raw.date_modified,
        // CurseForge 的 API 不给许可证。空着，界面那一栏就不出现。
        license: String::new(),
        game_versions,
        loaders,
        links,
        source: Source::CurseForge,
    })
}

/// 一个项目的文件，新的在前，最多 [`VERSION_LIMIT`] 个。
async fn raw_files(project: u64, filter: &str) -> Result<Vec<RawFile>> {
    let mut files = Vec::new();
    let mut index = 0;
    while index < VERSION_LIMIT {
        let page: Paged<RawFile> = upstream()?
            .get(&format!(
                "{API}/mods/{project}/files?index={index}&pageSize={PAGE_LIMIT}{filter}"
            ))
            .await?;
        let count = page.data.len() as u32;
        files.extend(page.data);
        index += PAGE_LIMIT;
        if count < PAGE_LIMIT || u64::from(index) >= page.pagination.total_count {
            break;
        }
    }
    // 服务端包是给开服用的，装进客户端没有意义。
    files.retain(|file| !file.is_server_pack);
    files.sort_by(|left, right| right.file_date.cmp(&left.file_date));
    Ok(files)
}

/// 一个项目的版本：最新的 [`VERSION_LIMIT`] 个，再加上适用于 `target` 的那些。
///
/// 只取最新的不够：JEI 有几千个文件，最新的两百个几乎全是新版本的，给 1.12.2
/// 的实例看就只剩四五个。全拉下来又是几十次请求。所以按目标实例再单独问一次，
/// 两份合在一起。
pub(super) async fn versions(
    project: &str,
    target: Option<(&str, LoaderKind)>,
) -> Result<Vec<ProjectVersion>> {
    let id = mod_id(project)?;
    let mut files = raw_files(id, "").await?;
    if let Some((game_version, loader)) = target.filter(|(version, _)| !version.is_empty()) {
        let mut filter = format!("&gameVersion={}", urlencoding(game_version));
        if let Some(value) = loader_type(loader) {
            filter.push_str(&format!("&modLoaderType={value}"));
        }
        for file in raw_files(id, &filter).await? {
            if !files.iter().any(|known| known.id == file.id) {
                files.push(file);
            }
        }
        files.sort_by(|left, right| right.file_date.cmp(&left.file_date));
    }
    Ok(files.into_iter().map(describe).collect())
}

fn describe(raw: RawFile) -> ProjectVersion {
    let (game_versions, loaders) = split_game_versions(&raw.game_versions);
    ProjectVersion {
        id: raw.id.to_string(),
        version_type: raw.version_type().to_owned(),
        name: raw.display_name.clone(),
        // 没有单独的版本号字段，显示名就是作者给这个版本起的名字。
        version_number: raw.display_name,
        game_versions,
        loaders,
        downloads: raw.download_count,
        date_published: raw.file_date,
        file_name: Some(raw.file_name).filter(|name| !name.is_empty()),
        source: Source::CurseForge,
        dependencies: dependencies(&raw.dependencies)
            .into_iter()
            .map(|(project_id, kind)| VersionDependency {
                project_id: Some(project_id),
                kind,
            })
            .collect(),
    }
}

impl From<RawFile> for Release {
    fn from(raw: RawFile) -> Self {
        let file = Some(ReleaseFile {
            url: raw.url(),
            sha1: raw.sha1(),
            name: raw.file_name.clone(),
            size: raw.file_length,
        })
        .filter(|file| !file.name.is_empty());
        Release {
            project_id: raw.mod_id.to_string(),
            version_id: raw.id.to_string(),
            name: raw.display_name.clone(),
            version_number: raw.display_name.clone(),
            file,
            dependencies: dependencies(&raw.dependencies)
                .into_iter()
                .map(|(project_id, kind)| Dependency {
                    project_id: Some(project_id),
                    version_id: None,
                    kind,
                })
                .collect(),
        }
    }
}

/// 一批文件 id 换成文件。整合包的清单里只有 id，一次问完。
async fn files_by_id(ids: &[u64]) -> Result<Vec<RawFile>> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Request<'a> {
        file_ids: &'a [u64],
    }
    let mut out = Vec::new();
    for chunk in ids.chunks(500) {
        let found: Vec<RawFile> =
            post(&format!("{API}/mods/files"), &Request { file_ids: chunk }).await?;
        out.extend(found);
    }
    Ok(out)
}

pub(super) async fn release(version_id: &str) -> Result<Release> {
    let id = file_id(version_id)?;
    files_by_id(&[id])
        .await?
        .into_iter()
        .find(|file| file.id == id)
        .map(Release::from)
        .ok_or_else(|| anyhow!("CurseForge 上没有这个文件"))
}

/// 依赖只给了项目时，挑一个装得上的版本：优先正式版，没有就用最新的。
pub(super) async fn candidate(
    project: &str,
    game_version: &str,
    loader: LoaderKind,
) -> Result<Option<Release>> {
    let project = mod_id(project)?;
    // Quilt 先找 Quilt 的，没有再找 Fabric 的。
    let types: Vec<Option<u32>> = match loader {
        LoaderKind::Quilt => vec![Some(5), Some(4)],
        other => vec![loader_type(other)],
    };
    for loader in types {
        let mut filter = String::new();
        if !game_version.is_empty() {
            filter.push_str(&format!("&gameVersion={}", urlencoding(game_version)));
        }
        if let Some(value) = loader {
            filter.push_str(&format!("&modLoaderType={value}"));
        }
        let mut files = raw_files(project, &filter).await?;
        let chosen = files
            .iter()
            .position(|file| file.release_type == 1)
            .or(if files.is_empty() { None } else { Some(0) });
        if let Some(index) = chosen {
            return Ok(Some(files.swap_remove(index).into()));
        }
    }
    Ok(None)
}

pub(super) async fn project_names(ids: &[String]) -> Result<HashMap<String, plan::Named>> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Request {
        mod_ids: Vec<u64>,
    }
    let mut unique: Vec<u64> = ids.iter().filter_map(|id| id.parse().ok()).collect();
    unique.sort_unstable();
    unique.dedup();
    if unique.is_empty() {
        return Ok(HashMap::new());
    }
    let found: Vec<RawMod> = post(&format!("{API}/mods"), &Request { mod_ids: unique }).await?;
    Ok(found
        .into_iter()
        .map(|item| {
            let icon_url = item.icon();
            (
                item.id.to_string(),
                plan::Named {
                    page: item
                        .links
                        .website_url
                        .clone()
                        .filter(|url| is_external_url(url))
                        .map(|url| url.trim_end_matches('/').to_owned()),
                    slug: item.slug,
                    title: item.name.trim().to_owned(),
                    icon_url,
                },
            )
        })
        .collect())
}

impl RawFile {
    fn known(&self) -> KnownVersion {
        let (game_versions, loaders) = split_game_versions(&self.game_versions);
        KnownVersion {
            project_id: self.mod_id.to_string(),
            version_id: self.id.to_string(),
            version_number: self.display_name.clone(),
            game_versions,
            loaders,
        }
    }
}

/// 一批指纹分别是哪个文件。查不到的不在返回值里。
pub(super) async fn versions_by_fingerprint(prints: &[u32]) -> Result<HashMap<u32, KnownVersion>> {
    #[derive(Serialize)]
    struct Request<'a> {
        fingerprints: &'a [u32],
    }
    let mut out = HashMap::new();
    for chunk in prints.chunks(500) {
        let found: RawMatches = post(
            &format!("{API}/fingerprints/{MINECRAFT}"),
            &Request {
                fingerprints: chunk,
            },
        )
        .await?;
        for item in found.exact_matches {
            // 上游的指纹按无符号 32 位算，JSON 里是个 long。
            let Ok(print) = u32::try_from(item.file.file_fingerprint) else {
                continue;
            };
            out.insert(print, item.file.known());
        }
    }
    Ok(out)
}

/// 这些项目各自最新的、**这个实例装得上的**文件，只要比手上那一份新的。
///
/// 项目上带着一张「每个游戏版本、每个加载器、每种发布类型的最新文件」的索引
/// （`latestFilesIndexes`），一次批量请求就能答完，不用每个模组翻一遍文件列表。
/// 正式版优先：用户手上是正式版，不该被推一个测试版。
///
/// `current` 是（项目 id，手上那个文件的 id）。CurseForge 的文件 id 是递增的，
/// 比它小的不是「新版」。
pub(super) async fn latest_for(
    current: &[(String, String)],
    game_version: &str,
    loader: LoaderKind,
) -> Result<HashMap<String, KnownVersion>> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Request {
        mod_ids: Vec<u64>,
    }
    let held: HashMap<u64, u64> = current
        .iter()
        .filter_map(|(project, file)| Some((project.parse().ok()?, file.parse().ok()?)))
        .collect();
    if held.is_empty() {
        return Ok(HashMap::new());
    }
    let accepted: Vec<u32> = match loader {
        LoaderKind::Quilt => vec![5, 4],
        other => loader_type(other).into_iter().collect(),
    };
    let mods: Vec<RawMod> = post(
        &format!("{API}/mods"),
        &Request {
            mod_ids: held.keys().copied().collect(),
        },
    )
    .await?;

    let mut chosen: HashMap<u64, u64> = HashMap::new();
    for item in &mods {
        let Some(&have) = held.get(&item.id) else {
            continue;
        };
        let fitting = item.latest_files_indexes.iter().filter(|index| {
            index.game_version == game_version
                && (accepted.is_empty()
                    || index
                        .mod_loader
                        .is_some_and(|value| accepted.contains(&value)))
        });
        let (releases, others): (Vec<&RawFileIndex>, Vec<&RawFileIndex>) =
            fitting.partition(|index| index.release_type == 1);
        let pool = if releases.is_empty() {
            others
        } else {
            releases
        };
        if let Some(best) = pool.iter().map(|index| index.file_id).max()
            && best > have
        {
            chosen.insert(item.id, best);
        }
    }
    if chosen.is_empty() {
        return Ok(HashMap::new());
    }

    let ids: Vec<u64> = chosen.values().copied().collect();
    Ok(files_by_id(&ids)
        .await?
        .into_iter()
        // 只能手动下载的新版不报：模组列表上的「更新」是一键的，没有地方摆
        // 「先去网站上下」这件事，报出来就是一个按了必然失败的按钮。
        .filter(|file| file.url().is_some())
        .map(|file| (file.mod_id.to_string(), file.known()))
        .collect())
}

/// 整合包里一个项目的身份：叫什么、文件放哪个目录、网页在哪。
pub(super) struct PackProject {
    pub title: String,
    pub directory: &'static str,
    pub page: Option<String>,
}

/// 整合包里一个文件：叫什么、从哪下、多大、sha1。
pub(super) struct PackFileInfo {
    pub id: u64,
    pub name: String,
    pub url: Option<String>,
    pub sha1: Option<String>,
    pub size: u64,
}

pub(super) async fn pack_projects(ids: &[u64]) -> Result<HashMap<u64, PackProject>> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Request<'a> {
        mod_ids: &'a [u64],
    }
    let mut out = HashMap::new();
    for chunk in ids.chunks(500) {
        let found: Vec<RawMod> = post(&format!("{API}/mods"), &Request { mod_ids: chunk }).await?;
        for item in found {
            // 整合包里也会带资源包和光影，各回各的目录；认不出的按模组放，
            // 和 CurseForge 自己的客户端一样。
            let directory = match item.class_id {
                12 => "resourcepacks",
                6552 => "shaderpacks",
                _ => "mods",
            };
            let page = item
                .links
                .website_url
                .clone()
                .filter(|url| is_external_url(url))
                .map(|url| url.trim_end_matches('/').to_owned());
            out.insert(
                item.id,
                PackProject {
                    title: item.name.trim().to_owned(),
                    directory,
                    page,
                },
            );
        }
    }
    Ok(out)
}

pub(super) async fn pack_files(ids: &[u64]) -> Result<Vec<PackFileInfo>> {
    Ok(files_by_id(ids)
        .await?
        .into_iter()
        .map(|file| PackFileInfo {
            id: file.id,
            url: file.url(),
            sha1: file.sha1(),
            name: file.file_name,
            size: file.file_length,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loaders_and_game_versions_are_pulled_apart() {
        let values: Vec<String> = ["1.20.1", "Forge", "NeoForge", "Client", "Java 17", "1.20"]
            .iter()
            .map(|value| (*value).to_owned())
            .collect();
        let (games, loaders) = split_game_versions(&values);
        assert_eq!(games, vec!["1.20.1", "1.20"]);
        // 小写，和 Modrinth 的标签是同一套——界面上的兼容性判断只写一遍。
        assert_eq!(loaders, vec!["forge", "neoforge"]);
    }

    #[test]
    fn relation_types_map_onto_the_same_four_kinds() {
        assert_eq!(dependency_kind(3), Some(plan::DependencyKind::Required));
        assert_eq!(dependency_kind(2), Some(plan::DependencyKind::Optional));
        // 「工具」不装也能跑。
        assert_eq!(dependency_kind(4), Some(plan::DependencyKind::Optional));
        assert_eq!(dependency_kind(5), Some(plan::DependencyKind::Incompatible));
        assert_eq!(dependency_kind(1), Some(plan::DependencyKind::Embedded));
        assert_eq!(dependency_kind(6), Some(plan::DependencyKind::Embedded));
        assert_eq!(dependency_kind(99), None);
    }

    #[test]
    fn a_file_without_permission_to_redistribute_has_no_url() {
        let raw: RawFile = serde_json::from_str(
            r#"{"id":5,"modId":7,"displayName":"Mod 1.0","fileName":"mod-1.0.jar",
                "releaseType":1,"fileLength":10,"downloadUrl":null,
                "hashes":[{"value":"ABCDEF","algo":1},{"value":"00","algo":2}],
                "gameVersions":["1.20.1","Fabric"],
                "dependencies":[{"modId":306612,"relationType":3}]}"#,
        )
        .expect("parse");
        let release = Release::from(raw);
        let file = release.file.expect("file");
        assert_eq!(file.url, None);
        // sha1 照样有：手动下载回来的那一份要拿它对。
        assert_eq!(file.sha1.as_deref(), Some("abcdef"));
        assert_eq!(release.project_id, "7");
        assert_eq!(release.version_id, "5");
        assert_eq!(
            release.dependencies[0].project_id.as_deref(),
            Some("306612")
        );
    }

    #[test]
    fn relevance_means_featured_for_mods_and_popularity_for_packs() {
        assert_eq!(sort_field("relevance", "jei", ResourceKind::Mod), 1);
        assert_eq!(sort_field("relevance", "atm", ResourceKind::Modpack), 2);
        assert_eq!(sort_field("relevance", "", ResourceKind::Mod), 2);
        assert_eq!(sort_field("downloads", "jei", ResourceKind::Mod), 6);
    }
}

/// 对着真的 CurseForge 跑。要构建时注入的密钥，也要联网，所以默认不跑：
///
/// ```text
/// cargo test -p fern-core --lib supply::curseforge::live -- --ignored --nocapture
/// ```
#[cfg(test)]
mod live {
    use super::*;

    /// JEI。热门、文件多、Forge 和 NeoForge 都有。
    const JEI: &str = "238222";

    #[tokio::test]
    #[ignore]
    async fn search_detail_and_versions_come_back_in_shape() {
        let hits = search(&SearchQuery {
            query: "jei".to_owned(),
            ..SearchQuery::default()
        })
        .await
        .expect("search");
        println!(
            "search total={} first={:?}",
            hits.total,
            hits.hits.first().map(|hit| (&hit.title, &hit.icon_url))
        );
        assert!(!hits.hits.is_empty());

        // Quilt 要同时带上 Fabric 的模组：这个参数的写法文档没写清，上游不认就会 400。
        let quilt = search(&SearchQuery {
            query: "sodium".to_owned(),
            loader: Some(LoaderKind::Quilt),
            game_version: "1.20.1".to_owned(),
            ..SearchQuery::default()
        })
        .await
        .expect("quilt search");
        println!(
            "quilt total={} first={:?}",
            quilt.total,
            quilt.hits.first().map(|hit| &hit.title)
        );
        assert!(!quilt.hits.is_empty());

        let detail = project(JEI).await.expect("project");
        println!(
            "detail {} type={} icon={:?} loaders={:?} versions={} links={:?}",
            detail.title,
            detail.project_type,
            detail.icon_url,
            detail.loaders,
            detail.game_versions.len(),
            detail.links
        );
        assert_eq!(detail.project_type, "mod");

        let list = versions(JEI, Some(("1.12.2", LoaderKind::Forge)))
            .await
            .expect("versions");
        let old = list
            .iter()
            .filter(|version| version.game_versions.iter().any(|it| it == "1.12.2"))
            .count();
        println!("1.12.2 versions={old}");
        // 只取最新两百个时这里只有四五个。
        assert!(old > 20);
        let first = list.first().expect("a version");
        println!(
            "versions={} first={} games={:?} loaders={:?} deps={}",
            list.len(),
            first.name,
            first.game_versions,
            first.loaders,
            first.dependencies.len()
        );
        assert!(!first.loaders.is_empty());

        let named = project_names(&[JEI.to_owned()]).await.expect("names");
        println!(
            "named page={:?}",
            named.get(JEI).and_then(|it| it.page.clone())
        );
    }

    #[tokio::test]
    #[ignore]
    async fn our_fingerprint_is_the_one_curseforge_computes() {
        // 拿一个真文件：下下来自己算，和上游给的对一遍，再拿去反查。
        let files = raw_files(mod_id(JEI).expect("id"), "")
            .await
            .expect("files");
        let file = files
            .iter()
            .find(|file| file.url().is_some() && file.file_length < 5_000_000)
            .expect("a downloadable file");
        let bytes = reqwest::get(file.url().expect("url"))
            .await
            .expect("download")
            .bytes()
            .await
            .expect("body");
        let ours = super::super::fingerprint::of_bytes(&bytes);
        println!(
            "{} ours={ours} theirs={}",
            file.file_name, file.file_fingerprint
        );
        assert_eq!(u64::from(ours), file.file_fingerprint);

        let found = versions_by_fingerprint(&[ours]).await.expect("lookup");
        let known = found.get(&ours).expect("upstream knows this fingerprint");
        assert_eq!(known.version_id, file.id.to_string());
        println!("matched {:?}", known);

        let latest = latest_for(
            &[(JEI.to_owned(), "1".to_owned())],
            "1.20.1",
            LoaderKind::Forge,
        )
        .await
        .expect("latest");
        println!("latest for 1.20.1 forge: {:?}", latest.get(JEI));
        assert!(latest.contains_key(JEI));
    }

    #[tokio::test]
    #[ignore]
    async fn a_dependency_already_in_mods_is_recognised_by_fingerprint() {
        use crate::supply::plan::{RequirementState, resolve};

        // Create Crafts & Additions 必需 Create。
        const ADDITIONS: &str = "439890";
        const CREATE: &str = "328085";

        let root = std::env::temp_dir().join(format!("fern-cf-live-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = crate::DataPaths::new(root.clone());
        let profile = crate::create_instance_with_loader(
            &paths,
            "live",
            "1.20.1",
            LoaderKind::Forge,
            Some("47.2.0"),
        )
        .expect("instance");
        let id = profile.id.as_str();

        let additions = candidate(ADDITIONS, "1.20.1", LoaderKind::Forge)
            .await
            .expect("lookup")
            .expect("a 1.20.1 forge file");
        let before = resolve(
            &paths,
            id,
            Source::CurseForge,
            &additions.version_id,
            ResourceKind::Mod,
            &crate::supply::Sideload::default(),
        )
        .await
        .expect("plan");
        let create = |plan: &crate::supply::plan::InstallPlan| {
            plan.requirements
                .iter()
                .find(|item| item.project_id == CREATE)
                .map(|item| (item.title.clone(), item.state))
        };
        println!("before: {:?} files={}", create(&before), before.files.len());
        assert_eq!(
            create(&before).map(|it| it.1),
            Some(RequirementState::Planned)
        );

        // 把 Create 自己放进 mods/，不经过我们的安装——就像用户拖进来的。
        let chosen = candidate(CREATE, "1.20.1", LoaderKind::Forge)
            .await
            .expect("lookup")
            .expect("create file");
        let file = chosen.file.expect("file");
        let bytes = reqwest::get(file.url.expect("url"))
            .await
            .expect("download")
            .bytes()
            .await
            .expect("body");
        let mods = crate::instance::paths_for(&paths, &profile)
            .game_directory(id)
            .join("mods");
        std::fs::create_dir_all(&mods).expect("mods");
        std::fs::write(mods.join(&file.name), &bytes).expect("write");

        let after = resolve(
            &paths,
            id,
            Source::CurseForge,
            &additions.version_id,
            ResourceKind::Mod,
            &crate::supply::Sideload::default(),
        )
        .await
        .expect("plan");
        println!("after: {:?} files={}", create(&after), after.files.len());
        assert_eq!(
            create(&after).map(|it| it.1),
            Some(RequirementState::Satisfied)
        );
        assert_eq!(after.files.len(), 1);

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    #[ignore]
    async fn a_real_modpack_resolves() {
        use crate::supply::{Sideload, cfpack, modpack};

        let hits = search(&SearchQuery {
            query: std::env::var("FERN_LIVE_PACK").unwrap_or_else(|_| "all the mods 9".to_owned()),
            kind: ResourceKind::Modpack,
            ..SearchQuery::default()
        })
        .await
        .expect("search");
        let pack = hits.hits.first().expect("a pack");
        let list = versions(&pack.project_id, None).await.expect("versions");
        let version = list.first().expect("a version");
        let release = release(&version.id).await.expect("release");
        let size = release.file.as_ref().map_or(0, |file| file.size);
        println!("{} {} {} bytes", pack.title, version.name, size);

        let directory = std::env::temp_dir().join(format!("fern-cf-pack-{}", std::process::id()));
        let file = release.file.expect("file");
        let bytes = reqwest::get(file.url.expect("pack url"))
            .await
            .expect("download")
            .bytes()
            .await
            .expect("body");
        std::fs::create_dir_all(&directory).expect("mkdir");
        let archive = directory.join(&file.name);
        std::fs::write(&archive, &bytes).expect("write");

        let summary = modpack::inspect(&archive).expect("inspect");
        println!("{summary:?}");
        let manifest = cfpack::read(&archive).expect("read").expect("manifest");
        let files = cfpack::resolve(&manifest, &Sideload::default())
            .await
            .expect("resolve");
        let manual: Vec<_> = files.iter().filter_map(|file| file.manual()).collect();
        let places: std::collections::BTreeMap<&str, usize> =
            files.iter().fold(Default::default(), |mut map, file| {
                *map.entry(file.path.split('/').next().unwrap_or(""))
                    .or_default() += 1;
                map
            });
        println!(
            "files={} by dir={places:?} manual={}",
            files.len(),
            manual.len()
        );
        for item in manual.iter().take(5) {
            println!("  manual: {} {} {}", item.title, item.file_name, item.page);
        }
        assert_eq!(files.len(), summary.files);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[tokio::test]
    #[ignore]
    async fn a_pack_that_cannot_be_fetched_comes_back_as_a_manual_download() {
        let hits = search(&SearchQuery {
            query: "Cobblemon SMP".to_owned(),
            kind: ResourceKind::Modpack,
            ..SearchQuery::default()
        })
        .await
        .expect("search");
        let pack = hits
            .hits
            .iter()
            .find(|hit| hit.title.starts_with("Cobblemon SMP"))
            .expect("the pack");
        let list = versions(&pack.project_id, None).await.expect("versions");
        let version = list.first().expect("a version");

        let directory = std::env::temp_dir().join(format!("fern-cf-manual-{}", std::process::id()));
        let (events, _rx) = tokio::sync::mpsc::unbounded_channel();
        let fetched = crate::supply::fetch_release_file(
            Source::CurseForge,
            &version.id,
            &directory,
            &crate::supply::Sideload::default(),
            &events,
        )
        .await
        .expect("fetch");
        println!("{fetched:?}");
        assert!(fetched.path.is_none());
        let manual = fetched.manual.expect("manual");
        assert!(manual.page.starts_with("https://www.curseforge.com/"));
        let _ = std::fs::remove_dir_all(directory);
    }
}
