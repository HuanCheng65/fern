//! 自适应层（设计文档 §6.4）。
//!
//! 这是整套方案里唯一一处比现有启动器多做的事：**第二次启动起，分配值由这个
//! 实例在这台机器上的真实行为决定**，不再由任何人的估算决定。
//!
//! 两条不对称是刻意的：
//!
//! - **上调即时，下调要滞回。** 宁可多给半 G，也不让玩家再撞一次 OOM；而下调
//!   如果也即时，分配值会在 8 → 7.5 → 8 之间来回震荡，每次启动都换一个数。
//! - **贴顶才上调，卡顿不算。** 回收之后水位仍然逼近上限，说明 live set 本身
//!   撑不下，加内存有效；而停顿频繁但水位健康，问题在 GC 参数上，加内存只会
//!   让停顿更长。这个区分挡住了「卡了就加内存」这条社区常见误判被自动化固化。

use super::history::Window;

/// 这一次为什么这么调。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Adjustment {
    /// 上次撞了 OOM。
    Recovering,
    /// 峰值贴到上限，或者出现了 allocation stall。
    Pressed,
    /// 峰值偏高，先加半 G。
    Warm,
    /// 水位健康，维持上次的量。
    Steady,
    /// 连续几次都用不到那么多，收回半 G。
    Cooling,
}

/// 从历史里学出来的那个值。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Learned {
    pub xmx_mb: u32,
    /// 窗口内回收后水位的 p90。对 live set 的估计。
    pub live_set_mb: u32,
    pub last_peak_mb: u32,
    pub last_xmx_mb: u32,
    pub sessions: usize,
    pub adjustment: Adjustment,
}

/// live set 到堆大小的系数。
///
/// 1.5–2 倍是 JVM 堆定容的经典启发式。ZGC 取更高的一档，因为并发回收在分配
/// 速率追上回收速率时会产生 allocation stall，需要比 G1 更大的余量吸收突发
/// 分配。
fn factor(zgc: bool) -> f64 {
    if zgc { 1.9 } else { 1.6 }
}

pub fn learn(window: &Window, zgc: bool) -> Option<Learned> {
    let sessions = window.valid();
    if sessions.len() < super::history::MINIMUM_SESSIONS {
        return None;
    }
    let last = sessions.last()?;

    let mut live_sets: Vec<u32> = sessions
        .iter()
        .map(|session| session.metrics.live_set_mb)
        .collect();
    live_sets.sort_unstable();
    let live_set_mb = live_sets[percentile_index(live_sets.len(), 0.90)];

    let base = super::estimate::round_to_step((f64::from(live_set_mb) * factor(zgc)) as u32);
    let previous = last.xmx_mb;
    let peak_share = if previous > 0 {
        f64::from(last.metrics.peak_mb) / f64::from(previous)
    } else {
        0.0
    };

    let cooling = sessions
        .iter()
        .rev()
        .take(3)
        .filter(|session| {
            session.xmx_mb > 0
                && f64::from(session.metrics.peak_mb) / f64::from(session.xmx_mb) < 0.55
        })
        .count()
        == 3;

    let adjustment = if last.oom {
        Adjustment::Recovering
    } else if peak_share > 0.90 || last.metrics.stalls > 0 {
        Adjustment::Pressed
    } else if peak_share > 0.80 {
        Adjustment::Warm
    } else if cooling {
        Adjustment::Cooling
    } else {
        Adjustment::Steady
    };

    let xmx_mb = match adjustment {
        // OOM 越过滞回：立刻加 2 G。
        Adjustment::Recovering => base.max(previous + 2048),
        Adjustment::Pressed => base.max(previous + 1024),
        Adjustment::Warm => base.max(previous + 512),
        Adjustment::Cooling => previous
            .saturating_sub(cooling_step(previous, base))
            .max(base),
        Adjustment::Steady => previous,
    };

    Some(Learned {
        xmx_mb: super::estimate::round_to_step(xmx_mb),
        live_set_mb,
        last_peak_mb: last.metrics.peak_mb,
        last_xmx_mb: previous,
        sessions: sessions.len(),
        adjustment,
    })
}

/// 这一次往回收多少。
///
/// 设计文档写的是固定的 512 M。实现时改成「离目标还有多远就收一半，至少
/// 512 M」——固定步长在首次估算给多了的时候要走几十次会话才收得回来（16 G 收
/// 到 6 G 是二十步，按一次会话一晚上算就是三周），而那期间每一次启动都在浪费
/// 一大块内存。**滞回是为了不震荡，不是为了慢。** 离目标越远步子越大，反而
/// 更稳：真正会震荡的是贴着目标反复横跳，那种情况下这个式子给出的正是最小的
/// 512 M。
fn cooling_step(previous: u32, base: u32) -> u32 {
    previous.saturating_sub(base).div_euclid(2).max(512)
}

fn percentile_index(length: usize, fraction: f64) -> usize {
    (((length as f64) * fraction).ceil() as usize)
        .saturating_sub(1)
        .min(length - 1)
}

#[cfg(test)]
mod tests {
    use super::super::gclog::SessionMetrics;
    use super::super::history::Session;
    use super::*;

    fn window(sessions: Vec<Session>) -> Window {
        Window {
            modlist_hash: "aaaa".to_owned(),
            sessions,
        }
    }

    fn session(xmx_mb: u32, peak_mb: u32, live_set_mb: u32) -> Session {
        Session {
            at: 0,
            minutes: 45.0,
            xmx_mb,
            metrics: SessionMetrics {
                peak_mb,
                live_set_mb,
                pause_p99_ms: 18.0,
                collections: 120,
                stalls: 0,
            },
            oom: false,
            zgc: false,
        }
    }

    #[test]
    fn one_session_is_never_enough() {
        assert!(learn(&window(vec![session(4096, 3000, 1800)]), false).is_none());
    }

    #[test]
    fn a_comfortable_instance_keeps_what_it_had() {
        let learned = learn(
            &window(vec![session(6144, 4200, 2200), session(6144, 4300, 2300)]),
            false,
        )
        .expect("two sessions");
        assert_eq!(learned.adjustment, Adjustment::Steady);
        assert_eq!(learned.xmx_mb, 6144);
    }

    #[test]
    fn an_out_of_memory_run_jumps_the_hysteresis() {
        let mut sessions = vec![session(4096, 4000, 2400), session(4096, 4090, 2600)];
        sessions[1].oom = true;
        let learned = learn(&window(sessions), false).expect("two sessions");
        assert_eq!(learned.adjustment, Adjustment::Recovering);
        assert_eq!(learned.xmx_mb, 4096 + 2048);
    }

    #[test]
    fn pressure_against_the_ceiling_adds_a_gigabyte_at_once() {
        let learned = learn(
            &window(vec![session(8192, 7000, 3800), session(8192, 7600, 4000)]),
            false,
        )
        .expect("two sessions");
        assert_eq!(learned.adjustment, Adjustment::Pressed);
        assert_eq!(learned.xmx_mb, 8192 + 1024);
    }

    #[test]
    fn an_allocation_stall_counts_as_pressure_even_at_a_healthy_water_line() {
        let mut sessions = vec![session(8192, 3000, 1600), session(8192, 3200, 1700)];
        sessions[1].metrics.stalls = 4;
        let learned = learn(&window(sessions), true).expect("two sessions");
        assert_eq!(learned.adjustment, Adjustment::Pressed);
        assert!(learned.xmx_mb > 8192);
    }

    #[test]
    fn giving_memory_back_takes_three_quiet_runs() {
        let quiet = || session(8192, 3000, 1600);
        // 两次还不够——一次异常（开了创造模式满世界飞）不该改变结论。
        let two = learn(&window(vec![quiet(), quiet()]), false).expect("two sessions");
        assert_eq!(two.adjustment, Adjustment::Steady);
        assert_eq!(two.xmx_mb, 8192);
        // 第三次才开始收，而且直接朝目标收一半：live set 1.6 G × 1.6 ≈ 2.5 G，
        // 从 8 G 到那里的一半是 2.8 G。
        let three = learn(&window(vec![quiet(), quiet(), quiet()]), false).expect("three sessions");
        assert_eq!(three.adjustment, Adjustment::Cooling);
        assert_eq!(three.xmx_mb, 5376);
    }

    #[test]
    fn cooling_converges_in_a_handful_of_sessions_not_dozens() {
        // 首次估算给多了是常态（静态层看不到实际用量）。收回来该以「几次会话」
        // 计，不是「几十次」——固定半 G 的步长从 16 G 收到 3 G 要二十六次。
        let mut allocation = 16384;
        let mut rounds = 0;
        while allocation > 3072 && rounds < 10 {
            let quiet = || session(allocation, allocation / 4, 1600);
            let learned =
                learn(&window(vec![quiet(), quiet(), quiet()]), false).expect("three sessions");
            assert_eq!(learned.adjustment, Adjustment::Cooling);
            assert!(learned.xmx_mb < allocation);
            allocation = learned.xmx_mb;
            rounds += 1;
        }
        assert!(allocation <= 3072, "还停在 {allocation} MB");
        assert!(rounds <= 6, "收敛用了 {rounds} 轮");
    }

    #[test]
    fn the_last_step_towards_the_target_is_never_a_hair() {
        // 贴着目标的时候步长是固定的 512 M。这里才是滞回真正要防的地方——
        // 一个越来越小的步长会让分配值每次启动都变一点点，永远不稳定。
        assert_eq!(cooling_step(5120, 4864), 512);
        assert_eq!(cooling_step(5120, 5120), 512);
    }

    #[test]
    fn cooling_never_dips_below_what_the_live_set_needs() {
        // live set 3 G × 1.6 ≈ 4.8 G：再怎么闲，也不能收到这条线以下。
        let busy = || session(5120, 2500, 3072);
        let learned = learn(&window(vec![busy(), busy(), busy()]), false).expect("three sessions");
        assert_eq!(learned.adjustment, Adjustment::Cooling);
        assert!(
            learned.xmx_mb >= 4864,
            "{} MB is below the live set",
            learned.xmx_mb
        );
    }

    #[test]
    fn zgc_asks_for_more_headroom_than_g1() {
        let sessions = vec![session(8192, 3000, 4096), session(8192, 3100, 4096)];
        // 两条路径的 live set 一样，但 ZGC 要更大的余量吸收突发分配。
        let g1 = learn(&window(sessions.clone()), false).expect("g1");
        let zgc = learn(&window(sessions), true).expect("zgc");
        assert!(zgc.xmx_mb >= g1.xmx_mb);
    }

    #[test]
    fn short_runs_do_not_dilute_the_conclusion() {
        let mut brief = session(8192, 200, 100);
        brief.minutes = 0.4;
        let learned = learn(
            &window(vec![
                session(8192, 7800, 4000),
                brief,
                session(8192, 7900, 4100),
            ]),
            false,
        )
        .expect("two valid sessions");
        assert_eq!(learned.sessions, 2);
        assert_eq!(learned.adjustment, Adjustment::Pressed);
    }
}
