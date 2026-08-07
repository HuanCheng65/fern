use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow};
use fern_download::{DownloadClient, DownloadEvent, DownloadTask, sha1_matches};
use fern_meta::{
    DownloadInfo, Library, RuleContext, VersionManifest, VersionMetadata, rules_allow,
};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedSender;

use crate::{DataPaths, Job, java, loader, rules, runtime, settings::source_order, version};

const VERSION_MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

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
    let downloader = DownloadClient::new(source_order(), 64);

    job.step("读取版本信息");
    let manifest_bytes = downloader
        .fetch(VERSION_MANIFEST_URL)
        .await
        .context("fetch version manifest")?;
    let manifest: VersionManifest =
        serde_json::from_slice(&manifest_bytes).context("parse version manifest")?;
    let entry = manifest
        .versions
        .iter()
        .find(|entry| entry.id == version_id)
        .ok_or_else(|| anyhow!("version {version_id} is absent from the Mojang manifest"))?;

    let version_bytes = downloader
        .fetch(&entry.url)
        .await
        .with_context(|| format!("fetch version metadata for {version_id}"))?;
    if entry
        .sha1
        .as_deref()
        .is_some_and(|expected| !sha1_matches(&version_bytes, expected))
    {
        return Err(anyhow!(
            "version metadata checksum mismatch for {version_id}"
        ));
    }
    let version_root = paths.versions.join(version_id);
    write_atomic(
        &version_root.join(format!("{version_id}.json")),
        &version_bytes,
    )
    .await?;
    // 原版那一份先解出来：装 NeoForge 之前要拿它里面的 client jar 地址。
    let vanilla: VersionMetadata =
        serde_json::from_slice(&version_bytes).context("parse version metadata")?;

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
        tasks.push(task_from_info(
            paths.assets.join("log_configs").join(name),
            &logging.file,
        )?);
    }

    if let Some(index) = &metadata.asset_index {
        let _ = events.send(DownloadEvent::Status {
            message: "读取资源索引".to_owned(),
        });
        let index_bytes = downloader
            .fetch(&index.url)
            .await
            .with_context(|| format!("fetch asset index {}", index.id))?;
        if index_bytes.len() as u64 != index.size || !sha1_matches(&index_bytes, &index.sha1) {
            return Err(anyhow!("asset index checksum mismatch for {}", index.id));
        }
        write_atomic(
            &paths
                .assets
                .join("indexes")
                .join(format!("{}.json", index.id)),
            &index_bytes,
        )
        .await?;
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

async fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let temporary = path.with_extension("part");
    tokio::fs::write(&temporary, bytes).await?;
    if tokio::fs::try_exists(path).await? {
        tokio::fs::remove_file(path).await?;
    }
    tokio::fs::rename(temporary, path).await?;
    Ok(())
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
