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
//!
//! 这个文件是声明层与发现层；发现层在 Windows 上还要读注册表，那部分在
//! `registry.rs`。下载层在 `runtime.rs`。

pub(crate) mod registry;
pub(crate) mod runtime;

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

/// 这份安装是完整的开发套件还是只含运行时。
///
/// 跑游戏两者没有区别，所以它**不参与选择**——在没有偏好的地方发明一个偏好
/// 只会让选择结果变得难以解释。它的作用只有一个：同一个大版本同时装了 JDK
/// 和 JRE 时，列表里那两行不会长得一模一样。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JavaImage {
    Jdk,
    #[default]
    Jre,
}

/// 一个能用来启动游戏的 Java。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaRuntime {
    /// 可执行文件本身，直接拿去 spawn。
    pub path: PathBuf,
    /// JDK/JRE 根目录。
    pub home: PathBuf,
    pub major: u16,
    /// 大版本之后的那一节：`1.8.0_492` 是 492，`21.0.5` 是 5。
    ///
    /// 有些已知会坏的组合卡在这一级上，大版本根本挡不住——见
    /// [`JavaRequirement::ceiling`]。
    #[serde(default)]
    pub update: u32,
    /// 完整版本号，给用户看的那一份。
    pub version: String,
    /// 归一化后的架构（`x86_64` / `aarch64` / …）。
    pub arch: String,
    /// 32 还是 64 位。32 位的 JVM 分配不到 1.5 GB 以上的堆，而现代整合包
    /// 一开口就是 4 GB。
    #[serde(default)]
    pub bits: u16,
    /// 这份运行时不带图形栈。
    ///
    /// Linux 发行版的 `-headless` 包就是这样：`java` 在、虚拟机在，
    /// `libawt_xawt.so` 不在。拿它启动客户端，游戏会在初始化窗口时抛
    /// `UnsatisfiedLinkError`，而报出来的东西和「你装的是 headless 包」毫无
    /// 字面关系。
    #[serde(default)]
    pub headless: bool,
    pub vendor: String,
    /// 由启动器自己下载并管理，放在 `runtimes/` 下面。
    #[serde(default)]
    pub managed: bool,
    /// 用户在设置中手动登记的路径，不在任何扫描目录里。
    #[serde(default)]
    pub added: bool,
    #[serde(default)]
    pub image: JavaImage,
    /// 与启动器进程同架构。
    ///
    /// Apple Silicon 上的 x64 Java 经 Rosetta 可以运行，但性能明显下降，
    /// 仅在没有原生版本时才应选中，且必须在界面上说明。
    #[serde(default)]
    pub native: bool,
    /// 安装占用的字节数。只对可删除的那些计算——不打算删的东西不必知道多大。
    #[serde(default)]
    pub size_bytes: u64,
}

/// 一个版本能接受的 Java 区间。
///
/// 两头都是硬的：比下限旧起不来，比上限新同样起不来——1.16.5 在 Java 21 上
/// 不是慢一点，是 LWJGL 2 和旧 launchwrapper 依赖的内部 API 已经没了。区间外
/// 的不算候选（见 [`select`]）；缺一个合适的就去下一个，而不是拿手边这个凑。
///
/// 唯一软的是 `preferred`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaRequirement {
    pub minimum: u16,
    pub maximum: Option<u16>,
    /// 装着的模组还要求的下界。
    ///
    /// 它是**软的**，所以不并进 `minimum`：模组要 Java 25 而这台机器上最新的是
    /// 21，该发生的事是「用 21 启动，并且预检查明说有模组要 25」，不是「启动器
    /// 拒绝启动」。真正的硬下界只有一条——游戏自己跑不起来的那条。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred: Option<u16>,
    /// 大版本之内还要卡到 update 一级的那条线。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ceiling: Option<UpdateCeiling>,
}

/// 「这个大版本可以，但不能新过某个小版本」。
///
/// 大版本挡不住的问题真实存在：Forge 34.1.27–36.2.24（1.16.3–1.16.5）里的
/// ModLauncher 8.1.x 反射了一个在 **8u321** 被改掉的 JDK 内部方法。「要 Java
/// 8」这个条件完全满足，游戏照样崩在 `NoSuchMethodError`。
///
/// 这条线只在指定的那个大版本上生效：别的大版本本来就已经被区间挡掉了，再
/// 拿一个 update 数去比毫无意义（`21.0.5` 的 5 和 `1.8.0_5` 的 5 不是一回事）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCeiling {
    pub major: u16,
    /// 允许的最大 update，含。
    pub update: u32,
}

impl JavaRequirement {
    pub fn accepts(&self, major: u16) -> bool {
        major >= self.minimum && self.maximum.is_none_or(|maximum| major <= maximum)
    }

    /// 连 update 一级也算上，这一份能不能用。
    pub fn accepts_runtime(&self, runtime: &JavaRuntime) -> bool {
        self.accepts(runtime.major)
            && self.ceiling.is_none_or(|ceiling| {
                ceiling.major != runtime.major || runtime.update <= ceiling.update
            })
    }

    /// 记下模组要求的那条下界。传 `None` 就是没有模组这么要求。
    pub fn preferring(mut self, preferred: Option<u16>) -> Self {
        self.preferred = preferred;
        self
    }

    /// 加上兼容规则给出的那条 update 线。传 `None` 就是没有规则这么说。
    pub fn capped(mut self, ceiling: Option<UpdateCeiling>) -> Self {
        if ceiling.is_some() {
            self.ceiling = ceiling;
        }
        self
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

/// 这个区间怎么念给人听。
///
/// 上下限相等时是「Java 8」而不是「Java 8–8」——那是把一个数据结构念了出来。
pub fn describe(requirement: &JavaRequirement) -> String {
    match requirement.maximum {
        Some(maximum) if maximum == requirement.minimum => format!("Java {maximum}"),
        Some(maximum) => format!("Java {}–{maximum}", requirement.minimum),
        None => format!("Java {} 或更高版本", requirement.minimum),
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
            preferred: None,
            ceiling: None,
        },
        Some(version) if version < (1, 18, 0) => JavaRequirement {
            minimum: 16,
            maximum: Some(17),
            preferred: None,
            ceiling: None,
        },
        Some(version) if version < (1, 20, 5) => JavaRequirement {
            minimum: 17,
            maximum: Some(21),
            preferred: None,
            ceiling: None,
        },
        Some(_) => JavaRequirement {
            minimum: 21,
            maximum: None,
            preferred: None,
            ceiling: None,
        },
        // 快照没有可比较的版本号，元数据的声明就是全部信息。
        None => JavaRequirement {
            minimum: declared.unwrap_or(21),
            maximum: None,
            preferred: None,
            ceiling: None,
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

    // 卡到 update 一级的那些（1.16.5 的 Forge 在 8u321 之后崩）不在这里：
    // 那要看加载器的具体版本，属于事前兼容规则表，见 `launch::compat` 与
    // [`JavaRequirement::capped`]。这个函数只回答「哪个大版本」。

    match loader {
        // 旧 Forge 的 coremod 直接反射 JDK 内部类，新 Java 上必崩。
        LoaderKind::Forge => requirement.tighten_maximum(8.max(requirement.minimum)),
        LoaderKind::NeoForge => requirement.tighten_minimum(21),
        // LiteLoader 只存在于 1.12.2 及更早，那几个版本本来就被夹在 Java 8，
        // 它自己不再收紧什么。
        LoaderKind::Vanilla | LoaderKind::Fabric | LoaderKind::Quilt | LoaderKind::LiteLoader => {
            requirement
        }
    }
}

/// 从候选里挑一个。挑不出来返回 `None`——调用方再决定是去下载还是报错。
///
/// **只在区间内挑。** 上限不是偏好，是「已知这个组合会坏」：1.16.5 在 Java 21
/// 上不是慢一点，是起不来。曾经把上限当偏好——区间外的也算候选，只是排在
/// 后面——于是一台只装了 Java 11 的机器上，1.7.2 挑中的是 11，
/// [`runtime::ensure_java`] 看见「已经有能用的」就不再去下 Java 8，而设置页
/// 一边把 11 显示成自动选中、一边把它列进「不兼容的版本」。
///
/// 区间内再排：优先够得着模组那条下界的、原生架构的，然后选**最小**的那个大
/// 版本（1.20.1 有 17 和 21 可选时，17 才是游戏被测试过的环境），同版本优先
/// 我们自己管的那份。
///
/// 模组那条下界排在游戏的区间之后：游戏的上限是「已知这个组合会坏」，一个模组
/// 想要更新的 Java 不足以推翻它。
/// 不带图形栈的那些排在最后，而不是直接踢出候选：只有它一个的时候，「用它
/// 启动然后由预检查说清楚」比「一个 Java 都挑不出来」要好——后者的表现是去
/// 下载一份两百兆的运行时，而下回来的那份可能还是同一个毛病。
pub fn select(runtimes: &[JavaRuntime], requirement: &JavaRequirement) -> Option<JavaRuntime> {
    runtimes
        .iter()
        .filter(|runtime| requirement.accepts_runtime(runtime))
        .min_by_key(|runtime| {
            (
                requirement
                    .preferred
                    .is_some_and(|wanted| runtime.major < wanted),
                runtime.headless,
                !runtime.native,
                runtime.major,
                !runtime.managed,
            )
        })
        .cloned()
}

/// 发现结果的短缓存。
///
/// 一次点击启动会走到 `discover` 两遍（补全里的 `ensure_java` 一遍、启动里挑
/// 运行时一遍），而每一遍都要探所有候选目录、给自管运行时遍历上万个文件算
/// 体积——这正是下载结束后「卡住没反馈」的一段。机器上装了什么 Java 在几十秒
/// 内不会变；会变的那几个时刻（装了、删了、登记了新路径）都在我们自己手里，
/// 由 [`invalidate_discovery`] 主动作废。
static DISCOVERY_CACHE: std::sync::Mutex<Option<DiscoveryCache>> = std::sync::Mutex::new(None);

struct DiscoveryCache {
    /// 扫描范围随有没有 `paths` 而不同，混在一起会把系统扫描的结果错发给
    /// 要求带自管运行时的调用方。
    scoped: bool,
    taken_at: std::time::Instant,
    runtimes: Vec<JavaRuntime>,
}

const DISCOVERY_TTL: std::time::Duration = std::time::Duration::from_secs(30);

/// 机器上的 Java 变了（装了、删了、登记或注销了路径），缓存作废。
pub(crate) fn invalidate_discovery() {
    if let Ok(mut cache) = DISCOVERY_CACHE.lock() {
        *cache = None;
    }
}

/// 这台机器上所有能用的 Java，按大版本排序。
///
/// `paths` 只用来把启动器自己下载的运行时也纳进来；传 `None` 就只扫系统。
pub fn discover(paths: Option<&DataPaths>) -> Vec<JavaRuntime> {
    if let Ok(cache) = DISCOVERY_CACHE.lock()
        && let Some(cached) = cache.as_ref()
        && cached.scoped == paths.is_some()
        && cached.taken_at.elapsed() < DISCOVERY_TTL
    {
        return cached.runtimes.clone();
    }
    let runtimes = discover_uncached(paths);
    if let Ok(mut cache) = DISCOVERY_CACHE.lock() {
        *cache = Some(DiscoveryCache {
            scoped: paths.is_some(),
            taken_at: std::time::Instant::now(),
            runtimes: runtimes.clone(),
        });
    }
    runtimes
}

fn discover_uncached(paths: Option<&DataPaths>) -> Vec<JavaRuntime> {
    let mut runtimes = Vec::new();
    let mut seen = HashSet::new();

    let mut homes = Vec::new();
    if let Some(paths) = paths {
        // 自己下载的排前面：同版本时优先用我们管得住的那一份。
        collect_children(&paths.runtimes, &mut homes);
    }
    let managed_count = homes.len();
    // 用户手动登记的：扫描路径的并集之外，只有他知道在哪。
    let added = crate::current_settings().java.extra_paths;
    let added_range = managed_count..managed_count + added.len();
    homes.extend(added);
    homes.extend(system_java_homes());

    for (index, home) in homes.into_iter().enumerate() {
        let Some(mut runtime) = probe_home(&home, index < managed_count) else {
            continue;
        };
        runtime.added = added_range.contains(&index);
        // 只给能删的那些算体积——不打算删的东西不必知道它多大，而算一次要
        // 走一万个文件。
        if runtime.managed {
            runtime.size_bytes = crate::storage::tree_bytes(&runtime.home);
        }
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
        return probe_home(path, false).ok_or_else(|| {
            // 「装了但不完整」和「这里根本没有 Java」是两件事，修法也不同：
            // 前者要重下，后者要换个目录。
            if path.join("bin").join(java_executable_name()).is_file() && !has_jvm_library(path) {
                anyhow!(
                    "{} 里的 Java 不完整：缺少虚拟机（{JVM_LIBRARY}）",
                    path.display()
                )
            } else {
                anyhow!("{} 不是有效的 Java 安装目录", path.display())
            }
        });
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
    let image = detect_image(&home);
    let arch = normalize_arch(env::consts::ARCH).to_owned();
    Ok(JavaRuntime {
        path: path.to_path_buf(),
        major,
        update: parse_update(&version),
        version,
        bits: arch_bits(&arch),
        arch,
        headless: detect_headless(&home),
        home,
        vendor: String::new(),
        managed: false,
        added: false,
        image,
        native: true,
        size_bytes: 0,
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
    let home = [
        home.join("Contents/Home"),
        home.join("jre.bundle/Contents/Home"),
    ]
    .into_iter()
    .find(|candidate| candidate.join("bin").is_dir())
    .unwrap_or_else(|| home.to_path_buf());
    let executable = home.join("bin").join(java_executable_name());
    if !executable.is_file() {
        return None;
    }
    // `bin/java` 本身跑不了字节码，它只是个几十 KB 的启动器，真正的虚拟机在
    // `jvm.dll` / `libjvm.so` 里。少了后者，启动出来的是一句
    // 「Error: missing `server' JVM at …\bin\server\jvm.dll」——那既不是崩溃
    // 报告也不是游戏日志，没人看得出问题出在 Java 上。
    //
    // 一份下到一半的运行时正是这个样子：`bin/java.exe` 和 `release` 已经在位，
    // 虚拟机还没下完。而 `release` 在，我们就不会去跑一次 `java -version`，于是
    // 这份残缺的安装会被当成一个完好的 Java 21，还因为是自己下的而**优先**于
    // 系统里那个真的能用的。
    if !has_jvm_library(&home) {
        return None;
    }
    if let Some(release) = read_release(&home) {
        return Some(runtime_from_release(&executable, &home, release, managed));
    }
    // Ubuntu 打包的 openjdk-8 就没有 release 文件，只能问它自己。
    let (major, version) = ask_java_itself(&executable).ok()?;
    let image = detect_image(&home);
    let arch = normalize_arch(env::consts::ARCH).to_owned();
    Some(JavaRuntime {
        path: executable,
        major,
        update: parse_update(&version),
        version,
        bits: arch_bits(&arch),
        arch,
        headless: detect_headless(&home),
        home,
        vendor: String::new(),
        managed,
        added: false,
        image,
        native: true,
        size_bytes: 0,
    })
}

fn runtime_from_release(
    executable: &Path,
    home: &Path,
    release: ReleaseFile,
    managed: bool,
) -> JavaRuntime {
    let version = release.java_version.unwrap_or_default();
    let arch = release
        .os_arch
        .as_deref()
        .map(normalize_arch)
        .unwrap_or(normalize_arch(env::consts::ARCH))
        .to_owned();
    JavaRuntime {
        path: executable.to_path_buf(),
        home: home.to_path_buf(),
        major: parse_major(&version).unwrap_or_default(),
        update: parse_update(&version),
        version,
        native: arch == normalize_arch(env::consts::ARCH),
        bits: arch_bits(&arch),
        headless: detect_headless(home),
        arch,
        vendor: release.implementor.unwrap_or_default(),
        managed,
        added: false,
        // Adoptium 等发行版在 release 里写了 IMAGE_TYPE；没写的按 javac
        // 在不在判断，这是 JDK 与 JRE 唯一稳定的外部差别。
        image: release
            .image_type
            .as_deref()
            .map(|value| {
                if value.eq_ignore_ascii_case("JDK") {
                    JavaImage::Jdk
                } else {
                    JavaImage::Jre
                }
            })
            .unwrap_or_else(|| detect_image(home)),
        size_bytes: 0,
    }
}

/// 虚拟机那个动态库的文件名。
const JVM_LIBRARY: &str = if cfg!(windows) {
    "jvm.dll"
} else if cfg!(target_os = "macos") {
    "libjvm.dylib"
} else {
    "libjvm.so"
};

/// 这份安装里有没有虚拟机。
///
/// 摆放位置分了好几代：JDK 9 起是 `lib/server/`（Windows 上是 `bin/server/`），
/// JDK 8 还多一层 `jre/`，Unix 上的 8 更是把架构也写进路径
/// （`jre/lib/amd64/server/`），32 位的 JRE 则只带 `client`。这里把这几种都认
/// 一遍——认漏了的代价是把一个能用的 Java 判成不能用，比放过一个残缺的更糟。
fn has_jvm_library(home: &Path) -> bool {
    let holds_jvm = |directory: &Path| {
        ["server", "client"]
            .iter()
            .any(|flavour| directory.join(flavour).join(JVM_LIBRARY).is_file())
    };
    for base in [home.to_path_buf(), home.join("jre")] {
        for middle in ["bin", "lib"] {
            let root = base.join(middle);
            if holds_jvm(&root) {
                return true;
            }
            // `lib/amd64/server/libjvm.so`：只有 JDK 8 的 Unix 版这么摆。
            for entry in fs::read_dir(&root).into_iter().flatten().flatten() {
                if holds_jvm(&entry.path()) {
                    return true;
                }
            }
        }
    }
    false
}

fn detect_image(home: &Path) -> JavaImage {
    if home.join("bin").join(javac_executable_name()).is_file() {
        JavaImage::Jdk
    } else {
        JavaImage::Jre
    }
}

fn javac_executable_name() -> &'static str {
    if cfg!(windows) { "javac.exe" } else { "javac" }
}

#[derive(Debug, Default)]
struct ReleaseFile {
    java_version: Option<String>,
    os_arch: Option<String>,
    implementor: Option<String>,
    image_type: Option<String>,
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
            "IMAGE_TYPE" => release.image_type = Some(value),
            "IMPLEMENTOR" => release.implementor = Some(value),
            _ => {}
        }
    }
    release.java_version.as_ref()?;
    Some(release)
}

fn ask_java_itself(executable: &Path) -> Result<(u16, String)> {
    let mut command = Command::new(executable);
    command.arg("-version");
    // 发现 Java 会挨个探测，每一次都是一个黑框闪一下。
    crate::process::without_console(&mut command);
    let output = command
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

/// 大版本之后的那一节：`1.8.0_402` → 402，`21.0.11` → 11，`17` → 0。
///
/// 两代版本号的写法完全不同（Java 8 把它写在下划线后面，9 之后写成第三段），
/// 但要回答的是同一个问题：**这份安装比某个已知的分界点新还是旧**。所以两代
/// 都归到一个数上，比较只在同一个大版本之内进行（见 [`UpdateCeiling`]）。
///
/// 带后缀的写法（`1.8.0_412-b08`、`21.0.5+11`）取到分隔符为止。
fn parse_update(version: &str) -> u32 {
    let version = version.trim();
    if let Some((_, update)) = version.split_once('_') {
        return leading_digits(update);
    }
    let version = version.strip_prefix("1.").unwrap_or(version);
    // 21.0.11 → 第三段；21 → 没有第三段，就是 0。
    version
        .split('.')
        .nth(2)
        .map(leading_digits)
        .unwrap_or_default()
}

fn leading_digits(text: &str) -> u32 {
    text.chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap_or_default()
}

/// 这份运行时带不带图形栈。
///
/// 只在 Linux 上判断：`-headless` 那种打包方式是发行版的做法，Windows 和
/// macOS 上的 JDK/JRE 一律完整。判据是 AWT 的那个原生库在不在——headless 包
/// 恰恰就是把它去掉了。找不到 `lib` 目录（不该发生）时不下结论：把一份能用
/// 的 Java 误判成 headless，比漏判更糟。
fn detect_headless(home: &Path) -> bool {
    if !cfg!(all(unix, not(target_os = "macos"))) {
        return false;
    }
    let mut looked = false;
    for base in [home.to_path_buf(), home.join("jre")] {
        let library = base.join("lib");
        if !library.is_dir() {
            continue;
        }
        looked = true;
        if library.join("libawt_xawt.so").is_file() {
            return false;
        }
        // JDK 8 的 Unix 版把架构也写进路径：`jre/lib/amd64/libawt_xawt.so`。
        for entry in fs::read_dir(&library).into_iter().flatten().flatten() {
            if entry.path().join("libawt_xawt.so").is_file() {
                return false;
            }
        }
    }
    looked
}

/// 归一化后的架构对应几位。
fn arch_bits(arch: &str) -> u16 {
    match arch {
        "x86" | "arm" => 32,
        _ => 64,
    }
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

/// 平台上惯例的安装位置，加上环境变量和（Windows 上）注册表指出来的那些。
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
        // 注册表是唯一能找到「装在非默认目录、又不在 PATH 上」那些 JDK 的
        // 办法，而 Windows 的安装器让人选目录，改到别的盘去很常见。
        homes.extend(crate::java::registry::java_homes());

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

/// 手动登记一个安装位置。
///
/// 先探一次再记：记下一个探不出版本的路径，等于在列表里放一行永远不会被
/// 选中的东西，而用户看不出为什么。
pub fn add_path(paths: &DataPaths, path: &Path) -> Result<JavaRuntime> {
    let mut runtime = probe(path)?;
    let home = runtime.home.clone();
    let mut settings = crate::data::settings::load(paths);
    if !settings.java.extra_paths.contains(&home) {
        settings.java.extra_paths.push(home);
        crate::data::settings::save(paths, &settings)?;
    }
    runtime.added = true;
    Ok(runtime)
}

/// 不再登记某个手动加进来的位置。只是从名单上划掉，不动磁盘上的任何东西。
pub fn forget_path(paths: &DataPaths, home: &Path) -> Result<()> {
    let mut settings = crate::data::settings::load(paths);
    let before = settings.java.extra_paths.len();
    settings.java.extra_paths.retain(|entry| entry != home);
    if settings.java.extra_paths.len() != before {
        crate::data::settings::save(paths, &settings)?;
    }
    Ok(())
}

/// 设置页那一节要显示的内容。
///
/// 按大版本分组，而不是平铺一串安装路径：用户的问题是「我缺什么」，平铺的
/// 列表只回答得了「我装了什么」。缺的那些也占一组，组里没有运行时——那一行
/// 正是要让人看见的。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaGroup {
    pub major: u16,
    /// 需要这个大版本的实例名。
    pub required_by: Vec<String>,
    /// 装在这台机器上的那些。空表示这一组还缺。
    pub runtimes: Vec<JavaRuntime>,
    /// 这一组里「自动」会挑中的那一份的 home。
    ///
    /// 同一个大版本装着两三份并不罕见（系统一份、Fern 下一份、手动登记一份），
    /// 而实例那一屏只说得出「会用 Java 21」。不指出是哪一个，两屏就对不上号。
    /// 判断用的是和启动同一个 [`select`]，不是另写一条规则。
    pub preferred: Option<PathBuf>,
}

/// 一个实例需要哪个大版本，以及这台机器上有没有。
///
/// 需求取自已经落盘的版本元数据（现在离线也读得到，见 metacache），拿不到时
/// 按版本号推——那时给出的是估计值，界面要说明。
pub fn overview(paths: &DataPaths, instances: &[crate::InstanceProfile]) -> Vec<JavaGroup> {
    let runtimes = discover(Some(paths));
    let mut groups: Vec<JavaGroup> = Vec::new();

    let group_for = |groups: &mut Vec<JavaGroup>, major: u16| -> usize {
        match groups.iter().position(|group| group.major == major) {
            Some(index) => index,
            None => {
                groups.push(JavaGroup {
                    major,
                    required_by: Vec::new(),
                    runtimes: Vec::new(),
                    preferred: None,
                });
                groups.len() - 1
            }
        }
    };

    for runtime in &runtimes {
        let index = group_for(&mut groups, runtime.major);
        groups[index].runtimes.push(runtime.clone());
    }

    for profile in instances {
        let declared = crate::declared_java_major(paths, profile);
        let requirement = requirement(&profile.game_version, profile.loader, declared);
        // 已经有能用的就记在那一组下面；一个都没有才记在「要装哪个」那一组，
        // 而要装的是下限——上限是我们避开已知坏组合用的，不是目标。
        let major = select(&runtimes, &requirement)
            .map(|runtime| runtime.major)
            .unwrap_or(requirement.minimum);
        let index = group_for(&mut groups, major);
        groups[index].required_by.push(profile.name.clone());
    }

    for group in &mut groups {
        // 「要是某个实例就要这个大版本，会用哪一份」——把区间夹死到这一档，
        // 剩下的交给启动时用的同一个选择函数。
        let requirement = JavaRequirement {
            minimum: group.major,
            maximum: Some(group.major),
            preferred: Some(group.major),
            ceiling: None,
        };
        group.preferred = select(&group.runtimes, &requirement).map(|runtime| runtime.home);
    }

    groups.sort_by_key(|group| group.major);
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime(major: u16, arch: &str, managed: bool) -> JavaRuntime {
        JavaRuntime {
            path: PathBuf::from(format!("/jvm/{major}/bin/java")),
            home: PathBuf::from(format!("/jvm/{major}")),
            major,
            update: 1,
            version: format!("{major}.0.1"),
            native: arch == normalize_arch(env::consts::ARCH),
            bits: arch_bits(arch),
            headless: false,
            arch: arch.to_owned(),
            vendor: "Test".to_owned(),
            managed,
            added: false,
            image: JavaImage::Jre,
            size_bytes: 0,
        }
    }

    #[test]
    fn the_overview_says_what_is_missing_not_just_what_is_installed() {
        use crate::{InstanceId, InstanceProfile};

        let root = env::temp_dir().join(format!("fern-java-overview-{}", std::process::id()));
        let paths = DataPaths::new(&root);
        let instances = vec![
            InstanceProfile::vanilla(InstanceId::parse("old").expect("id"), "旧世界", "1.12.2"),
            InstanceProfile::vanilla(InstanceId::parse("new").expect("id"), "新世界", "1.21.1"),
        ];

        let groups = overview(&paths, &instances);
        // 每个实例都必须落进某一组：落不进去就意味着界面上有一个实例的需求
        // 无人回答，而那正是这一节存在的理由。
        let named: Vec<&str> = groups
            .iter()
            .flat_map(|group| group.required_by.iter().map(String::as_str))
            .collect();
        assert!(named.contains(&"旧世界"));
        assert!(named.contains(&"新世界"));
        // 1.12.2 只能跑 Java 8，不会被算到别的组里去。
        let eight = groups.iter().find(|group| group.major == 8);
        assert!(eight.is_some_and(|group| group.required_by.contains(&"旧世界".to_owned())));
        // 组按大版本升序，界面不必自己再排一次。
        let majors: Vec<u16> = groups.iter().map(|group| group.major).collect();
        assert!(majors.windows(2).all(|pair| pair[0] < pair[1]));

        let _ = fs::remove_dir_all(&root);
    }

    /// 同一个大版本装着好几份是常事。实例那一屏只说得出「会用 Java 21」，
    /// 设置页要指得出是哪一份——两处对不上号，用户就没法判断该删哪一个。
    #[test]
    fn a_group_points_at_the_one_that_would_actually_be_used() {
        let mut group = JavaGroup {
            major: 21,
            required_by: Vec::new(),
            runtimes: vec![
                runtime(21, env::consts::ARCH, false),
                runtime(21, env::consts::ARCH, true),
            ],
            preferred: None,
        };
        // 两份都能用时，挑我们管得住的那一份——和启动时用的是同一个 select。
        group.runtimes[1].home = PathBuf::from("/fern/runtimes/21");
        let requirement = JavaRequirement {
            minimum: 21,
            maximum: Some(21),
            preferred: Some(21),
            ceiling: None,
        };
        assert_eq!(
            select(&group.runtimes, &requirement).map(|runtime| runtime.home),
            Some(PathBuf::from("/fern/runtimes/21"))
        );
        // 一份都没装的那一组不该指向任何东西——「缺」正是要让人看见的状态。
        assert_eq!(select(&[], &requirement), None);
    }

    #[test]
    fn parses_both_version_generations() {
        assert_eq!(parse_major("21.0.11"), Some(21));
        assert_eq!(parse_major("1.8.0_402"), Some(8));
        assert_eq!(parse_major("17"), Some(17));
        assert_eq!(parse_major("not a version"), None);
    }

    /// 两代版本号把 update 写在不同的位置，但要回答的是同一个问题。
    #[test]
    fn the_update_number_comes_out_of_both_layouts() {
        assert_eq!(parse_update("1.8.0_402"), 402);
        assert_eq!(parse_update("1.8.0_412-b08"), 412);
        assert_eq!(parse_update("1.8.0"), 0);
        assert_eq!(parse_update("21.0.11"), 11);
        assert_eq!(parse_update("21.0.5+11"), 5);
        assert_eq!(parse_update("17"), 0);
        assert_eq!(parse_update("说不清"), 0);
    }

    /// 大版本对了不等于能用。1.16.5 的 Forge 在 8u321 之后崩在
    /// `NoSuchMethodError`，而「要 Java 8」这个条件它完全满足。
    #[test]
    fn a_system_java_8_that_is_too_new_is_not_a_candidate() {
        let native = normalize_arch(env::consts::ARCH);
        let mut modern = runtime(8, native, false);
        modern.version = "1.8.0_492".to_owned();
        modern.update = 492;
        let mut legacy = runtime(8, native, true);
        legacy.version = "1.8.0_202".to_owned();
        legacy.update = 202;
        legacy.home = PathBuf::from("/fern/runtimes/jre-legacy");

        // 这条线由事前兼容规则给出（它才看得到加载器版本），这里只验它一旦
        // 加上就真的起作用。
        let capped =
            requirement("1.16.5", LoaderKind::Forge, Some(8)).capped(Some(UpdateCeiling {
                major: 8,
                update: 320,
            }));
        // 只有那一份系统 Java 的话，一个候选都挑不出来——补全据此去下
        // jre-legacy，而不是拿手边这个凑。
        assert!(select(std::slice::from_ref(&modern), &capped).is_none());
        assert_eq!(
            select(&[modern.clone(), legacy.clone()], &capped)
                .expect("jre-legacy 能用")
                .home,
            PathBuf::from("/fern/runtimes/jre-legacy")
        );

        // 没有规则说话的时候不该凭空多出一条线。
        let untouched = requirement("1.12.2", LoaderKind::Forge, Some(8));
        assert!(untouched.ceiling.is_none());
        assert!(select(std::slice::from_ref(&modern), &untouched).is_some());
    }

    /// 不带图形栈的那一份能被认出来，而且排在能用的那些后面。
    #[test]
    fn a_headless_runtime_is_the_last_resort() {
        let native = normalize_arch(env::consts::ARCH);
        let mut headless = runtime(8, native, true);
        headless.headless = true;
        headless.home = PathBuf::from("/jvm/headless");
        let full = runtime(8, native, false);

        let wanted = requirement("1.12.2", LoaderKind::Vanilla, Some(8));
        assert_eq!(
            select(&[headless.clone(), full.clone()], &wanted)
                .expect("有得挑")
                .home,
            full.home
        );
        // 只有它的时候还是要用：挑不出 Java 就去下一份两百兆的，而下回来的
        // 那份未必更好——这件事该由预检查说清楚，不是在这里拦住。
        assert_eq!(
            select(std::slice::from_ref(&headless), &wanted)
                .expect("只剩它")
                .home,
            headless.home
        );
    }

    /// 一份完好的安装：启动器、版本文件、虚拟机，三样都在。
    fn install_fake_java(home: &Path, version: &str, arch: &str) {
        fs::create_dir_all(home.join("bin")).expect("create bin");
        fs::write(home.join("bin").join(java_executable_name()), b"")
            .expect("write java executable");
        fs::write(
            home.join("release"),
            format!("JAVA_VERSION=\"{version}\"\nOS_ARCH=\"{arch}\"\n"),
        )
        .expect("write release file");
        let server = home.join("lib").join("server");
        fs::create_dir_all(&server).expect("create server directory");
        fs::write(server.join(JVM_LIBRARY), b"").expect("write jvm library");
    }

    #[test]
    fn discovers_a_macos_runtime_bundle_nested_under_its_install_root() {
        let root = env::temp_dir().join(format!("fern-java-bundle-{}", std::process::id()));
        let home = root.join("jre.bundle/Contents/Home");
        install_fake_java(&home, "25.0.1", "aarch64");

        let runtime = probe_home(&root, true).expect("discover nested bundle");
        assert_eq!(runtime.major, 25);
        assert_eq!(runtime.home, home);
        assert!(runtime.managed);

        fs::remove_dir_all(root).expect("remove bundle");
    }

    /// 下到一半的运行时不能被当成一个能用的 Java。
    ///
    /// 它是真出现过的：`bin/java.exe` 和 `release` 都在，虚拟机没下完，于是
    /// 我们把它当成 Java 21 交给游戏，游戏报了一句和 Java 毫无字面关系的
    /// 「missing `server' JVM」。而它还因为是自己下的而优先于系统里那个好的。
    #[test]
    fn an_unfinished_download_is_not_a_usable_java() {
        let root = env::temp_dir().join(format!("fern-java-partial-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let home = root.join("java-runtime-delta");
        install_fake_java(&home, "21.0.9", env::consts::ARCH);
        // 虚拟机没下完，剩下的都在。
        fs::remove_file(home.join("lib").join("server").join(JVM_LIBRARY)).expect("remove jvm");

        assert!(probe_home(&home, true).is_none());
        assert!(probe(&home).is_err());
        // 说的是「不完整」，不是「这不是 Java 目录」——一个要重下，一个要换路径。
        let complaint = probe(&home).expect_err("incomplete").to_string();
        assert!(complaint.contains(JVM_LIBRARY), "{complaint}");

        // 补齐之后就该认得出来。
        fs::write(home.join("lib").join("server").join(JVM_LIBRARY), b"").expect("write jvm");
        assert_eq!(probe_home(&home, true).expect("complete").major, 21);

        fs::remove_dir_all(root).expect("remove root");
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

    /// 模组要求的下界把选择往上抬，但抬不动游戏自己的上限。
    #[test]
    fn the_mods_lower_bound_moves_the_choice_up_but_not_past_the_ceiling() {
        let native = normalize_arch(env::consts::ARCH);
        let runtimes = vec![
            runtime(17, native, false),
            runtime(21, native, false),
            runtime(25, native, false),
        ];

        // 平时挑区间里最老的那个；有模组要 25 就挑 25。
        let modern = requirement("1.21.5", LoaderKind::Vanilla, Some(21));
        assert_eq!(select(&runtimes, &modern).expect("match").major, 21);
        assert_eq!(
            select(&runtimes, &modern.preferring(Some(25)))
                .expect("match")
                .major,
            25
        );

        // 够不着就退回原来的选择：模组的要求由预检查去说，不是拦住启动的理由。
        assert_eq!(
            select(&runtimes, &modern.preferring(Some(99)))
                .expect("match")
                .major,
            21
        );

        // 上限是「已知这个组合会坏」，一个模组想要更新的 Java 推翻不了它。
        let old = requirement("1.20.1", LoaderKind::Vanilla, Some(17));
        assert_eq!(
            select(&runtimes, &old.preferring(Some(25)))
                .expect("match")
                .major,
            17
        );
    }

    /// 区间外的不是「次一等的候选」，是不能用。挑不出来就该返回 `None`，
    /// 让补全那一步去下一个对的——上一版把上限当偏好，一台只装了 Java 11 的
    /// 机器上 1.7.2 就选中了 11，`ensure_java` 看见「已经有能用的」再也不去
    /// 下 Java 8，而设置页一边显示自动选中 11、一边把它列进不兼容。
    #[test]
    fn selection_stays_inside_the_interval_at_both_ends() {
        let native = normalize_arch(env::consts::ARCH);
        let runtimes = vec![runtime(8, native, false), runtime(17, native, false)];
        // 太旧：1.21.1 要 21。
        assert!(
            select(
                &runtimes,
                &requirement("1.21.1", LoaderKind::Vanilla, Some(21))
            )
            .is_none()
        );
        // 太新：1.7.2 只能跑在 Java 8 上，手上这两个都不行。
        assert!(
            select(
                &[runtime(11, native, false), runtime(17, native, false)],
                &requirement("1.7.2", LoaderKind::Forge, None)
            )
            .is_none()
        );
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
