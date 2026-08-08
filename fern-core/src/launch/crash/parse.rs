//! 把一次崩溃留下的几份文件读成事实。
//!
//! **这一层只提取，不判断。** 认原因是规则的事（`rules.rs`），归因到某个模组
//! 是 `suspect.rs` 的事。分开是因为规则表会一直改，而「崩溃报告长什么样」十年
//! 没变过——把它们绑在一起，每加一条规则都要重新相信解析。
//!
//! 崩溃报告是**分段文档加缩进树**，不是一行文本。所以这里只有两个通用原语——
//! [`Sections`] 和 [`Indented`]——其余的字段提取都是三五行。用一堆正则去啃一份
//! 本来就有结构的文档，是在把简单的事做难。
//!
//! ```text
//! ---- Minecraft Crash Report ----
//! Description: Rendering overlay          ← description
//!
//! java.lang.NullPointerException: ...     ← chain[0]
//!     at foo.Bar.baz(Bar.java:12)         ← frames
//! Caused by: java.lang.IllegalState...    ← chain[1]，根因在最后
//!
//! -- System Details --                    ← Sections
//! Details:
//!     Minecraft Version: 1.21.1           ← Indented::get
//!     Fabric Mods:
//!         sodium: Sodium 0.6.0            ← Indented::children
//! ```
//!
//! **每一个字段都是 `Option` 或空集合，解析失败绝不向上抛。** 这些文件是别人
//! 写的，什么畸形都遇得到；提不出结构时原文仍在，规则照样在原文上跑——新的一层
//! 最坏等于旧的那一层。

use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

/// 一次崩溃里能拿到的全部证据。
///
/// 不是一份文本：游戏来不及写崩溃报告时只有控制台，原生崩溃时两者都是空的、
/// 全部信息在 `hs_err_pid*.log` 里。
pub struct Evidence<'a> {
    /// `crash-reports/` 里那一份。最完整。
    pub report: Option<&'a str>,
    /// 控制台尾部，stdout 与 stderr 合流。
    pub console: &'a str,
    /// JVM 自己的崩溃日志。
    pub hs_err: Option<&'a str>,
}

/// 一条 Java 异常。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Throwable {
    /// 全限定类名，例如 `java.lang.NoSuchMethodError`。
    pub class: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub frames: Vec<Frame>,
}

/// 栈里的一帧。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Frame {
    /// 全限定类名。归因看的就是它的包前缀。
    pub class: String,
    pub method: String,
}

/// 崩溃报告里自报的一个模组。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportedMod {
    pub mod_id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// JVM 崩在哪个原生库上。
///
/// 这一行是原生崩溃唯一的线索：Java 侧什么都没有，而 `nvoglv64.dll` 就是
/// N 卡驱动、`ig9icd64.dll` 是 Intel 核显、`atio6axx.dll` 是 A 卡。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeFrame {
    /// 信号或异常种类，例如 `EXCEPTION_ACCESS_VIOLATION` / `SIGSEGV`。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal: Option<String>,
    /// `# C  [nvoglv64.dll+0x...]` 里那一整行。
    pub frame: String,
    /// 从中认出来的库名，认不出就没有。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub library: Option<String>,
}

/// Forge 一系在崩溃报告里自报的「是这个模组挂了」。
///
/// 它写成一个 `-- MOD <modid> --` 段。**这是加载器自己给出的归因**，不用翻栈，
/// 也不用本地装着那个 jar——分析一段别人贴过来的日志时同样有效。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailedMod {
    pub mod_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// 那句 `Failure message:`，加载器写给人看的。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// 从证据里提取到的一切。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Facts {
    /// 崩溃报告里那句话，例如 `Rendering overlay`。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// 异常链，最外层在前。**根因是最后一条**——归因和规则都该看它。
    pub chain: Vec<Throwable>,
    /// 报告自报的模组表。和实例里实际装的可能不一样（用户中途改过）。
    pub mods: Vec<ReportedMod>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minecraft: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub java: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_frame: Option<NativeFrame>,
    /// 加载器自己点名的那些模组。
    pub failed_mods: Vec<FailedMod>,
}

impl Facts {
    /// 根因。归因看这一条，不是最外层那条。
    pub fn root(&self) -> Option<&Throwable> {
        self.chain.last()
    }
}

/// 读一遍，什么都不判断。
pub fn extract(evidence: &Evidence<'_>) -> Facts {
    let mut facts = Facts::default();
    if let Some(report) = evidence.report {
        read_report(report, &mut facts);
    }
    // 报告里没有的才去控制台找：报告是游戏自己写的，比控制台干净。
    if facts.chain.is_empty() {
        facts.chain = throwables(evidence.console);
    }
    if let Some(hs_err) = evidence.hs_err {
        facts.native_frame = native_frame(hs_err);
    }
    facts
}

fn read_report(text: &str, facts: &mut Facts) {
    facts.description = line_value(text, "Description:");
    facts.chain = throwables(text);

    let sections = Sections::parse(text);
    facts.failed_mods = sections.failed_mods();
    let Some(details) = sections.get("System Details") else {
        return;
    };
    let details = Indented::parse(details);
    facts.minecraft = details.get("Minecraft Version").map(str::to_owned);
    facts.java = details.get("Java Version").map(str::to_owned);
    facts.mods = fabric_mods(&details)
        .or_else(|| forge_mods(&details))
        .unwrap_or_default();
}

/// `Fabric Mods:` 下面缩进一级的那一层：`sodium: Sodium 0.6.0`。
fn fabric_mods(details: &Indented<'_>) -> Option<Vec<ReportedMod>> {
    let children = details.children("Fabric Mods");
    if children.is_empty() {
        return None;
    }
    Some(
        children
            .into_iter()
            .map(|(mod_id, rest)| {
                // 值是「显示名 版本」，版本是最后一段。名字里可以有空格。
                let (name, version) = match rest.rsplit_once(' ') {
                    Some((name, version)) => (name.trim(), Some(version.trim().to_owned())),
                    None => (rest, None),
                };
                ReportedMod {
                    mod_id: mod_id.to_owned(),
                    name: if name.is_empty() {
                        mod_id.to_owned()
                    } else {
                        name.to_owned()
                    },
                    version,
                }
            })
            .collect(),
    )
}

/// Forge 1.13+ 的竖线表：
/// `sodium-0.6.0.jar |Sodium |sodium |0.6.0 |DONE |Manifest: NOSIGNATURE`
fn forge_mods(details: &Indented<'_>) -> Option<Vec<ReportedMod>> {
    let mods: Vec<ReportedMod> = details
        .block("Mod List")
        .into_iter()
        .filter_map(|line| {
            let columns: Vec<&str> = line.split('|').map(str::trim).collect();
            // 文件名 | 显示名 | modid | 版本 | …
            if columns.len() < 4 || !columns[0].ends_with(".jar") {
                return None;
            }
            Some(ReportedMod {
                mod_id: columns[2].to_owned(),
                name: columns[1].to_owned(),
                version: Some(columns[3].to_owned()).filter(|it| !it.is_empty()),
            })
        })
        .collect();
    (!mods.is_empty()).then_some(mods)
}

/// 异常链。`Caused by:` 串起来的每一段都是一条，最外层在前。
fn throwables(text: &str) -> Vec<Throwable> {
    static HEAD: LazyLock<Regex> = LazyLock::new(|| {
        // 行首（允许 `Caused by:` 前缀）一个全限定类名，可选一段消息。
        Regex::new(
            r"(?m)^(?:Caused by: )?(?<class>[a-zA-Z_$][\w.$]*(?:Exception|Error|Throwable))(?:: (?<message>.*))?$",
        )
        .expect("异常头的正则必须可编译")
    });
    static FRAME: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^\s*at (?<class>[\w.$/]+)\.(?<method>[\w$<>]+)\(")
            .expect("栈帧的正则必须可编译")
    });

    let mut chain: Vec<Throwable> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_end();
        if let Some(head) = HEAD.captures(trimmed.trim_start_matches('\t')) {
            chain.push(Throwable {
                class: head["class"].to_owned(),
                message: head
                    .name("message")
                    .map(|message| message.as_str().trim().to_owned())
                    .filter(|message| !message.is_empty()),
                frames: Vec::new(),
            });
            continue;
        }
        if let (Some(current), Some(frame)) = (chain.last_mut(), FRAME.captures(trimmed)) {
            current.frames.push(Frame {
                class: frame["class"].replace('/', "."),
                method: frame["method"].to_owned(),
            });
        }
    }
    chain
}

/// hs_err 里那几行。
fn native_frame(text: &str) -> Option<NativeFrame> {
    static SIGNAL: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"#\s+(?<signal>EXCEPTION_[A-Z_]+|SIG[A-Z]+)").expect("信号的正则必须可编译")
    });
    static LIBRARY: LazyLock<Regex> = LazyLock::new(|| {
        // `C  [nvoglv64.dll+0x1234]` / `V  [libjvm.so+0x...]`
        Regex::new(r"\[(?<library>[\w.+-]+?\.(?:dll|so|dylib))").expect("库名的正则必须可编译")
    });

    let mut lines = text.lines();
    let frame = loop {
        let line = lines.next()?;
        if line.contains("Problematic frame:") {
            // 下一行才是帧本身。
            break lines.next()?.trim_start_matches('#').trim().to_owned();
        }
    };
    Some(NativeFrame {
        signal: SIGNAL
            .captures(text)
            .map(|found| found["signal"].to_owned()),
        library: LIBRARY
            .captures(&frame)
            .map(|found| found["library"].to_owned()),
        frame,
    })
}

fn line_value(text: &str, key: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.trim().strip_prefix(key))
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// 原语一：把报告切成 `-- 段名 --` 划出来的段。
pub struct Sections<'a>(Vec<(&'a str, &'a str)>);

impl<'a> Sections<'a> {
    pub fn parse(text: &'a str) -> Self {
        let mut sections = Vec::new();
        let mut current: Option<(&str, usize)> = None;
        let mut offset = 0;
        for line in text.split_inclusive('\n') {
            let trimmed = line.trim();
            if let Some(name) = trimmed
                .strip_prefix("-- ")
                .and_then(|rest| rest.strip_suffix(" --"))
            {
                if let Some((open, start)) = current.take() {
                    sections.push((open, &text[start..offset]));
                }
                current = Some((name, offset + line.len()));
            }
            offset += line.len();
        }
        if let Some((open, start)) = current {
            sections.push((open, &text[start..]));
        }
        Self(sections)
    }

    pub fn get(&self, name: &str) -> Option<&'a str> {
        self.0
            .iter()
            .find(|(section, _)| *section == name)
            .map(|(_, body)| *body)
    }

    /// `-- MOD <modid> --` 那些段。
    fn failed_mods(&self) -> Vec<FailedMod> {
        self.0
            .iter()
            .filter_map(|(name, body)| {
                let mod_id = name.strip_prefix("MOD ")?.trim();
                let details = Indented::parse(body);
                Some(FailedMod {
                    mod_id: mod_id.to_owned(),
                    file: details.get("Mod File").map(str::to_owned),
                    // Failure message 常常跨行（第二行是「Currently, …」），
                    // 这里只要第一行——完整的那段在原文里，规则去读。
                    message: details.get("Failure message").map(str::to_owned),
                })
            })
            .collect()
    }
}

/// 原语二：按缩进分层的键值树。
///
/// System Details 就长这样。`Fabric Mods:` 这种「值在下一层」的写法，用行正则
/// 处理很难看，按缩进看就是天然的父子关系。
pub struct Indented<'a> {
    rows: Vec<Row<'a>>,
}

struct Row<'a> {
    depth: usize,
    key: &'a str,
    value: &'a str,
    /// 去掉缩进的原始一行。Forge 的竖线表要整行，拆成键值只会拆坏。
    raw: &'a str,
}

impl<'a> Indented<'a> {
    pub fn parse(text: &'a str) -> Self {
        let rows = text
            .lines()
            .filter_map(|line| {
                let raw = line.trim_start();
                if raw.is_empty() {
                    return None;
                }
                // 制表符按四格算：同一份报告里两种缩进都出现过。
                let width: usize = line
                    .chars()
                    .take_while(|character| character.is_whitespace())
                    .map(|character| if character == '\t' { 4 } else { 1 })
                    .sum();
                let (key, value) = match raw.split_once(':') {
                    Some((key, value)) => (key.trim(), value.trim()),
                    None => (raw, ""),
                };
                Some(Row {
                    depth: width / 4,
                    key,
                    value,
                    raw,
                })
            })
            .collect();
        Self { rows }
    }

    /// 同一行上的值。
    pub fn get(&self, key: &str) -> Option<&'a str> {
        self.rows
            .iter()
            .find(|row| row.key == key && !row.value.is_empty())
            .map(|row| row.value)
    }

    /// 下面深一级的那些行，按 (键, 值) 给出。
    pub fn children(&self, key: &str) -> Vec<(&'a str, &'a str)> {
        self.under(key)
            .filter(|row| row.depth == self.depth_of(key) + 1)
            .map(|row| (row.key, row.value))
            .collect()
    }

    /// 下面那一整块的原始行。
    pub fn block(&self, key: &str) -> Vec<&'a str> {
        self.under(key).map(|row| row.raw).collect()
    }

    fn depth_of(&self, key: &str) -> usize {
        self.rows
            .iter()
            .find(|row| row.key == key)
            .map(|row| row.depth)
            .unwrap_or(0)
    }

    fn under(&self, key: &str) -> impl Iterator<Item = &Row<'a>> {
        let start = self.rows.iter().position(|row| row.key == key);
        let depth = start.map(|index| self.rows[index].depth).unwrap_or(0);
        self.rows
            .iter()
            .skip(start.map(|index| index + 1).unwrap_or(usize::MAX))
            .take_while(move |row| row.depth > depth)
    }
}
