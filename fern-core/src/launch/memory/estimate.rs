//! 静态估算（设计文档 §5）。
//!
//! 形状沿用 PCL2 的「需求锚点 + 边际递减填充」，参数按 2026 年的版本现状重校。
//! 刻意保持简单：**静态层只负责第一次启动的合理性**，之后由自适应层用真实运行
//! 数据接管（`adaptive.rs`）。往这里堆更多信号是投错了地方——它修正的那点误差
//! 一个 session 之后就被覆盖了。

use super::signals::{Era, Graphics, Machine, Workload};

/// 一个实例的四个需求档位，GB。
///
/// `min` 是「低于这个数游戏起不来」，`t1` 是「勉强带得动」，`t2` 是「没什么
/// 问题」，`t3` 是「重度扩展也够」。填充算法在这四个数之间分段递减。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anchors {
    pub min: f64,
    pub t1: f64,
    pub t2: f64,
    pub t3: f64,
}

/// 原版基线。
///
/// 1.17 的 Java 16 迁移与 1.18 的世界高度扩展抬高了一截；26.1 的 4 G 不是我们
/// estimate 出来的，是 Mojang 实测之后写进默认值的，直接采信为 t2。
fn baseline(era: Era) -> Anchors {
    match era {
        Era::Legacy => Anchors {
            min: 0.5,
            t1: 1.0,
            t2: 2.0,
            t3: 3.0,
        },
        Era::Transitional => Anchors {
            min: 0.5,
            t1: 1.5,
            t2: 2.5,
            t3: 4.0,
        },
        Era::Modern => Anchors {
            min: 1.0,
            t1: 2.0,
            t2: 4.0,
            t3: 5.0,
        },
        Era::YearDrop => Anchors {
            min: 2.0,
            t1: 3.0,
            t2: 4.0,
            t3: 6.0,
        },
    }
}

/// 把实例的内容叠加到基线上。
///
/// Mod 数量走连续函数而不是阶梯表：阶梯有断崖——第 80 个 Mod 让需求跳一整格，
/// 而第 79 个和第 81 个之间实际上没有任何区别。
pub fn anchors(workload: &Workload) -> Anchors {
    let mut anchors = baseline(workload.era);
    if workload.modded {
        let count = f64::from(workload.mods.count);
        anchors.min += count / 150.0;
        anchors.t1 += count / 90.0;
        anchors.t2 += count / 50.0;
        anchors.t3 += count / 25.0;
    }
    if workload.shaders {
        anchors.t2 += 0.5;
        anchors.t3 += 1.0;
    }
    match workload.render_distance {
        Some(chunks) if chunks > 28 => anchors.t2 += 1.0,
        Some(chunks) if chunks > 16 => anchors.t2 += 0.5,
        _ => {}
    }
    anchors
}

/// 堆之外还要花掉的内存，GB。
///
/// 进程 RSS 通常比 `-Xmx` 高 0.5–1.5 G：Metaspace、DirectByteBuffer、LWJGL 的
/// native、显卡驱动映射全在堆外。不预留它，「可用内存」这个数就是假的。
pub fn reserve_gb(graphics: Graphics, zgc: bool) -> f64 {
    let mut reserve = 1.0;
    if graphics == Graphics::Shared {
        // 共享显存直接从系统内存里切。
        reserve += 0.5;
    }
    if zgc {
        // 着色指针与并发回收要更多堆外空间。
        reserve += 0.5;
    }
    reserve
}

/// 边际递减填充（§5.2）。
///
/// 预算 `budget` GB 按三段填进锚点：0→t1 全给，t1→t2 每 1 G 堆要花 1/0.7 G
/// 预算，t2→t3 是 1/0.4。**到 t3 为止。**
///
/// 设计文档还写了第四段（t3 → 2×t3，边际率 15%），这里没有实现它，因为它和
/// 文档自己那张验收表（§2.3：现代原版 4 G）直接冲突：在一台空闲 50 G 的机器
/// 上，第四段会被填满，原版实例拿到 10 G——而 t3 的定义就是「重度扩展也够」。
/// 超过它的部分不是余量，是明确的浪费，而过量分配本身有害（拖慢 GC、挤压页
/// 缓存与堆外）。真需要更多的判断留给自适应层：它看得见实际用量，而这里
/// 看不见。
fn fill(budget: f64, anchors: &Anchors) -> f64 {
    const RATES: [f64; 3] = [1.0, 0.7, 0.4];
    let widths = [
        anchors.t1,
        (anchors.t2 - anchors.t1).max(0.0),
        (anchors.t3 - anchors.t2).max(0.0),
    ];

    let mut left = budget.max(0.0);
    let mut heap = 0.0;
    for (width, rate) in widths.into_iter().zip(RATES) {
        if left <= 0.0 {
            break;
        }
        let cost = width / rate;
        if left >= cost {
            heap += width;
            left -= cost;
        } else {
            heap += left * rate;
            left = 0.0;
        }
    }
    heap
}

/// 两条上限（§5.4）。
///
/// 静态上限管长期合理性——最多敢要多少；实时约束管当下——现在实际能给多少。
/// 只看总量的方案会在用户开着浏览器和 IDE 时把系统压进 swap，那种卡顿比堆小
/// 一点难受得多。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    pub floor_mb: u32,
    pub static_cap_mb: u32,
    pub live_cap_mb: u32,
    pub hard_cap_mb: u32,
    /// 实时约束已经压到地板以下：这台机器此刻腾不出该给的量。
    pub tight: bool,
}

pub fn bounds(anchors: &Anchors, machine: &Machine, ceiling_mb: u32, zgc: bool) -> Bounds {
    let floor_mb = gigabytes_to_mb(anchors.min);
    let reserve_mb = gigabytes_to_mb(reserve_gb(machine.graphics, zgc));
    let live_cap_mb = machine.available_mb().saturating_sub(reserve_mb);
    let hard_cap_mb = ceiling_mb.min(live_cap_mb);
    Bounds {
        floor_mb,
        static_cap_mb: ceiling_mb,
        live_cap_mb,
        // 地板高于实时约束时仍按地板给：低于它游戏根本起不来，给一个起不来的
        // 数不叫保守，叫失败。这种情况由 `tight` 说出来，界面提示一次。
        hard_cap_mb: hard_cap_mb.max(floor_mb),
        tight: live_cap_mb < floor_mb,
    }
}

/// 静态估算的结果。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Estimate {
    pub xmx_mb: u32,
    pub anchors: Anchors,
    pub bounds: Bounds,
}

pub fn estimate(workload: &Workload, machine: &Machine, ceiling_mb: u32, zgc: bool) -> Estimate {
    let anchors = anchors(workload);
    let bounds = bounds(&anchors, machine, ceiling_mb, zgc);
    let reserve = reserve_gb(machine.graphics, zgc);
    let budget = f64::from(machine.available_mb()) / 1024.0 - reserve;
    let filled = gigabytes_to_mb(fill(budget, &anchors));
    Estimate {
        xmx_mb: round_to_step(filled.clamp(bounds.floor_mb, bounds.hard_cap_mb)),
        anchors,
        bounds,
    }
}

fn gigabytes_to_mb(value: f64) -> u32 {
    (value.max(0.0) * 1024.0).round() as u32
}

/// 分配值取 256 M 的整数倍。
///
/// 不是为了好看：`-Xmx6042M` 这种数字会让人以为它精确，而它的误差远大于
/// 42 M。取整之后那一行文案才说得出口。
pub fn round_to_step(megabytes: u32) -> u32 {
    const STEP: u32 = 256;
    (megabytes / STEP).max(1) * STEP
}

#[cfg(test)]
mod tests {
    use super::super::signals::{Era, ModsProfile};
    use super::*;

    fn machine(total_gb: u64, available_gb: u64) -> Machine {
        Machine {
            total_bytes: Some(total_gb * 1024 * 1024 * 1024),
            available_bytes: Some(available_gb * 1024 * 1024 * 1024),
            graphics: Graphics::Dedicated,
        }
    }

    fn vanilla(era: Era) -> Workload {
        Workload {
            era,
            modded: false,
            mods: ModsProfile::default(),
            shaders: false,
            render_distance: None,
        }
    }

    fn modded(count: u32) -> Workload {
        Workload {
            era: Era::Modern,
            modded: true,
            mods: ModsProfile {
                count,
                bytes: u64::from(count) * 2 * 1024 * 1024,
            },
            shaders: false,
            render_distance: None,
        }
    }

    /// 这一组是拿设计文档 §2.3 那张社区经验值表当验收标准的。
    ///
    /// 机器配置取**正在被使用的**那种（16 G 装着系统和浏览器，剩下 8 G），
    /// 不是刚开机的理想状态：算法本来就是按此刻可用量填的，拿一台空机器去量
    /// 它，量到的是上限而不是常态。
    #[test]
    fn a_modern_vanilla_instance_lands_near_the_four_gig_baseline() {
        // 社区经验值：现代原版 4 G。
        let value = estimate(&vanilla(Era::Modern), &machine(16, 8), 8192, false).xmx_mb;
        assert!(
            (3584..=5120).contains(&value),
            "{value} MB is outside the vanilla band"
        );
    }

    #[test]
    fn old_versions_get_less_than_modern_ones() {
        let old = estimate(&vanilla(Era::Legacy), &machine(16, 12), 8192, false).xmx_mb;
        let new = estimate(&vanilla(Era::Modern), &machine(16, 12), 8192, false).xmx_mb;
        assert!(old < new, "{old} should be below {new}");
    }

    #[test]
    fn a_large_modpack_lands_in_the_eight_to_twelve_gig_band() {
        // ATM 级 320 个 Mod：官方建议 8–12 G，而那个建议说的正是 16 G 机器。
        let value = estimate(&modded(320), &machine(16, 12), 8192, false).xmx_mb;
        assert!(
            (8192..=12288).contains(&value),
            "{value} MB is outside the big-pack band"
        );
    }

    #[test]
    fn a_roomy_machine_still_never_gives_away_more_than_half_of_itself() {
        // 32 G 的机器上同一个包会拿到更多——它确实有更多可以拿。但那条
        // 「不超过一半」的线仍然在，而且是上限而不是目标。
        let roomy = estimate(&modded(320), &machine(32, 28), 16384, false).xmx_mb;
        assert!(roomy > 8192, "{roomy} MB ignores the spare memory");
        assert!(roomy <= 16384, "{roomy} MB crossed half the machine");
    }

    #[test]
    fn more_mods_never_means_less_memory() {
        let sizes = [0, 20, 60, 120, 200, 320];
        let values: Vec<u32> = sizes
            .iter()
            .map(|count| estimate(&modded(*count), &machine(32, 28), 16384, false).xmx_mb)
            .collect();
        for pair in values.windows(2) {
            assert!(pair[1] >= pair[0], "{values:?} is not monotonic");
        }
    }

    #[test]
    fn what_the_machine_can_spare_right_now_wins_over_what_it_owns() {
        // 32 G 的机器，但此刻只剩 4 G 可用——分配必须跟着可用量走，否则
        // 一按启动整个系统开始换页。
        let busy = estimate(&modded(200), &machine(32, 4), 16384, false);
        let idle = estimate(&modded(200), &machine(32, 28), 16384, false);
        assert!(busy.xmx_mb < idle.xmx_mb);
        assert!(busy.xmx_mb <= busy.bounds.live_cap_mb.max(busy.bounds.floor_mb));
    }

    #[test]
    fn the_floor_holds_even_when_the_machine_has_nothing_left() {
        let cramped = estimate(&modded(200), &machine(4, 1), 4096, false);
        assert!(cramped.bounds.tight, "this machine really is out of memory");
        assert_eq!(cramped.xmx_mb, round_to_step(cramped.bounds.floor_mb));
    }

    #[test]
    fn shaders_and_render_distance_push_the_middle_anchor_up() {
        let plain = anchors(&vanilla(Era::Modern));
        let heavy = anchors(&Workload {
            shaders: true,
            render_distance: Some(32),
            ..vanilla(Era::Modern)
        });
        assert_eq!(heavy.t2, plain.t2 + 1.5);
        assert_eq!(heavy.t3, plain.t3 + 1.0);
    }

    #[test]
    fn zgc_and_shared_video_memory_both_cost_headroom() {
        assert_eq!(reserve_gb(Graphics::Dedicated, false), 1.0);
        assert_eq!(reserve_gb(Graphics::Shared, false), 1.5);
        assert_eq!(reserve_gb(Graphics::Dedicated, true), 1.5);
        assert_eq!(reserve_gb(Graphics::Shared, true), 2.0);
        // 探不到就不加保留——宁可少留，也不为一个猜出来的结论压小堆。
        assert_eq!(reserve_gb(Graphics::Unknown, false), 1.0);
    }

    #[test]
    fn the_ceiling_is_never_crossed() {
        let value = estimate(&modded(400), &machine(64, 60), 6144, false).xmx_mb;
        assert!(value <= 6144, "{value} MB crossed the ceiling");
    }
}
