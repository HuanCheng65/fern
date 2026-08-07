//! 整合包（Modrinth 的 `.mrpack`）。
//!
//! 一个 mrpack 就是个 zip：里面一份 `modrinth.index.json` 说清「要哪个游戏
//! 版本、哪个加载器、下哪些文件」，加一个 `overrides/` 目录直接铺进游戏目录
//! （配置、脚本、自带的资源包这些没法从网上按 hash 取的东西）。
//!
//! **装整合包不是「装东西到某个实例」，是「建一个实例」。** 它自带游戏版本和
//! 加载器，往一个已有实例上盖只会得到一个谁也说不清是什么的混合体。所以这里
//! 返回的是一个新建的 InstanceProfile。
//!
//! 安全上有三处关口，都是因为 zip 里的名字和 index 里的路径都来自网络：
//!
//! - `files[].path` 和 zip 条目名一律走 `safe_join`，挡 `../` 和绝对路径。
//! - `downloads[]` 只认 https。它们会被交给下载器，`file://` 之类没有道理。
//! - 加载器版本号进的是版本 id 和目录名，所以按 `是否只含安全字符` 过一遍。

use std::{
    io::Read,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow};
use fern_download::{DownloadClient, DownloadEvent, DownloadTask, safe_join};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedSender;

use crate::{DataPaths, InstanceProfile, LoaderKind};

/// 这一版只认得 formatVersion 1。往后的格式没见过就直说，别猜着装。
const SUPPORTED_FORMAT: u32 = 1;

/// index 里的元信息，够界面在装之前先说清「这是什么」。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackSummary {
    pub name: String,
    pub version: String,
    pub summary: String,
    pub game_version: String,
    pub loader: LoaderKind,
    pub loader_version: String,
    /// 要从网上取的文件数。
    pub files: usize,
}

// ——— index 的 JSON 形状 ———

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Index {
    format_version: u32,
    #[serde(default)]
    name: String,
    #[serde(default)]
    version_id: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    files: Vec<IndexFile>,
    #[serde(default)]
    dependencies: std::collections::HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IndexFile {
    path: String,
    #[serde(default)]
    hashes: Hashes,
    #[serde(default)]
    env: Option<Env>,
    #[serde(default)]
    downloads: Vec<String>,
    #[serde(default)]
    file_size: u64,
}

#[derive(Debug, Default, Deserialize)]
struct Hashes {
    #[serde(default)]
    sha1: String,
}

#[derive(Debug, Deserialize)]
struct Env {
    #[serde(default)]
    client: String,
}

/// 从 dependencies 里认出加载器。键名是格式定的，不是我们能选的。
fn loader_from(dependencies: &std::collections::HashMap<String, String>) -> (LoaderKind, String) {
    for (key, kind) in [
        ("fabric-loader", LoaderKind::Fabric),
        ("quilt-loader", LoaderKind::Quilt),
        ("neoforge", LoaderKind::NeoForge),
        ("forge", LoaderKind::Forge),
    ] {
        if let Some(version) = dependencies.get(key) {
            return (kind, version.clone());
        }
    }
    (LoaderKind::Vanilla, String::new())
}

/// 版本号会进版本 id 和目录名，所以只放行安全字符。
fn is_safe_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
}

fn read_index(archive_path: &Path) -> Result<Index> {
    let file = std::fs::File::open(archive_path)
        .with_context(|| format!("打开 {}", archive_path.display()))?;
    let mut archive = zip::ZipArchive::new(file).context("这个文件不是有效的 zip")?;
    let mut entry = archive
        .by_name("modrinth.index.json")
        .context("压缩包里没有 modrinth.index.json，它可能不是一个 mrpack")?;
    let mut text = String::new();
    entry.read_to_string(&mut text).context("读取 index")?;
    let index: Index = serde_json::from_str(&text).context("解析 modrinth.index.json")?;
    if index.format_version != SUPPORTED_FORMAT {
        return Err(anyhow!(
            "不支持的整合包格式版本 {}（本启动器支持 {SUPPORTED_FORMAT}）",
            index.format_version
        ));
    }
    Ok(index)
}

/// 先看一眼里面是什么，不动磁盘。
pub fn inspect(archive_path: &Path) -> Result<PackSummary> {
    let index = read_index(archive_path)?;
    let game_version = index
        .dependencies
        .get("minecraft")
        .cloned()
        .ok_or_else(|| anyhow!("整合包没有写明 Minecraft 版本"))?;
    let (loader, loader_version) = loader_from(&index.dependencies);
    Ok(PackSummary {
        name: if index.name.is_empty() {
            archive_path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_else(|| "整合包".to_owned())
        } else {
            index.name
        },
        version: index.version_id,
        summary: index.summary,
        game_version,
        loader,
        loader_version,
        files: index.files.len(),
    })
}

/// 客户端用不上的文件不下。整合包的 index 里常带一堆纯服务端的模组。
fn wanted(file: &IndexFile) -> bool {
    file.env
        .as_ref()
        .is_none_or(|env| env.client != "unsupported")
}

/// 建一个实例并把整合包铺进去。
///
/// 名字可以由调用方给（用户在导入时改过），不给就用 index 里的。
pub async fn install(
    paths: &DataPaths,
    archive_path: &Path,
    name: Option<&str>,
    events: &UnboundedSender<DownloadEvent>,
) -> Result<InstanceProfile> {
    let index = read_index(archive_path)?;
    let summary = inspect(archive_path)?;
    if summary.loader != LoaderKind::Vanilla && !is_safe_version(&summary.loader_version) {
        return Err(anyhow!("整合包写的加载器版本无法使用"));
    }

    let _ = events.send(DownloadEvent::Status {
        message: format!("准备 {}", summary.name),
    });

    let profile = crate::create_instance_with_loader(
        paths,
        name.unwrap_or(&summary.name),
        &summary.game_version,
        summary.loader,
        (summary.loader != LoaderKind::Vanilla).then_some(summary.loader_version.as_str()),
    )?;
    let game = paths.game_directory(profile.id.as_str());

    // 出了岔子就把这个半成品实例删掉。留一个下了一半的实例在曲库里，比直接
    // 失败更糟——它看起来是好的，点启动才发现不是。
    match lay_out(paths, &game, archive_path, index, events).await {
        Ok(()) => Ok(profile),
        Err(error) => {
            let _ = crate::delete_instance(paths, profile.id.as_str());
            Err(error)
        }
    }
}

async fn lay_out(
    _paths: &DataPaths,
    game: &Path,
    archive_path: &Path,
    index: Index,
    events: &UnboundedSender<DownloadEvent>,
) -> Result<()> {
    tokio::fs::create_dir_all(game).await?;

    let mut tasks = Vec::new();
    for file in index.files.iter().filter(|file| wanted(file)) {
        let Some(url) = file
            .downloads
            .iter()
            .find(|url| crate::is_external_url(url))
        else {
            return Err(anyhow!("{} 没有可用的 https 下载地址", file.path));
        };
        let destination = safe_join(game, Path::new(&file.path))
            .with_context(|| format!("整合包里的路径不安全：{}", file.path))?;
        if let Some(parent) = destination.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tasks.push(if file.hashes.sha1.is_empty() || file.file_size == 0 {
            DownloadTask::unverified(destination, url)?
        } else {
            DownloadTask::new(destination, url, &file.hashes.sha1, file.file_size)?
        });
    }

    if !tasks.is_empty() {
        let _ = events.send(DownloadEvent::Status {
            message: format!("下载整合包的 {} 个文件", tasks.len()),
        });
        DownloadClient::new(crate::settings::source_order(), 8)
            .download_all(tasks, events)
            .await?;
    }

    let _ = events.send(DownloadEvent::Status {
        message: "展开整合包自带的文件".to_owned(),
    });
    // overrides 先铺，client-overrides 后铺——后者按格式定义就是用来盖前者的。
    //
    // 解压是同步的，而且大包的 overrides 有几十兆。摆在 async 函数里直接调，
    // 占住的是跑下载的那条运行时的一个 worker——交给阻塞线程池。
    let archive = archive_path.to_path_buf();
    let destination = game.to_path_buf();
    tokio::task::spawn_blocking(move || extract_overrides(&archive, &destination)).await??;
    Ok(())
}

/// 从 Modrinth 装一个整合包。
///
/// 先把 `.mrpack` 下到数据目录下的缓存里，再照本地文件那条路走——装到一半
/// 失败时，那个文件还在，重来一次不用再下一遍。
pub async fn install_from_modrinth(
    paths: &DataPaths,
    version_id: &str,
    name: Option<&str>,
    events: &UnboundedSender<DownloadEvent>,
) -> Result<InstanceProfile> {
    let cache = paths.root.join("cache/modpacks");
    let archive = crate::modrinth::fetch_primary_file(version_id, &cache, events).await?;
    install(paths, &archive, name, events).await
}

/// 把 `overrides/` 和 `client-overrides/` 铺进游戏目录。
fn extract_overrides(archive_path: &Path, game: &Path) -> Result<()> {
    let file = std::fs::File::open(archive_path)?;
    let mut archive = zip::ZipArchive::new(file)?;

    for prefix in ["overrides/", "client-overrides/"] {
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index)?;
            // 用 enclosed_name 而不是 name：它已经拒了绝对路径和 `..`，
            // 这里再走一次 safe_join 是第二道。
            let Some(entry_path) = entry.enclosed_name() else {
                continue;
            };
            let Ok(relative) = entry_path.strip_prefix(prefix) else {
                continue;
            };
            if relative.as_os_str().is_empty() {
                continue;
            }
            let destination: PathBuf = safe_join(game, relative)?;
            if entry.is_dir() {
                std::fs::create_dir_all(&destination)?;
                continue;
            }
            if let Some(parent) = destination.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut target = std::fs::File::create(&destination)
                .with_context(|| format!("写入 {}", destination.display()))?;
            std::io::copy(&mut entry, &mut target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn pack(directory: &Path, index_json: &str, overrides: &[(&str, &str)]) -> PathBuf {
        std::fs::create_dir_all(directory).expect("create dir");
        let path = directory.join("pack.mrpack");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&path).expect("create"));
        let options = zip::write::SimpleFileOptions::default();
        writer
            .start_file("modrinth.index.json", options)
            .expect("start");
        writer.write_all(index_json.as_bytes()).expect("write");
        for (name, body) in overrides {
            writer.start_file(*name, options).expect("start");
            writer.write_all(body.as_bytes()).expect("write");
        }
        writer.finish().expect("finish");
        path
    }

    fn root(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("fern-mrpack-{tag}-{}", std::process::id()))
    }

    #[test]
    fn reads_the_version_and_loader_out_of_the_index() {
        let directory = root("inspect");
        let path = pack(
            &directory,
            r#"{
              "formatVersion": 1,
              "name": "苦力怕乐园",
              "versionId": "1.2.0",
              "summary": "一句话",
              "files": [{"path":"mods/a.jar","downloads":["https://cdn.modrinth.com/a.jar"]}],
              "dependencies": {"minecraft":"1.20.1","fabric-loader":"0.15.11"}
            }"#,
            &[],
        );

        let summary = inspect(&path).expect("inspect");
        assert_eq!(summary.name, "苦力怕乐园");
        assert_eq!(summary.game_version, "1.20.1");
        assert_eq!(summary.loader, LoaderKind::Fabric);
        assert_eq!(summary.loader_version, "0.15.11");
        assert_eq!(summary.files, 1);

        std::fs::remove_dir_all(directory).expect("clean");
    }

    #[test]
    fn a_future_format_version_is_refused_rather_than_guessed_at() {
        let directory = root("format");
        let path = pack(
            &directory,
            r#"{"formatVersion": 2, "dependencies": {}}"#,
            &[],
        );
        let error = inspect(&path).expect_err("should refuse");
        assert!(format!("{error:#}").contains("格式版本"));
        std::fs::remove_dir_all(directory).expect("clean");
    }

    #[test]
    fn overrides_cannot_escape_the_game_directory() {
        let directory = root("escape");
        let path = pack(
            &directory,
            r#"{"formatVersion":1,"dependencies":{"minecraft":"1.20.1"}}"#,
            &[
                ("overrides/config/ok.toml", "fine"),
                // zip 里塞一个爬出去的名字是整合包最经典的攻击面。
                ("overrides/../../escaped.txt", "nope"),
            ],
        );
        let game = directory.join("game");
        std::fs::create_dir_all(&game).expect("create game");

        extract_overrides(&path, &game).expect("extract");
        assert!(game.join("config/ok.toml").is_file());
        assert!(!directory.join("escaped.txt").exists());

        std::fs::remove_dir_all(directory).expect("clean");
    }

    #[test]
    fn server_only_files_are_not_downloaded() {
        let entry = |client: Option<&str>| IndexFile {
            path: "mods/a.jar".to_owned(),
            hashes: Hashes::default(),
            env: client.map(|client| Env {
                client: client.to_owned(),
            }),
            downloads: Vec::new(),
            file_size: 0,
        };
        // 整合包的 index 里常带一堆纯服务端的模组，下下来只是白占磁盘。
        assert!(!wanted(&entry(Some("unsupported"))));
        assert!(wanted(&entry(Some("required"))));
        assert!(wanted(&entry(Some("optional"))));
        // 没写 env 就是两边都要。
        assert!(wanted(&entry(None)));
    }
}
