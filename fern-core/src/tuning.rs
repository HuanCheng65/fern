//! 性能默认值（文档 §6）。
//!
//! 原则是「默认值做好比堆开关重要，克制」。所以这里只决定两件事：给多少内存，
//! 用什么 GC。其余的（进程优先级、独显选择、ZGC）留到有人真的报了问题再说——
//! 每多一个开关，用户就多一次「这个该不该动」的犹豫。
//!
//! 目前启动器一个 `-Xmx` 都没给，游戏跑在 JVM 的默认堆上（物理内存的 1/4，
//! 但很多环境下会被容器或 ergonomics 压到几百兆），大型整合包必然 OOM。

use std::path::Path;

/// 一个实例的 mods 目录长什么样。整合包的规模只能从这里看出来。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ModsProfile {
    pub count: u32,
    pub bytes: u64,
}

const MEGABYTE: u64 = 1024 * 1024;

/// 决定 `-Xmx`，单位 MB。
///
/// 基线是物理内存的四分之一、不低于 2 G，上限是物理内存的一半——给游戏留下
/// 的不能比留给系统的多，否则换页带来的卡顿比堆不够还难受。
///
/// `manual` 是实例设置里的滑杆。用户明确要了就照做，只在上限那里拦一下：
/// 填一个超过物理内存的数，游戏会直接起不来，那不是他想要的结果。
pub fn heap_megabytes(physical_bytes: Option<u64>, mods: ModsProfile, manual: Option<u32>) -> u32 {
    // 读不到物理内存时按 8 G 算：这是现在最常见的配置，猜错的代价也只是
    // 默认值不够贴合，用户还能在设置里改。
    let physical_mb = physical_bytes.map_or(8192, |bytes| (bytes / MEGABYTE) as u32);
    let ceiling = (physical_mb / 2).max(2048);

    if let Some(manual) = manual {
        return manual.clamp(512, ceiling);
    }

    let baseline = (physical_mb / 4).max(2048);
    // 整合包的内存需求和 mod 数量、体积都相关，两个指标取更高的那一档：
    // 一百来个小 mod 和二十个大 mod 都能把堆撑满。
    let for_mods = if mods.count >= 180 || mods.bytes >= 1024 * MEGABYTE {
        8192
    } else if mods.count >= 80 || mods.bytes >= 400 * MEGABYTE {
        6144
    } else {
        0
    };

    baseline.max(for_mods).min(ceiling)
}

/// GC 参数。
///
/// G1 加一组温和的参数就够了——这是客户端场景，目标是别卡顿，不是吞吐量。
/// Aikar flags 是给服务端调的，不照搬。
///
/// 只对 Java 17 以上生效：Java 8 上 `G1NewSizePercent` 要先解锁实验选项，
/// 为了几个参数去动 `-XX:+UnlockExperimentalVMOptions` 不划算，而跑 Java 8
/// 的都是老版本，本来也不吃内存。
pub fn gc_arguments(
    java_major: u16,
    collector: crate::GarbageCollector,
    existing: &[String],
) -> Vec<String> {
    if java_major < 17 {
        return Vec::new();
    }
    // 用户或元数据已经选了收集器就别插手——两个 -XX:+Use*GC 撞在一起，JVM
    // 直接拒绝启动。
    if existing
        .iter()
        .any(|argument| argument.starts_with("-XX:+Use") && argument.ends_with("GC"))
    {
        return Vec::new();
    }
    match collector {
        crate::GarbageCollector::G1 => vec![
            // G1NewSizePercent 到今天仍然是实验选项（Java 21、25 上都拒绝
            // 启动），而且解锁开关必须排在它前面。少了这一行，游戏根本起不
            // 来——「Could not create the Java Virtual Machine」。
            "-XX:+UnlockExperimentalVMOptions".to_owned(),
            "-XX:+UseG1GC".to_owned(),
            "-XX:G1NewSizePercent=20".to_owned(),
            "-XX:G1ReservePercent=20".to_owned(),
            "-XX:MaxGCPauseMillis=50".to_owned(),
        ],
        // ZGC 自 JDK 15 起就是正式选项，不需要解锁；也不该配 G1 那组参数。
        crate::GarbageCollector::Z => vec!["-XX:+UseZGC".to_owned()],
    }
}

/// 元数据和用户都没说堆大小时，我们才给。
pub fn heap_argument(existing: &[String], megabytes: u32) -> Option<String> {
    if existing.iter().any(|argument| argument.starts_with("-Xmx")) {
        return None;
    }
    Some(format!("-Xmx{megabytes}M"))
}

/// 扫一眼 mods 目录。不递归：子目录里的东西加载器本来也不读。
pub fn mods_profile(game_directory: &Path) -> ModsProfile {
    let mut profile = ModsProfile::default();
    let Ok(entries) = std::fs::read_dir(game_directory.join("mods")) else {
        return profile;
    };
    for entry in entries.flatten() {
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        // 停用的 mod 常见做法是加 `.disabled` 后缀，别把它算进规模里。
        if entry.path().extension().is_none_or(|ext| ext != "jar") {
            continue;
        }
        profile.count += 1;
        profile.bytes += metadata.len();
    }
    profile
}

/// 物理内存字节数。读不到返回 `None`，调用方自己决定怎么猜。
pub fn physical_memory_bytes() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        // MemTotal 那一行的单位是 kB，内核一直这么写。
        let text = std::fs::read_to_string("/proc/meminfo").ok()?;
        let kilobytes: u64 = text
            .lines()
            .find_map(|line| line.strip_prefix("MemTotal:"))?
            .split_whitespace()
            .next()?
            .parse()
            .ok()?;
        Some(kilobytes * 1024)
    }

    #[cfg(target_os = "macos")]
    {
        // 没有 /proc，也不想为一个数引一个平台依赖；sysctl 一辈子只跑一次。
        let output = std::process::Command::new("sysctl")
            .args(["-n", "hw.memsize"])
            .output()
            .ok()?;
        String::from_utf8_lossy(&output.stdout).trim().parse().ok()
    }

    #[cfg(windows)]
    {
        use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        // SAFETY: dwLength 按文档填好后，GlobalMemoryStatusEx 只写这一个结构体。
        unsafe {
            let mut status: MEMORYSTATUSEX = std::mem::zeroed();
            status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
            if GlobalMemoryStatusEx(&mut status) != 0 {
                Some(status.ullTotalPhys)
            } else {
                None
            }
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIGABYTE: u64 = 1024 * MEGABYTE;

    #[test]
    fn baseline_is_a_quarter_of_physical_memory_with_a_two_gig_floor() {
        assert_eq!(
            heap_megabytes(Some(16 * GIGABYTE), ModsProfile::default(), None),
            4096
        );
        // 4 G 的机器算出来是 1 G，抬到下限。
        assert_eq!(
            heap_megabytes(Some(4 * GIGABYTE), ModsProfile::default(), None),
            2048
        );
    }

    #[test]
    fn never_hands_the_game_more_than_half_the_machine() {
        // 2 G 的机器：下限 2 G 和上限 1 G 冲突，上限自己也有个 2 G 的底，
        // 否则算出来的堆会小到游戏根本起不来。
        assert_eq!(
            heap_megabytes(Some(2 * GIGABYTE), ModsProfile::default(), None),
            2048
        );
        // 大整合包想要 8 G，但机器只有 8 G，只能给 4 G。
        let heavy = ModsProfile {
            count: 300,
            bytes: 2 * GIGABYTE,
        };
        assert_eq!(heap_megabytes(Some(8 * GIGABYTE), heavy, None), 4096);
    }

    #[test]
    fn large_modpacks_get_more_than_the_baseline() {
        let plain = ModsProfile::default();
        let medium = ModsProfile {
            count: 120,
            bytes: 300 * MEGABYTE,
        };
        let large = ModsProfile {
            count: 250,
            bytes: 1500 * MEGABYTE,
        };
        assert_eq!(heap_megabytes(Some(32 * GIGABYTE), plain, None), 8192);
        assert_eq!(heap_megabytes(Some(32 * GIGABYTE), medium, None), 8192);
        assert_eq!(heap_megabytes(Some(32 * GIGABYTE), large, None), 8192);
        // 在小内存机器上才看得出档位的差别。
        assert_eq!(heap_megabytes(Some(16 * GIGABYTE), plain, None), 4096);
        assert_eq!(heap_megabytes(Some(16 * GIGABYTE), medium, None), 6144);
        assert_eq!(heap_megabytes(Some(16 * GIGABYTE), large, None), 8192);
    }

    #[test]
    fn a_manual_setting_wins_but_still_cannot_exceed_the_machine() {
        assert_eq!(
            heap_megabytes(Some(16 * GIGABYTE), ModsProfile::default(), Some(3072)),
            3072
        );
        // 32 G 的实例设置装不进 16 G 的机器。
        assert_eq!(
            heap_megabytes(Some(16 * GIGABYTE), ModsProfile::default(), Some(32768)),
            8192
        );
    }

    #[test]
    fn gc_flags_stay_out_of_the_way_of_an_existing_collector() {
        assert!(!gc_arguments(21, crate::GarbageCollector::G1, &[]).is_empty());
        assert!(gc_arguments(8, crate::GarbageCollector::G1, &[]).is_empty());
        assert!(
            gc_arguments(21, crate::GarbageCollector::G1, &["-XX:+UseZGC".to_owned()]).is_empty()
        );
        assert!(
            gc_arguments(
                21,
                crate::GarbageCollector::G1,
                &["-XX:+UseSerialGC".to_owned()]
            )
            .is_empty()
        );
        // 不相关的参数不该挡住默认值。
        assert!(!gc_arguments(21, crate::GarbageCollector::G1, &["-Xmx4G".to_owned()]).is_empty());
    }

    #[test]
    fn experimental_flags_are_unlocked_before_they_are_used() {
        // 端到端跑出来的教训：少了解锁开关，JVM 直接拒绝启动，而且解锁必须
        // 排在实验选项**前面**——顺序错了报的是一样的错。
        let arguments = gc_arguments(21, crate::GarbageCollector::G1, &[]);
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
        let z = gc_arguments(21, crate::GarbageCollector::Z, &[]);
        assert_eq!(z, vec!["-XX:+UseZGC"]);
        // ZGC 自 JDK 15 起是正式选项，不该再要求解锁实验开关。
        assert!(!z.iter().any(|a| a.contains("UnlockExperimental")));
        assert!(!z.iter().any(|a| a.contains("G1")));
    }

    #[test]
    fn heap_argument_defers_to_whatever_is_already_there() {
        assert_eq!(heap_argument(&[], 4096).as_deref(), Some("-Xmx4096M"));
        assert_eq!(heap_argument(&["-Xmx8G".to_owned()], 4096), None);
    }

    #[test]
    fn this_machine_reports_a_plausible_amount_of_memory() {
        let bytes = physical_memory_bytes().expect("physical memory should be readable here");
        assert!(bytes > GIGABYTE, "{bytes} bytes is implausibly small");
    }
}
