//! Modrinth 的那一半：URL 怎么拼、JSON 长什么样。
//!
//! 它的 API 公开、无需鉴权，以文件 sha1 为主键——一个 hash 永远对应同一个
//! 版本。和来源无关的部分（计划、安装、对账）在 `mod.rs` 和它的兄弟模块里，
//! 这里只负责把 Modrinth 的答案翻译成那些模块认的形状。

use std::collections::HashMap;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::{
    GalleryImage, ProjectDetail, ProjectLink, ProjectVersion, Release, ReleaseFile, SearchHit,
    SearchQuery, SearchResult, Source, VersionDependency, http::Upstream, is_external_url,
    loader_tags, plan, survey, urlencoding,
};
use crate::LoaderKind;

const API: &str = "https://api.modrinth.com/v2";

/// Modrinth 要求调用方表明身份，并据此做速率限制（每分钟 300 次）。
/// 匿名调用会被降级甚至拒绝。
pub(super) const USER_AGENT: &str = concat!(
    "HuanCheng65/fern/",
    env!("CARGO_PKG_VERSION"),
    " (github.com/HuanCheng65/fern)"
);

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

const UPSTREAM: Upstream = Upstream {
    name: "Modrinth",
    key: None,
};

async fn get<T: serde::de::DeserializeOwned>(url: &str) -> Result<T> {
    UPSTREAM.get(url).await
}

async fn post<T: serde::de::DeserializeOwned>(url: &str, body: &impl Serialize) -> Result<T> {
    UPSTREAM.post(url, body).await
}

/// facet 和 `loaders=` 要的是一个 JSON 字符串数组，塞进查询串里。
fn json_array(values: &[&str]) -> String {
    let quoted: Vec<String> = values.iter().map(|value| format!("\"{value}\"")).collect();
    format!("[{}]", quoted.join(","))
}

/// 搜索。
///
/// 条件全部来自调用方，一个都不从「当前实例」推断。不写条件就是不限，于是
/// 补给站默认是在浏览整个 Modrinth，而不是在浏览「这个实例装得上的东西」。
pub(super) async fn search(request: &SearchQuery) -> Result<SearchResult> {
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
                source: Source::Modrinth,
            })
            .collect(),
    })
}

/// 一个项目的全部版本，新的在前。
///
/// 不在这里按目标实例筛。详情页要把所有版本都摆出来，装不上的标出来而不是
/// 藏起来——「这个模组还没适配 1.21」是用户需要知道的事实，一个空列表说不了
/// 这句话。
pub(super) async fn versions(project: &str) -> Result<Vec<ProjectVersion>> {
    let raw: Vec<RawVersion> = get(&format!("{API}/project/{project}/version")).await?;
    Ok(raw.into_iter().map(describe).collect())
}

/// 项目详情。
pub(super) async fn project(slug: &str) -> Result<ProjectDetail> {
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
        source: Source::Modrinth,
    })
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

/// 一个版本，翻译成计划认的形状。
pub(super) async fn release(version_id: &str) -> Result<Release> {
    Ok(raw_version(version_id).await?.into())
}

/// 依赖只给了项目时，挑一个装得上的版本：优先正式版，没有就用最新的。
pub(super) async fn candidate(
    project: &str,
    game_version: &str,
    loader: LoaderKind,
) -> Result<Option<Release>> {
    let mut candidates = raw_versions(project, game_version, loader).await?;
    let chosen = candidates
        .iter()
        .position(|version| version.version_type == "release")
        .or(if candidates.is_empty() { None } else { Some(0) });
    Ok(chosen.map(|index| candidates.swap_remove(index).into()))
}

impl From<RawVersion> for Release {
    fn from(raw: RawVersion) -> Self {
        let file = raw.primary_file().map(|file| ReleaseFile {
            url: Some(file.url.clone()),
            name: file.filename.clone(),
            size: file.size,
            sha1: Some(file.hashes.sha1.clone()).filter(|hash| !hash.is_empty()),
        });
        Release {
            project_id: raw.project_id,
            version_id: raw.id,
            name: raw.name,
            version_number: raw.version_number,
            file,
            dependencies: raw
                .dependencies
                .into_iter()
                .map(|dependency| super::Dependency {
                    project_id: dependency.project_id.filter(|id| !id.is_empty()),
                    version_id: dependency.version_id.filter(|id| !id.is_empty()),
                    kind: plan::DependencyKind::from_api(&dependency.dependency_type),
                })
                .collect(),
        }
    }
}

/// 一批 sha1 分别是哪个版本。
///
/// 这是 Modrinth 上唯一可靠的「这个文件是什么」——文件名不是身份，同一个模组
/// 从不同渠道拿到的文件名可以完全不同。一次问一批，不是一个文件一次。
pub(super) async fn versions_by_hash(
    hashes: &[String],
) -> Result<HashMap<String, survey::KnownVersion>> {
    #[derive(Serialize)]
    struct Request<'a> {
        hashes: &'a [String],
        algorithm: &'a str,
    }

    let mut out = HashMap::new();
    // 一次问太多会被上游拒绝，也会把一次失败的代价放大到整批。
    for chunk in hashes.chunks(100) {
        let body = Request {
            hashes: chunk,
            algorithm: "sha1",
        };
        let found: HashMap<String, RawVersion> =
            post(&format!("{API}/version_files"), &body).await?;
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

/// 一批 sha1 各自最新的**这个实例装得上的**版本。
///
/// 筛选交给上游做（`loaders` 和 `game_versions` 两个参数），而不是拉回整份版本
/// 列表自己挑：一个实例三百个模组，那是三百次请求换一个上游一次就能答的问题。
///
/// 上游对每个 hash 回的可能就是手上这一份（已经是最新的了）。判断「算不算有
/// 新版」在调用方，这里只负责把答案带回来。
pub(super) async fn latest_by_hash(
    hashes: &[String],
    game_version: &str,
    loader: LoaderKind,
) -> Result<HashMap<String, survey::KnownVersion>> {
    let tags = loader_tags(loader);
    let mut out = HashMap::new();
    for chunk in hashes.chunks(100) {
        let mut body = serde_json::json!({
            "hashes": chunk,
            "algorithm": "sha1",
            "game_versions": [game_version],
        });
        // 原版实例没有加载器标签。传一个空数组等于要求「不属于任何加载器的
        // 版本」，那会把所有东西筛光。
        if !tags.is_empty() {
            body["loaders"] = serde_json::json!(tags);
        }
        let found: HashMap<String, RawVersion> =
            post(&format!("{API}/version_files/update"), &body).await?;
        for (hash, version) in found {
            out.insert(
                hash.to_ascii_lowercase(),
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
pub(super) async fn project_names(ids: &[String]) -> Result<HashMap<String, plan::Named>> {
    let mut unique: Vec<&str> = ids.iter().map(String::as_str).collect();
    unique.sort_unstable();
    unique.dedup();
    if unique.is_empty() {
        return Ok(HashMap::new());
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
        source: Source::Modrinth,
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
pub(crate) async fn known_files(hashes: &[String]) -> Result<HashMap<String, KnownFile>> {
    if hashes.is_empty() {
        return Ok(HashMap::new());
    }

    let raw: HashMap<String, RawVersion> = post(
        &format!("{API}/version_files"),
        &serde_json::json!({ "hashes": hashes, "algorithm": "sha1" }),
    )
    .await
    .context("向 Modrinth 反查模组来源")?;

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

#[cfg(test)]
mod tests {
    use super::*;

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
