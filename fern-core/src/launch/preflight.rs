//! 启动之前先看一眼。
//!
//! 最痛的失败是这样的：装完一个整合包，点启动，黑框一闪，没了。而那一类失败
//! **在点启动之前就能看出来**——缺前置、同一个模组装了两份、模组不适配这个游戏
//! 版本或这个加载器。这些答案全在那些 jar 自己的元数据里，读一遍几百毫秒。
//!
//! 与崩溃分析的分工很清楚：**崩溃分析回答「刚才为什么死了」，预检查回答「按现在
//! 这样点下去会不会死」。** 崩溃那边每条规则都要一条文本证据；这边一条文本都
//! 没有，全是事实比对。两者共用同一个 [`Action`]，于是界面上那颗按钮是同一颗。
//!
//! **只报确定的事。** 版本区间看不懂就当满足（见 `launch::ranges`），可选依赖
//! 缺了不报，禁用的模组不参与依赖判断但会被单独提一句。一个基于误解的警告会让
//! 用户去动一个本来没问题的模组，比不报更糟。
//!
//! 预检查**不阻止启动**。它给的是判断依据，不是许可——用户可能比我们更清楚。
//!
//! 和崩溃分析一样，这一层不产出句子：给出的是文案 id（`preflight.<kind>`）加
//! 一组参数，措辞与翻译都在 `fern-ui/src/lib/i18n/`。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    DataPaths, InstanceProfile, LoaderKind,
    instance::jar::{self, ModJar},
    launch::{crash::Action, ranges},
};

/// 一条要说给用户的话。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    /// 这一条在这次检查里的唯一键，界面拿它做列表的 key。
    pub id: String,
    /// 文案 id：界面按 `preflight.<kind>` 查。取值见 [`kind`] 模块。
    pub kind: String,
    pub severity: Severity,
    /// 文案里的占位符。
    pub args: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<Action>,
}

/// 预检查会说的话，全部在这里。
///
/// 列成常量而不是散在各处的字符串字面量：界面那边要为每一条备好文案，而
/// 「有哪些条」必须有一个地方说得清楚。
pub mod kind {
    /// 实例没有加载器，mods 里的东西不会被加载。
    pub const NO_LOADER: &str = "no-loader";
    /// 同一个模组装了多份。
    pub const DUPLICATE: &str = "duplicate";
    /// 模组是给另一个加载器的。
    pub const WRONG_LOADER: &str = "wrong-loader";
    /// 模组声明的游戏版本不含当前版本。
    pub const WRONG_GAME_VERSION: &str = "wrong-game-version";
    /// 必需的前置没装。
    pub const MISSING_DEPENDENCY: &str = "missing-dependency";
    /// 前置装了，但被关掉了。
    pub const DISABLED_DEPENDENCY: &str = "disabled-dependency";

    /// 全部取值。界面那边的文案表按它对齐。
    pub const ALL: [&str; 6] = [
        NO_LOADER,
        DUPLICATE,
        WRONG_LOADER,
        WRONG_GAME_VERSION,
        MISSING_DEPENDENCY,
        DISABLED_DEPENDENCY,
    ];
}

fn args<const N: usize>(pairs: [(&str, String); N]) -> BTreeMap<String, String> {
    pairs
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    /// 大概率起不来。
    Blocking,
    /// 可能有问题，但不一定。
    Warning,
}

/// 看一遍这个实例。没有问题就是空列表。
pub fn check(paths: &DataPaths, profile: &InstanceProfile) -> Vec<Finding> {
    let scoped = crate::instance::paths_for(paths, profile);
    let jars = jar::read_all(&jar::directory(&scoped, profile.id.as_str()));
    inspect(&jars, profile.loader, &profile.game_version)
}

/// 纯函数那一半：给定这些 jar 和这个上下文，有什么话要说。
///
/// 和磁盘分开，于是它能对着构造出来的元数据单独测——而这一层的价值全在判断上，
/// 不在读文件上。
pub fn inspect(jars: &[ModJar], loader: LoaderKind, minecraft: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    let enabled: Vec<&ModJar> = jars.iter().filter(|jar| jar.enabled).collect();
    if enabled.is_empty() {
        return findings;
    }

    // 原版实例里的 mods 目录不会被任何人读。这是最省事、也最容易被忽略的一条。
    if loader == LoaderKind::Vanilla {
        findings.push(Finding {
            id: kind::NO_LOADER.to_owned(),
            kind: kind::NO_LOADER.to_owned(),
            severity: Severity::Blocking,
            args: args([("count", enabled.len().to_string())]),
            action: None,
        });
        return findings;
    }

    findings.extend(duplicates(&enabled));
    findings.extend(wrong_loader(&enabled, loader));
    findings.extend(wrong_game_version(&enabled, minecraft));
    findings.extend(missing_dependencies(jars, &enabled, loader));
    findings.sort_by_key(|finding| finding.severity);
    findings
}

/// 同一个 modid 出现两次。加载器不知道该用哪个，多半直接拒绝启动。
fn duplicates(enabled: &[&ModJar]) -> Vec<Finding> {
    let mut seen: std::collections::HashMap<&str, Vec<&ModJar>> = std::collections::HashMap::new();
    for jar in enabled {
        if let Some(mod_id) = &jar.mod_id {
            seen.entry(mod_id).or_default().push(jar);
        }
    }
    let mut findings: Vec<Finding> = seen
        .into_iter()
        .filter(|(_, jars)| jars.len() > 1)
        .map(|(mod_id, jars)| {
            let names: Vec<&str> = jars.iter().map(|jar| jar.file_name.as_str()).collect();
            Finding {
                id: format!("{}:{mod_id}", kind::DUPLICATE),
                kind: kind::DUPLICATE.to_owned(),
                severity: Severity::Blocking,
                args: args([
                    ("mod", jars[0].name.clone()),
                    ("count", jars.len().to_string()),
                    ("files", names.join("、")),
                ]),
                // 保留第一份，其余的给一个能点的删除——多出来的那些才是问题。
                action: Some(Action::RemoveMod {
                    file: jars[1].file_name.clone(),
                }),
            }
        })
        .collect();
    findings.sort_by(|left, right| left.id.cmp(&right.id));
    findings
}

/// Fabric 的模组放进 Forge 实例里不会被加载，反之亦然。
fn wrong_loader(enabled: &[&ModJar], loader: LoaderKind) -> Vec<Finding> {
    enabled
        .iter()
        .filter(|jar| jar.loader != LoaderKind::Vanilla && !accepts(loader, jar.loader))
        .map(|jar| Finding {
            id: format!("{}:{}", kind::WRONG_LOADER, jar.file_name),
            kind: kind::WRONG_LOADER.to_owned(),
            severity: Severity::Blocking,
            // 加载器名是个术语，交给界面去译；这里只给取值。
            args: args([
                ("mod", jar.name.clone()),
                ("instanceLoader", tag(loader).to_owned()),
                ("modLoader", tag(jar.loader).to_owned()),
            ]),
            action: Some(Action::RemoveMod {
                file: jar.file_name.clone(),
            }),
        })
        .collect()
}

/// Quilt 读得了 Fabric 的模组，NeoForge 读不了 Forge 的（1.20.2 之后分家了）。
fn accepts(instance: LoaderKind, jar: LoaderKind) -> bool {
    instance == jar || (instance == LoaderKind::Quilt && jar == LoaderKind::Fabric)
}

/// 模组自己声明的 MC 版本区间不含这个实例的版本。
fn wrong_game_version(enabled: &[&ModJar], minecraft: &str) -> Vec<Finding> {
    enabled
        .iter()
        .filter_map(|jar| {
            let range = jar.minecraft_range()?;
            (!ranges::satisfies(range, minecraft)).then(|| Finding {
                id: format!("{}:{}", kind::WRONG_GAME_VERSION, jar.file_name),
                kind: kind::WRONG_GAME_VERSION.to_owned(),
                severity: Severity::Warning,
                args: args([
                    ("mod", jar.name.clone()),
                    ("minecraft", minecraft.to_owned()),
                    ("range", range.to_owned()),
                ]),
                action: None,
            })
        })
        .collect()
}

/// 必需的前置没装。这是最常见、也最容易修的一条。
fn missing_dependencies(all: &[ModJar], enabled: &[&ModJar], loader: LoaderKind) -> Vec<Finding> {
    // 加载器和游戏自己也是依赖项，但它们不在 mods 目录里，不该被当成缺失。
    let provided: std::collections::HashSet<String> = enabled
        .iter()
        .filter_map(|jar| jar.mod_id.clone())
        .chain(builtin(loader))
        .collect();

    let mut findings = Vec::new();
    let mut reported: std::collections::HashSet<String> = std::collections::HashSet::new();
    for jar in enabled {
        for dependency in &jar.depends {
            if !dependency.required
                || provided.contains(&dependency.mod_id)
                || !reported.insert(dependency.mod_id.clone())
            {
                continue;
            }
            // 装了但被关掉了是另一回事：让用户开回来，而不是再下一份。
            let disabled = all
                .iter()
                .find(|other| !other.enabled && other.mod_id.as_ref() == Some(&dependency.mod_id));
            findings.push(match disabled {
                Some(off) => Finding {
                    id: format!("{}:{}", kind::DISABLED_DEPENDENCY, dependency.mod_id),
                    kind: kind::DISABLED_DEPENDENCY.to_owned(),
                    severity: Severity::Blocking,
                    args: args([("dependency", off.name.clone()), ("mod", jar.name.clone())]),
                    action: None,
                },
                None => Finding {
                    id: format!("{}:{}", kind::MISSING_DEPENDENCY, dependency.mod_id),
                    kind: kind::MISSING_DEPENDENCY.to_owned(),
                    severity: Severity::Blocking,
                    args: args([
                        ("dependency", dependency.mod_id.clone()),
                        ("mod", jar.name.clone()),
                    ]),
                    action: Some(Action::InstallMod {
                        query: dependency.mod_id.clone(),
                    }),
                },
            });
        }
    }
    findings.sort_by(|left, right| left.id.cmp(&right.id));
    findings
}

/// 加载器自带的那些 id。模组会把它们写进 depends，但它们不是 mods 里的文件。
fn builtin(loader: LoaderKind) -> Vec<String> {
    let mut names = vec!["minecraft".to_owned(), "java".to_owned()];
    names.extend(
        match loader {
            LoaderKind::Fabric => ["fabricloader", "fabric-loader", "fabric"].as_slice(),
            LoaderKind::Quilt => {
                ["quilt_loader", "quilt_base", "fabricloader", "fabric"].as_slice()
            }
            LoaderKind::Forge => ["forge", "fml"].as_slice(),
            LoaderKind::NeoForge => ["neoforge", "forge", "fml"].as_slice(),
            LoaderKind::Vanilla => [].as_slice(),
        }
        .iter()
        .map(|name| (*name).to_owned()),
    );
    names
}

/// 加载器的机器名。界面上显示成什么由界面决定——「原版」要翻译，Fabric 不用。
fn tag(loader: LoaderKind) -> &'static str {
    match loader {
        LoaderKind::Vanilla => "vanilla",
        LoaderKind::Fabric => "fabric",
        LoaderKind::Quilt => "quilt",
        LoaderKind::Forge => "forge",
        LoaderKind::NeoForge => "neoforge",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instance::jar::Dependency;

    fn jar(name: &str, mod_id: &str, loader: LoaderKind) -> ModJar {
        ModJar {
            file_name: format!("{name}.jar"),
            enabled: true,
            mod_id: Some(mod_id.to_owned()),
            name: name.to_owned(),
            version: Some("1.0".to_owned()),
            loader,
            depends: Vec::new(),
            packages: Vec::new(),
        }
    }

    fn needs(mut jar: ModJar, mod_id: &str, range: &str) -> ModJar {
        jar.depends.push(Dependency {
            mod_id: mod_id.to_owned(),
            range: range.to_owned(),
            required: true,
        });
        jar
    }

    #[test]
    fn a_missing_dependency_is_found_before_the_game_starts() {
        let jars = vec![needs(
            jar("Sodium", "sodium", LoaderKind::Fabric),
            "fabric-api",
            "*",
        )];
        let findings = inspect(&jars, LoaderKind::Fabric, "1.21.1");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, kind::MISSING_DEPENDENCY);
        assert_eq!(findings[0].args["dependency"], "fabric-api");
        assert_eq!(
            findings[0].action,
            Some(Action::InstallMod {
                query: "fabric-api".to_owned()
            })
        );
    }

    /// 加载器和游戏自己也写在 depends 里，但它们不在 mods 目录里。
    #[test]
    fn the_loader_and_the_game_are_not_missing_dependencies() {
        let mut sodium = needs(
            jar("Sodium", "sodium", LoaderKind::Fabric),
            "minecraft",
            "*",
        );
        sodium = needs(sodium, "fabricloader", ">=0.15");
        assert!(inspect(&[sodium], LoaderKind::Fabric, "1.21.1").is_empty());
    }

    /// 装了但被关掉了，该说的是「打开它」，不是「去下一份」。
    #[test]
    fn a_disabled_dependency_asks_to_be_switched_on_not_downloaded() {
        let mut api = jar("Fabric API", "fabric-api", LoaderKind::Fabric);
        api.enabled = false;
        api.file_name = "fabric-api.jar.disabled".to_owned();
        let jars = vec![
            needs(
                jar("Sodium", "sodium", LoaderKind::Fabric),
                "fabric-api",
                "*",
            ),
            api,
        ];
        let findings = inspect(&jars, LoaderKind::Fabric, "1.21.1");
        assert_eq!(findings[0].id, "disabled-dependency:fabric-api");
        assert_eq!(findings[0].kind, kind::DISABLED_DEPENDENCY);
        assert!(findings[0].action.is_none());
    }

    #[test]
    fn two_copies_of_one_mod_are_reported_once() {
        let mut second = jar("Sodium", "sodium", LoaderKind::Fabric);
        second.file_name = "sodium-old.jar".to_owned();
        let findings = inspect(
            &[jar("Sodium", "sodium", LoaderKind::Fabric), second],
            LoaderKind::Fabric,
            "1.21.1",
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, kind::DUPLICATE);
        assert_eq!(findings[0].args["count"], "2");
    }

    #[test]
    fn a_fabric_mod_in_a_forge_instance_is_reported() {
        let findings = inspect(
            &[jar("Sodium", "sodium", LoaderKind::Fabric)],
            LoaderKind::NeoForge,
            "1.21.1",
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, kind::WRONG_LOADER);
        assert_eq!(findings[0].args["instanceLoader"], "neoforge");
        // Quilt 读得了 Fabric 的模组，那一种不该报。
        assert!(
            inspect(
                &[jar("Sodium", "sodium", LoaderKind::Fabric)],
                LoaderKind::Quilt,
                "1.21.1"
            )
            .is_empty()
        );
    }

    #[test]
    fn a_mod_built_for_another_game_version_is_a_warning_not_a_blocker() {
        let mut sodium = jar("Sodium", "sodium", LoaderKind::Fabric);
        sodium.depends.push(Dependency {
            mod_id: "minecraft".to_owned(),
            range: "1.20.x".to_owned(),
            required: true,
        });
        let findings = inspect(&[sodium], LoaderKind::Fabric, "1.21.1");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Warning);
    }

    /// 看不懂的区间一律当作满足。宁可漏报。
    #[test]
    fn an_unreadable_version_range_is_not_reported() {
        let mut sodium = jar("Sodium", "sodium", LoaderKind::Fabric);
        sodium.depends.push(Dependency {
            mod_id: "minecraft".to_owned(),
            range: "谁知道这是什么".to_owned(),
            required: true,
        });
        assert!(inspect(&[sodium], LoaderKind::Fabric, "1.21.1").is_empty());
    }

    #[test]
    fn mods_in_a_vanilla_instance_are_never_loaded() {
        let findings = inspect(
            &[jar("Sodium", "sodium", LoaderKind::Fabric)],
            LoaderKind::Vanilla,
            "1.21.1",
        );
        assert_eq!(findings[0].kind, kind::NO_LOADER);
    }

    #[test]
    fn a_healthy_instance_says_nothing() {
        let jars = vec![
            needs(
                jar("Sodium", "sodium", LoaderKind::Fabric),
                "fabric-api",
                "*",
            ),
            jar("Fabric API", "fabric-api", LoaderKind::Fabric),
        ];
        assert!(inspect(&jars, LoaderKind::Fabric, "1.21.1").is_empty());
    }
}
