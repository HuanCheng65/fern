//! Java 管理。
//!
//! 分三层，对应文档 §4：
//!
//!   声明层  这个版本需要什么 Java —— version JSON 的 `javaVersion` 给出下限，
//!           加载器再收紧上限。两者取交集才是真正能用的区间。
//!   发现层  这台机器上有什么 Java —— 扫候选目录，读 JDK 根目录的 `release`
//!           文件拿版本和架构。不起 `java -version` 进程：几十个候选各 fork
//!           一次太慢，而且 release 文件还顺带给出 `OS_ARCH`。只有连 release
//!           都没有的老 JDK（Ubuntu 的 openjdk-8 就是）才退回去跑一次。
//!   下载层  缺就自动下 —— 见 `runtime.rs`。
//!
//! 这一层对用户是隐形的：选哪个 Java 不该是一道题。只有用户在实例设置里
//! 明确指定了路径，我们才照做——那时候他要的是控制权，不是建议。

use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

use fern_meta::release_ordinal;

use crate::{DataPaths, LoaderKind};

/// 一个能用来启动游戏的 Java。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaRuntime {
    /// 可执行文件本身，直接拿去 spawn。
    pub path: PathBuf,
    /// JDK/JRE 根目录。
    pub home: PathBuf,
    pub major: u16,
    /// 完整版本号，给用户看的那一份。
    pub version: String,
    /// 归一化后的架构（`x86_64` / `aarch64` / …）。
    pub arch: String,
    pub vendor: String,
    /// 由启动器自己下载并管理，放在 `runtimes/` 下面。
    #[serde(default)]
    pub managed: bool,
}

impl JavaRuntime {
    /// 和启动器进程同架构。Apple Silicon 上的 x64 Java 走 Rosetta 能跑，
    /// 但性能明显下降，只在没有原生版本时才该选中。
    pub fn is_native_arch(&self) -> bool {
        self.arch == normalize_arch(env::consts::ARCH)
    }
}

/// 一个版本能接受的 Java 区间。
///
/// 下限是硬的：Java 比它旧，游戏根本起不来。上限是软的，用来表达「这个组合
/// 已知会出问题」——手上只有更新的 Java 时我们仍然让他启动，而不是拦住。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaRequirement {
    pub minimum: u16,
    pub maximum: Option<u16>,
}

impl JavaRequirement {
    pub fn accepts(&self, major: u16) -> bool {
        major >= self.minimum && self.maximum.is_none_or(|maximum| major <= maximum)
    }

    fn tighten_minimum(mut self, minimum: u16) -> Self {
        self.minimum = self.minimum.max(minimum);
        self
    }

    fn tighten_maximum(mut self, maximum: u16) -> Self {
        self.maximum = Some(self.maximum.map_or(maximum, |current| current.min(maximum)));
        self
    }
}

/// 版本要求 ∩ 加载器兼容区间。
///
/// `declared` 是 version JSON 里的 `javaVersion.majorVersion`——1.17 以后每个
/// 版本都有，是权威的下限。表格补的是元数据从来不表达的**上限**：1.16.5 在
/// Java 21 上不是「不够新」，是压根跑不了。
pub fn requirement(
    game_version: &str,
    loader: LoaderKind,
    declared: Option<u16>,
) -> JavaRequirement {
    let mut requirement = match release_ordinal(game_version) {
        // 1.16.5 及更早：LWJGL 2 与旧 launchwrapper 依赖 Java 8 的内部 API。
        Some(version) if version < (1, 17, 0) => JavaRequirement {
            minimum: 8,
            maximum: Some(8),
        },
        Some(version) if version < (1, 18, 0) => JavaRequirement {
            minimum: 16,
            maximum: Some(17),
        },
        Some(version) if version < (1, 20, 5) => JavaRequirement {
            minimum: 17,
            maximum: Some(21),
        },
        Some(_) => JavaRequirement {
            minimum: 21,
            maximum: None,
        },
        // 快照没有可比较的版本号，元数据的声明就是全部信息。
        None => JavaRequirement {
            minimum: declared.unwrap_or(21),
            maximum: None,
        },
    };

    if let Some(declared) = declared {
        requirement = requirement.tighten_minimum(declared);
        // 元数据说要 21，表格却写着上限 17，说明这个版本号没落进表格预期的
        // 区间——以元数据为准，把矛盾的上限让开。
        if requirement
            .maximum
            .is_some_and(|maximum| maximum < declared)
        {
            requirement.maximum = None;
        }
    }

    match loader {
        // 旧 Forge 的 coremod 直接反射 JDK 内部类，新 Java 上必崩。
        LoaderKind::Forge => requirement.tighten_maximum(8.max(requirement.minimum)),
        LoaderKind::NeoForge => requirement.tighten_minimum(21),
        LoaderKind::Vanilla | LoaderKind::Fabric | LoaderKind::Quilt => requirement,
    }
}

/// 从候选里挑一个。挑不出来返回 `None`——调用方再决定是去下载还是报错。
///
/// 排序意图：能跑 > 跑得好 > 跑得对。先滤掉低于下限的（那是硬伤），再优先
/// 落在推荐区间内的、原生架构的，最后在合格的里面选**最小**的那个大版本：
/// 1.20.1 有 17 和 25 可选时，17 才是游戏被测试过的环境。
pub fn select(runtimes: &[JavaRuntime], requirement: &JavaRequirement) -> Option<JavaRuntime> {
    runtimes
        .iter()
        .filter(|runtime| runtime.major >= requirement.minimum)
        .min_by_key(|runtime| {
            (
                !requirement.accepts(runtime.major),
                !runtime.is_native_arch(),
                runtime.major,
                !runtime.managed,
            )
        })
        .cloned()
}

/// 这台机器上所有能用的 Java，按大版本排序。
///
/// `paths` 只用来把启动器自己下载的运行时也纳进来；传 `None` 就只扫系统。
pub fn discover(paths: Option<&DataPaths>) -> Vec<JavaRuntime> {
    let mut runtimes = Vec::new();
    let mut seen = HashSet::new();

    let mut homes = Vec::new();
    if let Some(paths) = paths {
        // 自己下载的排前面：同版本时优先用我们管得住的那一份。
        collect_children(&paths.runtimes, &mut homes);
    }
    let managed_count = homes.len();
    homes.extend(system_java_homes());

    for (index, home) in homes.into_iter().enumerate() {
        let Some(runtime) = probe_home(&home, index < managed_count) else {
            continue;
        };
        // 同一个 JDK 会被好几条路径找到（`java-1.21.0-…` 是 `java-21-…` 的
        // 符号链接，PATH 上的 `java` 又指向其中之一），按真实路径去重。
        let identity = fs::canonicalize(&runtime.path).unwrap_or_else(|_| runtime.path.clone());
        if seen.insert(identity) {
            runtimes.push(runtime);
        }
    }

    runtimes.sort_by(|left, right| {
        left.major
            .cmp(&right.major)
            .then_with(|| left.version.cmp(&right.version))
    });
    runtimes
}

/// 用户在实例设置里填的那个路径。可以指到可执行文件，也可以指到 JDK 根目录。
pub fn probe(path: &Path) -> Result<JavaRuntime> {
    if path.is_dir() {
        return probe_home(path, false)
            .ok_or_else(|| anyhow!("{} 不像是一个 Java 安装目录", path.display()));
    }
    let home = path
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| path.to_path_buf());
    if let Some(runtime) =
        read_release(&home).map(|release| runtime_from_release(path, &home, release, false))
    {
        return Ok(runtime);
    }
    let (major, version) = ask_java_itself(path)?;
    Ok(JavaRuntime {
        path: path.to_path_buf(),
        home,
        major,
        version,
        arch: normalize_arch(env::consts::ARCH).to_owned(),
        vendor: String::new(),
        managed: false,
    })
}

/// 手上最新的一个 Java，不问版本要求。
///
/// 首次启动向导只需要知道「要不要为 Java 说点什么」——一台什么都没有的机器
/// 才值得开口。
pub fn detect_java() -> Option<JavaRuntime> {
    let paths = DataPaths::for_current_user().ok();
    discover(paths.as_ref()).into_iter().next_back()
}

fn probe_home(home: &Path, managed: bool) -> Option<JavaRuntime> {
    // macOS 的 JDK 是 bundle，目录本身不是 home。
    let home = if home.join("Contents/Home/bin").is_dir() {
        home.join("Contents/Home")
    } else {
        home.to_path_buf()
    };
    let executable = home.join("bin").join(java_executable_name());
    if !executable.is_file() {
        return None;
    }
    if let Some(release) = read_release(&home) {
        return Some(runtime_from_release(&executable, &home, release, managed));
    }
    // Ubuntu 打包的 openjdk-8 就没有 release 文件，只能问它自己。
    let (major, version) = ask_java_itself(&executable).ok()?;
    Some(JavaRuntime {
        path: executable,
        home,
        major,
        version,
        arch: normalize_arch(env::consts::ARCH).to_owned(),
        vendor: String::new(),
        managed,
    })
}

fn runtime_from_release(
    executable: &Path,
    home: &Path,
    release: ReleaseFile,
    managed: bool,
) -> JavaRuntime {
    let version = release.java_version.unwrap_or_default();
    JavaRuntime {
        path: executable.to_path_buf(),
        home: home.to_path_buf(),
        major: parse_major(&version).unwrap_or_default(),
        version,
        arch: release
            .os_arch
            .as_deref()
            .map(normalize_arch)
            .unwrap_or(normalize_arch(env::consts::ARCH))
            .to_owned(),
        vendor: release.implementor.unwrap_or_default(),
        managed,
    }
}

#[derive(Debug, Default)]
struct ReleaseFile {
    java_version: Option<String>,
    os_arch: Option<String>,
    implementor: Option<String>,
}

/// `release` 是 `KEY="VALUE"` 一行一条的纯文本，JDK 9 起每个发行版都带。
fn read_release(home: &Path) -> Option<ReleaseFile> {
    let text = fs::read_to_string(home.join("release")).ok()?;
    let mut release = ReleaseFile::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"').to_owned();
        match key.trim() {
            "JAVA_VERSION" => release.java_version = Some(value),
            "OS_ARCH" => release.os_arch = Some(value),
            "IMPLEMENTOR" => release.implementor = Some(value),
            _ => {}
        }
    }
    release.java_version.as_ref()?;
    Some(release)
}

fn ask_java_itself(executable: &Path) -> Result<(u16, String)> {
    let output = Command::new(executable)
        .arg("-version")
        .output()
        .with_context(|| format!("运行 {}", executable.display()))?;
    if !output.status.success() {
        return Err(anyhow!("{} 无法报告自己的版本", executable.display()));
    }
    // `java -version` 历来写在 stderr，新版本有的写 stdout，两边都收。
    let text = String::from_utf8_lossy(&output.stderr).into_owned()
        + &String::from_utf8_lossy(&output.stdout);
    let version = text
        .split_once("version \"")
        .and_then(|(_, rest)| rest.split('"').next())
        .map(str::to_owned)
        .or_else(|| {
            text.lines()
                .find_map(|line| line.trim().strip_prefix("openjdk "))
                .and_then(|rest| rest.split_whitespace().next())
                .map(str::to_owned)
        })
        .ok_or_else(|| anyhow!("无法解析 {} 的版本", executable.display()))?;
    let major = parse_major(&version)
        .ok_or_else(|| anyhow!("无法解析 {} 的版本：{version}", executable.display()))?;
    Ok((major, version))
}

/// `21.0.11` → 21，`1.8.0_402` → 8。
fn parse_major(version: &str) -> Option<u16> {
    let version = version.trim();
    let version = version.strip_prefix("1.").unwrap_or(version);
    let head: String = version.chars().take_while(char::is_ascii_digit).collect();
    head.parse().ok()
}

/// `amd64` / `x64` / `x86_64` 说的是同一件事，release 文件和 Rust 的写法不一致。
fn normalize_arch(arch: &str) -> &str {
    match arch {
        "amd64" | "x64" | "x86_64" => "x86_64",
        "arm64" | "aarch64" => "aarch64",
        "i386" | "i586" | "i686" | "x86" => "x86",
        other => other,
    }
}

fn java_executable_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "java.exe"
    } else {
        "java"
    }
}

/// 平台上惯例的安装位置，加上环境变量指出来的那些。
///
/// Windows 的注册表（`HKLM\SOFTWARE\JavaSoft\*`）没有扫：那要多引一个
/// 平台专用依赖，而所有主流发行版都会装进下面这些目录，`JAVA_HOME` 和
/// `PATH` 又兜住了装在别处的情况。
fn system_java_homes() -> Vec<PathBuf> {
    let mut homes = Vec::new();

    if let Some(home) = env::var_os("JAVA_HOME") {
        homes.push(PathBuf::from(home));
    }
    // PATH 上的 java 往往是符号链接，回溯到它真正的 home 才能读到 release。
    if let Some(path) = env::var_os("PATH") {
        for directory in env::split_paths(&path) {
            let executable = directory.join(java_executable_name());
            if !executable.is_file() {
                continue;
            }
            let resolved = fs::canonicalize(&executable).unwrap_or(executable);
            if let Some(home) = resolved.parent().and_then(Path::parent) {
                homes.push(home.to_path_buf());
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        for variable in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
            let Some(base) = env::var_os(variable).map(PathBuf::from) else {
                continue;
            };
            for vendor in [
                "Java",
                "Eclipse Adoptium",
                "Eclipse Foundation",
                "Zulu",
                "BellSoft",
                "Amazon Corretto",
                "Microsoft",
                "Programs\\Eclipse Adoptium",
            ] {
                collect_children(&base.join(vendor), &mut homes);
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        collect_children(Path::new("/Library/Java/JavaVirtualMachines"), &mut homes);
        collect_children(
            Path::new("/System/Library/Java/JavaVirtualMachines"),
            &mut homes,
        );
        if let Some(user) = env::var_os("HOME").map(PathBuf::from) {
            collect_children(&user.join("Library/Java/JavaVirtualMachines"), &mut homes);
        }
        for cellar in ["/opt/homebrew/opt", "/usr/local/opt"] {
            for entry in fs::read_dir(cellar).into_iter().flatten().flatten() {
                let path = entry.path();
                if path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("openjdk"))
                {
                    homes.push(path.join("libexec/openjdk.jdk/Contents/Home"));
                }
            }
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        for root in ["/usr/lib/jvm", "/usr/java", "/opt/java", "/opt/jdk"] {
            collect_children(Path::new(root), &mut homes);
        }
        if let Some(user) = env::var_os("HOME").map(PathBuf::from) {
            collect_children(&user.join(".sdkman/candidates/java"), &mut homes);
            collect_children(&user.join(".jdks"), &mut homes);
        }
    }

    homes
}

fn collect_children(root: &Path, homes: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(root).into_iter().flatten().flatten() {
        homes.push(entry.path());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime(major: u16, arch: &str, managed: bool) -> JavaRuntime {
        JavaRuntime {
            path: PathBuf::from(format!("/jvm/{major}/bin/java")),
            home: PathBuf::from(format!("/jvm/{major}")),
            major,
            version: format!("{major}.0.1"),
            arch: arch.to_owned(),
            vendor: "Test".to_owned(),
            managed,
        }
    }

    #[test]
    fn parses_both_version_generations() {
        assert_eq!(parse_major("21.0.11"), Some(21));
        assert_eq!(parse_major("1.8.0_402"), Some(8));
        assert_eq!(parse_major("17"), Some(17));
        assert_eq!(parse_major("not a version"), None);
    }

    #[test]
    fn old_versions_are_capped_and_new_ones_are_not() {
        let old = requirement("1.12.2", LoaderKind::Vanilla, Some(8));
        assert_eq!(old.minimum, 8);
        assert_eq!(old.maximum, Some(8));

        let modern = requirement("1.21.1", LoaderKind::Vanilla, Some(21));
        assert_eq!(modern.minimum, 21);
        assert_eq!(modern.maximum, None);

        let middle = requirement("1.20.1", LoaderKind::Vanilla, Some(17));
        assert!(middle.accepts(17));
        assert!(middle.accepts(21));
        assert!(!middle.accepts(25));
    }

    #[test]
    fn metadata_wins_when_the_table_disagrees() {
        // 元数据说 21，表格给 1.19 写的上限是 21——不该出现「下限高于上限」
        // 这种没有解的区间。
        let resolved = requirement("1.19.2", LoaderKind::Vanilla, Some(22));
        assert_eq!(resolved.minimum, 22);
        assert_eq!(resolved.maximum, None);
    }

    #[test]
    fn loaders_tighten_the_interval() {
        assert_eq!(
            requirement("1.20.1", LoaderKind::NeoForge, Some(17)).minimum,
            21
        );
        assert_eq!(
            requirement("1.12.2", LoaderKind::Forge, Some(8)).maximum,
            Some(8)
        );
    }

    #[test]
    fn snapshots_fall_back_to_the_declared_version() {
        let snapshot = requirement("24w14a", LoaderKind::Vanilla, Some(21));
        assert_eq!(snapshot.minimum, 21);
        assert_eq!(snapshot.maximum, None);
    }

    #[test]
    fn selection_prefers_the_oldest_runtime_inside_the_interval() {
        let native = normalize_arch(env::consts::ARCH);
        let runtimes = vec![
            runtime(8, native, false),
            runtime(17, native, false),
            runtime(21, native, false),
            runtime(25, native, false),
        ];
        let chosen = select(
            &runtimes,
            &requirement("1.20.1", LoaderKind::Vanilla, Some(17)),
        )
        .expect("a runtime should match");
        assert_eq!(chosen.major, 17);

        let legacy = select(
            &runtimes,
            &requirement("1.12.2", LoaderKind::Vanilla, Some(8)),
        )
        .expect("a runtime should match");
        assert_eq!(legacy.major, 8);
    }

    #[test]
    fn selection_never_returns_a_runtime_below_the_hard_minimum() {
        let native = normalize_arch(env::consts::ARCH);
        let runtimes = vec![runtime(8, native, false), runtime(17, native, false)];
        // 区间之外仍然要给出答案：拦住启动比用一个更新的 Java 试一次更糟。
        let chosen = select(
            &runtimes,
            &requirement("1.21.1", LoaderKind::Vanilla, Some(21)),
        );
        assert!(chosen.is_none());
    }

    #[test]
    fn native_architecture_wins_over_a_closer_version() {
        let native = normalize_arch(env::consts::ARCH);
        let foreign = if native == "x86_64" {
            "aarch64"
        } else {
            "x86_64"
        };
        let runtimes = vec![runtime(17, foreign, false), runtime(21, native, false)];
        let chosen = select(
            &runtimes,
            &requirement("1.20.1", LoaderKind::Vanilla, Some(17)),
        )
        .expect("a runtime should match");
        assert_eq!(chosen.major, 21);
    }

    #[test]
    fn discovery_finds_this_machines_runtimes() {
        let runtimes = discover(None);
        // 编译测试的机器上一定有 Java——退一步说，至少 PATH 上那个。
        assert!(!runtimes.is_empty(), "no Java found on the build machine");
        for runtime in &runtimes {
            assert!(runtime.major >= 8, "{runtime:?} has an implausible version");
            assert!(runtime.path.is_file(), "{runtime:?} is not executable");
        }
        let mut sorted = runtimes.clone();
        sorted.sort_by_key(|runtime| runtime.major);
        assert_eq!(
            sorted.iter().map(|r| r.major).collect::<Vec<_>>(),
            runtimes.iter().map(|r| r.major).collect::<Vec<_>>()
        );
    }
}
