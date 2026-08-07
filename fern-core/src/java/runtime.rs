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
    data::settings::source_order,
    java::{self, JavaRequirement, JavaRuntime},
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

/// 主动装一个大版本。
///
/// 和 `ensure_java` 的区别只在意图：那一条是「启动前发现缺了」，这一条是
/// 「现在就把它备好」，所以即使已经装过也当作满足——重复下载一份两百兆的
/// 运行时不是用户要的。
pub async fn install(
    paths: &DataPaths,
    major: u16,
    events: &UnboundedSender<DownloadEvent>,
) -> Result<JavaRuntime> {
    let requirement = JavaRequirement {
        minimum: major,
        maximum: Some(major),
    };
    ensure_java(paths, None, &requirement, events).await
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
    // 这个名字来自版本 JSON，而它要同时变成安装目录名和缓存文件名。
    if !crate::launch::version::is_safe_id(&component) {
        return Err(anyhow!("运行时组件名无法作为目录名：{component}"));
    }
    let install_root = paths.runtimes.join(&component);

    let _ = events.send(DownloadEvent::Status {
        message: format!("准备 Java 运行时（{component}）"),
    });

    let downloader = DownloadClient::new(source_order(), 64);
    // 这份索引一年动不了几次，而且 URL 本身就带着内容哈希——一天一次足够。
    let index_bytes = crate::data::metacache::mutable(
        &downloader,
        paths,
        "java-runtime-index.json",
        RUNTIME_INDEX_URL,
        crate::data::metacache::Freshness::Within(std::time::Duration::from_secs(24 * 60 * 60)),
    )
    .await
    .context("读取 Mojang 运行时清单")?
    .bytes;
    let index: HashMap<String, HashMap<String, Vec<RuntimeEntry>>> =
        serde_json::from_slice(&index_bytes).context("解析 Mojang 运行时清单")?;

    // Mojang 只为自己发行的平台组合提供运行时。落在外面（目前是
    // linux-aarch64）就去 Adoptium 拉一份 Temurin——文档 §4.3 说的兜底。
    let entry = match platform_key()
        .and_then(|platform| index.get(platform))
        .and_then(|components| components.get(&component))
        .and_then(|entries| entries.first())
    {
        Some(entry) => entry,
        None => {
            let install_root = adoptium::install(paths, requirement.minimum, events).await?;
            return finish(&install_root, requirement, "Temurin");
        }
    };

    // 文件清单带 sha1，是不可变的。缓存它的意义在重装/换实例时：同一份运行时
    // 的清单不会因为换了个实例就变。
    let manifest_bytes = crate::data::metacache::immutable(
        &downloader,
        &paths.cache.join(format!("java-runtime-{component}.json")),
        &entry.manifest.url,
        Some(&entry.manifest.sha1),
        None,
    )
    .await
    .with_context(|| format!("读取 {component} 的文件清单"))?;
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

    finish(&install_root, requirement, &component)
}

/// 装完之后统一确认一遍：读得出来，而且真的够新。
fn finish(install_root: &Path, requirement: &JavaRequirement, what: &str) -> Result<JavaRuntime> {
    let mut runtime = java::probe(install_root)
        .with_context(|| format!("下载完成后仍无法识别 {} 中的 Java", install_root.display()))?;
    // 是我们下的，就该由我们管：设置页要能列出来、能删掉，选择时也优先。
    runtime.managed = true;
    if runtime.major < requirement.minimum {
        return Err(anyhow!(
            "{what} 安装的是 Java {}，低于此版本要求的 Java {}",
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
        21..=24 => "java-runtime-delta",
        _ => "java-runtime-epsilon",
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

/// 删掉一份下载来的运行时。
///
/// 只接受 `runtimes/` 里面的目录，而且是 canonicalize 之后再比——否则一条
/// 软链接或者一个 `..` 就能让「删掉一份 Java」变成删掉别的东西。系统自带的
/// Java 不归我们管，也拒绝。
pub fn remove(paths: &DataPaths, home: &Path) -> Result<()> {
    let runtimes = std::fs::canonicalize(&paths.runtimes)
        .with_context(|| format!("读取 {}", paths.runtimes.display()))?;
    let home = std::fs::canonicalize(home).with_context(|| format!("读取 {}", home.display()))?;
    if home == runtimes || !home.starts_with(&runtimes) {
        return Err(anyhow!("{} 不是一份由 Fern 下载的运行时", home.display()));
    }
    std::fs::remove_dir_all(&home).with_context(|| format!("删除 {}", home.display()))
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
        assert_eq!(component_for(24), "java-runtime-delta");
        assert_eq!(component_for(25), "java-runtime-epsilon");
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
    fn removal_refuses_anything_outside_the_runtimes_directory() {
        let root = std::env::temp_dir().join(format!("fern-runtime-guard-{}", std::process::id()));
        let paths = DataPaths::new(&root);
        paths.ensure_exists().expect("create layout");

        let installed = paths.runtimes.join("java-runtime-delta");
        std::fs::create_dir_all(installed.join("bin")).expect("create runtime");

        // 系统自带的、别处的、以及 runtimes 目录自己，都不能删。
        assert!(remove(&paths, Path::new("/usr/lib/jvm")).is_err());
        assert!(remove(&paths, &paths.instances).is_err());
        assert!(remove(&paths, &paths.runtimes).is_err());
        // 绕一圈回到外面也不行——canonicalize 之后才比。
        assert!(remove(&paths, &paths.runtimes.join("../instances")).is_err());

        assert!(remove(&paths, &installed).is_ok());
        assert!(!installed.exists());

        std::fs::remove_dir_all(root).expect("remove test root");
    }
}

/// Adoptium 兜底（文档 §4.3）。
///
/// Mojang 只为它自己发行的平台组合提供运行时，linux-aarch64 就不在其中。
/// 那时候去 Adoptium 拉一份 Temurin：它按 os/arch/major 提供最新的 LTS，
/// 覆盖到了 Mojang 没覆盖的地方。
///
/// 这是兜底不是首选。能用 Mojang 的就用 Mojang 的——那和官方启动器装的是同
/// 一份，玩家遇到问题时能和别人对得上。
mod adoptium {
    use std::path::{Path, PathBuf};

    use anyhow::{Context, Result, anyhow};
    use fern_download::{DownloadClient, DownloadEvent};
    use serde::Deserialize;
    use sha2::{Digest, Sha256};
    use tokio::sync::mpsc::UnboundedSender;

    use crate::{DataPaths, data::settings::source_order};

    #[derive(Debug, Deserialize)]
    struct Asset {
        binary: Binary,
        release_name: String,
    }

    #[derive(Debug, Deserialize)]
    struct Binary {
        package: Package,
    }

    #[derive(Debug, Deserialize)]
    struct Package {
        link: String,
        name: String,
        size: u64,
        #[serde(default)]
        checksum: String,
    }

    /// Adoptium 的架构叫法。和 Rust 的 `consts::ARCH` 不完全一致。
    fn architecture() -> Option<&'static str> {
        match std::env::consts::ARCH {
            "x86_64" => Some("x64"),
            "aarch64" => Some("aarch64"),
            "arm" => Some("arm"),
            "x86" => Some("x86"),
            "powerpc64" => Some("ppc64le"),
            "s390x" => Some("s390x"),
            _ => None,
        }
    }

    fn operating_system() -> Option<&'static str> {
        match std::env::consts::OS {
            "linux" => Some("linux"),
            "macos" => Some("mac"),
            "windows" => Some("windows"),
            _ => None,
        }
    }

    /// 装一份 Temurin JRE，返回安装目录。
    pub async fn install(
        paths: &DataPaths,
        major: u16,
        events: &UnboundedSender<DownloadEvent>,
    ) -> Result<PathBuf> {
        let (os, arch) = operating_system()
            .zip(architecture())
            .ok_or_else(|| adoptium_unavailable(major))?;
        let url = format!(
            "https://api.adoptium.net/v3/assets/latest/{major}/hotspot\
             ?architecture={arch}&image_type=jre&os={os}&vendor=eclipse"
        );

        let _ = events.send(DownloadEvent::Status {
            message: format!("向 Adoptium 查询 Java {major}"),
        });
        // 不走 DownloadClient 的镜像重写：Adoptium 不在任何镜像上，rewrite
        // 对它是恒等的，但重试和源健康度仍然用得上。
        let downloader = DownloadClient::new(source_order(), 4);
        let listing = downloader
            .fetch(&url)
            .await
            .with_context(|| format!("查询 Adoptium 的 Java {major}"))?;
        let assets: Vec<Asset> =
            serde_json::from_slice(&listing).context("解析 Adoptium 的响应")?;
        let asset = assets
            .into_iter()
            .next()
            .ok_or_else(|| adoptium_unavailable(major))?;

        let install_root = paths
            .runtimes
            .join(format!("temurin-{}", asset.release_name));
        if crate::java::probe(&install_root).is_ok() {
            return Ok(install_root);
        }

        let _ = events.send(DownloadEvent::Status {
            message: format!(
                "下载 Temurin {} （{} MB）",
                asset.release_name,
                asset.binary.package.size / (1024 * 1024)
            ),
        });
        let archive = downloader
            .fetch(&asset.binary.package.link)
            .await
            .context("下载 Temurin")?;

        if !asset.binary.package.checksum.is_empty() {
            let actual = Sha256::digest(&archive)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            if !actual.eq_ignore_ascii_case(&asset.binary.package.checksum) {
                return Err(anyhow!("Temurin 校验和不匹配"));
            }
        }

        let _ = events.send(DownloadEvent::Status {
            message: "解压 Java 运行时".to_owned(),
        });
        let name = asset.binary.package.name.clone();
        let root = install_root.clone();
        tokio::task::spawn_blocking(move || unpack(&archive, &name, &root)).await??;

        Ok(install_root)
    }

    fn adoptium_unavailable(major: u16) -> anyhow::Error {
        anyhow!(
            "未找到适用于 {}-{} 的 Java {major}，请自行安装后在实例设置中指定路径",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    }

    /// 解压，并把顶层那一层目录剥掉。
    ///
    /// Temurin 的包解出来是 `jdk-21.0.12+8-jre/bin/java`，多一层。剥掉之后
    /// 目录结构就和 Mojang 的运行时一致，`probe` 那边不必分两种情况。
    fn unpack(archive: &[u8], name: &str, destination: &Path) -> Result<()> {
        if name.ends_with(".zip") {
            let mut zip = zip::ZipArchive::new(std::io::Cursor::new(archive))
                .context("读取 Temurin 压缩包")?;
            for index in 0..zip.len() {
                let mut entry = zip.by_index(index)?;
                if !entry.is_file() {
                    continue;
                }
                let Some(relative) = strip_top_level(entry.name()) else {
                    continue;
                };
                let path = fern_download::safe_join(destination, Path::new(&relative))?;
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let mut target = std::fs::File::create(&path)?;
                std::io::copy(&mut entry, &mut target)?;
            }
            return Ok(());
        }

        let decoder = flate2::read::GzDecoder::new(archive);
        let mut tar = tar::Archive::new(decoder);
        tar.set_preserve_permissions(true);
        for entry in tar.entries().context("读取 Temurin 压缩包")? {
            let mut entry = entry?;
            let path_in_archive = entry.path()?.to_string_lossy().into_owned();
            let Some(relative) = strip_top_level(&path_in_archive) else {
                continue;
            };
            // 压缩包来自网络，条目名不能原样拼路径。
            let path = fern_download::safe_join(destination, Path::new(&relative))?;
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            entry
                .unpack(&path)
                .with_context(|| format!("解压 {relative}"))?;
        }
        Ok(())
    }

    /// `jdk-21.0.12+8-jre/bin/java` → `bin/java`。顶层目录本身返回 `None`。
    fn strip_top_level(path: &str) -> Option<String> {
        let normalised = path.replace('\\', "/");
        let (_, rest) = normalised.split_once('/')?;
        (!rest.is_empty() && !rest.ends_with('/')).then(|| rest.to_owned())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn the_top_level_directory_is_stripped() {
            assert_eq!(
                strip_top_level("jdk-21.0.12+8-jre/bin/java").as_deref(),
                Some("bin/java")
            );
            assert_eq!(
                strip_top_level("jdk-21.0.12+8-jre/lib/server/libjvm.so").as_deref(),
                Some("lib/server/libjvm.so")
            );
            // 顶层目录本身和目录条目都不该产生文件。
            assert_eq!(strip_top_level("jdk-21.0.12+8-jre"), None);
            assert_eq!(strip_top_level("jdk-21.0.12+8-jre/bin/"), None);
        }

        #[test]
        fn adoptium_knows_this_machines_platform() {
            // 认不出来时会退化成一句「请自行安装」，那对主流平台是不可接受的。
            assert!(operating_system().is_some());
            assert!(architecture().is_some());
        }
    }
}
