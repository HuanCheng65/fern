//! 崩溃分析（文档 §5.4）。
//!
//! 游戏非正常退出时，用户手上只有一个退出码和几百行栈。这一层做的事是：把
//! 能认出来的原因翻成人话，认不出来的原样交出去折叠着——**绝不**猜。写一句
//! 听起来很像那么回事、其实不对的诊断，比老实说「不认识」更浪费用户的时间。
//!
//! 规则是数据文件不是代码：崩溃模式会一直冒出新的，加一条不该要重新编译。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 规则表跟着二进制走。用户装的是一个启动器，不是一个需要配套数据目录的东西。
const RULES: &str = include_str!("../rules/crash.json");

/// 尾部保留多少字节交给界面。够看清最后一段栈，又不至于把 IPC 塞爆。
const EXCERPT_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, Deserialize)]
struct CrashRule {
    id: String,
    /// 这些**全部**出现才算命中。
    #[serde(default)]
    all: Vec<String>,
    /// 这些**任意一条**出现就算命中。
    #[serde(default)]
    any: Vec<String>,
    title: String,
    detail: String,
}

impl CrashRule {
    fn matches(&self, text: &str) -> bool {
        if self.all.is_empty() && self.any.is_empty() {
            return false;
        }
        let all_present = self.all.iter().all(|needle| text.contains(needle));
        let any_present =
            self.any.is_empty() || self.any.iter().any(|needle| text.contains(needle));
        all_present && any_present
    }
}

/// 认出来的原因。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashDiagnosis {
    pub id: String,
    pub title: String,
    pub detail: String,
}

/// 一次非正常退出的全部所知。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashReport {
    pub instance_id: String,
    pub exit_code: Option<i32>,
    /// 认不出来就是 `None`，界面照实说。
    pub diagnosis: Option<CrashDiagnosis>,
    /// 游戏自己写的崩溃报告，比我们的日志尾部完整得多。
    pub report_path: Option<PathBuf>,
    /// 原始文本的尾部，折叠展示。
    pub excerpt: String,
}

/// 按规则表认一遍。第一条命中的胜出——规则文件里的顺序就是优先级。
pub fn diagnose(text: &str) -> Option<CrashDiagnosis> {
    let rules: Vec<CrashRule> = serde_json::from_str(RULES).expect("内置崩溃规则表必须是合法 JSON");
    rules
        .into_iter()
        .find(|rule| rule.matches(text))
        .map(|rule| CrashDiagnosis {
            id: rule.id,
            title: rule.title,
            detail: rule.detail,
        })
}

/// 游戏 `crash-reports/` 里最新的那一份。
///
/// 只认这次运行之后写的：上一次崩溃留下的报告会把人引到完全错误的方向。
pub fn latest_crash_report(game_directory: &Path, after: std::time::SystemTime) -> Option<PathBuf> {
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(game_directory.join("crash-reports"))
        .ok()?
        .flatten()
    {
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        if modified < after {
            continue;
        }
        if newest.as_ref().is_none_or(|(time, _)| modified > *time) {
            newest = Some((modified, entry.path()));
        }
    }
    newest.map(|(_, path)| path)
}

/// 把这次运行知道的一切收成一份报告。
pub fn build_report(
    instance_id: &str,
    game_directory: &Path,
    started_at: std::time::SystemTime,
    exit_code: Option<i32>,
    log_tail: &str,
) -> CrashReport {
    let report_path = latest_crash_report(game_directory, started_at);
    // 游戏自己的崩溃报告比控制台尾部完整，认原因优先看它。
    let report_text = report_path
        .as_ref()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_default();
    let haystack = format!("{report_text}\n{log_tail}");

    CrashReport {
        instance_id: instance_id.to_owned(),
        exit_code,
        diagnosis: diagnose(&haystack),
        report_path,
        excerpt: tail(log_tail, EXCERPT_BYTES),
    }
}

/// 尾部若干字节，切在字符边界上。
fn tail(text: &str, bytes: usize) -> String {
    if text.len() <= bytes {
        return text.to_owned();
    }
    let mut start = text.len() - bytes;
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    // 别从半行开始，第一行截断了反而看不懂。
    match text[start..].find('\n') {
        Some(offset) => text[start + offset + 1..].to_owned(),
        None => text[start..].to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_rule_table_is_valid() {
        let rules: Vec<CrashRule> = serde_json::from_str(RULES).expect("parse bundled rules");
        assert!(!rules.is_empty());
        for rule in &rules {
            assert!(
                !rule.all.is_empty() || !rule.any.is_empty(),
                "规则 {} 匹配不到任何东西",
                rule.id
            );
            assert!(!rule.title.is_empty() && !rule.detail.is_empty());
        }
    }

    #[test]
    fn recognises_the_common_causes() {
        let memory = diagnose("java.lang.OutOfMemoryError: Java heap space").expect("a diagnosis");
        assert_eq!(memory.id, "out-of-memory");

        let java = diagnose(
            "java.lang.UnsupportedClassVersionError: Foo has been compiled by a more recent version",
        )
        .expect("a diagnosis");
        assert_eq!(java.id, "java-too-old");

        let graphics = diagnose("org.lwjgl.LWJGLException: Pixel format not accelerated")
            .expect("a diagnosis");
        assert_eq!(graphics.id, "graphics-driver");
    }

    #[test]
    fn compound_rules_need_every_required_marker() {
        // 只有 mixin 包名，没有实际的失败，不该报「模组打架」。
        assert!(diagnose("org.spongepowered.asm.mixin.transformer loaded").is_none());
        let conflict = diagnose(
            "org.spongepowered.asm.mixin.injection.throwables.InvalidInjectionException: boom",
        )
        .expect("a diagnosis");
        assert_eq!(conflict.id, "mixin-conflict");
    }

    #[test]
    fn an_unfamiliar_crash_stays_unfamiliar() {
        // 认不出来就是认不出来，不能随便挑一条最像的。
        assert!(diagnose("java.lang.NullPointerException: 谁知道呢").is_none());
        assert!(diagnose("").is_none());
    }

    #[test]
    fn the_excerpt_keeps_the_end_and_never_splits_a_character() {
        let text = "前面的内容\n".repeat(4000);
        let excerpt = tail(&text, 1024);
        assert!(excerpt.len() <= 1024);
        assert!(excerpt.ends_with("前面的内容\n"));
        // 切在字符边界上，否则 String 根本构造不出来——这一步已经证明了。
        assert!(!excerpt.starts_with('\n'));

        let short = "两行\n内容";
        assert_eq!(tail(short, 1024), short);
    }

    #[test]
    fn only_reports_written_after_this_run_count() {
        let root = std::env::temp_dir().join(format!("fern-crash-test-{}", std::process::id()));
        let reports = root.join("crash-reports");
        std::fs::create_dir_all(&reports).expect("create crash-reports");
        std::fs::write(reports.join("crash-old.txt"), "上一次的").expect("write old report");

        // 把「本次启动」定在未来，旧报告就该被忽略。
        let future = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
        assert!(latest_crash_report(&root, future).is_none());

        let past = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
        assert!(latest_crash_report(&root, past).is_some());

        std::fs::remove_dir_all(root).expect("remove test root");
    }
}
