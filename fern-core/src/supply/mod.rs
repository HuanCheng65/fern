//! 补给站：Modrinth 与 CurseForge。
//!
//! 两家各在自己的文件里（`modrinth.rs`、`curseforge.rs`），只管「URL 怎么拼、
//! JSON 长什么样」。这里是和来源无关的那一半：搜索条件、详情、版本、安装计划
//! 和安装本身都按 [`Source`] 分派，界面只认中性的名字。
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

pub(crate) mod cfpack;
mod curseforge;
mod fingerprint;
mod http;
pub(crate) mod modpack;
mod modrinth;
pub(crate) mod plan;
pub(crate) mod sideload;
pub mod survey;

use anyhow::{Result, anyhow};
use fern_download::{DownloadEvent, DownloadTask};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedSender;

use crate::{DataPaths, LoaderKind};

pub use sideload::Sideload;

pub(crate) use modrinth::known_files;

/// 一次装依赖最多跟这么深。真实的依赖链两三层就到头，留点余量，主要是防
/// 上游数据里出现环让我们一直转下去。
const MAX_DEPTH: usize = 6;

/// 东西是从哪一家来的。
///
/// 它会被写进来源日志，而那份日志是磁盘上的、补不回来的东西——所以 serde 的
/// 名字和 [`Source::tag`] 必须是同一个词，测试盯着。
///
/// 两家的 id 类型不一样——Modrinth 是字符串（`AANobbMI`），CurseForge 是整数
/// （`394468`）。一律按字符串存、按字符串传：它只用来回查，不参与运算。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    #[default]
    Modrinth,
    #[serde(rename = "curseforge")]
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

    /// 这一份构建能不能问这一家。
    ///
    /// CurseForge 要一把密钥，在构建时注入；本地开发没配的话，补给站就只有
    /// Modrinth——界面据此决定摆不摆来源切换，而不是摆一个点了就报错的按钮。
    pub fn available(self) -> bool {
        match self {
            Self::Modrinth => true,
            Self::CurseForge => curseforge::available(),
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
    pub source: Source,
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
    pub source: Source,
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
    pub source: Source,
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

/// 一个版本，翻成两家都说得出的形状。安装计划只认这个。
#[derive(Debug, Clone)]
pub(crate) struct Release {
    pub project_id: String,
    pub version_id: String,
    pub name: String,
    pub version_number: String,
    /// 要装的那一个文件。没有就是这个版本没东西可装。
    pub file: Option<ReleaseFile>,
    pub dependencies: Vec<Dependency>,
}

#[derive(Debug, Clone)]
pub(crate) struct ReleaseFile {
    /// CurseForge 上作者不允许第三方分发的文件没有下载地址，只能去网页上下。
    pub url: Option<String>,
    pub name: String,
    pub size: u64,
    /// 老文件偶尔没有 sha1。没有就是一次不校验的下载。
    pub sha1: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct Dependency {
    pub project_id: Option<String>,
    /// 少数依赖钉死了版本而不给项目。
    pub version_id: Option<String>,
    pub kind: plan::DependencyKind,
}

/// 搜索。条件全部来自调用方，一个都不从「当前实例」推断。
pub async fn search(source: Source, request: &SearchQuery) -> Result<SearchResult> {
    match source {
        Source::Modrinth => modrinth::search(request).await,
        Source::CurseForge => curseforge::search(request).await,
    }
}

/// 项目详情。
pub async fn project(source: Source, id: &str) -> Result<ProjectDetail> {
    match source {
        Source::Modrinth => modrinth::project(id).await,
        Source::CurseForge => curseforge::project(id).await,
    }
}

/// 一个项目的全部版本，新的在前。
///
/// 不在这里按目标实例筛。详情页要把所有版本都摆出来，装不上的标出来而不是
/// 藏起来——「这个模组还没适配 1.21」是用户需要知道的事实，一个空列表说不了
/// 这句话。
///
/// `target` 是界面上「安装到」的那个实例的版本和加载器。Modrinth 一次就给全部
/// 版本，用不上它；CurseForge 只能分页取，要靠它把适用的那些一起取回来。
pub async fn versions(
    source: Source,
    project: &str,
    target: Option<(&str, LoaderKind)>,
) -> Result<Vec<ProjectVersion>> {
    match source {
        Source::Modrinth => modrinth::versions(project).await,
        Source::CurseForge => curseforge::versions(project, target).await,
    }
}

pub(crate) async fn release(source: Source, version_id: &str) -> Result<Release> {
    match source {
        Source::Modrinth => modrinth::release(version_id).await,
        Source::CurseForge => curseforge::release(version_id).await,
    }
}

pub(crate) async fn candidate(
    source: Source,
    project: &str,
    game_version: &str,
    loader: LoaderKind,
) -> Result<Option<Release>> {
    match source {
        Source::Modrinth => modrinth::candidate(project, game_version, loader).await,
        Source::CurseForge => curseforge::candidate(project, game_version, loader).await,
    }
}

/// 一批项目 id 分别叫什么。id 对用户没有任何意义。
pub(crate) async fn project_names(
    source: Source,
    ids: &[String],
) -> Result<std::collections::HashMap<String, plan::Named>> {
    match source {
        Source::Modrinth => modrinth::project_names(ids).await,
        Source::CurseForge => curseforge::project_names(ids).await,
    }
}

/// 那些 CurseForge 不给地址的文件，有没有一模一样的一份挂在 Modrinth 上。
///
/// 关掉 CurseForge 第三方分发的作者，很多自己也在 Modrinth 发同一个文件——
/// sha1 相同就是同一份字节，而作者在 Modrinth 上是允许分发的。从那里取不是
/// 绕过谁的设置，是走作者开着的那扇门。实测一个 117 个文件的热门整合包里有
/// 17 个只能手动下载，让用户点开 17 个网页是不现实的。
///
/// 问不到（网络不通、Modrinth 没有）就当没有：这些文件照样走手动下载。
pub(crate) async fn mirrored_on_modrinth(
    sha1s: &[String],
) -> std::collections::HashMap<String, String> {
    if sha1s.is_empty() {
        return std::collections::HashMap::new();
    }
    known_files(sha1s)
        .await
        .map(|found| {
            found
                .into_iter()
                .filter(|(_, file)| is_external_url(&file.url))
                .map(|(hash, file)| (hash, file.url))
                .collect()
        })
        .unwrap_or_default()
}

/// 把一个版本的主文件下到指定位置，返回落盘的路径。
///
/// 整合包要先把包体拿到手才能读里面的清单，所以它走这一条而不是 `install`
/// ——那一条的前提是「文件放进某个实例的某个目录」。
pub async fn fetch_release_file(
    source: Source,
    version_id: &str,
    directory: &std::path::Path,
    events: &UnboundedSender<DownloadEvent>,
) -> Result<std::path::PathBuf> {
    let version = release(source, version_id).await?;
    let file = version
        .file
        .ok_or_else(|| anyhow!("{} 这个版本没有可下载的文件", version.name))?;
    let url = file
        .url
        .ok_or_else(|| anyhow!("{} 的作者不允许第三方启动器下载这个文件", version.name))?;
    tokio::fs::create_dir_all(directory).await?;
    let destination = fern_download::safe_join(directory, std::path::Path::new(&file.name))?;
    let task = match file.sha1.as_deref() {
        Some(sha1) if file.size > 0 => {
            DownloadTask::new(destination.clone(), &url, sha1, file.size)?
        }
        _ => DownloadTask::unverified(destination.clone(), &url)?,
    };
    crate::data::downloader::client(4)
        .download_all(vec![task], events)
        .await?;
    Ok(destination)
}

/// 这个加载器在补给站上对应哪些标签。两家都翻成这同一套小写名字。
///
/// Quilt 能直接加载 Fabric 模组，而大多数作者只标 fabric——只按 quilt 过滤
/// 会得到一个几乎空的列表。反过来 Forge 和 NeoForge 不能混，别合并。
pub(crate) fn loader_tags(loader: LoaderKind) -> Vec<&'static str> {
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

/// 装一个版本，连同它缺的那些必需依赖。
///
/// 「缺的」是这里的关键词：计划由 [`plan::resolve`] 算，它看得见实例里已经有
/// 什么，所以已经装过的前置不会再下一份。界面上显示的也是同一份计划——显示
/// 一套、做另一套，是这类功能出错最常见的方式。
pub async fn install(
    paths: &DataPaths,
    instance_id: &str,
    source: Source,
    version_id: &str,
    kind: ResourceKind,
    sideload: &Sideload,
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

    let plan = plan::resolve(paths, instance_id, source, version_id, kind, sideload).await?;
    // 先于快照：缺文件的话这一次什么都不会写，拍一张就是白拍。
    let missing = plan.missing();
    if missing > 0 {
        return Err(anyhow!(
            "还有 {missing} 个文件需要从 CurseForge 网站手动下载"
        ));
    }
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
        let destination =
            fern_download::safe_join(&directory, std::path::Path::new(&file.file_name))?;
        // 手动下的那一份已经验过 sha1，复制进来就是了。
        if let Some(local) = &file.local {
            tokio::fs::copy(local, &destination)
                .await
                .map_err(|error| anyhow!("复制 {} 失败：{error}", local.display()))?;
            continue;
        }
        // 上游给了 sha1 和大小就校验；CurseForge 的老文件偶尔两样都没有。
        tasks.push(if file.sha1.is_empty() || file.bytes == 0 {
            DownloadTask::unverified(destination, &file.url)?
        } else {
            DownloadTask::new(destination, &file.url, &file.sha1, file.bytes)?
        });
    }

    job.step(if tasks.len() > 1 {
        format!("下载 {} 个文件（含依赖）", tasks.len())
    } else {
        "下载文件".to_owned()
    });
    if !tasks.is_empty() {
        let downloader = crate::data::downloader::client(8);
        downloader.download_all(tasks, events).await?;
    }

    // 记一笔谁放进来的。sha1 用上游给的那个——下载刚刚照着它校验过，再读一遍
    // 磁盘算一次是在为同一个答案付两次钱。上游没给的那几个才去读。
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
                sha1: if file.sha1.is_empty() {
                    crate::backup::sha1_of(&directory.join(&file.file_name)).unwrap_or_default()
                } else {
                    file.sha1.clone()
                },
                version: crate::instance::integrity::declared_version(
                    &directory.join(&file.file_name),
                ),
                origin: crate::instance::origin::Origin::Supply {
                    source,
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

/// 把一个模组换成新版。
///
/// 装上新的，再撤掉旧的那份文件——两步在一条命令里做完。让界面先调安装、再调
/// 删除的话，中间任何一次失败都会在 `mods/` 里留下同一个模组的两个版本，而那
/// 是一个起不来的游戏。
///
/// 旧的那份是禁用状态时，新的也跟着禁用：更新不是启用，用户没有要求过后者。
pub async fn update_mod(
    paths: &DataPaths,
    instance_id: &str,
    file_name: &str,
    source: Source,
    version_id: &str,
    job: &crate::Job,
) -> Result<InstallOutcome> {
    // 新版那份文件叫什么，装之前就要知道：和旧的同名时，那一份已经被覆盖了，
    // 再去删就是把刚下下来的东西删掉。
    let incoming = release(source, version_id)
        .await?
        .file
        .map(|file| file.name)
        .ok_or_else(|| anyhow!("这个版本没有可下载的文件"))?;

    let outcome = install(
        paths,
        instance_id,
        source,
        version_id,
        ResourceKind::Mod,
        &Sideload::default(),
        job,
    )
    .await?;

    if incoming != file_name {
        crate::instance::mods::remove(paths, instance_id, file_name)?;
    }
    if file_name.ends_with(".disabled") {
        crate::instance::mods::set_enabled(paths, instance_id, &incoming, false)?;
    }
    Ok(outcome)
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
/// 只转义查询串里真正会出事的那几个字符。
///
/// 不引一个 URL 编码库：这里拼的都是我们自己构造的 facet 和用户输入的搜索词，
/// 需要处理的字符是可枚举的。
pub(crate) fn urlencoding(value: &str) -> String {
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
    fn the_log_and_the_wire_spell_each_source_the_same_way() {
        // 来源日志里 serde 写的词和 `tag()` 必须一致：前者是 `Origin` 序列化
        // 出来的，后者是日志链里拼进 payload 的。两边一旦不同，老日志就对不上了。
        for source in [Source::Modrinth, Source::CurseForge] {
            assert_eq!(
                serde_json::to_value(source).expect("serialize"),
                serde_json::Value::String(source.tag().to_owned())
            );
        }
    }
}
