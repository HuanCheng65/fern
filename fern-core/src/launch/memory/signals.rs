//! 输入信号（设计文档 §4）。
//!
//! 全部是本地零成本的读取：读一次 `/proc/meminfo`、数一遍 `mods/`、解析一行
//! `options.txt`。**没有任何一条信号要联网**——分配决策发生在点下启动之后、
//! 进程起来之前，那条路径上不能有网络往返。
//!
//! 被砍掉的信号也记在这里，免得以后有人再想一遍：按 Modrinth 元数据给「重型
//! Mod」单独加权（要指纹匹配和网络往返，而它修正的误差在自适应层一个 session
//! 之后就被覆盖了）、扫材质包 PNG 分辨率（压力在显存不在堆，为此解压 zip 不
//! 值得）。

use std::path::Path;

/// 一个实例的 mods 目录长什么样。整合包的规模只能从这里看出来。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModsProfile {
    pub count: u32,
    pub bytes: u64,
}

pub const MEGABYTE: u64 = 1024 * 1024;

/// 显存从哪来。
///
/// 核显和统一内存把显存直接从系统内存里切走，同一台机器上能给堆的就更少。
/// 探测不到就是 `Unknown`——那时候**不**额外保留：宁可少留一点，也不要因为
/// 一个猜出来的结论把堆压小。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Graphics {
    /// 核显 / 统一内存：显存吃的是系统内存。
    Shared,
    /// 独显：显存是另一块。
    Dedicated,
    #[default]
    Unknown,
}

/// 这台机器现在什么样。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Machine {
    pub total_bytes: Option<u64>,
    /// 此刻可用的物理内存。这是「现在实际能给多少」的依据——只看总量的方案
    /// 会在用户开着浏览器和 IDE 的时候把系统压进 swap。
    pub available_bytes: Option<u64>,
    pub graphics: Graphics,
}

impl Machine {
    pub fn probe() -> Self {
        Self {
            total_bytes: physical_memory_bytes(),
            available_bytes: available_memory_bytes(),
            graphics: graphics(),
        }
    }

    pub fn total_mb(&self) -> u32 {
        megabytes(self.total_bytes).unwrap_or(8192)
    }

    /// 可用内存，MB。读不到就退回总量的一半——这是「机器上还跑着别的东西」
    /// 的一个保守假设，比当作全部可用安全。
    pub fn available_mb(&self) -> u32 {
        megabytes(self.available_bytes).unwrap_or_else(|| self.total_mb() / 2)
    }
}

fn megabytes(bytes: Option<u64>) -> Option<u32> {
    bytes.map(|bytes| (bytes / MEGABYTE) as u32)
}

/// 版本世代。
///
/// 原版基线在这几段之间是真的不一样：1.17 的 Java 16 迁移与 1.18 的世界高度
/// 扩展各抬高了一截，而 26.1 起 Mojang 自己把默认堆定在了 4 G。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Era {
    /// ≤ 1.12.2
    Legacy,
    /// 1.13 – 1.16.5
    Transitional,
    /// 1.17 – 1.21.x
    #[default]
    Modern,
    /// 26.1 及以后的 year.drop.hotfix 版本号。
    YearDrop,
}

/// 从版本号认世代。
///
/// 认不出来的按 `Modern` 算：认错方向的代价不对称——把新版本当老版本会把堆
/// 压小，而老版本多给一点内存什么也不会发生。
///
/// 快照（`24w14a`）只能按年份粗分，因为一年里的快照横跨两个正式版。这个粗糙
/// 是可以接受的：它只影响第一次启动的初值，之后由实测数据接管。
pub fn era(game_version: &str) -> Era {
    let core = game_version
        .split(['-', '+'])
        .next()
        .unwrap_or(game_version)
        .trim();

    if let Some((year, rest)) = core.split_once('w')
        && rest.chars().next().is_some_and(|c| c.is_ascii_digit())
        && let Ok(year) = year.parse::<u32>()
    {
        return match year {
            ..=16 => Era::Legacy,
            17..=20 => Era::Transitional,
            21..=25 => Era::Modern,
            _ => Era::YearDrop,
        };
    }

    let mut parts = core.split('.');
    let Some(Ok(major)) = parts.next().map(str::parse::<u32>) else {
        return Era::Modern;
    };
    if major != 1 {
        // 新的 year.drop 编号。26.1 是第一个，比它早的两位数编号不存在。
        return if major >= 26 {
            Era::YearDrop
        } else {
            Era::Modern
        };
    }
    match parts.next().and_then(|minor| minor.parse::<u32>().ok()) {
        Some(..=12) => Era::Legacy,
        Some(13..=16) => Era::Transitional,
        _ => Era::Modern,
    }
}

/// 这个实例要跑的东西有多重。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workload {
    pub era: Era,
    /// 装了加载器。原版和可装 Mod 的锚点差一整套。
    pub modded: bool,
    pub mods: ModsProfile,
    /// 光影环境：Iris / Oculus / OptiFine 在场，且 shaderpacks 目录非空。
    pub shaders: bool,
    /// options.txt 里的渲染距离。没玩过的实例读不到。
    pub render_distance: Option<u32>,
}

impl Workload {
    pub fn read(game_directory: &Path, game_version: &str, loader: crate::LoaderKind) -> Self {
        let mods = mods_profile(game_directory);
        Self {
            era: era(game_version),
            modded: loader != crate::LoaderKind::Vanilla,
            mods,
            shaders: shaders_present(game_directory),
            render_distance: render_distance(game_directory),
        }
    }
}

/// 数得上的 mod 文件后缀。`.disabled` 后缀的不算——它们不会被加载，也就不占堆。
const MOD_EXTENSIONS: [&str; 3] = ["jar", "zip", "litemod"];

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
        let path = entry.path();
        let matches = path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                MOD_EXTENSIONS
                    .iter()
                    .any(|known| extension.eq_ignore_ascii_case(known))
            });
        if !matches {
            continue;
        }
        profile.count += 1;
        profile.bytes += metadata.len();
    }
    profile
}

/// 光影跑起来了没有。
///
/// 两个条件都要：装了能读光影的东西，**并且** shaderpacks 里真的有包。只看
/// 前者会把「装了 Iris 但没装光影包」也算进来，那是很常见的一种实例。
fn shaders_present(game_directory: &Path) -> bool {
    let packs = std::fs::read_dir(game_directory.join("shaderpacks"))
        .map(|mut entries| entries.any(|entry| entry.is_ok()))
        .unwrap_or(false);
    if !packs {
        return false;
    }
    let Ok(entries) = std::fs::read_dir(game_directory.join("mods")) else {
        // OptiFine 装在 mods 之外也能读光影，那时候有包就当它开着。
        return true;
    };
    entries.flatten().any(|entry| {
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        name.contains("iris") || name.contains("oculus") || name.contains("optifine")
    })
}

/// options.txt 里那一行 `renderDistance:16`。
fn render_distance(game_directory: &Path) -> Option<u32> {
    let text = std::fs::read_to_string(game_directory.join("options.txt")).ok()?;
    text.lines()
        .find_map(|line| line.trim().strip_prefix("renderDistance:"))
        .and_then(|value| value.trim().parse().ok())
}

/// 物理内存字节数。读不到返回 `None`，调用方自己决定怎么猜。
pub fn physical_memory_bytes() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        meminfo_kilobytes("MemTotal:").map(|kilobytes| kilobytes * 1024)
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
        memory_status().map(|status| status.ullTotalPhys)
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        None
    }
}

/// 此刻可用的物理内存字节数。
///
/// Linux 上取 `MemAvailable` 而不是 `MemFree`：后者把页缓存算成「已用」，在
/// 一台正常使用的机器上永远是个小得吓人的数，照它算出来的堆会小到没法玩。
pub fn available_memory_bytes() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        meminfo_kilobytes("MemAvailable:").map(|kilobytes| kilobytes * 1024)
    }

    #[cfg(target_os = "macos")]
    {
        // vm_stat 报的是页数，页大小在头一行的括号里。free + inactive +
        // speculative 才是「可以马上拿来用的」——只算 free 会把大量可回收的
        // inactive 页当成用不了的。
        let output = std::process::Command::new("vm_stat").output().ok()?;
        let text = String::from_utf8_lossy(&output.stdout);
        let page_size: u64 = text
            .lines()
            .next()?
            .split("page size of ")
            .nth(1)?
            .split_whitespace()
            .next()?
            .parse()
            .ok()?;
        let pages = |label: &str| -> u64 {
            text.lines()
                .find_map(|line| line.trim().strip_prefix(label))
                .and_then(|value| value.trim().trim_end_matches('.').parse::<u64>().ok())
                .unwrap_or(0)
        };
        let usable = pages("Pages free:")
            + pages("Pages inactive:")
            + pages("Pages speculative:")
            + pages("Pages purgeable:");
        (usable > 0).then_some(usable * page_size)
    }

    #[cfg(windows)]
    {
        memory_status().map(|status| status.ullAvailPhys)
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        None
    }
}

#[cfg(target_os = "linux")]
fn meminfo_kilobytes(field: &str) -> Option<u64> {
    // 单位一律是 kB，内核一直这么写。
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    text.lines()
        .find_map(|line| line.strip_prefix(field))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

#[cfg(windows)]
fn memory_status() -> Option<windows_sys::Win32::System::SystemInformation::MEMORYSTATUSEX> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    // SAFETY: dwLength 按文档填好后，GlobalMemoryStatusEx 只写这一个结构体。
    unsafe {
        let mut status: MEMORYSTATUSEX = std::mem::zeroed();
        status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        (GlobalMemoryStatusEx(&mut status) != 0).then_some(status)
    }
}

/// 显存是不是从系统内存里切的。
///
/// 探不到就说探不到。这个信号只值 0.5 G 的保留量，为它引一个 GPU 枚举库、或者
/// 按显卡型号表猜，代价远大于收益。
fn graphics() -> Graphics {
    #[cfg(target_os = "macos")]
    {
        // Apple Silicon 是统一内存，没有例外。Intel Mac 有独显也有核显，
        // 分不出来。
        if cfg!(target_arch = "aarch64") {
            Graphics::Shared
        } else {
            Graphics::Unknown
        }
    }

    #[cfg(target_os = "linux")]
    {
        // DRM 节点的 vendor 是 PCI 厂商号。NVIDIA 在消费级机器上只出独显；
        // 只有 Intel 显示节点的机器基本就是核显。AMD 既做独显也做 APU，
        // 从这里分不出来——那就不分。
        let Ok(entries) = std::fs::read_dir("/sys/class/drm") else {
            return Graphics::Unknown;
        };
        let mut vendors = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !name.starts_with("card") || name.contains('-') {
                continue;
            }
            if let Ok(vendor) = std::fs::read_to_string(entry.path().join("device/vendor")) {
                vendors.push(vendor.trim().to_ascii_lowercase());
            }
        }
        if vendors.is_empty() {
            Graphics::Unknown
        } else if vendors.iter().any(|vendor| vendor == "0x10de") {
            Graphics::Dedicated
        } else if vendors.iter().all(|vendor| vendor == "0x8086") {
            Graphics::Shared
        } else {
            Graphics::Unknown
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Graphics::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_numbers_land_in_the_right_era() {
        assert_eq!(era("1.7.10"), Era::Legacy);
        assert_eq!(era("1.12.2"), Era::Legacy);
        assert_eq!(era("1.13"), Era::Transitional);
        assert_eq!(era("1.16.5"), Era::Transitional);
        assert_eq!(era("1.17.1"), Era::Modern);
        assert_eq!(era("1.21.4"), Era::Modern);
        assert_eq!(era("26.1"), Era::YearDrop);
        assert_eq!(era("27.3.1"), Era::YearDrop);
    }

    #[test]
    fn snapshots_and_pre_releases_are_recognised() {
        assert_eq!(era("1.21.4-pre1"), Era::Modern);
        assert_eq!(era("1.16-rc1"), Era::Transitional);
        assert_eq!(era("13w41a"), Era::Legacy);
        assert_eq!(era("18w22c"), Era::Transitional);
        assert_eq!(era("24w14a"), Era::Modern);
        assert_eq!(era("26w05a"), Era::YearDrop);
    }

    #[test]
    fn an_unreadable_version_leans_towards_the_larger_baseline() {
        // 认错方向的代价不对称：老版本多给一点内存什么也不会发生，新版本
        // 被当成老版本则会把堆压小。
        assert_eq!(era("fabulously-optimised"), Era::Modern);
        assert_eq!(era(""), Era::Modern);
    }

    #[test]
    fn this_machine_reports_a_plausible_amount_of_memory() {
        let machine = Machine::probe();
        let total = machine
            .total_bytes
            .expect("physical memory is readable here");
        assert!(
            total > MEGABYTE * 1024,
            "{total} bytes is implausibly small"
        );
        if let Some(available) = machine.available_bytes {
            assert!(available <= total, "available memory exceeds the machine");
        }
    }
}
