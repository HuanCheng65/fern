use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow};
use fern_download::{DownloadClient, DownloadEvent, DownloadTask};
use fern_meta::{
    DownloadInfo, Library, RuleContext, VersionManifest, VersionManifestEntry, VersionMetadata,
};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedSender;

use crate::{
    DataPaths, Job, JobText,
    data::{
        metacache::{self, Freshness},
        settings::source_order,
    },
    java::{self, runtime},
    launch::{loader, rules, version},
};

/// 版本清单在缓存目录里的名字。补全和「新建实例」的版本列表读的是同一份。
pub(crate) const MANIFEST_SLUG: &str = "version_manifest_v2.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareResult {
    pub instance_id: String,
    pub version_id: String,
    pub total_files: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Deserialize)]
struct AssetObjectIndex {
    objects: HashMap<String, AssetObject>,
    /// 1.6.x：资源要按原名摆一份出来，游戏不认按内容寻址的那套。
    #[serde(default, rename = "virtual")]
    is_virtual: bool,
    /// 1.5.x 及更早：那一份要摆进实例的 `resources/`。
    #[serde(default)]
    map_to_resources: bool,
}

#[derive(Debug, Deserialize)]
struct AssetObject {
    hash: String,
    size: u64,
}

/// 把这个实例补齐到能启动的状态。
///
/// 分四步说，而不是笼统地叫「检查文件」：装加载器和下载文件是性质完全不同的
/// 两件事——下载幂等、可并发、失败重试即可；装 Forge 要在本地跑一个第三方
/// 安装器，它拆开 client jar 重打，有副作用、不能并发、失败会留下半成品，而且
/// 必须排在下载之前（它决定了后面要补哪些库）。混成一步的代价是进度条撒谎：
/// 显示「检查游戏文件」的时候实际卡在 Forge 安装器上，一动不动一分钟。
///
/// 加载器那一步没有百分比可言，所以进度分两轴——纵轴是第几步，横轴才是这一步
/// 内部的字节数。硬把它们压成一个百分比就只能靠编。
pub async fn prepare_instance(
    paths: &DataPaths,
    instance_id: &str,
    job: &Job,
) -> Result<PrepareResult> {
    paths.ensure_exists()?;
    let mut profile = crate::read_instance(paths, instance_id)?;
    // 外部实例的文件补到它自己的目录里去。这一句要在最前面：它之后的每一个
    // `paths` 都是这个实例的那一套，而不是全局那一套。
    let scoped = crate::instance::paths_for(paths, &profile);
    let paths = &scoped;
    // 外部实例的加载器是别的启动器装好的，那份版本描述已经在磁盘上。再装一遍
    // 会按我们探到的版本号去上游取**另一份**，往用户的目录里塞一个他没要的
    // 版本；而探测本来就可能探不出版本号（老 Forge 的库坐标形状不一样）。
    let already_on_disk = |component: &crate::Component| {
        !component.version_id.is_empty()
            && paths
                .versions
                .join(&component.version_id)
                .join(format!("{}.json", component.version_id))
                .is_file()
    };
    // 要装的是**每一层**，不只是最外面那一个：Forge + LiteLoader 这样的实例
    // 有两层要装，只装一层的话另一层的 tweaker 永远不会出现在命令行上，而
    // 游戏照样能起来——少了一半模组，界面上看不出任何异常。
    let pending: Vec<crate::Component> = profile
        .components
        .iter()
        .filter(|component| component.kind != crate::LoaderKind::Vanilla)
        .filter(|component| !(profile.external.is_some() && already_on_disk(component)))
        .cloned()
        .collect();
    // 原版没有加载器要装，那一步就不该出现在分母里。装 Java 不再单占一步：
    // 它和下载游戏文件是「补全文件」这一步里并排的两条支线。
    job.expect(if pending.is_empty() { 2 } else { 3 });

    let version_id = profile.game_version.clone();
    let version_id = version_id.as_str();
    if !version::is_safe_id(version_id) {
        return Err(anyhow!("版本 id 无法作为目录名：{version_id}"));
    }
    let downloader = DownloadClient::new(source_order(), 64);

    job.step(JobText::id("job.stage.resolve-version"));
    // 客户端 jar 该落在哪、原版那份描述是哪一份，都由继承链说了算——外部实例
    // 的「原版」可能就是那份合并好的 JSON 自己，名字和游戏版本号对不上。
    let client_jar = version::client_jar(paths, &profile);
    // 原版那一份先解出来：装 NeoForge 之前要拿它里面的 client jar 地址。
    let vanilla = vanilla_metadata(paths, &downloader, &profile, version_id).await?;

    // 加载器的 profile 也要先落盘，它才是启动时真正读的那一份；原版那份是
    // 它的父。装完之后，下面所有的判断都基于合并结果——补全按一份、启动按
    // 另一份，会出现「文件明明下好了却说缺」这种最难查的问题。
    if !pending.is_empty() {
        job.step(
            JobText::id("job.stage.install-loader")
                .arg("loader", crate::loader_display_name(profile.loader))
                .arg("version", &pending[0].version),
        );
        let events = &job.downloads();
        // NeoForge / Forge 的 processors 要把原版 client jar 拆开重打，
        // 所以它必须先在磁盘上。Fabric 不需要，但多验一次已经存在的文件
        // 只是一次 sha1，不值得为它分叉。
        if let Some(client) = vanilla
            .downloads
            .as_ref()
            .and_then(|downloads| downloads.client.as_ref())
            && pending.iter().any(|component| {
                matches!(
                    component.kind,
                    crate::LoaderKind::NeoForge | crate::LoaderKind::Forge
                )
            })
        {
            let jar = task_from_info(client_jar.clone(), client)?;
            downloader.download_all(vec![jar], events).await?;
        }
        let mut changed = false;
        for component in pending {
            let installed = loader::install(
                paths,
                component.kind,
                version_id,
                &component.version,
                events,
            )
            .await?;
            // 建实例时还不知道上游会给哪个 id，装完才知道；记回实例文件，
            // 之后启动就不必再猜命名规则。改的是那一层，不是整份实例。
            if component.version_id != installed {
                for slot in &mut profile.components {
                    if slot.kind == component.kind && slot.version == component.version {
                        slot.version_id = installed.clone();
                    }
                }
                changed = true;
            }
        }
        if changed {
            crate::write_instance_profile(paths, &profile)?;
        }
    }
    job.step(JobText::id("job.stage.download-files"));
    let effective_id = crate::effective_version_id(&profile);
    // 补全和启动读的必须是同一份合并结果，所以这里也走层表那条路。
    let metadata: VersionMetadata = version::resolve_profile(paths, &profile)
        .with_context(|| format!("读取 {effective_id} 的版本描述"))?;

    let context = rules::context(rules::Features::default());
    let mut tasks = Vec::new();
    // 远古版本下完还要再摆一份，见 materialize_legacy_assets。
    let mut legacy_assets: Option<(String, AssetObjectIndex)> = None;
    // 客户端 jar 始终属于原版：加载器改的是启动方式，不是游戏本体。
    if let Some(client) = metadata
        .downloads
        .as_ref()
        .and_then(|downloads| downloads.client.as_ref())
    {
        tasks.push(task_from_info(client_jar.clone(), client)?);
    }
    // 补全要下的，正是启动要用的那一份名单——同一个函数算出来的，不是两边
    // 各算各的（见 `version` 模块开头那段）。
    for library in metadata.effective_libraries(&context) {
        append_library_tasks(&mut tasks, &paths.libraries, library, &context)?;
    }
    if let Some(logging) = metadata
        .logging
        .as_ref()
        .and_then(|logging| logging.client.as_ref())
    {
        let name = logging
            .file
            .id
            .clone()
            .unwrap_or_else(|| format!("{}.xml", logging.file.sha1));
        // 这个名字也是版本 JSON 给的，同样不能原样拼进路径。
        let destination =
            fern_download::safe_join(&paths.assets.join("log_configs"), Path::new(&name))?;
        tasks.push(task_from_info(destination, &logging.file)?);
    }

    if let Some(index) = &metadata.asset_index {
        // 只是此刻的细节：下载一开始，桥会用「检查并下载 N 个文件」换掉它。
        // 上一版这句话被当成阶段名顶上去、再也没人撤，整批下载的几分钟里
        // 界面一直写着「读取资源索引」。
        job.note(JobText::id("job.note.asset-index"));
        // 索引 id 来自版本 JSON，也就是来自网络，而它要直接变成文件名。
        if !version::is_safe_id(&index.id) {
            return Err(anyhow!("资源索引名无法作为文件名：{}", index.id));
        }
        // 索引带 sha1 和大小，是不可变的：本地那份对得上就不必再拉一遍。
        let index_bytes = metacache::immutable(
            &downloader,
            &paths
                .assets
                .join("indexes")
                .join(format!("{}.json", index.id)),
            &index.url,
            Some(&index.sha1),
            Some(index.size),
        )
        .await
        .with_context(|| format!("读取资源索引 {}", index.id))?;
        let asset_index: AssetObjectIndex =
            serde_json::from_slice(&index_bytes).context("parse asset index")?;
        for object in asset_index.objects.values() {
            if object.hash.len() < 2 {
                continue;
            }
            let prefix = &object.hash[..2];
            let url = format!(
                "https://resources.download.minecraft.net/{prefix}/{}",
                object.hash
            );
            tasks.push(DownloadTask::new(
                paths.assets.join("objects").join(prefix).join(&object.hash),
                &url,
                &object.hash,
                object.size,
            )?);
        }
        legacy_assets = Some((index.id.clone(), asset_index));
    }

    let mut unique = HashSet::new();
    tasks.retain(|task| unique.insert(task.path.clone()));
    let result = PrepareResult {
        instance_id: instance_id.to_owned(),
        version_id: effective_id.clone(),
        total_files: tasks.len() as u64,
        total_bytes: tasks.iter().filter_map(|task| task.size).sum(),
    };

    // Java 也是这个实例缺的文件之一，补全就该把它补上。而且它和游戏文件
    // 走的是两条互不相干的网络流——串行等于把两段下载时间相加。两条支线
    // 并排跑，字节都记在同一本账上；哪条失败整个补全就失败。
    let requirement = java::requirement(
        &profile.game_version,
        profile.loader,
        metadata
            .java_version
            .as_ref()
            .map(|version| version.major_version),
    );
    let component = metadata
        .java_version
        .as_ref()
        .map(|version| version.component.as_str());
    let files_track = job.track(JobText::id("job.track.download"));
    let java_track = job.track(JobText::id("job.track.java-runtime"));
    let events = &files_track.downloads();
    let java_events = java_track.downloads();
    let (files_outcome, java_outcome) = tokio::join!(
        downloader.download_all(tasks, events),
        runtime::ensure_java(paths, component, &requirement, &java_events),
    );
    files_outcome?;
    java_outcome?;
    java_track.done();

    if let Some((index_id, index)) = legacy_assets {
        materialize_legacy_assets(paths, instance_id, &index_id, &index, events).await?;
    }
    // 有些库要改过才能跑（老 FML 在 Java 8u20 之后必崩的那一句）。放在这里
    // 而不是启动那一刻：几百毫秒的重打 jar 该发生在「正在补全文件」里，改写
    // 失败也该在这时候就说出来。启动读到的是同一个函数产出的同一份产物。
    {
        let patch_paths = paths.clone();
        let patch_metadata = metadata.clone();
        let patch_context = context.clone();
        // 打哪几个补丁由兼容规则说了算，补全和启动问的是同一张表。
        let advice =
            crate::launch::compat::apply(&crate::launch::compat::Environment::of(&profile));
        let patch_profile = profile.clone();
        let patch_client_jar = client_jar.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let patches = crate::launch::compat::patches(&advice);
            crate::launch::patch::prepare_all(
                &patch_paths,
                &patch_metadata,
                &patch_context,
                &patches,
            )?;
            // jar mod 那一份同样在这里做完，启动时直接拿现成的。
            crate::launch::patch::with_jar_mods(&patch_paths, &patch_profile, &patch_client_jar)?;
            Ok(())
        })
        .await??;
    }
    files_track.done();

    Ok(result)
}

/// 原版那份版本 JSON。
///
/// 本地已经有了就直接读，**连清单都不去拉**。这一条是「已经装好的实例可以
/// 离线启动」的全部理由：一个版本 id 对应的 JSON 是 Mojang 发布的、不再改变
/// 的一份文件，而它此刻就躺在我们自己的数据目录里，还是启动时真正会读的那份。
/// 为了确认一份我们已经有的东西没变而必须联网，是在给每一次启动加一道无谓的
/// 门槛。
async fn vanilla_metadata(
    paths: &DataPaths,
    downloader: &DownloadClient,
    profile: &crate::InstanceProfile,
    version_id: &str,
) -> Result<VersionMetadata> {
    if let Ok(local) = version::read_one(paths, version_id) {
        return Ok(local);
    }
    // 磁盘上没有叫这个名字的版本，不代表原版那一份不在：外部实例的版本号是
    // 从 jar 或库坐标认出来的（见 instance::external），而那份合并好的 JSON
    // 仍然叫着别人起的名字。去上游拉一份同名的回来，等于往别人的目录里塞一
    // 个他没要的版本——这个模块的底线是不动别人的文件。
    if let Some(root) = version::chain(paths, &crate::effective_version_id(profile)).pop()
        && let Ok(local) = version::read_one(paths, &root)
    {
        return Ok(local);
    }
    let entry = manifest_entry(paths, downloader, version_id).await?;
    let bytes = metacache::immutable(
        downloader,
        &paths
            .versions
            .join(version_id)
            .join(format!("{version_id}.json")),
        &entry.url,
        entry.sha1.as_deref(),
        None,
    )
    .await
    .with_context(|| format!("读取 {version_id} 的版本描述"))?;
    serde_json::from_slice(&bytes).with_context(|| format!("解析 {version_id} 的版本描述"))
}

/// 清单里这个版本的那一条。
///
/// 清单是这条链上唯一真正会变的东西——它回答的是「现在有哪些版本」。缓存里
/// 找不到要的版本时强制刷一次再找：快照发布十分钟后就来建实例是正常用法，
/// 让人等六小时的 TTL 过去不是。
async fn manifest_entry(
    paths: &DataPaths,
    downloader: &DownloadClient,
    version_id: &str,
) -> Result<VersionManifestEntry> {
    let cached = metacache::mutable(
        downloader,
        paths,
        MANIFEST_SLUG,
        metacache::VERSION_MANIFEST_URL,
        Freshness::Within(metacache::LISTING_TTL),
    )
    .await?;
    let missing = || anyhow!("Mojang 版本清单中不存在 {version_id}");
    let find = |bytes: &[u8]| -> Result<Option<VersionManifestEntry>> {
        let manifest: VersionManifest = serde_json::from_slice(bytes).context("解析版本清单")?;
        Ok(manifest
            .versions
            .into_iter()
            .find(|entry| entry.id == version_id))
    };

    if let Some(entry) = find(&cached.bytes)? {
        return Ok(entry);
    }
    // 手上这份就是刚拉的，那就是真的没有。
    if !cached.from_cache {
        return Err(missing());
    }
    let fresh = metacache::mutable(
        downloader,
        paths,
        MANIFEST_SLUG,
        metacache::VERSION_MANIFEST_URL,
        Freshness::Force,
    )
    .await?;
    find(&fresh.bytes)?.ok_or_else(missing)
}

/// 1.6.x 及更早的资源布局。
///
/// 现代版本按内容寻址（`assets/objects/ab/abcdef…`），全局共享、多实例零重复。
/// 老版本不认这套，它要的是一棵按原名摆好的目录树。索引里的两个开关说明摆去
/// 哪儿：`virtual` 摆进共享的 `assets/virtual/<索引名>`，`map_to_resources`
/// 摆进这个实例自己的 `resources/`。
///
/// 用复制而不是硬链接：跨文件系统的硬链接会失败，而这些版本的资源总共也就
/// 几十兆，为省这点空间去处理一堆平台差异不划算。
async fn materialize_legacy_assets(
    paths: &DataPaths,
    instance_id: &str,
    index_id: &str,
    index: &AssetObjectIndex,
    events: &UnboundedSender<DownloadEvent>,
) -> Result<()> {
    let root = if index.map_to_resources {
        paths.game_directory(instance_id).join("resources")
    } else if index.is_virtual {
        paths.assets.join("virtual").join(index_id)
    } else {
        return Ok(());
    };

    let _ = events.send(DownloadEvent::StatusId {
        id: "job.note.legacy-assets".to_owned(),
        params: Vec::new(),
    });

    for (name, object) in &index.objects {
        if object.hash.len() < 2 {
            continue;
        }
        // 名字来自索引文件，会被直接拼成路径。
        let destination = fern_download::safe_join(&root, Path::new(name))?;
        // 已经摆好而且大小对得上就跳过——补全要能反复跑，不该每次都重抄一遍。
        if tokio::fs::metadata(&destination)
            .await
            .is_ok_and(|metadata| metadata.len() == object.size)
        {
            continue;
        }
        let source = paths
            .assets
            .join("objects")
            .join(&object.hash[..2])
            .join(&object.hash);
        if let Some(parent) = destination.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::copy(&source, &destination)
            .await
            .with_context(|| format!("摆放 {name}"))?;
    }
    Ok(())
}

fn task_from_info(path: PathBuf, info: &DownloadInfo) -> Result<DownloadTask> {
    DownloadTask::new(path, &info.url, &info.sha1, info.size)
}

/// 这一条库要下哪个文件。rules、去重、classifier、仓库地址都已经在
/// `effective_libraries` 与 `Library::file` 里定完了，这里只管把它变成一个
/// 下载任务——启动时拼 classpath 读的是同一个函数。
///
/// 坐标推不出路径的条目跳过而不是让整轮补全失败：加载器的元数据里确实会有
/// 不指向任何文件的占位条目。
fn append_library_tasks(
    tasks: &mut Vec<DownloadTask>,
    root: &Path,
    library: &Library,
    context: &RuleContext,
) -> Result<()> {
    let Some(file) = library.file(context) else {
        return Ok(());
    };
    // 地址是空串的条目下不了，也不该下：那是加载器的安装器在本地产出的文件
    // （Forge 1.12.2 的 `net.minecraftforge:forge:…` 就是这样一条），补全时
    // 它已经躺在 libraries 里了。启动那边照样把它算进 classpath。
    if file.url.is_empty() {
        return Ok(());
    }
    let path = fern_download::safe_join(root, Path::new(&file.path))?;
    tasks.push(match (file.sha1, file.size) {
        (Some(sha1), Some(size)) => DownloadTask::new(path, &file.url, sha1, size)?,
        // 老格式只给坐标，没有 sha1 可校验。
        _ => DownloadTask::unverified(path, &file.url)?,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_meta::LibraryDownloads;

    fn info(path: &str) -> DownloadInfo {
        DownloadInfo {
            id: None,
            sha1: "0000000000000000000000000000000000000000".to_owned(),
            size: 12,
            url: "https://libraries.minecraft.net/example.jar".to_owned(),
            path: Some(path.to_owned()),
        }
    }

    #[test]
    fn libraries_with_only_a_repository_url_get_their_path_from_the_coordinate() {
        let library = Library {
            name: "net.fabricmc:fabric-loader:0.16.5".to_owned(),
            url: Some("https://maven.fabricmc.net/".to_owned()),
            ..Library::default()
        };
        let mut tasks = Vec::new();
        append_library_tasks(
            &mut tasks,
            Path::new("libraries"),
            &library,
            &RuleContext::linux_x64(),
        )
        .expect("build library tasks");

        assert_eq!(tasks.len(), 1);
        assert_eq!(
            tasks[0].path,
            Path::new("libraries/net/fabricmc/fabric-loader/0.16.5/fabric-loader-0.16.5.jar")
        );
        assert_eq!(
            tasks[0].url.as_str(),
            "https://maven.fabricmc.net/net/fabricmc/fabric-loader/0.16.5/fabric-loader-0.16.5.jar"
        );
        // 拿不到 sha1 就得说拿不到，不能假装校验过。
        assert!(tasks[0].sha1.is_none());
    }

    #[test]
    fn a_repository_url_without_a_trailing_slash_still_works() {
        let library = Library {
            name: "net.fabricmc:tiny-mappings-parser:0.3.0".to_owned(),
            url: Some("https://maven.fabricmc.net".to_owned()),
            ..Library::default()
        };
        let mut tasks = Vec::new();
        append_library_tasks(
            &mut tasks,
            Path::new("libraries"),
            &library,
            &RuleContext::linux_x64(),
        )
        .expect("build library tasks");
        assert!(
            tasks[0]
                .url
                .as_str()
                .contains("/net/fabricmc/tiny-mappings-parser/")
        );
    }

    /// 连仓库都不写的库要走 Mojang 那个默认仓库。1.12.2 之前的 Forge 把整份
    /// 库清单抄进自己的版本描述，其中大半就是这么写的——包括主类所在的
    /// launchwrapper。曾经把这种条目当占位跳过：文件不下、classpath 里也没有，
    /// 游戏一启动就是「找不到主类」，退出码 1，一行日志都没有。
    #[test]
    fn a_bare_coordinate_comes_from_mojangs_repository() {
        let library = Library {
            name: "net.minecraft:launchwrapper:1.9".to_owned(),
            ..Library::default()
        };
        let mut tasks = Vec::new();
        append_library_tasks(
            &mut tasks,
            Path::new("libraries"),
            &library,
            &RuleContext::linux_x64(),
        )
        .expect("build library tasks");
        assert_eq!(
            tasks[0].url.as_str(),
            "https://libraries.minecraft.net/net/minecraft/launchwrapper/1.9/launchwrapper-1.9.jar"
        );
        assert!(tasks[0].sha1.is_none());
    }

    /// Forge 1.12.2 的版本描述里，它自己那个 jar 的地址是空串——文件由安装器
    /// 在本地产出。拿这个空串去解析地址，整轮补全会以「invalid download URL」
    /// 收场，而它和「Forge」两个字毫无关系。
    #[test]
    fn a_library_with_no_address_is_produced_locally_not_downloaded() {
        let library = Library {
            name: "net.minecraftforge:forge:1.12.2-14.23.5.2864".to_owned(),
            downloads: Some(LibraryDownloads {
                artifact: Some(DownloadInfo {
                    url: String::new(),
                    ..info("net/minecraftforge/forge/1.12.2-14.23.5.2864/forge-14.23.5.2864.jar")
                }),
                classifiers: None,
            }),
            ..Library::default()
        };
        let mut tasks = Vec::new();
        append_library_tasks(
            &mut tasks,
            Path::new("libraries"),
            &library,
            &RuleContext::linux_x64(),
        )
        .expect("build library tasks");
        assert!(tasks.is_empty());
    }

    #[test]
    fn a_library_that_points_at_nothing_is_skipped_not_fatal() {
        let library = Library {
            name: "这不是一个坐标".to_owned(),
            ..Library::default()
        };
        let mut tasks = Vec::new();
        append_library_tasks(
            &mut tasks,
            Path::new("libraries"),
            &library,
            &RuleContext::linux_x64(),
        )
        .expect("placeholder entries must not fail the whole prepare");
        assert!(tasks.is_empty());
    }

    /// native 记录下的是 classifier 那一份。它那个 artifact 不下：那是同坐标
    /// classpath 那条的同一个 jar，由那一条负责。
    #[test]
    fn a_native_record_downloads_its_classifier_and_nothing_else() {
        let library = Library {
            name: "org.example:render:1.0".to_owned(),
            downloads: Some(LibraryDownloads {
                artifact: Some(info("org/example/render/1.0/render-1.0.jar")),
                classifiers: Some(HashMap::from([(
                    "natives-linux-64".to_owned(),
                    info("org/example/render/1.0/render-1.0-natives-linux-64.jar"),
                )])),
            }),
            natives: Some(HashMap::from([(
                "linux".to_owned(),
                "natives-linux-${arch}".to_owned(),
            )])),
            ..Library::default()
        };
        let mut tasks = Vec::new();
        append_library_tasks(
            &mut tasks,
            Path::new("libraries"),
            &library,
            &RuleContext::linux_x64(),
        )
        .expect("build library tasks");
        assert_eq!(tasks.len(), 1);
        assert_eq!(
            tasks[0].path,
            Path::new("libraries/org/example/render/1.0/render-1.0-natives-linux-64.jar")
        );
    }
}
