use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow};
use fern_download::{DownloadClient, DownloadEvent, DownloadTask};
use fern_meta::{
    DownloadInfo, Library, RuleContext, VersionManifest, VersionManifestEntry, VersionMetadata,
    rules_allow,
};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedSender;

use crate::{
    DataPaths, Job, java, loader,
    metacache::{self, Freshness},
    rules, runtime,
    settings::source_order,
    version,
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
    // 原版没有加载器要装，那一步就不该出现在分母里。
    let needs_loader = profile.loader != crate::LoaderKind::Vanilla;
    job.expect(if needs_loader { 4 } else { 3 });

    let events = &job.downloads();
    let version_id = profile.game_version.clone();
    let version_id = version_id.as_str();
    if !version::is_safe_id(version_id) {
        return Err(anyhow!("版本 id 无法作为目录名：{version_id}"));
    }
    let downloader = DownloadClient::new(source_order(), 64);

    job.step("读取版本信息");
    let version_root = paths.versions.join(version_id);
    // 原版那一份先解出来：装 NeoForge 之前要拿它里面的 client jar 地址。
    let vanilla = vanilla_metadata(paths, &downloader, version_id).await?;

    // 加载器的 profile 也要先落盘，它才是启动时真正读的那一份；原版那份是
    // 它的父。装完之后，下面所有的判断都基于合并结果——补全按一份、启动按
    // 另一份，会出现「文件明明下好了却说缺」这种最难查的问题。
    if needs_loader && let Some(loader) = profile.loader_profile.clone() {
        {
            job.step(format!(
                "安装 {} {}",
                crate::loader_display_name(profile.loader),
                loader.version
            ));
            // NeoForge / Forge 的 processors 要把原版 client jar 拆开重打，
            // 所以它必须先在磁盘上。Fabric 不需要，但多验一次已经存在的文件
            // 只是一次 sha1，不值得为它分叉。
            if let Some(client) = vanilla
                .downloads
                .as_ref()
                .and_then(|downloads| downloads.client.as_ref())
                && matches!(
                    profile.loader,
                    crate::LoaderKind::NeoForge | crate::LoaderKind::Forge
                )
            {
                let jar = task_from_info(version_root.join(format!("{version_id}.jar")), client)?;
                downloader.download_all(vec![jar], events).await?;
            }
            let installed =
                loader::install(paths, profile.loader, version_id, &loader.version, events).await?;
            // 建实例时还不知道上游会给哪个 id，装完才知道；记回实例文件，
            // 之后启动就不必再猜命名规则。
            if loader.version_id != installed {
                let mut updated = loader.clone();
                updated.version_id = installed;
                profile.loader_profile = Some(updated);
                crate::write_instance_profile(paths, &profile)?;
            }
        }
    }
    job.step("补全游戏文件");
    let effective_id = crate::effective_version_id(&profile);
    let metadata: VersionMetadata = version::resolve(paths, &effective_id)
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
        tasks.push(task_from_info(
            version_root.join(format!("{version_id}.jar")),
            client,
        )?);
    }
    for library in &metadata.libraries {
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
        let _ = events.send(DownloadEvent::Status {
            message: "读取资源索引".to_owned(),
        });
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
    downloader.download_all(tasks, events).await?;

    if let Some((index_id, index)) = legacy_assets {
        materialize_legacy_assets(paths, instance_id, &index_id, &index, events).await?;
    }

    job.step("准备 Java");
    // Java 也是这个实例缺的文件之一，补全就该把它补上。放在这里而不是启动
    // 时：启动那一步不该再有几百兆的下载，而补全本来就是「跑一遍直到齐活」。
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
    runtime::ensure_java(paths, component, &requirement, events).await?;

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
    version_id: &str,
) -> Result<VersionMetadata> {
    if let Ok(local) = version::read_one(paths, version_id) {
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

    let _ = events.send(DownloadEvent::Status {
        message: "整理旧版资源".to_owned(),
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

fn append_library_tasks(
    tasks: &mut Vec<DownloadTask>,
    root: &Path,
    library: &Library,
    context: &RuleContext,
) -> Result<()> {
    if !rules_allow(library.rules.as_deref(), context) {
        return Ok(());
    }
    let Some(downloads) = &library.downloads else {
        // 第三方 Maven（Fabric、Forge）只给一个仓库前缀，路径和文件名都要
        // 从坐标推出来，也没有 sha1 可校验。
        return append_maven_task(tasks, root, library);
    };
    if let Some(artifact) = &downloads.artifact {
        let relative = artifact
            .path
            .as_deref()
            .ok_or_else(|| anyhow!("library {} has no artifact path", library.name))?;
        tasks.push(task_from_info(root.join(relative), artifact)?);
    }
    let Some(natives) = &library.natives else {
        return Ok(());
    };
    let Some(classifiers) = &downloads.classifiers else {
        return Ok(());
    };
    let Some(template) = natives.get(&context.os_name) else {
        return Ok(());
    };
    let arch = if context.os_arch.contains("64") {
        "64"
    } else {
        "32"
    };
    let classifier = template.replace("${arch}", arch);
    let Some(native) = classifiers.get(&classifier) else {
        return Ok(());
    };
    let relative = native
        .path
        .as_deref()
        .ok_or_else(|| anyhow!("library {} has no native path", library.name))?;
    tasks.push(task_from_info(root.join(relative), native)?);
    Ok(())
}

/// 只有 `url` 前缀的库。
fn append_maven_task(tasks: &mut Vec<DownloadTask>, root: &Path, library: &Library) -> Result<()> {
    let Some(repository) = library.url.as_deref() else {
        // 既没有 downloads 也没有 url：这一条不指向任何可下载的东西。加载器
        // 的元数据里确实会出现这种占位条目，跳过而不是让整轮补全失败。
        return Ok(());
    };
    let Some(relative) = fern_meta::maven_path(&library.name) else {
        return Err(anyhow!("无法从库坐标 {} 推导路径", library.name));
    };
    let url = format!("{}{relative}", ensure_trailing_slash(repository));
    tasks.push(DownloadTask::unverified(
        fern_download::safe_join(root, Path::new(&relative))?,
        &url,
    )?);
    Ok(())
}

fn ensure_trailing_slash(url: &str) -> String {
    if url.ends_with('/') {
        url.to_owned()
    } else {
        format!("{url}/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_meta::{LibraryDownloads, Rule, RuleAction};

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

    #[test]
    fn a_library_that_points_at_nothing_is_skipped_not_fatal() {
        let library = Library {
            name: "org.example:placeholder:1.0".to_owned(),
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

    #[test]
    fn library_tasks_follow_rules_and_include_native_classifier() {
        let library = Library {
            name: "org.example:render:1.0".to_owned(),
            downloads: Some(LibraryDownloads {
                artifact: Some(info("org/example/render/1.0/render-1.0.jar")),
                classifiers: Some(HashMap::from([(
                    "natives-linux-64".to_owned(),
                    info("org/example/render/1.0/render-1.0-natives-linux-64.jar"),
                )])),
            }),
            rules: Some(vec![Rule {
                action: RuleAction::Allow,
                os: None,
                features: None,
            }]),
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
        assert_eq!(tasks.len(), 2);
    }
}
