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
/// 2 人气、3 最近更新、6 下载量、11 发布日期、12 评分。
///
/// 「相关度」它没有对应的字段：有搜索词时不传排序，交给上游按匹配程度排；
/// 没有搜索词时相关度无从谈起，按人气排。「关注数」也没有，评分是最近的那个。
fn sort_field(sort: &str, query: &str) -> Option<u32> {
    match sort {
        "downloads" => Some(6),
        "updated" => Some(3),
        "newest" => Some(11),
        "follows" => Some(12),
        _ if query.trim().is_empty() => Some(2),
        _ => None,
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
    if let Some(field) = sort_field(&request.sort, &request.query) {
        url.push_str(&format!("&sortField={field}&sortOrder=desc"));
    }

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
                title: item.name,
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
        title: raw.name,
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

pub(super) async fn versions(project: &str) -> Result<Vec<ProjectVersion>> {
    let files = raw_files(mod_id(project)?, "").await?;
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
                    title: item.name,
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
                    title: item.name,
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
    fn relevance_is_left_to_the_server_only_when_there_is_a_query() {
        assert_eq!(sort_field("relevance", "jei"), None);
        assert_eq!(sort_field("relevance", ""), Some(2));
        assert_eq!(sort_field("downloads", "jei"), Some(6));
    }
}
