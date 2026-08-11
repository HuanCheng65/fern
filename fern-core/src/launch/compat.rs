//! 事前兼容规则（文档 §4）。
//!
//! 崩溃规则回答的是「刚才为什么崩了」，证据是日志。这一张回答的是「这么启动
//! 会崩，换个法子」，证据是**环境**：游戏版本、加载器、加载器版本、系统、
//! 架构、要用的那份 Java。两张表共用同一套区间语法，但不合成一张——输入和
//! 时机都不同，硬合并会让 `match` 语言同时要表达两件事。
//!
//! ## 形状
//!
//! 一条规则 = 若干只做收窄的守卫 + 一串**有序的**备选动作。引擎取第一个可行
//! 的：Forge 1.16.5 那条先试「换一个够老的 Java 8」，Mojang 不给这个平台发
//! Java 8 时才退到「升级 Forge」。全都落空就说清楚为什么，而不是挑一个装作
//! 可行的。
//!
//! ## 什么时候求值
//!
//! 两次，因为有些动作要在挑 Java **之前**给出约束，而有些守卫要看挑中的是
//! 哪一份 Java：
//!
//! ```text
//! 挑 Java 之前   java 那几个守卫一律不命中 → 拿到 runtime-select 之类的约束
//! 挑 Java 之后   全部守卫都能求值           → 拿到 block / heap-ceiling 之类
//! ```
//!
//! 同一个函数、同一张表，区别只在 [`Environment::java`] 是不是 `None`。
//!
//! ## 看不懂的守卫不命中
//!
//! 和 `crash.toml` 相反（那边看不懂当作满足）。理由是后果不对称：崩溃规则认
//! 宽了只是多一句解释，这一张认宽了会真的改掉启动方式——去下一份没必要的
//! Java、给一个不该给的开关、改一个不该改的 jar。

use std::{collections::BTreeMap, sync::LazyLock};

use serde::{Deserialize, Serialize};

use crate::{LoaderKind, java::JavaRuntime, launch::ranges};

/// 规则表跟着二进制走，和崩溃规则一样。
const TABLE: &str = include_str!("../../rules/compat.toml");

/// 这一次启动的环境。
///
/// `java` 为 `None` 表示「还没挑」，那时带 java 守卫的规则一律不命中。
#[derive(Debug, Clone, Default)]
pub struct Environment {
    pub minecraft: String,
    pub loader: LoaderKind,
    /// 加载器的版本号，原版是空串。
    pub loader_version: String,
    /// `windows` / `linux` / `osx`，和版本元数据里 rules 用的是同一套写法。
    pub os: String,
    /// 归一化后的架构：`x86_64` / `aarch64` / `x86`。
    pub arch: String,
    pub java: Option<JavaFacts>,
}

/// 规则要看的那几件关于 Java 的事。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JavaFacts {
    pub major: u16,
    pub update: u32,
    pub bits: u16,
    pub headless: bool,
}

impl From<&JavaRuntime> for JavaFacts {
    fn from(runtime: &JavaRuntime) -> Self {
        Self {
            major: runtime.major,
            update: runtime.update,
            bits: runtime.bits,
            headless: runtime.headless,
        }
    }
}

impl Environment {
    /// 这台机器此刻的样子。
    pub fn here(minecraft: &str, loader: LoaderKind, loader_version: &str) -> Self {
        Self {
            minecraft: minecraft.to_owned(),
            loader,
            loader_version: loader_version.to_owned(),
            os: super::rules::os_name().to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            java: None,
        }
    }

    pub fn with_java(mut self, java: Option<JavaFacts>) -> Self {
        self.java = java;
        self
    }
}

/// 规则能做的事，按侵入性排（文档 §4.2）。
///
/// 封闭枚举：每一种背后都要有真正执行它的那段代码。缺的那几种
/// （`jvm-arg-remove`、`env-set`、`artifact-replace`）等第一条真的要用它们的
/// 规则出现时再加——先摆出来只会得到一堆没人执行的动作。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Action {
    /// 挑一份满足附加条件的 Java。
    RuntimeSelect {
        major: u16,
        #[serde(rename = "max-update", default)]
        max_update: Option<u32>,
    },
    /// 加一个 JVM 参数。
    JvmArgAdd { argument: String },
    /// 堆上限不得超过这么多 MB。
    HeapCeiling { mb: u32 },
    /// 换一个上游已经修好的加载器构建。**我们不会自动换**——换加载器版本会
    /// 改变模组的兼容性，那是用户的决定。它的作用是把话说明白。
    LoaderVersion { minimum: String },
    /// 改一个产物，`patch` 是 [`super::patch`] 里那条补丁的 id。
    ArtifactPatch { patch: String },
    /// 这么启动不行，而且没有别的办法。
    Block,
}

/// 一条命中的规则，以及最后选定的那个动作。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Applied {
    /// 规则 id，同时是文案 id：`compat.<id>`。
    pub id: String,
    pub action: Action,
    /// 前面那些备选为什么没用上。空表示第一个就可行。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<Action>,
}

#[derive(Debug, Deserialize)]
struct RawRule {
    id: String,
    #[serde(default)]
    minecraft: Option<String>,
    #[serde(default)]
    loader: Vec<String>,
    #[serde(rename = "loader-version", default)]
    loader_version: Option<String>,
    #[serde(default)]
    os: Vec<String>,
    #[serde(default)]
    arch: Vec<String>,
    #[serde(rename = "java-major", default)]
    java_major: Option<u16>,
    #[serde(rename = "java-update", default)]
    java_update: Option<String>,
    #[serde(rename = "java-bits", default)]
    java_bits: Option<u16>,
    #[serde(rename = "java-headless", default)]
    java_headless: Option<bool>,
    actions: Vec<Action>,
}

#[derive(Debug, Deserialize)]
struct RuleFile {
    rule: Vec<RawRule>,
}

fn table() -> &'static [RawRule] {
    static RULES: LazyLock<Vec<RawRule>> =
        LazyLock::new(|| compile(TABLE).expect("内置兼容规则表必须可用"));
    &RULES
}

fn compile(text: &str) -> Result<Vec<RawRule>, String> {
    let file: RuleFile =
        toml::from_str(text).map_err(|error| format!("兼容规则表读不动：{error}"))?;
    for rule in &file.rule {
        if rule.actions.is_empty() {
            return Err(format!("规则 {} 一个动作都没有", rule.id));
        }
    }
    Ok(file.rule)
}

/// 表里全部规则的 id。界面那边要为每一条备好文案。
pub fn ids() -> Vec<String> {
    table().iter().map(|rule| rule.id.clone()).collect()
}

/// 这个环境下该做的事。
pub fn apply(environment: &Environment) -> Vec<Applied> {
    table()
        .iter()
        .filter(|rule| matches(rule, environment))
        .filter_map(|rule| choose(rule, environment))
        .collect()
}

/// 取第一个可行的备选；一个都不可行时给出 [`Action::Block`]，并把试过的都记
/// 下来——「都不行」这句话本身要说得出理由。
fn choose(rule: &RawRule, environment: &Environment) -> Option<Applied> {
    let mut skipped = Vec::new();
    for action in &rule.actions {
        if feasible(action, environment) {
            return Some(Applied {
                id: rule.id.clone(),
                action: action.clone(),
                skipped,
            });
        }
        skipped.push(action.clone());
    }
    Some(Applied {
        id: rule.id.clone(),
        action: Action::Block,
        skipped,
    })
}

/// 这个动作在这台机器上做得到吗。
fn feasible(action: &Action, environment: &Environment) -> bool {
    match action {
        // 已经装着一份合用的，或者这个平台上取得到官方运行时。Apple Silicon
        // 与 ARM Windows 上没有 jre-legacy，Mojang 根本不为 ARM 发 Java 8，
        // 第一备选在那里就是落空——那正是备选要有序的理由（文档 §4.5）。
        Action::RuntimeSelect { major, .. } => crate::java::runtime::obtainable(*major),
        // 换加载器版本会改变模组的兼容性，只能由用户决定。
        Action::LoaderVersion { .. } => false,
        Action::ArtifactPatch { patch } => {
            super::patch::PATCHES.iter().any(|known| known.id == patch)
        }
        Action::JvmArgAdd { .. } | Action::HeapCeiling { .. } | Action::Block => {
            let _ = environment;
            true
        }
    }
}

/// 守卫。只会让规则更窄。
fn matches(rule: &RawRule, environment: &Environment) -> bool {
    if let Some(range) = &rule.minecraft
        && !within(range, &environment.minecraft)
    {
        return false;
    }
    if !rule.loader.is_empty() {
        let current = format!("{:?}", environment.loader).to_lowercase();
        if !rule
            .loader
            .iter()
            .any(|name| name.to_lowercase() == current)
        {
            return false;
        }
    }
    if let Some(range) = &rule.loader_version
        && !within(range, &environment.loader_version)
    {
        return false;
    }
    if !rule.os.is_empty() && !rule.os.iter().any(|name| name == &environment.os) {
        return false;
    }
    if !rule.arch.is_empty() && !rule.arch.iter().any(|name| name == &environment.arch) {
        return false;
    }

    let wants_java = rule.java_major.is_some()
        || rule.java_update.is_some()
        || rule.java_bits.is_some()
        || rule.java_headless.is_some();
    if !wants_java {
        return true;
    }
    // 还没挑 Java 的那一轮，带 java 守卫的规则一律不命中。
    let Some(java) = environment.java else {
        return false;
    };
    if rule.java_major.is_some_and(|major| major != java.major) {
        return false;
    }
    if let Some(range) = &rule.java_update
        && !within(range, &java.update.to_string())
    {
        return false;
    }
    if rule.java_bits.is_some_and(|bits| bits != java.bits) {
        return false;
    }
    if rule
        .java_headless
        .is_some_and(|headless| headless != java.headless)
    {
        return false;
    }
    true
}

/// 区间守卫。**看不懂就是不命中**，见模块开头。
fn within(range: &str, value: &str) -> bool {
    ranges::contains(range, value).unwrap_or(false)
}

/// 这些动作合起来对 Java 的要求。挑 Java 那一步要用。
pub fn runtime_ceiling(applied: &[Applied]) -> Option<crate::java::UpdateCeiling> {
    applied.iter().find_map(|entry| match &entry.action {
        Action::RuntimeSelect {
            major,
            max_update: Some(update),
        } => Some(crate::java::UpdateCeiling {
            major: *major,
            update: *update,
        }),
        _ => None,
    })
}

/// 这些动作要加的 JVM 参数，保持规则表里的顺序。
pub fn jvm_arguments(applied: &[Applied]) -> Vec<String> {
    applied
        .iter()
        .filter_map(|entry| match &entry.action {
            Action::JvmArgAdd { argument } => Some(argument.clone()),
            _ => None,
        })
        .collect()
}

/// 这些动作允许的最小堆上限，MB。没有限制就是 `None`。
pub fn heap_ceiling(applied: &[Applied]) -> Option<u32> {
    applied
        .iter()
        .filter_map(|entry| match &entry.action {
            Action::HeapCeiling { mb } => Some(*mb),
            _ => None,
        })
        .min()
}

/// 这些动作点名要打的补丁。
pub fn patches(applied: &[Applied]) -> Vec<&str> {
    applied
        .iter()
        .filter_map(|entry| match &entry.action {
            Action::ArtifactPatch { patch } => Some(patch.as_str()),
            _ => None,
        })
        .collect()
}

/// 要说给用户听的那几条：拦下来的，和「本来想这么办、办不到」的。
///
/// 静悄悄就能办妥的（换个 Java、加个开关、打个补丁）不出现在这里——那些是
/// 启动器该自己解决的事，不是要用户读的通知。
pub fn notices(applied: &[Applied]) -> Vec<Notice> {
    applied
        .iter()
        .filter(|entry| matches!(entry.action, Action::Block))
        .map(|entry| Notice {
            id: entry.id.clone(),
            args: entry
                .skipped
                .iter()
                .filter_map(|action| match action {
                    Action::LoaderVersion { minimum } => {
                        Some(("loaderVersion".to_owned(), minimum.clone()))
                    }
                    _ => None,
                })
                .collect(),
        })
        .collect()
}

/// 一条拦下来的说明。和别处一样：只给文案 id 和参数，句子在界面那边。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Notice {
    /// 文案 id：`compat.<id>`。
    pub id: String,
    pub args: BTreeMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("rules/compat-fixtures")
    }

    /// fixture 是一份环境描述，和崩溃规则那边的一段日志是一回事。
    #[derive(Deserialize)]
    #[serde(rename_all = "kebab-case")]
    struct Fixture {
        minecraft: String,
        #[serde(default)]
        loader: String,
        #[serde(default)]
        loader_version: String,
        #[serde(default = "linux")]
        os: String,
        #[serde(default = "x86_64")]
        arch: String,
        #[serde(default)]
        java: Option<RawJava>,
    }

    #[derive(Deserialize)]
    struct RawJava {
        major: u16,
        #[serde(default)]
        update: u32,
        #[serde(default = "sixty_four")]
        bits: u16,
        #[serde(default)]
        headless: bool,
    }

    fn linux() -> String {
        "linux".to_owned()
    }

    fn x86_64() -> String {
        "x86_64".to_owned()
    }

    fn sixty_four() -> u16 {
        64
    }

    fn environment(text: &str) -> Environment {
        let fixture: Fixture = toml::from_str(text).expect("fixture 读不动");
        Environment {
            minecraft: fixture.minecraft,
            loader: match fixture.loader.as_str() {
                "forge" => LoaderKind::Forge,
                "neoforge" => LoaderKind::NeoForge,
                "fabric" => LoaderKind::Fabric,
                "quilt" => LoaderKind::Quilt,
                _ => LoaderKind::Vanilla,
            },
            loader_version: fixture.loader_version,
            os: fixture.os,
            arch: fixture.arch,
            java: fixture.java.map(|java| JavaFacts {
                major: java.major,
                update: java.update,
                bits: java.bits,
                headless: java.headless,
            }),
        }
    }

    #[test]
    fn the_bundled_table_compiles() {
        let rules = compile(TABLE).expect("规则表");
        assert!(!rules.is_empty());
        let mut seen = std::collections::HashSet::new();
        for rule in &rules {
            assert!(seen.insert(rule.id.clone()), "{} 出现了两次", rule.id);
            // id 会被拼进文案 id（`compat.<id>`），也是 fixture 的文件名。
            assert!(
                rule.id
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'),
                "{} 不是 kebab-case",
                rule.id
            );
        }
    }

    /// **每条规则都必须有一份环境，而且必须在那份环境上命中。**
    #[test]
    fn every_rule_has_a_fixture_that_it_matches() {
        for rule in compile(TABLE).expect("规则表") {
            let path = fixtures().join(format!("{}.toml", rule.id));
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|_| panic!("规则 {} 没有 fixture：{}", rule.id, path.display()));
            let found = apply(&environment(&text));
            assert!(
                found.iter().any(|entry| entry.id == rule.id),
                "{} 的 fixture 没能命中它自己，命中的是 {:?}",
                rule.id,
                found.iter().map(|entry| &entry.id).collect::<Vec<_>>()
            );
        }
    }

    /// 反过来：没有规则要的 fixture 是遗留垃圾。
    #[test]
    fn no_fixture_is_left_behind() {
        let ids: std::collections::HashSet<String> = compile(TABLE)
            .expect("规则表")
            .into_iter()
            .map(|rule| rule.id)
            .collect();
        for entry in std::fs::read_dir(fixtures())
            .expect("fixtures 目录")
            .flatten()
        {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('_') {
                continue;
            }
            assert!(
                ids.contains(name.trim_end_matches(".toml")),
                "{name} 没有对应的规则了"
            );
        }
    }

    /// **一个正常的环境不许命中任何规则。**
    ///
    /// 这张表最常见的腐化方式和崩溃表一样：某条守卫写得太宽，把好好的组合也
    /// 拦下来——而这一张拦错的后果是启动方式被改掉。
    #[test]
    fn an_ordinary_setup_is_left_alone() {
        let text = std::fs::read_to_string(fixtures().join("_clean.toml")).expect("干净的环境");
        let found = apply(&environment(&text));
        assert!(found.is_empty(), "正常的环境被改成了 {found:?}");
    }

    /// 挑 Java 之前那一轮，带 java 守卫的规则一律不命中——那时候还没有 Java
    /// 可看，命中就等于凭空断言。
    #[test]
    fn java_guards_wait_until_there_is_a_java() {
        let mut environment = Environment::here("1.12.2", LoaderKind::Vanilla, "");
        assert!(apply(&environment).is_empty());
        environment.java = Some(JavaFacts {
            major: 8,
            update: 202,
            bits: 32,
            headless: false,
        });
        let found = apply(&environment);
        assert_eq!(heap_ceiling(&found), Some(1024));
    }

    /// 1.16.5 那条的第一备选是换 Java，换不到才轮到「升级 Forge」；两条都
    /// 落空时给出的是 Block，而且说得出试过什么。
    #[test]
    fn the_alternatives_are_taken_in_order() {
        let environment = Environment::here("1.16.5", LoaderKind::Forge, "36.2.20");
        let found = apply(&environment);
        let entry = found
            .iter()
            .find(|entry| entry.id == "modlauncher-8-breaks-on-a-new-java-8")
            .expect("这一条该命中");
        match &entry.action {
            // 能拿到 Java 8 的平台上走第一条。
            Action::RuntimeSelect { major, max_update } => {
                assert_eq!(*major, 8);
                assert_eq!(*max_update, Some(320));
                assert_eq!(
                    runtime_ceiling(&found),
                    Some(crate::java::UpdateCeiling {
                        major: 8,
                        update: 320
                    })
                );
            }
            // ARM 上拿不到，两条备选都落空，只剩说清楚。
            Action::Block => {
                assert_eq!(entry.skipped.len(), 2);
                assert_eq!(
                    notices(&found)
                        .iter()
                        .find(|notice| notice.id == entry.id)
                        .and_then(|notice| notice.args.get("loaderVersion"))
                        .map(String::as_str),
                    Some("36.2.25")
                );
            }
            other => panic!("没想到会选中 {other:?}"),
        }
        // 修好的那些构建不该被拦。
        assert!(
            apply(&Environment::here("1.16.5", LoaderKind::Forge, "36.2.39"))
                .iter()
                .all(|entry| entry.id != "modlauncher-8-breaks-on-a-new-java-8")
        );
    }

    /// 规则点名的补丁必须真的存在，否则它是一条永远办不到的规则。
    #[test]
    fn every_named_patch_exists() {
        for rule in compile(TABLE).expect("规则表") {
            for action in &rule.actions {
                if let Action::ArtifactPatch { patch } = action {
                    assert!(
                        super::super::patch::PATCHES
                            .iter()
                            .any(|known| known.id == patch),
                        "规则 {} 点名的补丁 {patch} 不存在",
                        rule.id
                    );
                }
            }
        }
    }
}
