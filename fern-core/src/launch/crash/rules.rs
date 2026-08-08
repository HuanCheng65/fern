//! 认原因的那张表。
//!
//! 形状只有一句话：**一条规则等于一个必须命中的正则，加若干只做收窄的守卫。**
//! 没有 and/or 嵌套，没有表达式语言——那种东西写着爽、读着痛，还要连它的求值器
//! 和错误信息一起养。守卫只有三种（在哪份文本里找、哪个加载器、哪些 MC 版本），
//! 而且只能让规则更窄。于是「这条规则会不会命中」永远只要看一个正则。
//!
//! 取值只有一种机制：**命名捕获组**。捕获到的东西作为参数交给界面，这就是
//! 「从分类到指名」的全部实现——上一版只说得出「模组依赖缺失」，现在说得出
//! 「缺少前置模组：fabric-api」。
//!
//! **这一层不产出句子。** 规则给出的是一个文案 id 加一组参数，措辞和翻译都在
//! 界面那边（`fern-ui/src/lib/i18n/`）。理由有两个：一是全应用的文案本来就在
//! 那里，两处各存一份迟早对不上；二是同一条诊断要能换语言，而规则表不该为此
//! 每加一种语言就长一倍。
//!
//! 不是首个命中获胜，是**排序**：同一次崩溃可以同时命中几条，认得出具体是哪两个
//! 东西撞了的那条该排在「GL 调用失败」前面。三档用名字而不是数字，因为数字优先级
//! 迟早变成一场没人敢动的调参。
//!
//! 纯事实的判断（Java 太老、堆超过物理内存）**不在这里**，它们属于启动前预检查。
//! 每条规则都必须有一条文本证据，这条不变式让 schema 保持简单。

use std::{
    collections::{BTreeMap, HashMap},
    sync::LazyLock,
};

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::{LoaderKind, launch::ranges};

use super::parse::Evidence;

/// 规则表跟着二进制走。用户装的是一个启动器，不是一个需要配套数据目录的东西。
const TABLE: &str = include_str!("../../../rules/crash.toml");

/// 认得有多准。排序用，也决定界面上先说哪一条。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// 只认得出类别：「GL 调用失败」。
    Generic,
    /// 认得出类别，而且说得出主语：「缺少前置：fabric-api」。
    Named,
    /// 认得出具体是哪两个东西撞在一起了。
    Exact,
}

/// 在哪一份证据里找。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Scope {
    /// 游戏自己写的崩溃报告。
    Report,
    /// 控制台输出。
    Console,
    /// JVM 的原生崩溃日志。
    HsErr,
    #[default]
    Any,
}

/// 认出来之后能替用户做的那一件事。
///
/// 封闭枚举，不是自由字符串：每一种背后都要有真实的界面和命令，开放集等于允许
/// 写出点了没反应的按钮。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Action {
    /// 去补给站找这个模组装上。
    InstallMod {
        query: String,
    },
    /// 从 mods 目录里移掉这个文件。
    RemoveMod {
        file: String,
    },
    /// 换一个大版本的 Java。
    UseJava {
        major: u16,
    },
    /// 把这个实例的内存上限改成这么多 MB。
    SetMemory {
        mb: u32,
    },
    /// 打开某个文件或目录（多半是坏掉的那份配置）。
    OpenPath {
        path: String,
    },
    OpenUrl {
        url: String,
    },
    /// 把 `mods/` 恢复成某一张快照里的样子。
    ///
    /// **只动模组。** 存档不在范围内——把世界一起回滚会让这颗按钮从「修复」
    /// 变成「损失」，而按下它的人正是在找一条退路。范围之内、快照里没有的
    /// 文件会被删掉，那正是「多出来的东西」要的处理。
    ///
    /// 不出现在 `crash.toml` 里：快照 id 不可能来自正则捕获组，它只能由知道
    /// 「变化发生在哪一刻」的那一方算出来。
    RestoreMods {
        /// 快照 id，也就是拍摄时刻的 Unix 秒。
        snapshot: String,
    },
}

/// 一条认出来的原因。
///
/// 没有句子，只有 id 和参数：文案在界面那边按 `crash.<id>` 查，参数就是正则里
/// 捕获到的那些命名组。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnosis {
    /// 规则 id，同时也是文案 id：`crash.<id>`。
    pub id: String,
    pub level: Level,
    /// 命名捕获组的值，供文案插值。
    pub args: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<Action>,
}

/// 这次崩溃发生在什么上下文里。守卫要用。
///
/// 默认值是「什么都不知道」：加载器算原版、版本为空。语料扫描那类场合手上只有
/// 一份文本，那时候带守卫的规则本来就不该命中。
#[derive(Default)]
pub struct Context {
    pub loader: LoaderKind,
    pub minecraft: String,
}

/// 规则文件里一条的原样。
#[derive(Debug, Deserialize)]
struct RawRule {
    id: String,
    level: Level,
    #[serde(default)]
    scope: Scope,
    /// 只在这些加载器上考虑。空表示不限。
    #[serde(default)]
    loader: Vec<String>,
    /// 只在这些 MC 版本上考虑，写成区间。
    #[serde(default)]
    minecraft: Option<String>,
    #[serde(rename = "match")]
    pattern: String,
    #[serde(default)]
    action: Option<RawAction>,
}

/// 动作的原样：字段先是字符串，因为它们可能整段来自捕获组。
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum RawAction {
    InstallMod { query: String },
    RemoveMod { file: String },
    UseJava { major: String },
    SetMemory { mb: String },
    OpenPath { path: String },
    OpenUrl { url: String },
}

#[derive(Debug, Deserialize)]
struct RuleFile {
    rule: Vec<RawRule>,
}

struct Rule {
    raw: RawRule,
    pattern: Regex,
}

fn table() -> &'static [Rule] {
    static RULES: LazyLock<Vec<Rule>> =
        LazyLock::new(|| compile(TABLE).expect("内置规则表必须可用"));
    &RULES
}

fn compile(text: &str) -> Result<Vec<Rule>, String> {
    let file: RuleFile = toml::from_str(text).map_err(|error| format!("规则表读不动：{error}"))?;
    file.rule
        .into_iter()
        .map(|raw| {
            let pattern = Regex::new(&raw.pattern)
                .map_err(|error| format!("规则 {} 的正则不合法：{error}", raw.id))?;
            Ok(Rule { raw, pattern })
        })
        .collect()
}

/// 表里全部规则的 id。界面那边要为每一条备好文案。
pub fn ids() -> Vec<String> {
    table().iter().map(|rule| rule.raw.id.clone()).collect()
}

/// 按规则表认一遍，排好序。
///
/// 排序：认得越具体的越靠前；同档时按证据来源——崩溃报告是游戏自己写的，比
/// 控制台可信。
pub fn apply(evidence: &Evidence<'_>, context: &Context) -> Vec<Diagnosis> {
    let mut found: Vec<(Level, u8, Diagnosis)> = Vec::new();
    for rule in table() {
        if !allows(&rule.raw, context) {
            continue;
        }
        for (rank, text) in texts(evidence, rule.raw.scope) {
            let Some(captures) = rule.pattern.captures(text) else {
                continue;
            };
            let values = named(&rule.pattern, &captures);
            found.push((
                rule.raw.level,
                rank,
                Diagnosis {
                    id: rule.raw.id.clone(),
                    level: rule.raw.level,
                    action: rule
                        .raw
                        .action
                        .as_ref()
                        .and_then(|action| bind(action, &values)),
                    args: values.into_iter().collect(),
                },
            ));
            break;
        }
    }
    found.sort_by(|left, right| right.0.cmp(&left.0).then(left.1.cmp(&right.1)));
    found
        .into_iter()
        .map(|(_, _, diagnosis)| diagnosis)
        .collect()
}

/// 守卫。只会让规则更窄，不会让它更宽。
fn allows(rule: &RawRule, context: &Context) -> bool {
    if !rule.loader.is_empty() {
        let current = format!("{:?}", context.loader).to_lowercase();
        if !rule
            .loader
            .iter()
            .any(|name| name.to_lowercase() == current)
        {
            return false;
        }
    }
    match &rule.minecraft {
        Some(range) => ranges::satisfies(range, &context.minecraft),
        None => true,
    }
}

/// 该在哪些文本里找，以及它们的可信次序（小的更可信）。
fn texts<'a>(evidence: &Evidence<'a>, scope: Scope) -> Vec<(u8, &'a str)> {
    let report = evidence.report.map(|text| (0, text));
    let console = Some((1, evidence.console));
    let hs_err = evidence.hs_err.map(|text| (2, text));
    match scope {
        Scope::Report => report.into_iter().collect(),
        Scope::Console => console.into_iter().collect(),
        Scope::HsErr => hs_err.into_iter().collect(),
        Scope::Any => report.into_iter().chain(console).chain(hs_err).collect(),
    }
}

fn named(pattern: &Regex, captures: &regex::Captures<'_>) -> HashMap<String, String> {
    pattern
        .capture_names()
        .flatten()
        .filter_map(|name| {
            captures
                .name(name)
                .map(|value| (name.to_owned(), value.as_str().trim().to_owned()))
        })
        .collect()
}

/// `{need}` 换成捕获到的东西。没捕获到的原样留着——留着比留一个空更容易发现。
fn fill(template: &str, values: &HashMap<String, String>) -> String {
    let mut output = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        output.push_str(&rest[..start]);
        let Some(end) = rest[start..].find('}').map(|offset| start + offset) else {
            output.push_str(&rest[start..]);
            return output;
        };
        let key = &rest[start + 1..end];
        match values.get(key) {
            Some(value) => output.push_str(value),
            None => output.push_str(&rest[start..=end]),
        }
        rest = &rest[end + 1..];
    }
    output.push_str(rest);
    output
}

/// 动作里的占位符也要换掉，换完还得是个能用的值。
///
/// 数字换完不是数字就整条丢掉：**宁可没有按钮，也不能有一个点了会出错的按钮。**
fn bind(action: &RawAction, values: &HashMap<String, String>) -> Option<Action> {
    Some(match action {
        // 日志里点的名可能是 Fabric API 的某个模块（`fabric-rendering-v1`），
        // 而那不是一个能在补给站搜到的东西。要装的是哪一个由 preflight 那条
        // 现成的映射回答——同一件事实，不该在两处各写一份。
        RawAction::InstallMod { query } => Action::InstallMod {
            query: crate::launch::preflight::supplier(&fill(query, values)).to_owned(),
        },
        RawAction::RemoveMod { file } => Action::RemoveMod {
            file: fill(file, values),
        },
        RawAction::UseJava { major } => Action::UseJava {
            major: fill(major, values).parse().ok()?,
        },
        RawAction::SetMemory { mb } => Action::SetMemory {
            mb: fill(mb, values).parse().ok()?,
        },
        RawAction::OpenPath { path } => Action::OpenPath {
            path: fill(path, values),
        },
        RawAction::OpenUrl { url } => Action::OpenUrl {
            url: fill(url, values),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placeholders(template: &str) -> Vec<String> {
        let mut names = Vec::new();
        let mut rest = template;
        while let Some(start) = rest.find('{') {
            let Some(end) = rest[start..].find('}').map(|offset| start + offset) else {
                break;
            };
            names.push(rest[start + 1..end].to_owned());
            rest = &rest[end + 1..];
        }
        names
    }

    /// 表本身必须编得动，而且每条规则的正则必须合法。
    #[test]
    fn the_bundled_table_compiles() {
        let rules = compile(TABLE).expect("规则表");
        assert!(!rules.is_empty());
        let mut seen = std::collections::HashSet::new();
        for rule in &rules {
            assert!(
                seen.insert(rule.raw.id.clone()),
                "{} 出现了两次",
                rule.raw.id
            );
            // id 会被拼进文案 id（`crash.<id>`），也会出现在 fixture 的文件名里。
            assert!(
                rule.raw
                    .id
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'),
                "{} 不是 kebab-case",
                rule.raw.id
            );
        }
    }

    /// **写错的占位符要在这里死掉，不是在用户屏幕上。**
    ///
    /// 动作里的 `{need}` 拼错了，按钮会带着一个字面量去搜索。文案那一侧的同类
    /// 检查在界面那边（`i18n/keys.ts` 与文案表的类型对齐）。
    #[test]
    fn every_placeholder_has_a_capture_group_behind_it() {
        for rule in compile(TABLE).expect("规则表") {
            let groups: Vec<&str> = rule.pattern.capture_names().flatten().collect();
            let mut templates: Vec<String> = Vec::new();
            if let Some(action) = &rule.raw.action {
                templates.push(match action {
                    RawAction::InstallMod { query } => query.clone(),
                    RawAction::RemoveMod { file } => file.clone(),
                    RawAction::UseJava { major } => major.clone(),
                    RawAction::SetMemory { mb } => mb.clone(),
                    RawAction::OpenPath { path } => path.clone(),
                    RawAction::OpenUrl { url } => url.clone(),
                });
            }
            for template in templates {
                for name in placeholders(&template) {
                    assert!(
                        groups.contains(&name.as_str()),
                        "规则 {} 用了 {{{name}}}，但正则里没有这个命名组",
                        rule.raw.id
                    );
                }
            }
        }
    }

    #[test]
    fn a_more_specific_rule_wins_over_a_general_one() {
        assert!(Level::Exact > Level::Named);
        assert!(Level::Named > Level::Generic);
    }

    #[test]
    fn placeholders_are_replaced_and_unknown_ones_are_left_alone() {
        let values = HashMap::from([("need".to_owned(), "fabric-api".to_owned())]);
        assert_eq!(fill("缺少前置：{need}", &values), "缺少前置：fabric-api");
        assert_eq!(fill("{missing}", &values), "{missing}");
    }

    #[test]
    fn an_action_whose_number_did_not_survive_is_dropped() {
        // 宁可没有按钮，也不能有一个点了会出错的按钮。
        let values = HashMap::from([("major".to_owned(), "不是数字".to_owned())]);
        assert!(
            bind(
                &RawAction::UseJava {
                    major: "{major}".to_owned()
                },
                &values
            )
            .is_none()
        );
    }

    /// fixture 的第一行可以写一句 `#! loader=fabric mc=1.21.1 scope=console`，
    /// 说明这段文本该被当成哪一种证据、在什么上下文里。缺省是 Fabric、1.21.1、
    /// 崩溃报告。
    fn subject(text: &str) -> (Evidence<'_>, Context) {
        let (header, body) = match text.strip_prefix("#!") {
            Some(rest) => match rest.split_once('\n') {
                Some((header, body)) => (header, body),
                None => (rest, ""),
            },
            None => ("", text),
        };
        let field = |key: &str, fallback: &str| {
            header
                .split_whitespace()
                .find_map(|pair| pair.strip_prefix(key)?.strip_prefix('='))
                .unwrap_or(fallback)
                .to_owned()
        };
        let loader = match field("loader", "fabric").as_str() {
            "quilt" => LoaderKind::Quilt,
            "forge" => LoaderKind::Forge,
            "neoforge" => LoaderKind::NeoForge,
            "vanilla" => LoaderKind::Vanilla,
            _ => LoaderKind::Fabric,
        };
        let evidence = match field("scope", "report").as_str() {
            "console" => Evidence {
                report: None,
                console: body,
                hs_err: None,
            },
            "hs-err" => Evidence {
                report: None,
                console: "",
                hs_err: Some(body),
            },
            _ => Evidence {
                report: Some(body),
                console: "",
                hs_err: None,
            },
        };
        (
            evidence,
            Context {
                loader,
                minecraft: field("mc", "1.21.1"),
            },
        )
    }

    fn fixtures() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("rules/fixtures")
    }

    /// **每条规则都必须有一份对应的文本，而且必须命中它。**
    ///
    /// 没有 fixture 的规则不许进表——这条纪律同时也是语料计划的执行方式：语料
    /// 不是「攒够了再开始」，是「加一条规则配一份」。
    #[test]
    fn every_rule_has_a_fixture_that_it_matches() {
        for rule in compile(TABLE).expect("规则表") {
            let path = fixtures().join(format!("{}.txt", rule.raw.id));
            let text = std::fs::read_to_string(&path).unwrap_or_else(|_| {
                panic!("规则 {} 没有 fixture：{}", rule.raw.id, path.display())
            });
            let (evidence, context) = subject(&text);
            let found = apply(&evidence, &context);
            assert!(
                found.iter().any(|diagnosis| diagnosis.id == rule.raw.id),
                "{} 的 fixture 没能命中它自己，认出来的是 {:?}",
                rule.raw.id,
                found.iter().map(|d| &d.id).collect::<Vec<_>>()
            );
        }
    }

    /// 反过来：没有规则要的 fixture 是遗留垃圾，删规则时要一起删。
    #[test]
    fn no_fixture_is_left_behind() {
        let ids: std::collections::HashSet<String> = compile(TABLE)
            .expect("规则表")
            .into_iter()
            .map(|rule| rule.raw.id)
            .collect();
        for entry in std::fs::read_dir(fixtures())
            .expect("fixtures 目录")
            .flatten()
        {
            let name = entry.file_name().to_string_lossy().into_owned();
            // `_` 开头的是给别的测试用的，不属于任何规则。
            if name.starts_with('_') {
                continue;
            }
            let id = name.trim_end_matches(".txt");
            assert!(ids.contains(id), "{name} 没有对应的规则了");
        }
    }

    /// **一份正常退出的日志不许命中任何规则。**
    ///
    /// 规则表最常见的腐化方式是某条正则写得太宽，把好日志也认成崩溃。
    #[test]
    fn a_clean_session_is_not_diagnosed() {
        let text = std::fs::read_to_string(fixtures().join("_clean.txt")).expect("干净的日志");
        let (evidence, context) = subject(&text);
        let found = apply(&evidence, &context);
        assert!(found.is_empty(), "正常的日志被认成了 {found:?}");
    }

    /// **不许有任何一条规则锚在会被翻译的句子上。**
    ///
    /// fabric-loader 的解析错误正文走 `Localization.format`，按系统语言翻译。
    /// 锚在那些句子上的规则在非英文用户那里会静默失效——不是少认一条，是一条
    /// 都不认。这件事查不出来：CI 跑在英文环境里，语料也是英文的。
    ///
    /// 所以把 loader 的英文文案表原样放进 `rules/fabric-messages.properties`，
    /// 当禁区用：占位符换成一个像 modid 的词之后，规则的正则不许命中其中任何
    /// 一句。该锚的是异常类名和 loader 自己打的结构化行，那些不进翻译表。
    #[test]
    fn no_rule_anchors_on_a_translatable_string() {
        const BUNDLE: &str = include_str!("../../../rules/fabric-messages.properties");
        // `{0}`、`{5, choice, 1# is|2#s are}` 一律换成一个既像 modid 又像散文的
        // 词：不换的话正则里的 `[A-Za-z0-9_.-]+` 碰上 `{0}` 不会命中，这条测试
        // 就成了摆设。
        let placeholder = Regex::new(r"\{[^{}]*\}").expect("占位符");
        let rules = compile(TABLE).expect("规则表");
        for line in BUNDLE.lines() {
            let line = line.trim();
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            // MessageFormat 里单引号写成两个。
            let sentence = placeholder
                .replace_all(value, "fabric-api")
                .replace("''", "'");
            for rule in &rules {
                assert!(
                    !rule.pattern.is_match(&sentence),
                    "规则 {} 锚在 {key} 上，那句话会被翻译：{sentence}",
                    rule.raw.id
                );
            }
        }
    }

    /// 日志点的是模块名，能装的是整个 Fabric API。
    #[test]
    fn an_install_action_names_something_that_can_be_installed() {
        let text = std::fs::read_to_string(fixtures().join("fabric-suggested-fix.txt"))
            .expect("suggested fix fixture");
        let (evidence, context) = subject(&text);
        let found = apply(&evidence, &context);
        let fix = found
            .iter()
            .find(|diagnosis| diagnosis.id == "fabric-suggested-fix")
            .expect("命中");
        // 说的仍然是日志里那个模块……
        assert_eq!(fix.args["need"], "fabric-rendering-v1");
        // ……但按钮指向真的能下下来的那一个。
        assert_eq!(
            fix.action,
            Some(Action::InstallMod {
                query: "fabric-api".to_owned()
            })
        );
    }

    /// 中文日志里认出来的东西，不比英文日志少。
    #[test]
    fn a_translated_log_is_diagnosed_just_the_same() {
        let text = std::fs::read_to_string(fixtures().join("fabric-incompatible-mods.txt"))
            .expect("中文 fixture");
        let (evidence, context) = subject(&text);
        assert!(
            apply(&evidence, &context)
                .iter()
                .any(|diagnosis| diagnosis.id == "fabric-incompatible-mods")
        );
    }

    /// 认得越具体的排越前面。
    #[test]
    fn the_more_specific_diagnosis_comes_first() {
        let text = std::fs::read_to_string(fixtures().join("mixin-failure-named.txt"))
            .expect("mixin fixture");
        let (evidence, context) = subject(&text);
        let found = apply(&evidence, &context);
        // 这一份同时命中 named 与 generic 两条，具体的那条要在前面。
        assert_eq!(found[0].id, "mixin-failure-named");
        assert!(found.iter().any(|d| d.id == "mixin-failure"));
        assert_eq!(found[0].args["config"], "sodium");
    }
}
