//! 游戏日志的解析（文档 §5.4）。
//!
//! 进程的 stdout/stderr 必须一直读——不读，管道缓冲满了游戏就卡死。既然
//! 无论如何都要读，顺手把每行分出等级和内容，日志查看器才有得着色和过滤。
//!
//! 两种格式都认：
//!
//!   纯文本  `[12:34:56] [Render thread/INFO]: 内容`，这是 Minecraft 自带
//!           log4j 配置的默认输出，绝大多数情况看到的都是它。
//!   XML     `<log4j:Event level="WARN" …><log4j:Message><![CDATA[…]]>`，
//!           某些启动器会注入带 XMLLayout 的配置。我们自己不注入（那要占掉
//!           `-Dlog4j.configurationFile`，而那个位置留给了 Log4Shell 的
//!           缓解配置），但用户换了配置我们也该认得。
//!
//! 认不出来的行不丢：原样交出去，等级按它来自哪个流猜。日志查看器宁可显示
//! 一行等级不准的，也不能吞掉一行。

use crate::LogLevel;

/// 一行解析好的日志。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    pub level: LogLevel,
    pub message: String,
}

/// 逐行喂进来，攒够一条就吐出来。
///
/// XML 事件跨多行，所以这里必须是个有状态的东西，不能是纯函数。
#[derive(Debug, Default)]
pub struct LogParser {
    pending: Option<String>,
}

impl LogParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// `stderr` 用来给认不出格式的行兜底定级。
    pub fn push(&mut self, line: &str, stderr: bool) -> Option<LogLine> {
        if let Some(buffer) = &mut self.pending {
            buffer.push('\n');
            buffer.push_str(line);
            if !line.contains("</log4j:Event>") {
                return None;
            }
            let complete = self.pending.take()?;
            return parse_xml_event(&complete);
        }

        let trimmed = line.trim_start();
        if trimmed.starts_with("<log4j:Event") {
            if !line.contains("</log4j:Event>") {
                self.pending = Some(line.to_owned());
                return None;
            }
            return parse_xml_event(line);
        }

        Some(parse_plain(line, stderr))
    }

    /// 进程结束时可能还攒着半条 XML，别让它消失。
    pub fn flush(&mut self, stderr: bool) -> Option<LogLine> {
        let pending = self.pending.take()?;
        parse_xml_event(&pending).or_else(|| Some(parse_plain(&pending, stderr)))
    }
}

/// `[12:34:56] [Render thread/INFO]: 内容`
fn parse_plain(line: &str, stderr: bool) -> LogLine {
    let level = extract_bracketed_level(line).unwrap_or({
        // 认不出格式时按流分：stderr 上的东西通常是异常栈和 JVM 的抱怨。
        if stderr {
            LogLevel::Warn
        } else {
            LogLevel::Info
        }
    });
    LogLine {
        level,
        message: line.to_owned(),
    }
}

/// 从 `[thread/LEVEL]` 里取等级。只认方括号里带斜杠的那一段，避免把消息
/// 正文里偶然出现的 `ERROR` 当成等级。
fn extract_bracketed_level(line: &str) -> Option<LogLevel> {
    let mut rest = line;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let close = after.find(']')?;
        let inside = &after[..close];
        if let Some(level) = inside
            .rsplit_once('/')
            .and_then(|(_, name)| level_from_name(name))
        {
            return Some(level);
        }
        rest = &after[close..];
    }
    None
}

fn parse_xml_event(text: &str) -> Option<LogLine> {
    let level = attribute(text, "level")
        .and_then(|value| level_from_name(&value))
        .unwrap_or(LogLevel::Info);
    let message = between(text, "<![CDATA[", "]]>")
        .or_else(|| between(text, "<log4j:Message>", "</log4j:Message>"))
        .unwrap_or_else(|| text.to_owned());
    Some(LogLine {
        level,
        message: message.trim().to_owned(),
    })
}

fn attribute(text: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=\"");
    let start = text.find(&needle)? + needle.len();
    let end = text[start..].find('"')? + start;
    Some(text[start..end].to_owned())
}

fn between(text: &str, open: &str, close: &str) -> Option<String> {
    let start = text.find(open)? + open.len();
    let end = text[start..].find(close)? + start;
    Some(text[start..end].to_owned())
}

fn level_from_name(name: &str) -> Option<LogLevel> {
    match name.trim().to_ascii_uppercase().as_str() {
        "TRACE" => Some(LogLevel::Trace),
        "DEBUG" => Some(LogLevel::Debug),
        "INFO" => Some(LogLevel::Info),
        "WARN" | "WARNING" => Some(LogLevel::Warn),
        "ERROR" | "SEVERE" | "FATAL" => Some(LogLevel::Error),
        _ => None,
    }
}

/// 这一行说明窗口已经开出来了。
///
/// 判断启动成功用得上：进程还活着不等于玩家看得到画面，很多崩溃发生在
/// 初始化完成之前。这几条都出现在渲染真正跑起来之后。
pub fn signals_window_ready(message: &str) -> bool {
    const MARKERS: [&str; 4] = [
        "Setting user:",
        "OpenAL initialized",
        "Created: 1024x512 minecraft:textures",
        "[Render thread/INFO]: Started ",
    ];
    MARKERS.iter().any(|marker| message.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_level_out_of_minecrafts_default_format() {
        let mut parser = LogParser::new();
        let line = parser
            .push("[12:34:56] [Render thread/WARN]: 材质缺失", false)
            .expect("a line");
        assert_eq!(line.level, LogLevel::Warn);
        assert!(line.message.contains("材质缺失"));
    }

    #[test]
    fn a_level_word_in_the_message_body_is_not_a_level() {
        let mut parser = LogParser::new();
        let line = parser
            .push(
                "[12:34:56] [Client thread/INFO]: ERROR 是这条消息的内容",
                false,
            )
            .expect("a line");
        assert_eq!(line.level, LogLevel::Info);
    }

    #[test]
    fn unparseable_lines_survive_with_a_level_guessed_from_the_stream() {
        let mut parser = LogParser::new();
        let out = parser
            .push("\\tat net.minecraft.Foo.bar(Foo.java:1)", true)
            .expect("a line");
        assert_eq!(out.level, LogLevel::Warn);
        assert!(out.message.contains("net.minecraft.Foo"));

        let plain = parser.push("裸的一行", false).expect("a line");
        assert_eq!(plain.level, LogLevel::Info);
    }

    #[test]
    fn xml_events_are_reassembled_across_lines() {
        let mut parser = LogParser::new();
        assert!(
            parser
                .push(
                    r#"<log4j:Event logger="net.minecraft.Foo" level="ERROR" thread="main">"#,
                    false
                )
                .is_none()
        );
        assert!(
            parser
                .push("<log4j:Message><![CDATA[炸了]]></log4j:Message>", false)
                .is_none()
        );
        let line = parser
            .push("</log4j:Event>", false)
            .expect("a complete event");
        assert_eq!(line.level, LogLevel::Error);
        assert_eq!(line.message, "炸了");
    }

    #[test]
    fn a_half_written_event_is_not_swallowed_when_the_process_dies() {
        let mut parser = LogParser::new();
        parser.push(r#"<log4j:Event level="WARN">"#, false);
        let leftover = parser.flush(false).expect("the partial event");
        assert!(leftover.message.contains("log4j:Event"));
        assert!(parser.flush(false).is_none());
    }

    #[test]
    fn window_readiness_needs_a_real_marker() {
        assert!(signals_window_ready(
            "[12:00:00] [Render thread/INFO]: Setting user: Steve"
        ));
        assert!(!signals_window_ready(
            "[12:00:00] [main/INFO]: Loading libraries"
        ));
    }
}
