//! 崩溃分析（文档 §5.4）。
//!
//! 游戏非正常退出时，用户手上只有一个退出码和几百行栈。这一层把能认出来的翻成
//! 人话，认不出来的原样交出去折叠着——**绝不猜**。一句听起来很像那么回事、其实
//! 不对的诊断，比老实说「不认识」更浪费用户的时间。
//!
//! 三步流水线，每一步都是纯函数，能单独测：
//!
//! ```text
//! Evidence ──parse──▶ Facts ──┬── rules ────▶ Vec<Diagnosis>   排好序
//!                             └── suspect ──▶ Vec<Suspect>     与规则无关
//! ```
//!
//! 分开的理由：规则表会一直改，而「崩溃报告长什么样」十年没变；归因不依赖规则
//! 命中，所以**一条规则都没认出来的时候，仍然说得出「崩在 Sodium 的代码里」**。

pub(crate) mod parse;
pub(crate) mod rules;
pub(crate) mod suspect;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use rules::{Action, Diagnosis, Level};
pub use suspect::{Known, Reason, Suspect};

/// 尾部保留多少字节交给界面。够看清最后一段栈，又不至于把 IPC 塞爆。
const EXCERPT_BYTES: usize = 8 * 1024;

/// 一次非正常退出的全部所知。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashReport {
    pub instance_id: String,
    pub exit_code: Option<i32>,
    /// 认出来的原因，认得越具体的排越前面。一条都没有时界面照实说。
    pub diagnoses: Vec<Diagnosis>,
    /// 可能有关的模组。**和 `diagnoses` 无关**——认不出原因时它往往是唯一的线索。
    pub suspects: Vec<Suspect>,
    /// 游戏自己写的崩溃报告，比我们的日志尾部完整得多。
    pub report_path: Option<PathBuf>,
    /// JVM 自己的崩溃日志，原生崩溃时才有。
    pub hs_err_path: Option<PathBuf>,
    /// 原始文本的尾部，折叠展示。
    pub excerpt: String,
}

impl CrashReport {
    /// 最该说的那一条。
    pub fn headline(&self) -> Option<&Diagnosis> {
        self.diagnoses.first()
    }
}

/// 这次崩溃发生在什么上下文里。
pub struct Situation<'a> {
    pub instance_id: &'a str,
    pub game_directory: &'a Path,
    pub started_at: std::time::SystemTime,
    pub exit_code: Option<i32>,
    pub loader: crate::LoaderKind,
    pub minecraft: &'a str,
    /// 这个实例装了哪些模组，用来归因。读 jar 是调用方的事。
    pub mods: Vec<Known>,
}

/// 把这次运行知道的一切收成一份报告。
pub fn build_report(situation: &Situation<'_>, log_tail: &str) -> CrashReport {
    let report_path = latest_file(
        &situation.game_directory.join("crash-reports"),
        situation.started_at,
        |name| name.ends_with(".txt"),
    );
    let hs_err_path = latest_file(situation.game_directory, situation.started_at, |name| {
        name.starts_with("hs_err_pid") && name.ends_with(".log")
    });
    let report_text = report_path
        .as_ref()
        .and_then(|path| std::fs::read_to_string(path).ok());
    let hs_err_text = hs_err_path
        .as_ref()
        .and_then(|path| std::fs::read_to_string(path).ok());

    let evidence = parse::Evidence {
        report: report_text.as_deref(),
        console: log_tail,
        hs_err: hs_err_text.as_deref(),
    };
    let facts = parse::extract(&evidence);
    let diagnoses = rules::apply(
        &evidence,
        &rules::Context {
            loader: situation.loader,
            minecraft: situation.minecraft.to_owned(),
        },
    );
    let suspects = suspect::identify(&facts, &situation.mods);

    CrashReport {
        instance_id: situation.instance_id.to_owned(),
        exit_code: situation.exit_code,
        diagnoses,
        suspects,
        report_path,
        hs_err_path,
        excerpt: tail(log_tail, EXCERPT_BYTES),
    }
}

/// 某个 mods 目录里的模组，转成归因要的形状。
///
/// 只在崩了之后读：一个大整合包有几百个 jar，这一步几百毫秒，不该占着启动的路。
pub fn known_in(mods_directory: &Path) -> Vec<Known> {
    crate::instance::jar::read_all(mods_directory)
        .into_iter()
        .filter(|jar| jar.enabled && !jar.packages.is_empty())
        .map(|jar| Known {
            mod_id: jar.mod_id.clone().unwrap_or_else(|| jar.name.clone()),
            name: jar.name,
            version: jar.version,
            packages: jar.packages,
            provides: jar.provides,
        })
        .collect()
}

/// 一份文本里能指认出的模组。
///
/// 不需要本地装着那些 jar：加载器自己点的名和失败的 mixin 配置都写在文本里。
/// 给「粘一段日志给我看看」那类入口用，也给语料统计用。
pub fn attribute_crash(text: &str) -> Vec<Suspect> {
    let facts = parse::extract(&parse::Evidence {
        report: Some(text),
        console: text,
        hs_err: None,
    });
    suspect::identify(&facts, &[])
}

/// 一份文本里能认出的原因。给界面上「粘一段日志给我看看」那类入口用。
pub fn diagnose(text: &str, context: rules::Context) -> Vec<Diagnosis> {
    rules::apply(
        &parse::Evidence {
            report: None,
            console: text,
            hs_err: None,
        },
        &context,
    )
}

/// 目录里这次运行之后写的最新一个文件。
///
/// 只认这次运行之后的：上一次崩溃留下的报告会把人引到完全错误的方向。
fn latest_file(
    directory: &Path,
    after: std::time::SystemTime,
    accept: impl Fn(&str) -> bool,
) -> Option<PathBuf> {
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(directory).ok()?.flatten() {
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_file() || !accept(&entry.file_name().to_string_lossy()) {
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
    use crate::LoaderKind;

    fn context(loader: LoaderKind) -> rules::Context {
        rules::Context {
            loader,
            minecraft: "1.21.1".to_owned(),
        }
    }

    #[test]
    fn recognises_the_common_causes() {
        let memory = diagnose(
            "java.lang.OutOfMemoryError: Java heap space",
            context(LoaderKind::Fabric),
        );
        assert_eq!(memory[0].id, "out-of-memory");

        // 端到端跑出来的真实栈：1.21.1 在没有显示器的机器上就是这一条。
        let glfw = diagnose(
            "java.lang.IllegalStateException: Failed to initialize GLFW, errors: GLFW error during init: [0x1000E]Failed to detect any supported platform",
            context(LoaderKind::Vanilla),
        );
        assert_eq!(glfw[0].id, "graphics-unavailable");
    }

    /// 守卫只收窄：Fabric 的报错格式不该在 Forge 实例上被认出来。
    #[test]
    fn a_guard_keeps_a_rule_off_the_wrong_loader() {
        let text =
            "- Mod 'Sodium' (sodium) 0.6.0 requires any version of fabric-api, which is missing!";
        assert_eq!(
            diagnose(text, context(LoaderKind::Fabric))[0].id,
            "fabric-missing-dependency"
        );
        assert!(diagnose(text, context(LoaderKind::Forge)).is_empty());
    }

    /// 认出来之后要能替用户做点什么。
    #[test]
    fn a_missing_dependency_comes_with_something_to_press() {
        let found = diagnose(
            "- Mod 'Sodium' (sodium) 0.6.0 requires any version of fabric-api, which is missing!",
            context(LoaderKind::Fabric),
        );
        assert_eq!(found[0].args["need"], "fabric-api");
        assert_eq!(
            found[0].action,
            Some(Action::InstallMod {
                query: "fabric-api".to_owned()
            })
        );
    }

    #[test]
    fn nothing_recognised_is_reported_as_nothing_recognised() {
        assert!(diagnose("一段谁也认不出来的话", context(LoaderKind::Fabric)).is_empty());
    }
}
