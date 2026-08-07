//! NeoForge 与 Forge 的安装（文档 §2.5 第二、三阶段）。
//!
//! 和 Fabric 不同，这两家没有 meta server 现成的 profile JSON——安装信息藏在
//! 一个 installer jar 里，而且安装期要**真的在本地跑起若干个 Java 进程**做
//! deobf 和 patch：拆 jar、合并 mapping、重命名、打二进制补丁。装完才有一份
//! 能启动的 version JSON。
//!
//! 两代格式：
//!
//!   1.13 以后（含全部 NeoForge）  `install_profile.json` 带 `processors[]`，
//!                                 按序执行，`{变量}` 和 `[maven坐标]` 现算。
//!   1.12.2 及更早                 没有 processors，`versionInfo` 直接内嵌在
//!                                 `install_profile.json` 里，附带一个 universal
//!                                 jar 要摆进 libraries。纯解压，没有执行。
//!
//! 安装是幂等的：装完在版本目录下留一个标记，再跑一遍直接返回。processors
//! 那一段要几十秒到几分钟，不该每次补全都重来。

use std::{
    collections::HashMap,
    io::Read,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow};
use fern_download::{DownloadClient, DownloadEvent, DownloadTask};
use serde::Deserialize;
use tokio::sync::mpsc::UnboundedSender;

use crate::{DataPaths, LoaderKind, java, runtime, settings::source_order, version};

/// 装完留下的标记。存的是安装器的版本，将来换版本能看出来。
const MARKER: &str = ".fern-installed";

#[derive(Debug, Deserialize)]
struct InstallProfile {
    /// 1.13 之后才有。没有这个字段就是老格式。
    #[serde(default)]
    spec: Option<u32>,
    /// version JSON 在 jar 里的路径，通常是 `/version.json`。
    #[serde(default)]
    json: Option<String>,
    #[serde(default)]
    libraries: Vec<ProfileLibrary>,
    #[serde(default)]
    processors: Vec<Processor>,
    #[serde(default)]
    data: HashMap<String, SidedValue>,
    /// 老格式：版本描述直接内嵌。
    #[serde(rename = "versionInfo", default)]
    version_info: Option<serde_json::Value>,
    /// 老格式：universal jar 的信息。
    #[serde(default)]
    install: Option<LegacyInstall>,
}

#[derive(Debug, Deserialize)]
struct LegacyInstall {
    /// universal jar 在 installer 里的路径。
    #[serde(rename = "filePath")]
    file_path: String,
    /// 它该被摆到 libraries 里的哪个坐标下。
    path: String,
}

#[derive(Debug, Deserialize)]
struct ProfileLibrary {
    #[allow(dead_code)]
    name: String,
    #[serde(default)]
    downloads: Option<LibraryDownloads>,
}

#[derive(Debug, Deserialize)]
struct LibraryDownloads {
    #[serde(default)]
    artifact: Option<Artifact>,
}

#[derive(Debug, Deserialize)]
struct Artifact {
    path: String,
    url: String,
    sha1: String,
    size: u64,
}

#[derive(Debug, Deserialize)]
struct SidedValue {
    client: String,
}

#[derive(Debug, Deserialize)]
struct Processor {
    jar: String,
    #[serde(default)]
    classpath: Vec<String>,
    #[serde(default)]
    args: Vec<String>,
    /// 只在这些 side 上跑。缺省表示两边都跑。
    #[serde(default)]
    sides: Option<Vec<String>>,
    /// 产物和它们的 sha1。有的话可以据此跳过已经做完的那一步。
    #[serde(default)]
    outputs: HashMap<String, String>,
}

/// 装一个基于 installer 的加载器，返回版本 id。
pub async fn install(
    paths: &DataPaths,
    kind: LoaderKind,
    game_version: &str,
    loader_version: &str,
    events: &UnboundedSender<DownloadEvent>,
) -> Result<String> {
    let installer_url = installer_url(kind, game_version, loader_version)?;
    let downloader = DownloadClient::new(source_order(), 16);

    let _ = events.send(DownloadEvent::Status {
        message: format!("读取 {} {loader_version} 的安装信息", display(kind)),
    });
    let installer_path = paths
        .root
        .join("installers")
        .join(format!("{}-{loader_version}.jar", slug(kind)));
    if !tokio::fs::try_exists(&installer_path).await? {
        let bytes = downloader
            .fetch(&installer_url)
            .await
            .with_context(|| format!("下载 {} 的安装器", display(kind)))?;
        tokio::fs::create_dir_all(installer_path.parent().expect("installer directory")).await?;
        let temporary = installer_path.with_extension("jar.part");
        tokio::fs::write(&temporary, &bytes).await?;
        tokio::fs::rename(&temporary, &installer_path).await?;
    }

    let (profile, version_json) = read_profile(&installer_path)?;
    let version_id = write_version_json(paths, &version_json)?;

    // 装过就别再来一遍：processors 那一段要几十秒到几分钟。
    let marker = paths.versions.join(&version_id).join(MARKER);
    if tokio::fs::read_to_string(&marker).await.ok().as_deref() == Some(loader_version) {
        return Ok(version_id);
    }

    if profile.spec.is_none() {
        // 老 Forge：没有 processors，把 universal jar 摆进 libraries 就完了。
        install_legacy(paths, &installer_path, &profile, events)?;
    } else {
        run_processors(
            paths,
            &installer_path,
            &profile,
            game_version,
            events,
            &downloader,
        )
        .await?;
    }

    tokio::fs::write(&marker, loader_version).await?;
    Ok(version_id)
}

pub(crate) fn installer_url(
    kind: LoaderKind,
    game_version: &str,
    loader_version: &str,
) -> Result<String> {
    match kind {
        LoaderKind::NeoForge => Ok(format!(
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/\
             {loader_version}/neoforge-{loader_version}-installer.jar"
        )),
        LoaderKind::Forge => Ok(format!(
            "https://maven.minecraftforge.net/net/minecraftforge/forge/\
             {game_version}-{loader_version}/forge-{game_version}-{loader_version}-installer.jar"
        )),
        other => Err(anyhow!("{other:?} 不是基于 installer 的加载器")),
    }
}

fn display(kind: LoaderKind) -> &'static str {
    crate::loader::display_name(kind)
}

fn slug(kind: LoaderKind) -> &'static str {
    match kind {
        LoaderKind::NeoForge => "neoforge",
        _ => "forge",
    }
}

/// 从 installer jar 里取出 install_profile 和它指向的 version JSON。
fn read_profile(installer: &Path) -> Result<(InstallProfile, serde_json::Value)> {
    let file =
        std::fs::File::open(installer).with_context(|| format!("打开 {}", installer.display()))?;
    let mut archive = zip::ZipArchive::new(file).context("读取安装器")?;

    let profile: InstallProfile = {
        let mut entry = archive
            .by_name("install_profile.json")
            .context("安装器里没有 install_profile.json")?;
        let mut text = String::new();
        entry.read_to_string(&mut text)?;
        serde_json::from_str(&text).context("解析 install_profile.json")?
    };

    // 新格式指向 jar 里的另一个文件；老格式直接内嵌。
    let version_json = match (&profile.json, &profile.version_info) {
        (Some(path), _) => {
            let name = path.trim_start_matches('/');
            let mut entry = archive
                .by_name(name)
                .with_context(|| format!("安装器里没有 {name}"))?;
            let mut text = String::new();
            entry.read_to_string(&mut text)?;
            serde_json::from_str(&text).context("解析安装器里的版本描述")?
        }
        (None, Some(info)) => info.clone(),
        (None, None) => return Err(anyhow!("安装器里找不到版本描述")),
    };

    Ok((profile, version_json))
}

fn write_version_json(paths: &DataPaths, version_json: &serde_json::Value) -> Result<String> {
    let id = version_json
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow!("安装器提供的版本描述缺少 id"))?
        .to_owned();
    // id 会变成目录名，而它来自下载来的 jar。
    if !version::is_safe_id(&id) {
        return Err(anyhow!("版本 id 无法作为目录名：{id}"));
    }
    // 解得出 VersionMetadata 才算数，否则问题会推迟到启动那一刻。
    serde_json::from_value::<fern_meta::VersionMetadata>(version_json.clone())
        .context("安装器提供的版本描述无法解析")?;

    let path = version::metadata_path(paths, &id);
    std::fs::create_dir_all(path.parent().expect("version directory"))?;
    std::fs::write(&path, serde_json::to_vec_pretty(version_json)?)?;
    Ok(id)
}

/// 1.12.2 及更早：把 universal jar 从安装器里掏出来摆进 libraries。
fn install_legacy(
    paths: &DataPaths,
    installer: &Path,
    profile: &InstallProfile,
    events: &UnboundedSender<DownloadEvent>,
) -> Result<()> {
    let install = profile
        .install
        .as_ref()
        .ok_or_else(|| anyhow!("旧版安装器缺少 install 段"))?;
    let _ = events.send(DownloadEvent::Status {
        message: "摆放 Forge 的核心库".to_owned(),
    });

    let relative = fern_meta::maven_path(&install.path)
        .ok_or_else(|| anyhow!("无法从坐标 {} 推导路径", install.path))?;
    let destination = fern_download::safe_join(&paths.libraries, Path::new(&relative))?;
    std::fs::create_dir_all(destination.parent().expect("library directory"))?;

    let file = std::fs::File::open(installer)?;
    let mut archive = zip::ZipArchive::new(file)?;
    let name = install.file_path.trim_start_matches('/');
    let mut entry = archive
        .by_name(name)
        .with_context(|| format!("安装器里没有 {name}"))?;
    let mut target = std::fs::File::create(&destination)?;
    std::io::copy(&mut entry, &mut target)?;
    Ok(())
}

/// 把 `data` 里的引用解析成具体的值。
///
/// 三种形态：`[maven坐标]` 指向 libraries 里的一个文件，`/路径` 要从安装器里
/// 掏出来，`'字面量'` 就是它自己。
fn resolve_data(
    paths: &DataPaths,
    installer: &Path,
    profile: &InstallProfile,
    work: &Path,
) -> Result<HashMap<String, String>> {
    let mut resolved = HashMap::new();
    let mut archive = zip::ZipArchive::new(std::fs::File::open(installer)?)?;

    for (key, value) in &profile.data {
        let raw = value.client.as_str();
        let concrete =
            if let Some(coordinate) = raw.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
                let relative = fern_meta::maven_path(coordinate)
                    .ok_or_else(|| anyhow!("无法从坐标 {coordinate} 推导路径"))?;
                fern_download::safe_join(&paths.libraries, Path::new(&relative))?
                    .display()
                    .to_string()
            } else if let Some(inside) = raw.strip_prefix('/') {
                // 从安装器里掏出来放进工作目录。
                let destination = fern_download::safe_join(work, Path::new(inside))?;
                std::fs::create_dir_all(destination.parent().expect("work directory"))?;
                let mut entry = archive
                    .by_name(inside)
                    .with_context(|| format!("安装器里没有 {inside}"))?;
                let mut target = std::fs::File::create(&destination)?;
                std::io::copy(&mut entry, &mut target)?;
                destination.display().to_string()
            } else {
                raw.trim_matches('\'').to_owned()
            };
        resolved.insert(key.clone(), concrete);
    }
    Ok(resolved)
}

/// 展开一个 processor 参数里的 `{变量}` 和 `[maven坐标]`。
fn expand(argument: &str, data: &HashMap<String, String>, libraries: &Path) -> Result<String> {
    // 整个参数就是一个坐标的情况最常见，先处理掉。
    if let Some(coordinate) = argument.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        let relative = fern_meta::maven_path(coordinate)
            .ok_or_else(|| anyhow!("无法从坐标 {coordinate} 推导路径"))?;
        return Ok(fern_download::safe_join(libraries, Path::new(&relative))?
            .display()
            .to_string());
    }

    let mut output = String::with_capacity(argument.len());
    let mut rest = argument;
    while let Some(start) = rest.find('{') {
        output.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else {
            output.push_str(&rest[start..]);
            return Ok(output);
        };
        let key = &after[..end];
        match data.get(key) {
            Some(value) => output.push_str(value),
            // 未知变量原样留着——某些加载器有私有变量，猜错不如不动。
            None => {
                output.push('{');
                output.push_str(key);
                output.push('}');
            }
        }
        rest = &after[end + 1..];
    }
    output.push_str(rest);
    Ok(output)
}

async fn run_processors(
    paths: &DataPaths,
    installer: &Path,
    profile: &InstallProfile,
    game_version: &str,
    events: &UnboundedSender<DownloadEvent>,
    downloader: &DownloadClient,
) -> Result<()> {
    // 安装器自带的库要先下齐，processors 的 classpath 全指向它们。
    let _ = events.send(DownloadEvent::Status {
        message: "下载安装期需要的库".to_owned(),
    });
    let mut tasks = Vec::new();
    for library in &profile.libraries {
        let Some(artifact) = library.downloads.as_ref().and_then(|d| d.artifact.as_ref()) else {
            continue;
        };
        if artifact.url.is_empty() {
            // 有些条目只是占位，真正的文件由某个 processor 产出。
            continue;
        }
        tasks.push(DownloadTask::new(
            fern_download::safe_join(&paths.libraries, Path::new(&artifact.path))?,
            &artifact.url,
            &artifact.sha1,
            artifact.size,
        )?);
    }
    downloader.download_all(tasks, events).await?;

    // processors 是 Java 程序，得先有 Java。用游戏本身要的那个版本——
    // installertools 的要求不会比它更高。
    let requirement = java::requirement(game_version, LoaderKind::NeoForge, None);
    let runtime = runtime::ensure_java(paths, None, &requirement, events).await?;

    let work = paths.root.join("installers").join("work");
    std::fs::create_dir_all(&work)?;
    let mut data = resolve_data(paths, installer, profile, &work)?;
    // 内置变量。`{SIDE}` 固定是 client——Fern 不装服务端。
    data.insert("SIDE".to_owned(), "client".to_owned());
    data.insert("ROOT".to_owned(), paths.root.display().to_string());
    data.insert("INSTALLER".to_owned(), installer.display().to_string());
    data.insert(
        "LIBRARY_DIR".to_owned(),
        paths.libraries.display().to_string(),
    );
    data.insert(
        "MINECRAFT_JAR".to_owned(),
        paths
            .versions
            .join(game_version)
            .join(format!("{game_version}.jar"))
            .display()
            .to_string(),
    );

    let client_side: Vec<&Processor> = profile
        .processors
        .iter()
        .filter(|processor| {
            processor
                .sides
                .as_ref()
                .is_none_or(|sides| sides.iter().any(|side| side == "client"))
        })
        .collect();

    for (index, processor) in client_side.iter().enumerate() {
        let _ = events.send(DownloadEvent::Status {
            message: format!(
                "安装 {}/{}：{}",
                index + 1,
                client_side.len(),
                short(&processor.jar)
            ),
        });
        run_one(paths, processor, &data, &runtime.path).await?;
    }
    Ok(())
}

fn short(coordinate: &str) -> &str {
    coordinate.split(':').nth(1).unwrap_or(coordinate)
}

async fn run_one(
    paths: &DataPaths,
    processor: &Processor,
    data: &HashMap<String, String>,
    java_binary: &Path,
) -> Result<()> {
    // 产物齐了就跳过。Forge 会给 outputs，NeoForge 目前不给——给了就用上。
    if !processor.outputs.is_empty() {
        let mut satisfied = true;
        for (path, sha1) in &processor.outputs {
            let path = expand(path, data, &paths.libraries)?;
            let expected = expand(sha1, data, &paths.libraries)?;
            let matches = std::fs::read(&path)
                .map(|bytes| fern_download::sha1_matches(&bytes, &expected))
                .unwrap_or(false);
            if !matches {
                satisfied = false;
                break;
            }
        }
        if satisfied {
            return Ok(());
        }
    }

    let jar = library_path(paths, &processor.jar)?;
    let main_class = main_class(&jar)?;

    let separator = if cfg!(target_os = "windows") {
        ";"
    } else {
        ":"
    };
    let mut classpath = vec![jar.display().to_string()];
    for entry in &processor.classpath {
        classpath.push(library_path(paths, entry)?.display().to_string());
    }

    let mut arguments = vec![
        "-cp".to_owned(),
        classpath.join(separator),
        main_class.clone(),
    ];
    for argument in &processor.args {
        arguments.push(expand(argument, data, &paths.libraries)?);
    }

    let binary = java_binary.to_owned();
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new(&binary).args(arguments).output()
    })
    .await?
    .with_context(|| format!("运行 {main_class}"))?;

    if !output.status.success() {
        // 安装器的输出是诊断这一步唯一的线索，别吞掉。
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if stderr.trim().is_empty() {
            stdout
        } else {
            stderr
        };
        return Err(anyhow!(
            "{main_class} 执行失败：{}",
            detail.lines().rev().take(6).collect::<Vec<_>>().join(" / ")
        ));
    }
    Ok(())
}

fn library_path(paths: &DataPaths, coordinate: &str) -> Result<PathBuf> {
    let relative = fern_meta::maven_path(coordinate)
        .ok_or_else(|| anyhow!("无法从坐标 {coordinate} 推导路径"))?;
    fern_download::safe_join(&paths.libraries, Path::new(&relative))
}

/// 从 jar 的 MANIFEST 里读 `Main-Class`。
///
/// MANIFEST 的行长上限是 72 字节，超了就折行，续行以一个空格开头——不处理
/// 折行的话，长包名的主类会被截断，报出来的错和真正的原因毫无关系。
fn main_class(jar: &Path) -> Result<String> {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(jar)?)
        .with_context(|| format!("读取 {}", jar.display()))?;
    let mut text = String::new();
    archive
        .by_name("META-INF/MANIFEST.MF")
        .with_context(|| format!("{} 里没有 MANIFEST", jar.display()))?
        .read_to_string(&mut text)?;

    let mut value: Option<String> = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("Main-Class:") {
            value = Some(rest.trim().to_owned());
        } else if let Some(continuation) = line.strip_prefix(' ') {
            if let Some(current) = value.as_mut() {
                current.push_str(continuation.trim_end_matches(['\r', '\n']));
            }
        } else if value.is_some() {
            break;
        }
    }
    value.ok_or_else(|| anyhow!("{} 的 MANIFEST 里没有 Main-Class", jar.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> HashMap<String, String> {
        HashMap::from([
            ("SIDE".to_owned(), "client".to_owned()),
            ("ROOT".to_owned(), "/fern".to_owned()),
            ("MC_SLIM".to_owned(), "/fern/libraries/slim.jar".to_owned()),
        ])
    }

    #[test]
    fn variables_expand_inside_a_larger_argument() {
        let libraries = Path::new("/fern/libraries");
        assert_eq!(
            expand("{ROOT}/run.sh", &data(), libraries).unwrap(),
            "/fern/run.sh"
        );
        assert_eq!(expand("{SIDE}", &data(), libraries).unwrap(), "client");
        assert_eq!(
            expand("--input={MC_SLIM}", &data(), libraries).unwrap(),
            "--input=/fern/libraries/slim.jar"
        );
    }

    #[test]
    fn a_bare_coordinate_becomes_a_library_path() {
        let libraries = Path::new("/fern/libraries");
        assert_eq!(
            expand("[net.neoforged:neoform:1.21.1@zip]", &data(), libraries).unwrap(),
            "/fern/libraries/net/neoforged/neoform/1.21.1/neoform-1.21.1.zip"
        );
    }

    #[test]
    fn unknown_variables_are_left_alone() {
        // 某些加载器有私有变量。猜错不如原样留着，让安装器自己报。
        let libraries = Path::new("/fern/libraries");
        assert_eq!(
            expand("{MYSTERY}", &data(), libraries).unwrap(),
            "{MYSTERY}"
        );
        // 没有闭合的花括号也不该把参数吃掉。
        assert_eq!(
            expand("{unclosed", &data(), libraries).unwrap(),
            "{unclosed"
        );
    }

    #[test]
    fn a_coordinate_that_escapes_the_libraries_directory_is_refused() {
        let libraries = Path::new("/fern/libraries");
        assert!(expand("[..:evil:1.0]", &data(), libraries).is_err());
    }

    #[test]
    fn manifest_continuation_lines_are_joined() {
        // 72 字节折行是 MANIFEST 的规矩，长包名一定会碰上。
        let manifest = "Manifest-Version: 1.0\nMain-Class: net.neoforged.installertools.Conso\n leEntryPoint\nBuild-Jdk: 17\n";
        let root = std::env::temp_dir().join(format!("fern-manifest-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("create root");
        let jar = root.join("tool.jar");

        let mut writer = zip::ZipWriter::new(std::fs::File::create(&jar).expect("create jar"));
        writer
            .start_file(
                "META-INF/MANIFEST.MF",
                zip::write::SimpleFileOptions::default(),
            )
            .expect("start entry");
        use std::io::Write;
        writer.write_all(manifest.as_bytes()).expect("write");
        writer.finish().expect("finish");

        assert_eq!(
            main_class(&jar).expect("read main class"),
            "net.neoforged.installertools.ConsoleEntryPoint"
        );
        std::fs::remove_dir_all(root).expect("remove root");
    }

    #[test]
    fn installer_urls_follow_each_projects_layout() {
        // Forge 的坐标里带游戏版本，NeoForge 的不带——写反了会 404。
        assert!(
            installer_url(LoaderKind::NeoForge, "1.21.1", "21.1.248")
                .unwrap()
                .ends_with("/neoforge/21.1.248/neoforge-21.1.248-installer.jar")
        );
        assert!(
            installer_url(LoaderKind::Forge, "1.12.2", "14.23.5.2859")
                .unwrap()
                .ends_with("/forge/1.12.2-14.23.5.2859/forge-1.12.2-14.23.5.2859-installer.jar")
        );
        assert!(installer_url(LoaderKind::Fabric, "1.21.1", "0.16.5").is_err());
    }
}
