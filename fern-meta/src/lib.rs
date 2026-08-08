//! Mojang version metadata models and resolution.
//!
//! The crate is independent from filesystem, network, Tauri, and Pearl. It
//! parses both generations of the launcher protocol and provides deterministic
//! inheritance and rules evaluation for the download and launch crates.

use std::collections::{HashMap, HashSet};

use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionManifestEntry {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub url: String,
    #[serde(rename = "releaseTime", default)]
    pub release_time: Option<String>,
    #[serde(default)]
    pub time: Option<String>,
    #[serde(default)]
    pub sha1: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionManifest {
    pub versions: Vec<VersionManifestEntry>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionMetadata {
    pub id: String,
    #[serde(default)]
    pub inherits_from: Option<String>,
    #[serde(rename = "type", default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub main_class: Option<String>,
    #[serde(default)]
    pub downloads: Option<VersionDownloads>,
    #[serde(default)]
    pub libraries: Vec<Library>,
    #[serde(default)]
    pub asset_index: Option<AssetIndex>,
    #[serde(default)]
    pub arguments: Option<Arguments>,
    #[serde(default)]
    pub minecraft_arguments: Option<String>,
    #[serde(default)]
    pub java_version: Option<JavaVersion>,
    #[serde(default)]
    pub logging: Option<Logging>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionDownloads {
    #[serde(default)]
    pub client: Option<DownloadInfo>,
    #[serde(default)]
    pub client_mappings: Option<DownloadInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadInfo {
    #[serde(default)]
    pub id: Option<String>,
    pub sha1: String,
    pub size: u64,
    pub url: String,
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetIndex {
    pub id: String,
    pub sha1: String,
    pub size: u64,
    #[serde(default)]
    pub total_size: Option<u64>,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JavaVersion {
    #[serde(rename = "component")]
    pub component: String,
    #[serde(rename = "majorVersion")]
    pub major_version: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Logging {
    #[serde(default)]
    pub client: Option<LoggingClient>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoggingClient {
    pub argument: String,
    pub file: DownloadInfo,
    #[serde(rename = "type", default)]
    pub type_name: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Arguments {
    #[serde(default)]
    pub game: Vec<Argument>,
    #[serde(default)]
    pub jvm: Vec<Argument>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Argument {
    Plain(String),
    Conditional {
        rules: Vec<Rule>,
        value: ArgumentValue,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ArgumentValue {
    String(String),
    List(Vec<String>),
}

impl ArgumentValue {
    fn append_to(&self, output: &mut Vec<String>) {
        match self {
            Self::String(value) => output.push(value.clone()),
            Self::List(values) => output.extend(values.iter().cloned()),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Library {
    pub name: String,
    #[serde(default)]
    pub downloads: Option<LibraryDownloads>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub rules: Option<Vec<Rule>>,
    #[serde(default)]
    pub natives: Option<HashMap<String, String>>,
    #[serde(default)]
    pub extract: Option<ExtractRule>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryDownloads {
    #[serde(default)]
    pub artifact: Option<DownloadInfo>,
    #[serde(default)]
    pub classifiers: Option<HashMap<String, DownloadInfo>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractRule {
    #[serde(default)]
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub action: RuleAction,
    #[serde(default)]
    pub os: Option<OsRule>,
    #[serde(default)]
    pub features: Option<HashMap<String, bool>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleAction {
    Allow,
    Disallow,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OsRule {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arch: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuleContext {
    pub os_name: String,
    pub os_arch: String,
    pub os_version: String,
    pub features: HashMap<String, bool>,
}

impl RuleContext {
    pub fn linux_x64() -> Self {
        Self {
            os_name: "linux".to_owned(),
            os_arch: "x86_64".to_owned(),
            ..Self::default()
        }
    }
}

impl Rule {
    fn matches(&self, context: &RuleContext) -> bool {
        let os_matches = self.os.as_ref().is_none_or(|os| {
            os.name.as_ref().is_none_or(|name| name == &context.os_name)
                && os.arch.as_ref().is_none_or(|arch| arch == &context.os_arch)
                && os.version.as_ref().is_none_or(|pattern| {
                    Regex::new(pattern)
                        .map(|regex| regex.is_match(&context.os_version))
                        .unwrap_or(false)
                })
        });
        let features_match = self.features.as_ref().is_none_or(|features| {
            features
                .iter()
                .all(|(key, expected)| context.features.get(key) == Some(expected))
        });
        os_matches && features_match
    }
}

/// Maven 坐标推落盘路径。
///
/// `com.mojang:brigadier:1.0.18` → `com/mojang/brigadier/1.0.18/brigadier-1.0.18.jar`
///
/// 官方元数据每个库都带完整的 `downloads.artifact.path`，用不上这个函数；
/// 需要它的是第三方 Maven——Forge 和 Fabric 的库常常只给一个 `url` 前缀，
/// 路径只能由坐标推出来。
///
/// 支持 `group:artifact:version[:classifier][@extension]`。坐标是从网上拿来
/// 的，会被直接拼进本地路径，所以任何一段里出现分隔符或 `..` 都判为非法——
/// 一个 `..:x:1` 就能把 jar 写到数据目录外面去。
pub fn maven_path(coordinate: &str) -> Option<String> {
    let (coordinate, extension) = match coordinate.split_once('@') {
        Some((head, extension)) => (head, extension),
        None => (coordinate, "jar"),
    };
    let mut parts = coordinate.split(':');
    let group = parts.next()?;
    let artifact = parts.next()?;
    let version = parts.next()?;
    let classifier = parts.next().filter(|value| !value.is_empty());
    if parts.next().is_some() {
        return None;
    }

    // artifact / version / classifier / extension 原样进路径，`..` 是真的上跳。
    let plain = |value: &str| {
        !value.is_empty() && value != ".." && !value.contains('/') && !value.contains('\\')
    };
    // group 的点会变成斜杠，所以要逐段看：`..` 这样的整段会拆成空段（点之间
    // 没有内容），空段只会拼出 `//`，但那已经不是一个能对上仓库布局的路径了。
    let group_ok = !group.contains('/')
        && !group.contains('\\')
        && !group.is_empty()
        && group.split('.').all(|segment| !segment.is_empty());
    if !group_ok || !plain(artifact) || !plain(version) || !plain(extension) {
        return None;
    }
    if classifier.is_some_and(|value| !plain(value)) {
        return None;
    }

    let directory = group.replace('.', "/");
    let file = match classifier {
        Some(classifier) => format!("{artifact}-{version}-{classifier}.{extension}"),
        None => format!("{artifact}-{version}.{extension}"),
    };
    Some(format!("{directory}/{artifact}/{version}/{file}"))
}

/// `1.20.4` → `(1, 20, 4)`。快照、预发布这类比不了的返回 `None`。
///
/// 版本号是启动协议的一部分：哪一代 LWJGL、哪一版 log4j、要哪个 Java，全靠
/// 它来分。放在这里而不是各处各写一遍。
pub fn release_ordinal(version: &str) -> Option<(u16, u16, u16)> {
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = match parts.next() {
        Some(patch) => patch.parse().ok()?,
        None => 0,
    };
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

pub fn rules_allow(rules: Option<&[Rule]>, context: &RuleContext) -> bool {
    let Some(rules) = rules else { return true };
    let mut allowed = false;
    for rule in rules {
        if rule.matches(context) {
            allowed = matches!(rule.action, RuleAction::Allow);
        }
    }
    allowed
}

impl VersionMetadata {
    /// Merge a child version over its already loaded parent.
    pub fn merge(parent: &Self, child: &Self) -> Self {
        // 只接起来，不在这里去重：同一个坐标的两条记录可能是**按 rules 分开
        // 的两个变体**（1.12.2 就给 macOS 单列了一份旧 LWJGL），在还没有平台
        // 上下文的地方挑一条，挑掉的可能正是这台机器唯一能用的那条。谁赢谁输
        // 由 `effective_libraries` 在过完 rules 之后决定。
        //
        // 子在前：同版本的重复条目由它代表。
        let mut libraries = Vec::with_capacity(parent.libraries.len() + child.libraries.len());
        libraries.extend(child.libraries.iter().cloned());
        libraries.extend(parent.libraries.iter().cloned());

        let arguments = match (&parent.arguments, &child.arguments) {
            (None, None) => None,
            (Some(parent), None) => Some(parent.clone()),
            (None, Some(child)) => Some(child.clone()),
            (Some(parent), Some(child)) => Some(Arguments {
                game: parent
                    .game
                    .iter()
                    .chain(child.game.iter())
                    .cloned()
                    .collect(),
                jvm: parent.jvm.iter().chain(child.jvm.iter()).cloned().collect(),
            }),
        };

        Self {
            id: child.id.clone(),
            inherits_from: child.inherits_from.clone(),
            kind: child.kind.clone().or_else(|| parent.kind.clone()),
            main_class: child
                .main_class
                .clone()
                .or_else(|| parent.main_class.clone()),
            downloads: child.downloads.clone().or_else(|| parent.downloads.clone()),
            libraries,
            asset_index: child
                .asset_index
                .clone()
                .or_else(|| parent.asset_index.clone()),
            arguments,
            minecraft_arguments: child
                .minecraft_arguments
                .clone()
                .or_else(|| parent.minecraft_arguments.clone()),
            java_version: child
                .java_version
                .clone()
                .or_else(|| parent.java_version.clone()),
            logging: child.logging.clone().or_else(|| parent.logging.clone()),
        }
    }

    /// 这台机器上真正要用的那些库，顺序保持不变。
    ///
    /// 两步，顺序不能反：**先按 rules 过滤，再按坐标去重**。反过来做，1.12.2
    /// 在 macOS 上就没有 LWJGL 了——那个版本给 macOS 单列了一份旧的，两条记录
    /// 坐标相同、版本不同，谁能用是 rules 说了算。
    ///
    /// 去重留版本高的那一份。一份「Fabric 装好之后」的版本 JSON 里，原版的
    /// `org.ow2.asm:asm:9.6` 和加载器要的 `9.9` 会同时在列（有的启动器写出来的
    /// 就是这样一份合并好的 JSON，没有 `inheritsFrom` 可依）——两份 ASM 一起
    /// 进 classpath，Fabric 加载器开机第一件事就是拒绝启动：
    /// 「duplicate ASM classes found on classpath」。
    ///
    /// 版本一样的重复条目只留第一条：那是同一个 jar 被写了两遍，谁代表都一样。
    ///
    /// 为什么是「版本高的赢」而不是「加载器那份赢」：一份合并好的 JSON 里没有
    /// 谁是加载器这回事，只有顺序，而顺序在不同启动器手里是相反的。版本能比，
    /// 而加载器发布得比它对应的游戏版本晚，带的库只会更新——Forge 与 NeoForge
    /// 还把库版本写死在 `-p` 的模块路径里，选中更新的那份正好是它们要的。
    pub fn effective_libraries(&self, context: &RuleContext) -> Vec<&Library> {
        let allowed: Vec<&Library> = self
            .libraries
            .iter()
            .filter(|library| rules_allow(library.rules.as_deref(), context))
            .collect();

        let mut best: HashMap<String, &str> = HashMap::new();
        for library in &allowed {
            let identity = library_identity(&library.name);
            let version = library_version(&library.name).unwrap_or_default();
            match best.get(&identity) {
                Some(current) if compare_versions(version, current).is_le() => {}
                _ => {
                    best.insert(identity, version);
                }
            }
        }

        let mut taken = HashSet::new();
        allowed
            .into_iter()
            .filter(|library| {
                let identity = library_identity(&library.name);
                let version = library_version(&library.name).unwrap_or_default();
                best.get(&identity) == Some(&version) && taken.insert(identity)
            })
            .collect()
    }

    pub fn resolved_arguments(&self, context: &RuleContext) -> (Vec<String>, Vec<String>) {
        if let Some(arguments) = &self.arguments {
            return (
                resolve_argument_list(&arguments.jvm, context),
                resolve_argument_list(&arguments.game, context),
            );
        }
        let game = self
            .minecraft_arguments
            .as_deref()
            .unwrap_or_default()
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        let jvm = vec![
            "-Djava.library.path=${natives_directory}".to_owned(),
            "-Dminecraft.launcher.brand=${launcher_name}".to_owned(),
            "-Dminecraft.launcher.version=${launcher_version}".to_owned(),
            "-cp".to_owned(),
            "${classpath}".to_owned(),
        ];
        (jvm, game)
    }
}

fn library_identity(name: &str) -> String {
    let parts = name.split(':').collect::<Vec<_>>();
    match parts.as_slice() {
        [group, artifact, _version] => format!("{group}:{artifact}"),
        [group, artifact, _version, classifier] => {
            format!("{group}:{artifact}:{classifier}")
        }
        _ => name.to_owned(),
    }
}

/// `org.ow2.asm:asm:9.9` → `9.9`。形状不对就没有版本可言。
fn library_version(name: &str) -> Option<&str> {
    let mut parts = name.split(':');
    parts.next()?;
    parts.next()?;
    parts.next().filter(|version| !version.is_empty())
}

/// 库版本号的大小。
///
/// 不是 SemVer：`2.9.4-nightly-20150209`、`1.0.18`、`9.9`、`3.3.3` 都要能比。
/// 所以按分隔符切段逐段比——两段都是数字就按数字比（`9.10` > `9.9`，字符串比
/// 会得出相反的结论），否则按字面比。前缀相同时段多的更大（`1.0.1` > `1.0`）。
fn compare_versions(left: &str, right: &str) -> std::cmp::Ordering {
    let split = |version: &str| -> Vec<String> {
        version
            .split(['.', '-', '_', '+'])
            .filter(|segment| !segment.is_empty())
            .map(str::to_owned)
            .collect()
    };
    let (left, right) = (split(left), split(right));
    for (left, right) in left.iter().zip(right.iter()) {
        let ordering = match (left.parse::<u64>(), right.parse::<u64>()) {
            (Ok(left), Ok(right)) => left.cmp(&right),
            _ => left.cmp(right),
        };
        if ordering.is_ne() {
            return ordering;
        }
    }
    left.len().cmp(&right.len())
}

fn resolve_argument_list(arguments: &[Argument], context: &RuleContext) -> Vec<String> {
    let mut resolved = Vec::new();
    for argument in arguments {
        match argument {
            Argument::Plain(value) => resolved.push(value.clone()),
            Argument::Conditional { rules, value } if rules_allow(Some(rules), context) => {
                value.append_to(&mut resolved)
            }
            Argument::Conditional { .. } => {}
        }
    }
    resolved
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library(name: &str) -> Library {
        Library {
            name: name.to_owned(),
            ..Library::default()
        }
    }

    #[test]
    fn maven_coordinates_become_repository_paths() {
        assert_eq!(
            maven_path("com.mojang:brigadier:1.0.18").as_deref(),
            Some("com/mojang/brigadier/1.0.18/brigadier-1.0.18.jar")
        );
        assert_eq!(
            maven_path("org.lwjgl:lwjgl:3.3.3:natives-macos").as_deref(),
            Some("org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3-natives-macos.jar")
        );
        // Forge 的 installer 用 @zip 指定扩展名。
        assert_eq!(
            maven_path("net.minecraftforge:forge:1.20.1-47.2.0:installer@zip").as_deref(),
            Some("net/minecraftforge/forge/1.20.1-47.2.0/forge-1.20.1-47.2.0-installer.zip")
        );
    }

    #[test]
    fn coordinates_cannot_be_used_to_escape_the_libraries_directory() {
        // 坐标是从网上拿来的，会被直接拼进本地路径。
        assert!(maven_path("..:evil:1.0").is_none());
        assert!(maven_path("com.example:..:1.0").is_none());
        assert!(maven_path("com.example:evil:../../etc").is_none());
        assert!(maven_path("com/example:evil:1.0").is_none());
        assert!(maven_path("com.example:evil:1.0@../sh").is_none());
        // 空段拼出来是 `com///evil`，对不上任何仓库布局，一样判非法。
        assert!(maven_path("com...:evil:1.0").is_none());
        assert!(maven_path(":evil:1.0").is_none());
        assert!(maven_path("nonsense").is_none());
        assert!(maven_path("a:b:c:d:e").is_none());
    }

    #[test]
    fn rules_use_last_matching_action_and_require_all_features() {
        let rules = vec![
            Rule {
                action: RuleAction::Allow,
                os: Some(OsRule {
                    name: Some("linux".to_owned()),
                    ..OsRule::default()
                }),
                features: None,
            },
            Rule {
                action: RuleAction::Disallow,
                os: None,
                features: Some(HashMap::from([("is_demo_user".to_owned(), true)])),
            },
        ];
        let mut context = RuleContext::linux_x64();
        assert!(rules_allow(Some(&rules), &context));
        context.features.insert("is_demo_user".to_owned(), true);
        assert!(!rules_allow(Some(&rules), &context));
    }

    #[test]
    fn the_childs_library_version_wins_and_arguments_append() {
        let parent = VersionMetadata {
            id: "base".to_owned(),
            libraries: vec![
                library("org.example:core:1.0"),
                library("org.example:parent:1.0"),
            ],
            arguments: Some(Arguments {
                game: vec![Argument::Plain("--parent".to_owned())],
                jvm: vec![Argument::Plain("-Xmx2G".to_owned())],
            }),
            ..VersionMetadata::default()
        };
        let child = VersionMetadata {
            id: "modded".to_owned(),
            libraries: vec![
                library("org.example:core:2.0"),
                library("org.example:child:1.0"),
            ],
            arguments: Some(Arguments {
                game: vec![Argument::Plain("--child".to_owned())],
                jvm: vec![Argument::Plain("-Dmodded=true".to_owned())],
            }),
            ..VersionMetadata::default()
        };

        let merged = VersionMetadata::merge(&parent, &child);
        // 合并只负责接起来，子在前；哪一份进 classpath 由 effective_libraries 说。
        assert_eq!(
            merged
                .libraries
                .iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<_>>(),
            vec![
                "org.example:core:2.0",
                "org.example:child:1.0",
                "org.example:core:1.0",
                "org.example:parent:1.0",
            ]
        );
        assert_eq!(
            merged
                .effective_libraries(&RuleContext::linux_x64())
                .iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<_>>(),
            vec![
                "org.example:core:2.0",
                "org.example:child:1.0",
                "org.example:parent:1.0",
            ]
        );
        let arguments = merged.resolved_arguments(&RuleContext::linux_x64());
        assert_eq!(arguments.0, vec!["-Xmx2G", "-Dmodded=true"]);
        assert_eq!(arguments.1, vec!["--parent", "--child"]);
    }

    #[test]
    fn merge_keeps_each_native_classifier() {
        let parent = VersionMetadata {
            id: "base".to_owned(),
            libraries: vec![
                library("org.lwjgl:lwjgl:3.3.3"),
                library("org.lwjgl:lwjgl:3.3.3:natives-macos"),
            ],
            ..VersionMetadata::default()
        };
        let child = VersionMetadata {
            id: "modded".to_owned(),
            libraries: vec![library("org.lwjgl:lwjgl:3.3.3:natives-macos-arm64")],
            ..VersionMetadata::default()
        };

        let merged = VersionMetadata::merge(&parent, &child);
        assert_eq!(merged.libraries.len(), 3);
    }

    /// 别的启动器写出来的版本 JSON 常常是已经合并好的一整份，没有
    /// `inheritsFrom`——原版的 ASM 和加载器的 ASM 就并排躺在同一个数组里。
    /// 两份都进 classpath，Fabric 加载器会直接拒绝启动。
    #[test]
    fn one_flat_version_json_still_keeps_a_single_asm() {
        let metadata = VersionMetadata {
            id: "Simply Craftmine".to_owned(),
            libraries: vec![
                library("org.ow2.asm:asm:9.6"),
                library("org.slf4j:slf4j-api:2.0.16"),
                library("org.ow2.asm:asm:9.9"),
                library("net.fabricmc:fabric-loader:0.18.4"),
            ],
            ..VersionMetadata::default()
        };
        assert_eq!(
            metadata
                .effective_libraries(&RuleContext::linux_x64())
                .iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<_>>(),
            vec![
                "org.slf4j:slf4j-api:2.0.16",
                "org.ow2.asm:asm:9.9",
                "net.fabricmc:fabric-loader:0.18.4",
            ]
        );
    }

    /// 1.12.2 给 macOS 单列了一份旧 LWJGL：坐标相同、版本不同，用哪一份是
    /// rules 说了算。先去重再看 rules，macOS 上就一个 LWJGL 都不剩了。
    #[test]
    fn rules_decide_before_versions_do() {
        let allow_except_macos = |name: &str| Library {
            name: name.to_owned(),
            rules: Some(vec![
                Rule {
                    action: RuleAction::Allow,
                    os: None,
                    features: None,
                },
                Rule {
                    action: RuleAction::Disallow,
                    os: Some(OsRule {
                        name: Some("osx".to_owned()),
                        ..OsRule::default()
                    }),
                    features: None,
                },
            ]),
            ..Library::default()
        };
        let macos_only = |name: &str| Library {
            name: name.to_owned(),
            rules: Some(vec![Rule {
                action: RuleAction::Allow,
                os: Some(OsRule {
                    name: Some("osx".to_owned()),
                    ..OsRule::default()
                }),
                features: None,
            }]),
            ..Library::default()
        };
        let metadata = VersionMetadata {
            id: "1.12.2".to_owned(),
            libraries: vec![
                allow_except_macos("org.lwjgl.lwjgl:lwjgl:2.9.4-nightly-20150209"),
                macos_only("org.lwjgl.lwjgl:lwjgl:2.9.2-nightly-20140822"),
            ],
            ..VersionMetadata::default()
        };

        let linux = metadata.effective_libraries(&RuleContext::linux_x64());
        assert_eq!(linux.len(), 1);
        assert_eq!(
            linux[0].name,
            "org.lwjgl.lwjgl:lwjgl:2.9.4-nightly-20150209"
        );

        let macos = RuleContext {
            os_name: "osx".to_owned(),
            os_arch: "x86_64".to_owned(),
            ..RuleContext::default()
        };
        let picked = metadata.effective_libraries(&macos);
        assert_eq!(picked.len(), 1);
        assert_eq!(
            picked[0].name,
            "org.lwjgl.lwjgl:lwjgl:2.9.2-nightly-20140822"
        );
    }

    #[test]
    fn versions_compare_by_number_not_by_letter() {
        use std::cmp::Ordering;
        assert_eq!(compare_versions("9.10", "9.9"), Ordering::Greater);
        assert_eq!(compare_versions("9.9", "9.6"), Ordering::Greater);
        assert_eq!(compare_versions("1.0.1", "1.0"), Ordering::Greater);
        assert_eq!(compare_versions("0.18.4", "0.18.4"), Ordering::Equal);
        assert_eq!(
            compare_versions("2.9.4-nightly-20150209", "2.9.2-nightly-20140822"),
            Ordering::Greater
        );
        // 比不出来的写法不能 panic，给个稳定的答案就行。
        assert_eq!(compare_versions("release", "1.0"), Ordering::Greater);
    }

    #[test]
    fn legacy_arguments_receive_required_jvm_defaults() {
        let metadata = VersionMetadata {
            id: "1.12.2".to_owned(),
            minecraft_arguments: Some("--username ${auth_player_name}".to_owned()),
            ..VersionMetadata::default()
        };
        let (jvm, game) = metadata.resolved_arguments(&RuleContext::linux_x64());
        assert!(jvm.iter().any(|item| item == "-cp"));
        assert_eq!(game, vec!["--username", "${auth_player_name}"]);
    }
}
