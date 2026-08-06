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
        let mut libraries = Vec::with_capacity(parent.libraries.len() + child.libraries.len());
        let mut seen = HashSet::new();
        for library in child.libraries.iter().chain(parent.libraries.iter()) {
            let key = library
                .name
                .rsplit_once(':')
                .map_or(library.name.as_str(), |(head, _)| head);
            if seen.insert(key.to_owned()) {
                libraries.push(library.clone());
            }
        }

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
    fn merge_prefers_child_library_version_and_appends_arguments() {
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
        assert_eq!(
            merged
                .libraries
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
