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

use crate::{DataPaths, settings::source_order};

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
}

#[derive(Debug, Deserialize)]
struct AssetObject {
    hash: String,
    size: u64,
}

pub async fn prepare_instance(
    paths: &DataPaths,
    instance_id: &str,
    events: &UnboundedSender<DownloadEvent>,
) -> Result<PrepareResult> {
    paths.ensure_exists()?;
    let profile = crate::list_instances(paths)?
        .into_iter()
        .find(|profile| profile.id.as_str() == instance_id)
        .ok_or_else(|| anyhow!("instance {instance_id} does not exist"))?;
    let version_id = profile.game_version.as_str();
    let downloader = DownloadClient::new(source_order(), 64);

    let _ = events.send(DownloadEvent::Status {
        message: "读取版本清单".to_owned(),
    });
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

    let _ = events.send(DownloadEvent::Status {
        message: "读取版本元数据".to_owned(),
    });
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
    let metadata: VersionMetadata =
        serde_json::from_slice(&version_bytes).context("parse version metadata")?;
    let version_root = paths.versions.join(version_id);
    write_atomic(
        &version_root.join(format!("{version_id}.json")),
        &version_bytes,
    )
    .await?;

    let context = current_rule_context();
    let mut tasks = Vec::new();
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
        for object in asset_index.objects.into_values() {
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
                object.hash,
                object.size,
            )?);
        }
    }

    let mut unique = HashSet::new();
    tasks.retain(|task| unique.insert(task.path.clone()));
    let result = PrepareResult {
        instance_id: instance_id.to_owned(),
        version_id: version_id.to_owned(),
        total_files: tasks.len() as u64,
        total_bytes: tasks.iter().map(|task| task.size).sum(),
    };
    let _ = events.send(DownloadEvent::Status {
        message: "开始补全文件".to_owned(),
    });
    downloader.download_all(tasks, events).await?;
    Ok(result)
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
        return Ok(());
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

fn current_rule_context() -> RuleContext {
    RuleContext {
        os_name: if cfg!(target_os = "windows") {
            "windows"
        } else if cfg!(target_os = "macos") {
            "osx"
        } else {
            "linux"
        }
        .to_owned(),
        os_arch: std::env::consts::ARCH.to_owned(),
        os_version: String::new(),
        features: HashMap::new(),
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
