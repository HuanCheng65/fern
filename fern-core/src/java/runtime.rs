//! Java 运行时的下载层（文档 §4.3）。
//!
//! 首选 Mojang 官方运行时：有清单、有 sha1、BMCLAPI 有镜像，而且和官方启动器
//! 装的是同一份——玩家遇到的问题能和别人对得上。清单结构又和 assets 几乎一样，
//! 下载器直接复用，这一层只是把 JSON 翻译成下载任务。
//!
//! 体验上这一层必须是隐形的：点启动，没有合适的 Java，就去下，只在进度条上
//! 体现。绝不能变成一句「请先安装 Java」然后把人丢回浏览器。

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow};
use fern_download::{Codec, DownloadEvent, DownloadTask};
use serde::Deserialize;
use tokio::sync::mpsc::UnboundedSender;

use crate::{
    DataPaths,
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
    /// 同一个文件的 lzma 变体。**不是每个文件都有**（java-runtime-gamma 的 133
    /// 个文件里有 7 个没有），所以这里是 `Option`，没有就照原样下。
    ///
    /// 有的那些省得很可观——`lib/server/libjvm.so` 是 23.1 MB 对 5.3 MB。整体
    /// 省不到那么多，因为最大的 `lib/modules`（55.5 MB）恰恰是没有变体的那个：
    /// 它本身就是压好的 jimage。gamma 全组算下来是 95.2 MB 变 68.2 MB。
    ///
    /// `raw` 仍然是这个任务描述的东西：落盘的、校验的、记进账本的都是它。
    lzma: Option<RuntimeArtifact>,
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
        preferred: None,
        ceiling: None,
    };
    ensure_java(paths, None, &requirement, events).await
}

/// 装上这个大版本，并且把这个实例钉在它上面。
///
/// 诊断给出的那颗「换用 Java N」按下去做的就是这两件事，而**钉住是必要的一半**：
/// 自动挑选在区间内取最小的那个（见 [`java::select`] 里的理由），只把 N 装上去，
/// 下一次点启动挑中的仍然是原来那份。
///
/// 会走到这颗按钮，说明自动那条路已经没认出来——按下它的人比我们更清楚，那就
/// 照他说的记下来，而不是记一个下次又会被算掉的偏好。
pub async fn use_java(
    paths: &DataPaths,
    instance_id: &str,
    major: u16,
    events: &UnboundedSender<DownloadEvent>,
) -> Result<JavaRuntime> {
    let runtime = install(paths, major, events).await?;
    let mut profile = crate::read_instance(paths, instance_id)?;
    profile.settings.java_path = Some(runtime.path.clone());
    crate::update_instance_settings(paths, instance_id, profile.settings)?;
    Ok(runtime)
}

/// 保证有一个满足要求的 Java，必要时下载。
///
/// `component` 来自 version JSON 的 `javaVersion.component`——Mojang 自己指定了
/// 用哪一份运行时，照做就是和官方启动器一致。老版本没有这个字段，按下限推。
///
/// **模组那条下界也会让它去下载。** 它在挑选时是软的（见
/// [`JavaRequirement::preferred`]），但「手上只有 17，而这一屋子模组要 21」时该
/// 发生的事是去下一份 21，不是拿 17 凑合然后崩在类加载上。够不着的那些例外：
/// 区间容不下就当没说过——游戏的上限是「已知这个组合会坏」，一个模组推翻不了它。
pub async fn ensure_java(
    paths: &DataPaths,
    component: Option<&str>,
    requirement: &JavaRequirement,
    events: &UnboundedSender<DownloadEvent>,
) -> Result<JavaRuntime> {
    let target = target_major(requirement);
    let installed = java::select(&java::discover(Some(paths)), requirement);
    if let Some(runtime) = installed.as_ref().filter(|runtime| runtime.major >= target) {
        return Ok(runtime.clone());
    }

    let component = if target > requirement.minimum {
        // 要下的是模组那条下界，而 `component` 说的是游戏那条。
        component_for(target).to_owned()
    } else {
        component
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| component_for(target).to_owned())
    };
    match download(paths, component, target, events).await {
        Ok(runtime) => Ok(runtime),
        // 手上这份跑得动游戏，只是跑不了某个模组。为它去下一份更新的没成功，
        // 那就用它启动，该说的话留给预检查——总好过整个补全就此失败。
        Err(error) => match installed {
            Some(runtime) => {
                let _ = paths.append_log(&format!("[java] 未能取得 Java {target}：{error:#}"));
                Ok(runtime)
            }
            None => Err(error),
        },
    }
}

/// 这一趟至少要拿到哪个大版本。
///
/// 游戏那条下界永远算数；模组那条先抬到真拿得到的那一档（见 [`deliverable`]），
/// 再看区间容不容得下——容不下就当没说过，游戏的上限是「已知这个组合会坏」，
/// 一个模组想要更新的 Java 推翻不了它。
fn target_major(requirement: &JavaRequirement) -> u16 {
    requirement
        .preferred
        .map(deliverable)
        .filter(|major| requirement.accepts(*major))
        .unwrap_or(requirement.minimum)
        .max(requirement.minimum)
}

/// 把某一份组件装到位。`target` 是它至少该给出的那个大版本。
async fn download(
    paths: &DataPaths,
    component: String,
    target: u16,
    events: &UnboundedSender<DownloadEvent>,
) -> Result<JavaRuntime> {
    // 这个名字来自版本 JSON，而它要同时变成安装目录名和缓存文件名。
    if !crate::launch::version::is_safe_id(&component) {
        return Err(anyhow!("运行时组件名无法作为目录名：{component}"));
    }
    let install_root = paths.runtimes.join(&component);

    let _ = events.send(DownloadEvent::StatusId {
        id: "job.note.java-prepare".to_owned(),
        params: vec![("component".to_owned(), component.clone())],
    });

    let downloader = crate::data::downloader::client(64);
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
            // Adoptium 也不是什么都有：Apple Silicon 上的 Java 8 它就没发过。
            // 那一格 Azul 的 Zulu 有，排在最后。
            return match adoptium::install(paths, target, events).await {
                Ok(install_root) => finish(&install_root, target, "Temurin"),
                Err(adoptium_error) => match zulu::install(paths, target, events).await {
                    Ok(install_root) => finish(&install_root, target, "Zulu"),
                    Err(zulu_error) => {
                        let _ = paths.append_log(&format!(
                            "[java] Adoptium：{adoptium_error:#}；Zulu：{zulu_error:#}"
                        ));
                        Err(adoptium_error)
                    }
                },
            };
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
                let mut task = DownloadTask::new(
                    destination.clone(),
                    &downloads.raw.url,
                    &downloads.raw.sha1,
                    downloads.raw.size,
                )?;
                // 有压缩变体就走压缩那条线。落盘的仍然是 `raw` 描述的那份，
                // 而且它的 sha1 在解压之后照验不误。
                if let Some(lzma) = &downloads.lzma {
                    task = task.via(Codec::Lzma, &lzma.url, &lzma.sha1, lzma.size)?;
                }
                tasks.push(task);
                if *executable {
                    executables.push(destination);
                }
            }
            RuntimeFile::Link { target } => links.push((destination, target.clone())),
        }
    }

    let _ = events.send(DownloadEvent::StatusId {
        id: "job.note.java-download".to_owned(),
        params: vec![("version".to_owned(), entry.version.name.clone())],
    });
    downloader.download_all(tasks, events).await?;

    // 下载器只管字节；能不能执行、软链接指向哪，是清单里另外说的。
    for path in executables {
        make_executable(&path).await?;
    }
    for (path, target) in links {
        create_link(&path, &target).await?;
    }

    finish(&install_root, target, &component)
}

/// 装完之后统一确认一遍：读得出来，而且真的够新。
fn finish(install_root: &Path, target: u16, what: &str) -> Result<JavaRuntime> {
    // 机器上多了一份 Java，发现缓存里那份名单已经过时。不作废的话，接下来
    // 的启动会「看不见」刚装好的运行时。
    java::invalidate_discovery();
    let mut runtime = java::probe(install_root)
        .with_context(|| format!("下载完成后仍无法识别 {} 中的 Java", install_root.display()))?;
    // 是我们下的，就该由我们管：设置页要能列出来、能删掉，选择时也优先。
    runtime.managed = true;
    if runtime.major < target {
        return Err(anyhow!(
            "{what} 安装的是 Java {}，低于此版本要求的 Java {target}",
            runtime.major
        ));
    }
    Ok(runtime)
}

/// 想要不低于 `floor` 的 Java，实际拿得到的是哪个大版本。
///
/// Mojang 只发那么几份运行时，中间那些大版本根本没有对应的组件：一个按 Java 18
/// 编译的模组，`component_for(18)` 给的是 gamma，而 gamma 装出来是 **17**——照着
/// 18 去装，装完一验就发现还是不够，白下一趟两百兆，而且每次启动都白下一趟。
///
/// 下界就是下界：Java 21 满足「不低于 18」。所以这里把它抬到真拿得到的那一档。
pub(crate) fn deliverable(floor: u16) -> u16 {
    match floor {
        0..=8 => 8,
        9..=16 => 16,
        17 => 17,
        18..=21 => 21,
        22..=25 => 25,
        // 比我们知道的最新那档还高：照它说的报出去，让上面那层去判断够不够得着。
        other => other,
    }
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

/// 这台机器上还拿得到这个大版本的 Java 吗——已经装着的，或者下得下来的。
///
/// 兼容规则里「换一份 Java」这条备选可不可行，问的就是它（文档 §4.5）。
/// **实测**：`windows-arm64` 与 `mac-os-arm64` 没有 `jre-legacy`，Mojang 根本
/// 不为 ARM 发 Java 8；那种机器上第一备选直接落空，只能退到下一条。
pub(crate) fn obtainable(major: u16) -> bool {
    let installed = java::discover(None)
        .iter()
        .any(|runtime| runtime.major == major);
    installed || published(component_for(major))
}

/// Mojang 为这个平台发布了这个组件吗。
fn published(component: &str) -> bool {
    let Some(platform) = platform_key() else {
        return false;
    };
    // ARM 上只有新版本，Java 8 那一份从来没有过。
    !(component == "jre-legacy" && matches!(platform, "windows-arm64" | "mac-os-arm64"))
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
    std::fs::remove_dir_all(&home).with_context(|| format!("删除 {}", home.display()))?;
    java::invalidate_discovery();
    Ok(())
}

/// 起不来的那个文件。Windows 的「不是有效的 Win32 应用程序」和 Unix 的
/// ENOEXEC 说的是同一件事：它根本不是一个可执行程序。
#[cfg(windows)]
const NOT_A_PROGRAM: i32 = 193;
#[cfg(not(windows))]
const NOT_A_PROGRAM: i32 = 8;

/// 启动一份 Java 失败了，把它变成一句说得清的话——顺便，如果坏的是我们自己
/// 下的那一份，就地扔掉它。
///
/// 一份下坏了的运行时不会自己好：`probe` 只看目录结构（`bin/java` 在、虚拟机
/// 在），它照样通过；`ensure_java` 看见「已经有一份能用的」就不再下载。于是
/// 每一次启动都撞同一堵墙，而墙上写的是一句用户无从下手的系统错误。
///
/// 删掉它，下一次启动的补全会重新下一份。系统里自带的 Java 不归我们管，只
/// 报出来。
pub(crate) fn unrunnable(paths: &DataPaths, binary: &Path, error: std::io::Error) -> anyhow::Error {
    if error.raw_os_error() != Some(NOT_A_PROGRAM) {
        return anyhow::Error::new(error).context(format!("启动 {}", binary.display()));
    }
    let Some(root) = installed_root(&paths.runtimes, binary) else {
        return anyhow!(
            "{} 不是可执行文件，这份 Java 已损坏。请在实例设置中指定其他 Java。",
            binary.display()
        );
    };
    match remove(paths, &root) {
        Ok(()) => anyhow!(
            "{} 不是可执行文件，这份 Java 下载时已损坏。已将其删除，下次启动会重新下载。",
            binary.display()
        ),
        Err(error) => anyhow!(
            "{} 不是可执行文件，这份 Java 下载时已损坏，且无法删除：{error:#}",
            binary.display()
        ),
    }
}

/// 这个可执行文件属于 `runtimes/` 下面的哪一份安装。
///
/// 要的是**那一层**，不是它的上两级：macOS 的运行时是
/// `<组件>/jre.bundle/Contents/Home/bin/java`，按上两级删只会掏空一个壳。
fn installed_root(runtimes: &Path, binary: &Path) -> Option<PathBuf> {
    let runtimes = std::fs::canonicalize(runtimes).ok()?;
    let binary = std::fs::canonicalize(binary).ok()?;
    let first = binary.strip_prefix(&runtimes).ok()?.components().next()?;
    Some(runtimes.join(first))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 模组要的那份够得着就装它，够不着就当没说过。
    #[test]
    fn the_mods_floor_decides_what_gets_downloaded_when_the_interval_allows_it() {
        // 1.20.1：区间 17–21，元数据说 17。
        let requirement = JavaRequirement {
            minimum: 17,
            maximum: Some(21),
            preferred: None,
            ceiling: None,
        };
        assert_eq!(target_major(&requirement), 17);
        // 一个按 21 编译的模组：该下的是 21，不是元数据那份 17。
        assert_eq!(target_major(&requirement.preferring(Some(21))), 21);
        // 要 25 的那条区间容不下：游戏的上限说了算，仍然装 17，剩下的话由预检查说。
        assert_eq!(target_major(&requirement.preferring(Some(25))), 17);
        // 比下限还低的一条不该把要装的那份往回拉。
        assert_eq!(target_major(&requirement.preferring(Some(8))), 17);
        // 18 没有对应的运行时，够得着它的是 21——照着 18 去装，装回来的是 17。
        assert_eq!(target_major(&requirement.preferring(Some(18))), 21);
        assert_eq!(deliverable(18), 21);
        assert_eq!(deliverable(23), 25);
        assert_eq!(deliverable(21), 21);
    }

    #[test]
    fn a_broken_runtime_is_traced_back_to_the_installation_it_belongs_to() {
        let root = std::env::temp_dir().join(format!("fern-unrunnable-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = DataPaths::new(&root);
        paths.ensure_exists().expect("create layout");

        // macOS 那种多包一层的结构也要认到组件那一层，不是它的上两级。
        let bundled = paths
            .runtimes
            .join("jre-legacy/jre.bundle/Contents/Home/bin");
        std::fs::create_dir_all(&bundled).expect("create runtime");
        std::fs::write(bundled.join("java"), b"broken").expect("write");
        assert_eq!(
            installed_root(&paths.runtimes, &bundled.join("java")),
            Some(std::fs::canonicalize(paths.runtimes.join("jre-legacy")).expect("canonicalize"))
        );

        // 系统自带的那些不归我们管，认不出来就该说认不出来。
        let outside = root.join("usr/bin");
        std::fs::create_dir_all(&outside).expect("create system java");
        std::fs::write(outside.join("java"), b"fine").expect("write");
        assert_eq!(installed_root(&paths.runtimes, &outside.join("java")), None);

        std::fs::remove_dir_all(root).expect("remove test root");
    }

    /// 只有「它不是一个程序」才该动磁盘。别的失败（权限、文件没了）原样报出去。
    #[test]
    fn only_a_bad_executable_format_discards_the_runtime() {
        let root =
            std::env::temp_dir().join(format!("fern-unrunnable-keep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = DataPaths::new(&root);
        paths.ensure_exists().expect("create layout");
        let home = paths.runtimes.join("jre-legacy");
        std::fs::create_dir_all(home.join("bin")).expect("create runtime");
        let binary = home.join("bin/java");
        std::fs::write(&binary, b"broken").expect("write");

        unrunnable(
            &paths,
            &binary,
            std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        );
        assert!(home.exists(), "权限不足不代表这份运行时坏了");

        unrunnable(
            &paths,
            &binary,
            std::io::Error::from_raw_os_error(NOT_A_PROGRAM),
        );
        assert!(
            !home.exists(),
            "坏掉的那份该被扔掉，否则下次启动还撞同一堵墙"
        );

        std::fs::remove_dir_all(root).expect("remove test root");
    }

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

        // 压缩变体要读出来。丢掉它等于让每个用户白下三四倍的字节，而这件事
        // 编译得过、测试全绿，只有账单上看得见。
        let RuntimeFile::File { downloads, .. } = &manifest.files["bin/java"] else {
            panic!("bin/java 该是个文件");
        };
        let lzma = downloads.lzma.as_ref().expect("清单里的 lzma 变体");
        assert_eq!(lzma.url, "https://example.invalid/a");
        assert_eq!(downloads.raw.url, "https://example.invalid/b");

        // 落盘的仍然是 raw 描述的那一份：压缩只是运输方式。
        let task = DownloadTask::new(
            "/fern/bin/java",
            &downloads.raw.url,
            &downloads.raw.sha1,
            downloads.raw.size,
        )
        .expect("build task")
        .via(Codec::Lzma, &lzma.url, &lzma.sha1, lzma.size)
        .expect("attach the compressed variant");
        assert_eq!(task.sha1.as_deref(), Some("bb"));
        assert_eq!(task.size, Some(2));
        assert_eq!(task.wire.expect("compressed variant").sha1, "aa");
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

/// Azul Zulu，Adoptium 之后的最后一道兜底。
///
/// 只为一个真实的空缺存在：Apple Silicon 上的 Java 8。Mojang 不为 ARM 发
/// `jre-legacy`，Adoptium 没有 macOS aarch64 的 Temurin 8，而 1.16.5 及更早的
/// 版本要的正是 Java 8。Zulu 的社区版免费下载，元数据 API 公开，包上带 sha256。
///
/// 两步问：列表接口按平台挑出最新的那一份，详情接口给 sha256 和大小。
mod zulu {
    use std::path::PathBuf;

    use anyhow::{Context, Result, anyhow};
    use fern_download::DownloadEvent;
    use serde::Deserialize;
    use sha2::{Digest, Sha256};
    use tokio::sync::mpsc::UnboundedSender;

    use crate::DataPaths;

    const API: &str = "https://api.azul.com/metadata/v1/zulu/packages";

    #[derive(Debug, Deserialize)]
    struct Listing {
        package_uuid: String,
    }

    #[derive(Debug, Deserialize)]
    struct Package {
        name: String,
        download_url: String,
        #[serde(default)]
        sha256_hash: String,
        #[serde(default)]
        size: u64,
        #[serde(default)]
        java_version: Vec<u32>,
    }

    /// Azul 的平台写法：系统、架构、压缩包格式。
    fn platform() -> Option<(&'static str, &'static str, &'static str)> {
        let os = match std::env::consts::OS {
            "macos" => "macos",
            "linux" => "linux",
            "windows" => "windows",
            _ => return None,
        };
        let arch = match std::env::consts::ARCH {
            "aarch64" => "aarch64",
            "x86_64" => "x64",
            _ => return None,
        };
        let archive = if os == "windows" { "zip" } else { "tar.gz" };
        Some((os, arch, archive))
    }

    fn listing_url(major: u16, os: &str, arch: &str, archive: &str) -> String {
        format!(
            "{API}/?java_version={major}&os={os}&arch={arch}&archive_type={archive}\
             &java_package_type=jre&javafx_bundled=false&release_status=ga\
             &availability_types=CA&latest=true&page_size=1"
        )
    }

    pub async fn install(
        paths: &DataPaths,
        major: u16,
        events: &UnboundedSender<DownloadEvent>,
    ) -> Result<PathBuf> {
        let (os, arch, archive) =
            platform().ok_or_else(|| anyhow!("Zulu 不提供这个平台的 Java {major}"))?;
        let _ = events.send(DownloadEvent::StatusId {
            id: "job.note.java-zulu-query".to_owned(),
            params: vec![("major".to_owned(), major.to_string())],
        });
        let downloader = crate::data::downloader::client(4);
        let listing: Vec<Listing> = serde_json::from_slice(
            &downloader
                .fetch(&listing_url(major, os, arch, archive))
                .await
                .with_context(|| format!("查询 Zulu 的 Java {major}"))?,
        )
        .context("解析 Zulu 的列表")?;
        let uuid = listing
            .into_iter()
            .next()
            .map(|item| item.package_uuid)
            .ok_or_else(|| anyhow!("Zulu 没有 {os}-{arch} 的 Java {major}"))?;
        let package: Package = serde_json::from_slice(
            &downloader
                .fetch(&format!("{API}/{uuid}"))
                .await
                .context("查询 Zulu 的包信息")?,
        )
        .context("解析 Zulu 的包信息")?;
        if !crate::is_external_url(&package.download_url) {
            return Err(anyhow!("Zulu 给的下载地址不是 https"));
        }

        let version = package
            .java_version
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(".");
        let install_root = paths.runtimes.join(format!("zulu-{version}"));
        if !version.is_empty() && crate::java::probe(&install_root).is_ok() {
            return Ok(install_root);
        }

        let _ = events.send(DownloadEvent::StatusId {
            id: "job.note.java-zulu-download".to_owned(),
            params: vec![
                ("name".to_owned(), version.clone()),
                (
                    "size".to_owned(),
                    (package.size / (1024 * 1024)).to_string(),
                ),
            ],
        });
        let bytes = downloader
            .fetch(&package.download_url)
            .await
            .context("下载 Zulu")?;
        // 详情接口总给 sha256；真没给就不装——这是一份要执行的二进制。
        let actual = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        if package.sha256_hash.is_empty() || !actual.eq_ignore_ascii_case(&package.sha256_hash) {
            return Err(anyhow!("Zulu 校验和不匹配"));
        }

        let _ = events.send(DownloadEvent::StatusId {
            id: "job.note.java-extract".to_owned(),
            params: Vec::new(),
        });
        let root = install_root.clone();
        let name = package.name.clone();
        // Zulu 的包和 Temurin 一样多一层顶层目录，macOS 上里面是
        // `Contents/Home`——探测那边本来就认这个形状。
        tokio::task::spawn_blocking(move || super::adoptium::unpack(&bytes, &name, &root))
            .await??;
        Ok(install_root)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn the_listing_asks_for_one_plain_jre_for_this_platform() {
            let url = listing_url(8, "macos", "aarch64", "tar.gz");
            assert!(url.starts_with("https://api.azul.com/metadata/v1/zulu/packages/?"));
            for part in [
                "java_version=8",
                "os=macos",
                "arch=aarch64",
                "archive_type=tar.gz",
                "java_package_type=jre",
                "javafx_bundled=false",
                "latest=true",
            ] {
                assert!(url.contains(part), "{part} missing from {url}");
            }
            assert!(!url.contains(' '));
        }

        /// 真装一份，要联网，默认不跑：
        /// `cargo test -p fern-core --lib java::runtime::zulu -- --ignored`
        #[tokio::test]
        #[ignore]
        async fn live_a_zulu_8_installs_and_is_recognised() {
            let root = std::env::temp_dir().join(format!("fern-zulu-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            let paths = DataPaths::new(root.clone());
            let (events, _rx) = tokio::sync::mpsc::unbounded_channel();
            let installed = install(&paths, 8, &events).await.expect("install");
            let runtime = crate::java::probe(&installed).expect("probe");
            println!("{} {} {}", runtime.version, runtime.arch, runtime.vendor);
            assert_eq!(runtime.major, 8);
            let _ = std::fs::remove_dir_all(root);
        }
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
    use fern_download::DownloadEvent;
    use serde::Deserialize;
    use sha2::{Digest, Sha256};
    use tokio::sync::mpsc::UnboundedSender;

    use crate::DataPaths;

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

        let _ = events.send(DownloadEvent::StatusId {
            id: "job.note.java-adoptium-query".to_owned(),
            params: vec![("major".to_owned(), major.to_string())],
        });
        // 不走 DownloadClient 的镜像重写：Adoptium 不在任何镜像上，rewrite
        // 对它是恒等的，但重试和源健康度仍然用得上。
        let downloader = crate::data::downloader::client(4);
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

        let _ = events.send(DownloadEvent::StatusId {
            id: "job.note.java-adoptium-download".to_owned(),
            params: vec![
                ("name".to_owned(), asset.release_name.clone()),
                (
                    "size".to_owned(),
                    (asset.binary.package.size / (1024 * 1024)).to_string(),
                ),
            ],
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

        let _ = events.send(DownloadEvent::StatusId {
            id: "job.note.java-extract".to_owned(),
            params: Vec::new(),
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
    pub(super) fn unpack(archive: &[u8], name: &str, destination: &Path) -> Result<()> {
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
