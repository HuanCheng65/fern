//! GC 决策树与参数集生成（设计文档 §7）。
//!
//! 一条贯穿全文件的规矩：**用户写过的开关，我们一个都不碰。** 检测到用户自己
//! 给了 `-Xmx`，自动分配整个让位；检测到任何一个 GC 旗标，整棵决策树让位。
//! 两个 `-XX:+Use*GC` 撞在一起 JVM 直接拒绝启动，而那时候用户看到的只有一句
//! 「Could not create the Java Virtual Machine」——他不会想到是启动器加的。
//!
//! 另一条：**26.1 起的原版不干预。** Mojang 自己已经完成了 ZGC 时代的默认参数
//! 调校（4 G 默认堆、分代 ZGC、UseCompactObjectHeaders），版本 JSON 里带着的
//! 那份就是最优解，我们只按需覆盖 `-Xmx`。

use super::signals::Era;

/// 这次启动走哪条 GC 路径。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GcPath {
    /// 不干预：沿用版本 JSON 自带的参数，或用户已经自己选好了。
    #[default]
    Untouched,
    /// 分代 ZGC。
    Zgc,
    /// G1 加一组客户端向的参数。
    G1,
}

impl GcPath {
    pub fn is_zgc(self) -> bool {
        self == Self::Zgc
    }

    /// 自适应层的系数走哪一档。
    ///
    /// 不干预路径按 ZGC 算：26.1+ 的默认收集器就是 ZGC。
    pub fn behaves_like_zgc(self) -> bool {
        self != Self::G1
    }
}

/// 这台机器上的系统版本，用来判断 ZGC 跑不跑得动。
///
/// ZGC 要求 Windows 10 1809 以上。低于它的机器上 JVM 会在启动时直接失败，
/// 所以这个判断必须在参数生成阶段做完，不能等到进程起不来。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Platform {
    pub zgc_supported: bool,
}

impl Platform {
    pub fn probe() -> Self {
        Self {
            zgc_supported: zgc_supported(),
        }
    }
}

#[cfg(windows)]
fn zgc_supported() -> bool {
    // `os_version()` 在 Windows 上是 `major.minor.build`，build 17763 就是
    // 1809。Win7 报的是 `6.1.7601`，同样被这条比较挡下。读不出来就当支持：
    // 一个读不到版本号的 Windows 十有八九是新的。
    crate::launch::rules::os_version()
        .split('.')
        .nth(2)
        .and_then(|build| build.parse::<u32>().ok())
        .is_none_or(|build| build >= 17763)
}

#[cfg(not(windows))]
fn zgc_supported() -> bool {
    true
}

/// 用户或版本元数据已经把堆钉死了。
pub fn heap_is_pinned(arguments: &[String]) -> bool {
    arguments
        .iter()
        .any(|argument| argument.starts_with("-Xmx") || argument.starts_with("-XX:MaxHeapSize"))
}

/// 用户或版本元数据已经选了收集器。
///
/// 认得比 `-XX:+Use*GC` 宽一点：`-XX:+UseZGC` 之外还有 `-XX:-UseG1GC` 这种
/// 关掉某一个的写法，以及 ZGC 的分代开关。碰上任何一个都当作「他知道自己在
/// 干什么」。
pub fn collector_is_pinned(arguments: &[String]) -> bool {
    arguments.iter().any(|argument| {
        let flag = argument.trim_start_matches("-XX:");
        let flag = flag.strip_prefix(['+', '-']).unwrap_or(flag);
        flag.ends_with("GC") && flag.starts_with("Use") || flag.starts_with("ZGenerational")
    })
}

/// 走哪条路（§7.1）。
///
/// `existing` 是版本元数据和用户参数合起来的那一份——两边任何一处已经表态，
/// 整棵树就让位。
pub fn choose(
    era: Era,
    modded: bool,
    java_major: u16,
    platform: Platform,
    existing: &[String],
) -> GcPath {
    if collector_is_pinned(existing) {
        return GcPath::Untouched;
    }
    // 26.1 起的原版：Mojang 的默认参数就是调校过的，别插手。
    if era == Era::YearDrop && !modded {
        return GcPath::Untouched;
    }
    // 非分代 ZGC 在客户端有实测的 FPS 损失，所以 21 以下一律 G1。
    if java_major >= 21 && platform.zgc_supported {
        GcPath::Zgc
    } else {
        GcPath::G1
    }
}

/// 这条路径要加的参数。
pub fn arguments(path: GcPath, java_major: u16) -> Vec<String> {
    let mut arguments = match path {
        GcPath::Untouched => Vec::new(),
        GcPath::Zgc => {
            let mut zgc = vec!["-XX:+UseZGC".to_owned()];
            if java_major < 23 {
                // 21 与 22 上分代 ZGC 要显式打开；23 起它就是默认，再给这个
                // 开关只会收到一条 deprecated 警告。
                zgc.push("-XX:+ZGenerational".to_owned());
            }
            zgc
        }
        GcPath::G1 => vec![
            // G1NewSizePercent 到今天仍然是实验选项（Java 21、25 上都拒绝
            // 启动），而且解锁开关必须排在它前面。少了这一行，游戏根本起不
            // 来——「Could not create the Java Virtual Machine」。
            "-XX:+UnlockExperimentalVMOptions".to_owned(),
            "-XX:+UseG1GC".to_owned(),
            "-XX:G1NewSizePercent=20".to_owned(),
            "-XX:G1ReservePercent=20".to_owned(),
            "-XX:G1HeapRegionSize=32M".to_owned(),
            // 37 来自 brucethemoose 的客户端基准：更频繁但感知不到的短停顿，
            // 优于默认 50 下偶发的长停顿。这是客户端的取舍，不是服务端的。
            "-XX:MaxGCPauseMillis=37".to_owned(),
            "-XX:+PerfDisableSharedMem".to_owned(),
        ],
    };

    if path == GcPath::G1 && java_major >= 12 {
        // 允许堆收缩：客户端会被挂到后台，那时候内存该还给系统。这和
        // 「只设 Xmx 不设 Xms」是同一套哲学——堆保持弹性。
        arguments.push("-XX:MinHeapFreeRatio=25".to_owned());
        arguments.push("-XX:MaxHeapFreeRatio=40".to_owned());
    }
    if path != GcPath::Untouched && java_major >= 24 {
        // 对象头从 12/16 字节压到 8，堆里省下的是实打实的。24 起可用。
        arguments.push("-XX:+UseCompactObjectHeaders".to_owned());
    }
    arguments
}

/// 和性能无关、但必须带的那些（§7.4）。
pub fn safety_arguments(java_major: u16, existing: &[String]) -> Vec<String> {
    let mut arguments = Vec::new();
    // Log4Shell。受影响的是 1.7–1.18.1，但这个开关在任何版本上都无害，
    // 与其维护一张版本表，不如一直带着。
    if !existing
        .iter()
        .any(|argument| argument.contains("log4j2.formatMsgNoLookups"))
    {
        arguments.push("-Dlog4j2.formatMsgNoLookups=true".to_owned());
    }
    // Java 18 起默认字符集变成 UTF-8，而一批老 Mod 的配置文件是按平台编码
    // 写的，读出来就是乱码。21 起 `COMPAT` 已被移除，那时候只能靠 Mod 自己
    // 修好——所以这一段只对 18–20 生效。
    if (18..=20).contains(&java_major)
        && !existing
            .iter()
            .any(|argument| argument.starts_with("-Dfile.encoding"))
    {
        arguments.push("-Dfile.encoding=COMPAT".to_owned());
    }
    arguments
}

/// 只设 Xmx，不设 Xms（§7.3）。
///
/// `Xms=Xmx` 加预触碰是服务端独占机器的逻辑：那里内存压力恒定，启动一次跑
/// 几个月。客户端的压力多变、玩家会挂后台，把堆一次性吃满只是让别的程序更
/// 早开始换页。Mojang 在 26.1 之后的快照里主动调低了初始堆，判断一致。
pub fn heap_argument(megabytes: u32) -> String {
    format!("-Xmx{megabytes}M")
}

#[cfg(test)]
mod tests {
    use super::*;

    const DESKTOP: Platform = Platform {
        zgc_supported: true,
    };

    #[test]
    fn a_collector_the_user_already_chose_stops_the_whole_tree() {
        for pinned in [
            "-XX:+UseZGC",
            "-XX:+UseSerialGC",
            "-XX:-UseG1GC",
            "-XX:+ZGenerational",
        ] {
            assert_eq!(
                choose(Era::Modern, true, 21, DESKTOP, &[pinned.to_owned()]),
                GcPath::Untouched,
                "{pinned} should have silenced the tree"
            );
        }
        // 不相关的参数不该挡住默认值。
        assert_ne!(
            choose(Era::Modern, true, 21, DESKTOP, &["-Xmx4G".to_owned()]),
            GcPath::Untouched
        );
    }

    #[test]
    fn modern_vanilla_keeps_mojangs_own_tuning() {
        assert_eq!(
            choose(Era::YearDrop, false, 25, DESKTOP, &[]),
            GcPath::Untouched
        );
        // 但装了加载器就不一样了：那份默认参数是按原版调的。
        assert_eq!(choose(Era::YearDrop, true, 25, DESKTOP, &[]), GcPath::Zgc);
    }

    #[test]
    fn zgc_only_where_it_is_generational_and_supported() {
        assert_eq!(choose(Era::Modern, true, 21, DESKTOP, &[]), GcPath::Zgc);
        // 20 及以下只有非分代 ZGC，客户端上实测掉帧，走 G1。
        assert_eq!(choose(Era::Modern, true, 20, DESKTOP, &[]), GcPath::G1);
        assert_eq!(choose(Era::Legacy, false, 8, DESKTOP, &[]), GcPath::G1);
        // 系统太老跑不了 ZGC。
        let old_windows = Platform {
            zgc_supported: false,
        };
        assert_eq!(choose(Era::Modern, true, 21, old_windows, &[]), GcPath::G1);
    }

    #[test]
    fn experimental_flags_are_unlocked_before_they_are_used() {
        // 端到端跑出来的教训：少了解锁开关，JVM 直接拒绝启动，而且解锁必须
        // 排在实验选项**前面**——顺序错了报的是一样的错。
        let arguments = arguments(GcPath::G1, 21);
        let unlock = arguments
            .iter()
            .position(|argument| argument == "-XX:+UnlockExperimentalVMOptions")
            .expect("实验选项必须先解锁");
        for (index, argument) in arguments.iter().enumerate() {
            if argument.starts_with("-XX:G1NewSizePercent") {
                assert!(index > unlock, "解锁开关必须排在实验选项前面");
            }
        }
    }

    #[test]
    fn zgc_does_not_drag_the_g1_tuning_along() {
        let z = arguments(GcPath::Zgc, 21);
        assert!(z.contains(&"-XX:+UseZGC".to_owned()));
        assert!(z.contains(&"-XX:+ZGenerational".to_owned()));
        assert!(!z.iter().any(|flag| flag.contains("G1")));
        assert!(!z.iter().any(|flag| flag.contains("UnlockExperimental")));
        // 23 起分代是默认，再给开关只会收到 deprecated 警告。
        assert!(!arguments(GcPath::Zgc, 25).contains(&"-XX:+ZGenerational".to_owned()));
    }

    #[test]
    fn an_untouched_path_adds_nothing_at_all() {
        assert!(arguments(GcPath::Untouched, 25).is_empty());
    }

    #[test]
    fn compact_object_headers_only_where_they_exist() {
        assert!(
            arguments(GcPath::Zgc, 25)
                .iter()
                .any(|flag| flag.contains("UseCompactObjectHeaders"))
        );
        assert!(
            !arguments(GcPath::Zgc, 21)
                .iter()
                .any(|flag| flag.contains("UseCompactObjectHeaders"))
        );
    }

    #[test]
    fn the_log4shell_switch_is_not_added_twice() {
        assert!(safety_arguments(21, &[]).contains(&"-Dlog4j2.formatMsgNoLookups=true".to_owned()));
        assert!(
            safety_arguments(21, &["-Dlog4j2.formatMsgNoLookups=false".to_owned()])
                .iter()
                .all(|flag| !flag.contains("formatMsgNoLookups"))
        );
    }

    #[test]
    fn a_pinned_heap_is_recognised_in_both_spellings() {
        assert!(heap_is_pinned(&["-Xmx8G".to_owned()]));
        assert!(heap_is_pinned(&["-XX:MaxHeapSize=8g".to_owned()]));
        assert!(!heap_is_pinned(&["-Xms1G".to_owned()]));
    }
}
