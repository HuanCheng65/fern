//! GC 日志的注入与解析（设计文档 §6.1、§6.2）。
//!
//! Fern 是拉起 JVM 的那一方，所以拿运行时数据的成本几乎为零：多给一个 `-Xlog`
//! 参数而已。不走 JMX（要注入 management agent 并开端口，侵入性过高），不依赖
//! spark 之类的 Mod（基础设施不能建在用户装不装某个 Mod 上），也不只看进程
//! RSS（那分不出堆内水位和堆外开销）。
//!
//! 两代格式都认。Java 9 起是统一日志：
//!
//! ```text
//! [2.345s] GC(3) Pause Young (Normal) (G1 Evacuation Pause) 246M->48M(512M) 12.345ms
//! [8.901s] GC(9) Major Collection (Allocation Rate) 1204M(30%)->402M(10%) 45.678ms
//! ```
//!
//! Java 8 是另一套：
//!
//! ```text
//! [GC (Allocation Failure)  262144K->31768K(1005056K), 0.0295492 secs]
//! ```
//!
//! 形状不同，但要的东西在同一个位置：一个 `前->后(总)` 的三元组和一个时长。
//! 所以解析器只认这两样，别的一律跳过——**日志格式会变，认得少才活得久**。

use std::path::Path;

/// 一次会话从日志里读出来的那些数。
#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMetrics {
    /// 会话内堆使用的峰值，MB。
    pub peak_mb: u32,
    /// 回收之后的堆水位取 p90，MB。这是对 live set 的估计。
    ///
    /// 用 p90 而不是任何单次值：探图和蹲家的内存曲线差别极大，单次会话的
    /// 噪声比信号大。
    pub live_set_mb: u32,
    /// 停顿时长 p99，毫秒。
    pub pause_p99_ms: f64,
    pub collections: u32,
    /// ZGC 的 allocation stall 次数：分配速率追上回收速率的证据。
    ///
    /// 只在 JVM 恰好把它写进日志时才有值。这一项是加分项不是依据——`peak_mb`
    /// 贴近上限已经说明同一件事。
    pub stalls: u32,
}

impl SessionMetrics {
    pub fn is_empty(&self) -> bool {
        self.collections == 0
    }
}

/// 注入日志参数。
///
/// 落在 Fern 自己的实例日志目录里，不进游戏的 `logs/`——那是游戏的地方，
/// 我们往里丢文件，用户清理日志时会连我们的一起清掉，反过来也一样。
pub fn log_arguments(java_major: u16, path: &Path) -> Vec<String> {
    let path = path.display();
    if java_major >= 9 {
        vec![format!(
            "-Xlog:gc*:file={path}:time,uptime:filecount=3,filesize=10M"
        )]
    } else {
        vec![
            format!("-Xloggc:{path}"),
            "-XX:+PrintGCDetails".to_owned(),
            "-XX:+PrintGCTimeStamps".to_owned(),
        ]
    }
}

/// 把一份日志读成一次会话的统计。一条也没读到就返回 `None`。
pub fn parse(text: &str) -> Option<SessionMetrics> {
    let mut peak_mb = 0u32;
    let mut after = Vec::new();
    let mut pauses = Vec::new();
    let mut stalls = 0u32;

    for line in text.lines() {
        if line.contains("Allocation Stall") {
            stalls += 1;
        }
        let Some((before, settled)) = heap_transition(line) else {
            continue;
        };
        peak_mb = peak_mb.max(before);
        after.push(settled);
        if let Some(pause) = pause_milliseconds(line) {
            pauses.push(pause);
        }
    }

    if after.is_empty() {
        return None;
    }
    Some(SessionMetrics {
        peak_mb,
        live_set_mb: percentile(&mut after, 0.90),
        pause_p99_ms: percentile_f64(&mut pauses, 0.99),
        collections: after.len() as u32,
        stalls,
    })
}

/// 一行里的 `前->后`，两边都换算成 MB。
fn heap_transition(line: &str) -> Option<(u32, u32)> {
    let arrow = line.find("->")?;
    let before = megabytes_before(&line[..arrow])?;
    let after = megabytes_after(&line[arrow + 2..])?;
    Some((before, after))
}

/// 箭头左边那个量。可能带一个百分比尾巴：`1204M(30%)`。
fn megabytes_before(segment: &str) -> Option<u32> {
    let segment = segment.trim_end();
    let segment = match segment.strip_suffix(')') {
        // 只吃掉百分比括号。`(512M)` 那种是总量，出现在箭头右边，不在这里。
        Some(head) if head.ends_with('%') => &head[..head.rfind('(')?],
        _ => segment,
    };
    let bytes = segment.as_bytes();
    let unit = *bytes.last()?;
    let digits_end = bytes.len() - 1;
    let mut start = digits_end;
    while start > 0 && (bytes[start - 1].is_ascii_digit() || bytes[start - 1] == b'.') {
        start -= 1;
    }
    scale(segment[start..digits_end].parse().ok()?, unit)
}

/// 箭头右边那个量。后面通常还跟着 `(总量)` 和时长，都不管。
fn megabytes_after(segment: &str) -> Option<u32> {
    let segment = segment.trim_start();
    let bytes = segment.as_bytes();
    let mut end = 0;
    while end < bytes.len() && (bytes[end].is_ascii_digit() || bytes[end] == b'.') {
        end += 1;
    }
    if end == 0 {
        return None;
    }
    scale(segment[..end].parse().ok()?, *bytes.get(end)?)
}

fn scale(value: f64, unit: u8) -> Option<u32> {
    let megabytes = match unit {
        b'K' | b'k' => value / 1024.0,
        b'M' | b'm' => value,
        b'G' | b'g' => value * 1024.0,
        b'B' => value / (1024.0 * 1024.0),
        _ => return None,
    };
    Some(megabytes.round() as u32)
}

/// 这一行报的停顿时长，毫秒。
fn pause_milliseconds(line: &str) -> Option<f64> {
    // Java 8：`, 0.0295492 secs]`
    if let Some(head) = line
        .split(" secs")
        .next()
        .filter(|_| line.contains(" secs"))
    {
        let seconds = head.rsplit([' ', ',']).find(|token| !token.is_empty())?;
        if let Ok(seconds) = seconds.parse::<f64>() {
            return Some(seconds * 1000.0);
        }
    }
    // 统一日志：行尾的 `12.345ms`。
    let token = line.split_whitespace().last()?;
    token.strip_suffix("ms")?.parse().ok()
}

fn percentile(values: &mut [u32], fraction: f64) -> u32 {
    if values.is_empty() {
        return 0;
    }
    values.sort_unstable();
    values[index_for(values.len(), fraction)]
}

fn percentile_f64(values: &mut [f64], fraction: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|left, right| left.total_cmp(right));
    values[index_for(values.len(), fraction)]
}

fn index_for(length: usize, fraction: f64) -> usize {
    (((length as f64) * fraction).ceil() as usize)
        .saturating_sub(1)
        .min(length - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    const UNIFIED: &str = "\
[2026-08-07T12:00:00.000+0800][0.512s] Using G1
[2026-08-07T12:00:01.000+0800][1.512s] GC(0) Pause Young (Normal) (G1 Evacuation Pause) 246M->48M(512M) 12.345ms
[2026-08-07T12:00:09.000+0800][9.512s] GC(1) Pause Young (Normal) (G1 Evacuation Pause) 500M->120M(1024M) 8.100ms
[2026-08-07T12:00:20.000+0800][20.51s] GC(2) Pause Full (System.gc()) 900M->300M(1024M) 210.500ms
";

    const ZGC: &str = "\
[3.100s] GC(0) Minor Collection (Allocation Rate) 1204M(30%)->402M(10%) 45.678ms
[9.200s] GC(1) Major Collection (Proactive) 2048M(50%)->600M(15%) 60.100ms
[9.900s] Allocation Stall (Render thread) 12.300ms
";

    const JAVA_8: &str = "\
2.345: [GC (Allocation Failure)  262144K->31768K(1005056K), 0.0295492 secs]
9.876: [Full GC (System.gc())  524288K->102400K(1005056K), 0.4501230 secs]
";

    #[test]
    fn unified_logs_yield_peak_and_live_set() {
        let metrics = parse(UNIFIED).expect("three collections");
        assert_eq!(metrics.collections, 3);
        assert_eq!(metrics.peak_mb, 900);
        // 回收后的水位是 48 / 120 / 300，p90 落在最大的那个。
        assert_eq!(metrics.live_set_mb, 300);
        assert!((metrics.pause_p99_ms - 210.5).abs() < 0.01);
    }

    #[test]
    fn zgc_percentages_do_not_confuse_the_scanner() {
        let metrics = parse(ZGC).expect("two collections");
        assert_eq!(metrics.collections, 2);
        assert_eq!(metrics.peak_mb, 2048);
        assert_eq!(metrics.live_set_mb, 600);
        assert_eq!(metrics.stalls, 1);
    }

    #[test]
    fn java_eight_logs_report_seconds_not_milliseconds() {
        let metrics = parse(JAVA_8).expect("two collections");
        assert_eq!(metrics.collections, 2);
        // 512 MB 和 256 MB，从 K 换算过来。
        assert_eq!(metrics.peak_mb, 512);
        assert_eq!(metrics.live_set_mb, 100);
        assert!((metrics.pause_p99_ms - 450.123).abs() < 0.01);
    }

    #[test]
    fn a_log_without_collections_says_so_instead_of_reporting_zeroes() {
        assert!(parse("").is_none());
        assert!(parse("[0.512s] Using G1\n[0.513s] Heap region size: 4M").is_none());
    }

    #[test]
    fn the_log_argument_matches_the_java_generation() {
        let modern = log_arguments(21, Path::new("/tmp/gc.log"));
        assert_eq!(modern.len(), 1);
        assert!(modern[0].starts_with("-Xlog:gc*:file=/tmp/gc.log"));
        let ancient = log_arguments(8, Path::new("/tmp/gc.log"));
        assert!(ancient.contains(&"-Xloggc:/tmp/gc.log".to_owned()));
        assert!(ancient.contains(&"-XX:+PrintGCDetails".to_owned()));
    }
}
