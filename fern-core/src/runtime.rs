//! Java 运行时的下载层（文档 §4.3）。
//!
//! 首选 Mojang 官方运行时：有清单、有 sha1、BMCLAPI 有镜像，而且和官方启动器
//! 装的是同一份——玩家遇到的问题能和别人对得上。清单结构又和 assets 几乎一样，
//! 下载器直接复用，这一层只是把 JSON 翻译成下载任务。
//!
//! 体验上这一层必须是隐形的：点启动，没有合适的 Java，就去下，只在进度条上
//! 体现。绝不能变成一句「请先安装 Java」然后把人丢回浏览器。

use std::{collections::HashMap, path::Path};

use anyhow::{Context, Result, anyhow};
use fern_download::{DownloadClient, DownloadEvent, DownloadTask};
use serde::Deserialize;
use tokio::sync::mpsc::UnboundedSender;

use crate::{
    DataPaths,
    java::{self, JavaRequirement, JavaRuntime},
    settings::source_order,
};

const RUNTIME_INDEX_URL: &str = "https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";

#[derive(Debug, Deserialize)]
struct RuntimeEntry {
    manifest: ManifestReference,
    version: RuntimeVersion,
}

#[derive(Debug, Deserialize)]
struct ManifestReference {
    sha1: String,
    url: String,
}

#[derive(Debug, Deserialize)]
struct RuntimeVersion {
    name: String,
}

#[derive(Debug, Deserialize)]
struct RuntimeManifest {
    files: HashMap<String, RuntimeFile>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum RuntimeFile {
    Directory,
    File {
        downloads: RuntimeDownloads,
        #[serde(default)]
        executable: bool,
    },
    Link {
        target: String,
    },
}

#[derive(Debug, Deserialize)]
struct RuntimeDownloads {
    raw: RuntimeArtifact,
}

#[derive(Debug, Deserialize)]
struct RuntimeArtifact {
    sha1: String,
    size: u64,
    url: String,
}

/// 保证有一个满足要求的 Java，必要时下载。
///
/// `component` 来自 version JSON 的 `javaVersion.component`——Mojang 自己指定了
/// 用哪一份运行时，照做就是和官方启动器一致。老版本没有这个字段，按下限推。
pub async fn ensure_java(
    paths: &DataPaths,
    component: Option<&str>,
    requirement: &JavaRequirement,
    events: &UnboundedSender<DownloadEvent>,
) -> Result<JavaRuntime> {
    if let Some(runtime) = java::select(&java::discover(Some(paths)), requirement) {
        return Ok(runtime);
    }

    let component = component
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| component_for(requirement.minimum).to_owned());
    let install_root = paths.runtimes.join(&component);

    let _ = events.send(DownloadEvent::Status {
        message: format!("准备 Java 运行时（{component}）"),
    });

    let downloader = DownloadClient::new(source_order(), 64);
    let index_bytes = downloader
        .fetch(RUNTIME_INDEX_URL)
        .await
        .context("读取 Mojang 运行时清单")?;
    let index: HashMap<String, HashMap<String, Vec<RuntimeEntry>>> =
        serde_json::from_slice(&index_bytes).context("解析 Mojang 运行时清单")?;

    let platform = platform_key().ok_or_else(|| {
        anyhow!(
            "Mojang 没有为 {}-{} 发布运行时，请在实例设置里指定一个 Java {} 的路径",
            std::env::consts::OS,
            std::env::consts::ARCH,
            requirement.minimum
        )
    })?;
    let entry = index
        .get(platform)
        .and_then(|components| components.get(&component))
        .and_then(|entries| entries.first())
        .ok_or_else(|| anyhow!("Mojang 的 {platform} 运行时里没有 {component}"))?;

    let manifest_bytes = downloader
        .fetch(&entry.manifest.url)
        .await
        .with_context(|| format!("读取 {component} 的文件清单"))?;
    if !fern_download::sha1_matches(&manifest_bytes, &entry.manifest.sha1) {
        return Err(anyhow!("{component} 的文件清单校验失败"));
    }
    let manifest: RuntimeManifest =
        serde_json::from_slice(&manifest_bytes).context("解析运行时文件清单")?;

    let mut tasks = Vec::new();
    let mut executables = Vec::new();
    let mut links = Vec::new();
    for (relative, file) in &manifest.files {
        let destination = fern_download::safe_join(&install_root, Path::new(relative))?;
        match file {
            RuntimeFile::Directory => tokio::fs::create_dir_all(&destination).await?,
            RuntimeFile::File {
                downloads,
                executable,
            } => {
                tasks.push(DownloadTask::new(
                    destination.clone(),
                    &downloads.raw.url,
                    &downloads.raw.sha1,
                    downloads.raw.size,
                )?);
                if *executable {
                    executables.push(destination);
                }
            }
            RuntimeFile::Link { target } => links.push((destination, target.clone())),
        }
    }

    let _ = events.send(DownloadEvent::Status {
        message: format!("下载 Java {}", entry.version.name),
    });
    downloader.download_all(tasks, events).await?;

    // 下载器只管字节；能不能执行、软链接指向哪，是清单里另外说的。
    for path in executables {
        make_executable(&path).await?;
    }
    for (path, target) in links {
        create_link(&path, &target).await?;
    }

    let mut runtime = java::probe(&install_root)
        .with_context(|| format!("下载完成后仍然读不出 {} 里的 Java", install_root.display()))?;
    // 是我们下的，就该由我们管：设置页要能列出来、能删掉，选择时也优先。
    runtime.managed = true;
    if runtime.major < requirement.minimum {
        return Err(anyhow!(
            "{component} 装出来的是 Java {}，达不到这个版本要求的 Java {}",
            runtime.major,
            requirement.minimum
        ));
    }
    Ok(runtime)
}

/// 元数据没说要哪一份时，按下限推一个。
fn component_for(minimum: u16) -> &'static str {
    match minimum {
        0..=8 => "jre-legacy",
        9..=16 => "java-runtime-alpha",
        17..=20 => "java-runtime-gamma",
        _ => "java-runtime-delta",
    }
}

/// Mojang 的平台键。返回 `None` 表示这个组合官方没有发布——目前只有
/// linux-aarch64 会落到这里，那时候只能请用户自己装一个。
fn platform_key() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Some("windows-x64"),
        ("windows", "x86") => Some("windows-x86"),
        ("windows", "aarch64") => Some("windows-arm64"),
        ("macos", "x86_64") => Some("mac-os"),
        ("macos", "aarch64") => Some("mac-os-arm64"),
        ("linux", "x86_64") => Some("linux"),
        ("linux", "x86") => Some("linux-i386"),
        _ => None,
    }
}

async fn make_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = tokio::fs::metadata(path).await?.permissions();
        // 读得到就执行得了：644 → 755，755 → 755。
        let mode = permissions.mode();
        permissions.set_mode(mode | ((mode & 0o444) >> 2));
        tokio::fs::set_permissions(path, permissions).await?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

async fn create_link(path: &Path, target: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    if tokio::fs::symlink_metadata(path).await.is_ok() {
        tokio::fs::remove_file(path).await?;
    }
    #[cfg(unix)]
    tokio::fs::symlink(target, path).await?;
    // Windows 的运行时清单里没有 link 条目，真出现了也不该为它要管理员权限。
    #[cfg(not(unix))]
    {
        let _ = target;
    }
    Ok(())
}

/// 已经下载好的运行时。设置页要能看见启动器自己占了多少地方。
pub fn installed(paths: &DataPaths) -> Vec<JavaRuntime> {
    java::discover(Some(paths))
        .into_iter()
        .filter(|runtime| runtime.managed)
        .collect()
}

/// 删掉一份下载来的运行时。系统自带的不归我们管，拒绝。
pub fn remove(paths: &DataPaths, component: &str) -> Result<()> {
    let target = fern_download::safe_join(&paths.runtimes, Path::new(component))?;
    if !target.starts_with(&paths.runtimes) || !target.is_dir() {
        return Err(anyhow!("{component} 不是一份由 Fern 下载的运行时"));
    }
    std::fs::remove_dir_all(&target).with_context(|| format!("删除 {}", target.display()))
}

/// 这一份运行时占了多少字节。
pub fn disk_usage(root: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata.is_dir() {
                stack.push(entry.path());
            } else if metadata.is_file() {
                total += metadata.len();
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn components_follow_the_required_major() {
        assert_eq!(component_for(8), "jre-legacy");
        assert_eq!(component_for(16), "java-runtime-alpha");
        assert_eq!(component_for(17), "java-runtime-gamma");
        assert_eq!(component_for(21), "java-runtime-delta");
        assert_eq!(component_for(25), "java-runtime-delta");
    }

    #[test]
    fn runtime_manifest_entries_parse_into_their_three_shapes() {
        let manifest: RuntimeManifest = serde_json::from_str(
            r#"{
              "files": {
                "bin": { "type": "directory" },
                "bin/java": {
                  "type": "file",
                  "executable": true,
                  "downloads": {
                    "lzma": { "sha1": "aa", "size": 1, "url": "https://example.invalid/a" },
                    "raw": { "sha1": "bb", "size": 2, "url": "https://example.invalid/b" }
                  }
                },
                "lib/libjvm.so": { "type": "link", "target": "../server/libjvm.so" }
              }
            }"#,
        )
        .expect("parse runtime manifest");

        assert!(matches!(manifest.files["bin"], RuntimeFile::Directory));
        assert!(matches!(
            manifest.files["bin/java"],
            RuntimeFile::File {
                executable: true,
                ..
            }
        ));
        assert!(matches!(
            manifest.files["lib/libjvm.so"],
            RuntimeFile::Link { .. }
        ));
    }

    #[test]
    fn runtime_files_cannot_escape_the_install_root() {
        let root = Path::new("/fern/runtimes/java-runtime-delta");
        assert!(fern_download::safe_join(root, Path::new("bin/java")).is_ok());
        assert!(fern_download::safe_join(root, Path::new("../../../etc/passwd")).is_err());
    }

    #[test]
    fn removal_refuses_paths_outside_the_runtimes_directory() {
        let paths = DataPaths::new("/tmp/fern-runtime-guard");
        assert!(remove(&paths, "../instances").is_err());
        assert!(remove(&paths, "not-installed").is_err());
    }
}
