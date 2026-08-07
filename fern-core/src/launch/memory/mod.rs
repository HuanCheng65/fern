//! 自动内存分配与 JVM 参数生成（`docs/fern-memory-allocation-design.md`）。
//!
//! 用户默认不需要理解「内存分配」这个概念，也不需要碰滑块；同时保留完整的手动
//! 路径，而且手动路径的优先级高于这里的一切。
//!
//! 核心判断：**静态估算只负责第一次启动的合理性，真实运行数据负责之后的
//! 精确性。** 所以静态层刻意保持简单（`estimate.rs`），工程投入放在反馈闭环上
//! （`gclog.rs` → `history.rs` → `adaptive.rs`）。这是本方案与 HMCL（纯静态、
//! 不看实例内容）和 PCL2（静态但精细）的根本区别。
//!
//! 决策优先级链，命中即停：
//!
//! ```text
//! 0. 实例设置里手填的值      绝对优先，自动逻辑完全静默
//! 1. 用户 JVM 参数里的 -Xmx  自动分配让位，一个内存参数都不注入
//! 2. 历史实测值              有足够会话且 mod 列表没变
//! 3. 静态估算                首次启动、历史失效，或以上都不可用
//! ```
//!
//! 设计文档里在 1 和 2 之间还有一层「整合包作者推荐值」。它依赖整合包导入时把
//! manifest 里的推荐内存留下来，那件事还没做——**没有数据源的层不占位**，等
//! 真的有了再插进来。
//!
//! `explanation` 是一等公民：可解释性由数据结构保证，而不是让界面事后拼凑。
//! 「自动」这两个字本身不解释任何事，只有把判断依据摊开，用户才知道要不要动它。

pub mod adaptive;
pub mod estimate;
pub mod gclog;
pub mod history;
pub mod jvm_args;
pub mod signals;

use std::path::Path;

use serde::{Deserialize, Serialize};

pub use jvm_args::{GcPath, Platform};
pub use signals::{Machine, ModsProfile, Workload, mods_profile, physical_memory_bytes};

use history::Window;

/// 这个数字是谁定的。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AllocationSource {
    /// 实例设置里手填的。
    Manual,
    /// 用户自己的 JVM 参数里已经有 `-Xmx`，我们不插手。
    UserJvmArgs,
    /// 这个实例在这台机器上的实测数据。
    Adaptive,
    /// 静态估算。
    Static,
}

/// 解释里的一条。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Topic {
    /// 依据：这个实例有什么。
    Basis,
    /// 实测：上次跑成什么样。
    History,
    /// 约束：什么东西挡住了它。
    Limit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExplanationItem {
    pub topic: Topic,
    /// 一个短句，界面直接显示。
    pub text: String,
}

fn item(topic: Topic, text: impl Into<String>) -> ExplanationItem {
    ExplanationItem {
        topic,
        text: text.into(),
    }
}

/// 一次分配的全部结论。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AllocationDecision {
    pub xmx_mb: u32,
    pub source: AllocationSource,
    pub gc: GcPath,
    pub explanation: Vec<ExplanationItem>,
    /// 要加到命令行上的参数，按顺序。
    pub arguments: Vec<String>,
    /// 这台机器此刻腾不出该给的量，只能按地板发。界面提示一次，不阻塞启动。
    pub tight: bool,
}

/// 算一次分配要知道的全部东西。全部是值，不做任何 IO——IO 在 `plan()` 里。
#[derive(Debug, Clone)]
pub struct Request<'a> {
    pub workload: Workload,
    pub machine: Machine,
    pub java_major: u16,
    pub platform: Platform,
    /// 实例设置里手填的堆。
    pub manual_mb: Option<u32>,
    /// 用户拥有的那条线：最多把多少内存交给游戏。
    pub ceiling_mb: u32,
    /// 版本元数据和用户自定义合起来的既有参数。任何一处表态，我们就让位。
    pub existing: &'a [String],
    /// 用户在设置里明确点过的收集器。`None` 是「你决定」。
    pub gc_preference: Option<GcPath>,
    pub history: Option<&'a Window>,
    /// GC 日志写到哪。`None` 表示这次不采集。
    pub gc_log: Option<&'a Path>,
}

/// 设置里的三档映射到路径。`Auto` 交给决策树。
pub fn preference(collector: crate::GarbageCollector) -> Option<GcPath> {
    match collector {
        crate::GarbageCollector::Auto => None,
        crate::GarbageCollector::G1 => Some(GcPath::G1),
        crate::GarbageCollector::Z => Some(GcPath::Zgc),
    }
}

/// 优先级链的实现。纯函数：同样的输入永远得到同样的输出。
pub fn resolve(request: &Request<'_>) -> AllocationDecision {
    // 用户在原始 JVM 参数里已经写了收集器，那连他在设置里点的那一档都不该
    // 覆盖它——两个 `-XX:+Use*GC` 撞在一起，JVM 直接拒绝启动。
    let gc = if jvm_args::collector_is_pinned(request.existing) {
        GcPath::Untouched
    } else if let Some(chosen) = request.gc_preference {
        chosen
    } else {
        jvm_args::choose(
            request.workload.era,
            request.workload.modded,
            request.java_major,
            request.platform,
            request.existing,
        )
    };
    let zgc = gc.behaves_like_zgc();
    let static_estimate =
        estimate::estimate(&request.workload, &request.machine, request.ceiling_mb, zgc);

    let (xmx_mb, source, mut explanation) = if jvm_args::heap_is_pinned(request.existing) {
        // 用户自己写了 -Xmx。不注入，也不假装我们决定了什么。
        (
            0,
            AllocationSource::UserJvmArgs,
            vec![item(Topic::Basis, "自定义 JVM 参数里已经指定了堆大小")],
        )
    } else if let Some(manual) = request.manual_mb {
        let value = manual.clamp(512, request.ceiling_mb);
        let mut explanation = vec![item(Topic::Basis, "这个实例手动指定了堆大小")];
        if value != manual {
            explanation.push(item(
                Topic::Limit,
                format!("受上限 {} 约束", gigabytes(request.ceiling_mb)),
            ));
        }
        (value, AllocationSource::Manual, explanation)
    } else if let Some(learned) = request
        .history
        .and_then(|window| adaptive::learn(window, zgc))
    {
        let bounds = static_estimate.bounds;
        let value = learned
            .xmx_mb
            .clamp(bounds.floor_mb, bounds.hard_cap_mb.max(bounds.floor_mb));
        let mut explanation = vec![item(
            Topic::History,
            format!(
                "依据最近 {} 次运行，上次峰值 {}",
                learned.sessions,
                gigabytes(learned.last_peak_mb)
            ),
        )];
        explanation.push(item(Topic::History, adjustment_text(&learned)));
        if value != learned.xmx_mb {
            explanation.push(item(Topic::Limit, limit_text(&static_estimate, value)));
        }
        (value, AllocationSource::Adaptive, explanation)
    } else {
        let mut explanation = basis_of(&request.workload);
        if static_estimate.xmx_mb >= static_estimate.bounds.hard_cap_mb {
            explanation.push(item(
                Topic::Limit,
                limit_text(&static_estimate, static_estimate.xmx_mb),
            ));
        }
        (
            static_estimate.xmx_mb,
            AllocationSource::Static,
            explanation,
        )
    };

    if static_estimate.bounds.tight && source != AllocationSource::UserJvmArgs {
        explanation.push(item(
            Topic::Limit,
            format!(
                "此刻可用内存只有 {}，已经按最低需求发放",
                gigabytes(request.machine.available_mb())
            ),
        ));
    }

    let mut arguments = Vec::new();
    if source != AllocationSource::UserJvmArgs {
        arguments.push(jvm_args::heap_argument(xmx_mb));
    }
    arguments.extend(jvm_args::arguments(gc, request.java_major));
    arguments.extend(jvm_args::safety_arguments(
        request.java_major,
        request.existing,
    ));
    if let Some(path) = request.gc_log {
        arguments.extend(gclog::log_arguments(request.java_major, path));
    }

    AllocationDecision {
        xmx_mb,
        source,
        gc,
        explanation,
        arguments,
        tight: static_estimate.bounds.tight,
    }
}

/// 静态估算那一行的依据。
fn basis_of(workload: &Workload) -> Vec<ExplanationItem> {
    let mut basis = Vec::new();
    if workload.modded && workload.mods.count > 0 {
        basis.push(item(
            Topic::Basis,
            format!("{} 个 Mod", workload.mods.count),
        ));
    } else if !workload.modded {
        basis.push(item(Topic::Basis, "原版"));
    }
    if workload.shaders {
        basis.push(item(Topic::Basis, "光影"));
    }
    if let Some(chunks) = workload.render_distance.filter(|chunks| *chunks > 16) {
        basis.push(item(Topic::Basis, format!("{chunks} 区块渲染距离")));
    }
    if basis.is_empty() {
        basis.push(item(Topic::Basis, "这个版本的常规需求"));
    }
    basis
}

fn adjustment_text(learned: &adaptive::Learned) -> String {
    use adaptive::Adjustment::*;
    match learned.adjustment {
        Recovering => "上次因内存不足退出，这次多给 2 GB".to_owned(),
        Pressed => "上次几乎用满，这次多给 1 GB".to_owned(),
        Warm => "上次用量偏高，这次多给 512 MB".to_owned(),
        Steady => format!("水位健康，维持 {}", gigabytes(learned.xmx_mb)),
        Cooling => "连续几次都用不满，收回 512 MB".to_owned(),
    }
}

fn limit_text(estimate: &estimate::Estimate, value: u32) -> String {
    if estimate.bounds.live_cap_mb < estimate.bounds.static_cap_mb {
        format!(
            "受此刻可用内存约束，最多 {}",
            gigabytes(estimate.bounds.live_cap_mb.max(value))
        )
    } else {
        format!("受上限 {} 约束", gigabytes(estimate.bounds.static_cap_mb))
    }
}

/// MB 变成一句人话。整数不带小数点——`8 GB` 比 `8.0 GB` 更像一个决定。
pub fn gigabytes(megabytes: u32) -> String {
    let value = f64::from(megabytes) / 1024.0;
    if (value - value.round()).abs() < 0.05 {
        format!("{} GB", value.round() as u32)
    } else {
        format!("{value:.1} GB")
    }
}

/// 设置页要回答的那两个数。
///
/// 「上限」两个字本身不解释任何事——只有把这台机器有多少、现在这条线在哪
/// 一起摆出来，用户才知道要不要动它。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryBudget {
    pub physical_mb: u32,
    pub ceiling_mb: u32,
}

pub fn memory_budget(preference: Option<u32>) -> MemoryBudget {
    let physical = physical_memory_bytes();
    MemoryBudget {
        physical_mb: physical.map_or(8192, |bytes| (bytes / signals::MEGABYTE) as u32),
        ceiling_mb: heap_ceiling(physical, preference),
    }
}

/// 这台机器上最多把多少内存交给游戏，MB。
///
/// 这条线是整套自动分配里**唯一一个只有用户知道答案**的量（机器上还跑着什么
/// 只有他清楚），所以它是设置里那一行；其余参数是我们的判断。
///
/// 不设的时候取物理内存的一半，并封顶 16 G：一半是「给游戏的不该比留给系统的
/// 多」，16 G 覆盖到 400 个 Mod 的巨型包仍有余量——再往上给，收益只剩更长的
/// GC 停顿。设了就照设的来，只夹在 [2 G, 整机] 之间：2 G 的地板不是偏好，是
/// 「低于这个数游戏根本起不来」；设成超过整台机器也没有意义，那不是「多给
/// 一点」，是保证换页。
pub fn heap_ceiling(physical_bytes: Option<u64>, preference: Option<u32>) -> u32 {
    let physical_mb = physical_bytes.map_or(8192, |bytes| (bytes / signals::MEGABYTE) as u32);
    match preference {
        Some(chosen) => chosen.clamp(2048, physical_mb.max(2048)),
        None => (physical_mb / 2).clamp(2048, 16384),
    }
}

/// 带 IO 的那一层：探机器、读实例目录、读历史，然后调 `resolve`。
#[allow(clippy::too_many_arguments)]
pub fn plan(
    paths: &crate::DataPaths,
    profile: &crate::InstanceProfile,
    game_directory: &Path,
    java_major: u16,
    manual_mb: Option<u32>,
    ceiling_mb: u32,
    collector: crate::GarbageCollector,
    existing: &[String],
    gc_log: Option<&Path>,
) -> AllocationDecision {
    let workload = Workload::read(game_directory, &profile.game_version, profile.loader);
    let hash = history::modlist_hash(game_directory);
    let window = history::read(paths, profile.id.as_str(), &hash);
    resolve(&Request {
        workload,
        machine: Machine::probe(),
        java_major,
        platform: Platform::probe(),
        manual_mb,
        ceiling_mb,
        existing,
        gc_preference: preference(collector),
        history: window.as_ref(),
        gc_log,
    })
}

#[cfg(test)]
mod tests {
    use super::signals::{Era, Graphics};
    use super::*;

    fn machine() -> Machine {
        Machine {
            total_bytes: Some(32 * 1024 * 1024 * 1024),
            available_bytes: Some(24 * 1024 * 1024 * 1024),
            graphics: Graphics::Dedicated,
        }
    }

    fn workload() -> Workload {
        Workload {
            era: Era::Modern,
            modded: true,
            mods: ModsProfile {
                count: 120,
                bytes: 240 * 1024 * 1024,
            },
            shaders: false,
            render_distance: None,
        }
    }

    fn request<'a>(existing: &'a [String], manual_mb: Option<u32>) -> Request<'a> {
        Request {
            workload: workload(),
            machine: machine(),
            java_major: 21,
            platform: Platform {
                zgc_supported: true,
            },
            manual_mb,
            ceiling_mb: 16384,
            existing,
            gc_preference: None,
            history: None,
            gc_log: None,
        }
    }

    #[test]
    fn a_manual_setting_silences_the_automatic_layer() {
        let decision = resolve(&request(&[], Some(3072)));
        assert_eq!(decision.source, AllocationSource::Manual);
        assert_eq!(decision.xmx_mb, 3072);
        assert!(decision.arguments.contains(&"-Xmx3072M".to_owned()));
    }

    #[test]
    fn a_manual_setting_still_cannot_cross_the_line_the_user_drew() {
        let mut plan = request(&[], Some(32768));
        plan.ceiling_mb = 8192;
        let decision = resolve(&plan);
        assert_eq!(decision.xmx_mb, 8192);
        assert!(
            decision
                .explanation
                .iter()
                .any(|item| item.topic == Topic::Limit)
        );
    }

    #[test]
    fn a_user_supplied_heap_flag_stops_us_from_adding_one() {
        let existing = vec!["-Xmx10G".to_owned()];
        let decision = resolve(&request(&existing, None));
        assert_eq!(decision.source, AllocationSource::UserJvmArgs);
        assert!(
            !decision
                .arguments
                .iter()
                .any(|argument| argument.starts_with("-Xmx")),
            "{:?} still injects a heap flag",
            decision.arguments
        );
    }

    #[test]
    fn a_user_supplied_collector_stops_the_whole_gc_tree() {
        let existing = vec!["-XX:+UseSerialGC".to_owned()];
        let decision = resolve(&request(&existing, None));
        assert_eq!(decision.gc, GcPath::Untouched);
        assert!(
            !decision
                .arguments
                .iter()
                .any(|argument| argument.contains("GC")),
            "{:?} would collide with the user's collector",
            decision.arguments
        );
        // 但堆还是要给——收集器和堆大小是两件事。
        assert!(
            decision
                .arguments
                .iter()
                .any(|argument| argument.starts_with("-Xmx"))
        );
    }

    #[test]
    fn history_beats_the_static_estimate() {
        let sessions = vec![
            history::Session {
                at: 0,
                minutes: 40.0,
                xmx_mb: 6144,
                metrics: gclog::SessionMetrics {
                    peak_mb: 5900,
                    live_set_mb: 3000,
                    pause_p99_ms: 20.0,
                    collections: 200,
                    stalls: 0,
                },
                oom: false,
                zgc: true,
            },
            history::Session {
                at: 1,
                minutes: 50.0,
                xmx_mb: 6144,
                metrics: gclog::SessionMetrics {
                    peak_mb: 5960,
                    live_set_mb: 3100,
                    pause_p99_ms: 22.0,
                    collections: 240,
                    stalls: 0,
                },
                oom: false,
                zgc: true,
            },
        ];
        let window = Window {
            modlist_hash: "aaaa".to_owned(),
            sessions,
        };
        let mut plan = request(&[], None);
        plan.history = Some(&window);
        let decision = resolve(&plan);
        assert_eq!(decision.source, AllocationSource::Adaptive);
        // 上次峰值贴到 97%，这次该多给。
        assert!(decision.xmx_mb > 6144, "{} MB", decision.xmx_mb);
        assert!(
            decision
                .explanation
                .iter()
                .any(|item| item.topic == Topic::History)
        );
    }

    #[test]
    fn the_explanation_says_what_it_looked_at() {
        let mut plan = request(&[], None);
        plan.workload.shaders = true;
        plan.workload.render_distance = Some(24);
        let decision = resolve(&plan);
        let texts: Vec<&str> = decision
            .explanation
            .iter()
            .map(|item| item.text.as_str())
            .collect();
        assert!(texts.iter().any(|text| text.contains("120 个 Mod")));
        assert!(texts.iter().any(|text| text.contains("光影")));
        assert!(texts.iter().any(|text| text.contains("24 区块")));
    }

    #[test]
    fn gc_logging_is_injected_only_when_someone_asked_for_it() {
        let quiet = resolve(&request(&[], None));
        assert!(
            !quiet
                .arguments
                .iter()
                .any(|argument| argument.contains("Xlog"))
        );
        let mut plan = request(&[], None);
        let path = Path::new("/tmp/fern/gc.log");
        plan.gc_log = Some(path);
        let watched = resolve(&plan);
        assert!(
            watched
                .arguments
                .iter()
                .any(|argument| argument.contains("Xlog:gc"))
        );
    }

    #[test]
    fn megabytes_read_as_a_decision_not_as_a_measurement() {
        assert_eq!(gigabytes(8192), "8 GB");
        assert_eq!(gigabytes(6451), "6.3 GB");
        assert_eq!(gigabytes(512), "0.5 GB");
    }
}
