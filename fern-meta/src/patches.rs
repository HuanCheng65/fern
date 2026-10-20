//! 按 Prism Launcher 的库补丁表改写一份版本描述。
//!
//! Mojang 只为它自己发行的平台组合准备原生库。1.18.2 及更早的版本只带
//! x86_64 的（LWJGL 3.2.x 及以前、LWJGL 2），Apple Silicon 上的原生 Java 加载
//! 不了。Prism 维护着一张表，把这些库换成带 arm64 原生库的构建；表本身是数据，
//! 由 `.github/osx-arm64-library-patches.py` 取过来，这里只负责照它的语义应用。
//!
//! ## 为什么要连规则语义一起照搬
//!
//! 表是按 Prism 的规则语义写的，而它和 Mojang 原本的写法差一处：在 Prism 里，
//! 规则里的系统名 `osx` **只匹配 x86 的 Mac**，Apple Silicon 是 `osx-arm64`。
//! 表里「在 osx-arm64 上禁用旧 LWJGL、换成新的」正是靠这一点成立的。用 Mojang
//! 的语义去读同一张表，1.12.2 会挑中那份只为 Intel Mac 准备的旧 LWJGL。
//!
//! 所以这里分两步，和 Prism 自己的顺序一样：先把补丁打进库清单（合并
//! override、追加新库），再按 Prism 的语义为 `platform` 求值规则和原生库，
//! 求完之后把规则去掉、原生库的键改回 `generic`。交出去的是一份不再需要
//! 平台判断的清单，后面按 Mojang 语义读它的代码不用知道这里发生过什么。
//!
//! ## 只碰需要碰的版本
//!
//! 表里也有几条对新版本生效（较新的 JNA、LWJGL 3.4.1）。新版本本来就带
//! arm64 原生库、本来就跑得起来，切到另一套语义只会多出风险。所以只有清单里
//! 有**旧 LWJGL**（2.x、3.1、3.2）时才动手，其余原样返回 `None`。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{Library, LibraryDownloads, Rule, RuleAction, VersionMetadata};

/// 补丁表文件的形状。
#[derive(Debug, Clone, Deserialize)]
pub struct PatchTable {
    pub patches: Vec<LibraryPatch>,
}

/// 一条补丁。字段名就是 Prism 的。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryPatch {
    /// 命中哪些库：完整坐标，`group:artifact:version`。
    #[serde(rename = "match")]
    pub matches: Vec<String>,
    /// 合并进命中的那条库。列表是追加，表是合并（补丁优先）。
    #[serde(default, rename = "override")]
    pub overrides: Option<LibraryOverride>,
    /// 追加到清单末尾的库。
    #[serde(default)]
    pub additional_libraries: Vec<Library>,
    /// 追加的那几条自己也要再过一遍补丁表。
    #[serde(default)]
    pub patch_additional_libraries: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LibraryOverride {
    #[serde(default)]
    pub rules: Option<Vec<Rule>>,
    #[serde(default)]
    pub natives: Option<HashMap<String, String>>,
    #[serde(default)]
    pub downloads: Option<LibraryDownloads>,
}

impl LibraryPatch {
    fn applies(&self, library: &Library) -> bool {
        self.matches.iter().any(|name| name == &library.name)
    }
}

/// 照补丁表改写，并按 `platform`（Prism 的写法，如 `osx-arm64`）求值。
///
/// `generic` 是求值之后原生库的键要改成的那个名字（Mojang 的写法，如 `osx`）
/// ——后面的代码按它找 classifier。没有旧 LWJGL 时返回 `None`：这份描述不需要
/// 动。
pub fn apply(
    metadata: &VersionMetadata,
    patches: &[LibraryPatch],
    platform: &str,
    generic: &str,
) -> Option<VersionMetadata> {
    if !metadata
        .libraries
        .iter()
        .any(|library| is_legacy_lwjgl(&library.name))
    {
        return None;
    }

    let mut libraries = Vec::with_capacity(metadata.libraries.len());
    let mut added: Vec<Library> = Vec::new();
    for library in &metadata.libraries {
        let (patched, extra) = patch_library(library.clone(), patches);
        libraries.push(patched);
        for library in extra {
            if !added.contains(&library) {
                added.push(library);
            }
        }
    }
    libraries.extend(added);

    let libraries = libraries
        .into_iter()
        .filter_map(|library| settle(library, platform, generic))
        .collect();
    Some(VersionMetadata {
        libraries,
        ..metadata.clone()
    })
}

/// 旧 LWJGL：没有 arm64 原生库的那几代。
fn is_legacy_lwjgl(name: &str) -> bool {
    let mut parts = name.split(':');
    let (Some(group), Some(_), Some(version)) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    match group {
        "org.lwjgl.lwjgl" => version.starts_with("2."),
        "org.lwjgl" => version.starts_with("3.1.") || version.starts_with("3.2."),
        _ => false,
    }
}

/// Prism 的 `patch_library`：打补丁，返回这条库连带出来的新库。
///
/// `patchAdditionalLibraries` 的那些要再过一遍表，改写落在它们自己身上——同名
/// 的库可以有两条（一条进 classpath、一条带原生库表），所以按位置记，不按名字。
fn patch_library(library: Library, patches: &[LibraryPatch]) -> (Library, Vec<Library>) {
    let mut root = library;
    let mut added: Vec<Library> = Vec::new();
    // `None` 是传进来的那条，`Some(i)` 是 `added[i]`。
    let mut queue: std::collections::VecDeque<Option<usize>> = [None].into();
    while let Some(slot) = queue.pop_front() {
        let target = match slot {
            None => &mut root,
            Some(index) => &mut added[index],
        };
        let mut arrivals: Vec<(Vec<Library>, bool)> = Vec::new();
        for patch in patches {
            if !patch.applies(target) {
                continue;
            }
            if let Some(overrides) = &patch.overrides {
                merge(target, overrides);
            }
            if !patch.additional_libraries.is_empty() {
                arrivals.push((
                    patch.additional_libraries.clone(),
                    patch.patch_additional_libraries,
                ));
            }
        }
        allow_arch_native(target);
        for (libraries, again) in arrivals {
            for library in libraries {
                added.push(library);
                if again {
                    queue.push_back(Some(added.len() - 1));
                }
            }
        }
    }
    (root, added)
}

/// Prism 的 `MetaBase.merge`：列表追加，表合并（补丁优先），其余覆盖。
fn merge(target: &mut Library, overrides: &LibraryOverride) {
    if let Some(rules) = &overrides.rules {
        target
            .rules
            .get_or_insert_with(Vec::new)
            .extend(rules.iter().cloned());
    }
    if let Some(natives) = &overrides.natives {
        let map = target.natives.get_or_insert_with(HashMap::new);
        for (key, value) in natives {
            map.insert(key.clone(), value.clone());
        }
    }
    if let Some(downloads) = &overrides.downloads {
        let existing = target
            .downloads
            .get_or_insert_with(LibraryDownloads::default);
        if let Some(artifact) = &downloads.artifact {
            existing.artifact = Some(artifact.clone());
        }
        if let Some(classifiers) = &downloads.classifiers {
            let map = existing.classifiers.get_or_insert_with(HashMap::new);
            for (key, value) in classifiers {
                map.insert(key.clone(), value.clone());
            }
        }
    }
}

/// Prism 的 `allow_arch_native`：只按通用系统名放行的架构专属原生库，改成只对
/// 那个架构放行。
///
/// Mojang 把 `natives-macos-arm64` 只挂在 `osx` 下面；在 Prism 的语义里 `osx`
/// 不匹配 Apple Silicon，不改写的话它就被筛掉了。
fn allow_arch_native(library: &mut Library) {
    const ARCH_NATIVE_OS: [(&str, &str, &str); 2] = [
        ("macos-arm64", "osx-arm64", "osx"),
        ("macos-aarch64", "osx-arm64", "osx"),
    ];
    let mut parts = library.name.split(':');
    let artifact = parts.nth(1).unwrap_or_default();
    let classifier = parts.nth(1).unwrap_or_default();
    let native_name = format!("{artifact}-{classifier}");
    let Some(rules) = library.rules.as_mut() else {
        return;
    };
    if rules
        .iter()
        .any(|rule| rule.action == RuleAction::Allow && rule.os.is_none())
    {
        return;
    }
    let names = |action: RuleAction, rules: &[Rule]| -> Vec<String> {
        rules
            .iter()
            .filter(|rule| rule.action == action)
            .filter_map(|rule| rule.os.as_ref()?.name.clone())
            .collect()
    };
    let allowed = names(RuleAction::Allow, rules);
    let disallowed = names(RuleAction::Disallow, rules);
    for (native, os, generic) in ARCH_NATIVE_OS {
        if !native_name.contains(native)
            || disallowed.iter().any(|name| name == os)
            || !allowed.iter().any(|name| name == generic)
        {
            continue;
        }
        rules.retain(|rule| {
            !(rule.action == RuleAction::Allow
                && rule.os.as_ref().and_then(|os| os.name.as_deref()) == Some(generic))
        });
        if !allowed.iter().any(|name| name == os) {
            rules.push(Rule {
                action: RuleAction::Allow,
                os: Some(crate::OsRule {
                    name: Some(os.to_owned()),
                    ..crate::OsRule::default()
                }),
                features: None,
            });
        }
    }
}

/// 按 Prism 的语义为 `platform` 求值：不该要的返回 `None`，该要的去掉规则、
/// 原生库只留这个平台的那一项并改回通用名。
fn settle(mut library: Library, platform: &str, generic: &str) -> Option<Library> {
    if let Some(rules) = library.rules.take() {
        let mut allowed = false;
        for rule in &rules {
            let applies = rule
                .os
                .as_ref()
                .is_none_or(|os| os.name.as_deref().is_none_or(|name| name == platform));
            if applies {
                allowed = rule.action == RuleAction::Allow;
            }
        }
        if !allowed {
            return None;
        }
    }
    if let Some(natives) = library.natives.take() {
        // 有原生库表、却没有这个平台的那一项：这条库在这台机器上什么都不带。
        let classifier = natives.get(platform)?;
        library.natives = Some(HashMap::from([(generic.to_owned(), classifier.clone())]));
    }
    Some(library)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DownloadInfo, RuleContext};

    fn info(url: &str) -> DownloadInfo {
        DownloadInfo {
            id: None,
            sha1: "00".to_owned(),
            size: 1,
            url: url.to_owned(),
            path: None,
        }
    }

    fn table() -> Vec<LibraryPatch> {
        let text = include_str!("../../fern-core/rules/library-patches-osx-arm64.json");
        serde_json::from_str::<PatchTable>(text)
            .expect("the committed table parses")
            .patches
    }

    /// 1.16.5 那种写法：同一个坐标两条，一条进 classpath，一条带原生库表。
    fn lwjgl_322() -> Vec<Library> {
        vec![
            Library {
                name: "org.lwjgl:lwjgl:3.2.2".to_owned(),
                downloads: Some(LibraryDownloads {
                    artifact: Some(info("https://libraries.minecraft.net/lwjgl-3.2.2.jar")),
                    classifiers: None,
                }),
                ..Library::default()
            },
            Library {
                name: "org.lwjgl:lwjgl:3.2.2".to_owned(),
                downloads: Some(LibraryDownloads {
                    artifact: Some(info("https://libraries.minecraft.net/lwjgl-3.2.2.jar")),
                    classifiers: Some(HashMap::from([(
                        "natives-macos".to_owned(),
                        info("https://libraries.minecraft.net/lwjgl-3.2.2-natives-macos.jar"),
                    )])),
                }),
                natives: Some(HashMap::from([(
                    "osx".to_owned(),
                    "natives-macos".to_owned(),
                )])),
                ..Library::default()
            },
            Library {
                name: "com.mojang:brigadier:1.0.17".to_owned(),
                downloads: Some(LibraryDownloads {
                    artifact: Some(info("https://libraries.minecraft.net/brigadier.jar")),
                    classifiers: None,
                }),
                ..Library::default()
            },
        ]
    }

    fn mac() -> RuleContext {
        RuleContext {
            os_name: "osx".to_owned(),
            os_arch: "aarch64".to_owned(),
            ..RuleContext::default()
        }
    }

    #[test]
    fn old_lwjgl_is_swapped_for_a_build_with_arm64_natives() {
        let metadata = VersionMetadata {
            libraries: lwjgl_322(),
            ..VersionMetadata::default()
        };
        let patched = apply(&metadata, &table(), "osx-arm64", "osx").expect("legacy LWJGL");
        let names: Vec<&str> = patched
            .effective_libraries(&mac())
            .iter()
            .map(|library| library.name.as_str())
            .collect();
        assert!(!names.contains(&"org.lwjgl:lwjgl:3.2.2"));
        assert!(names.contains(&"org.lwjgl:lwjgl:3.3.1-mmachina.1"));
        // 和 LWJGL 无关的库原样留着。
        assert!(names.contains(&"com.mojang:brigadier:1.0.17"));

        // 原生库那一条要能被按 Mojang 语义找到：键是 osx，classifier 是 arm64 的那份。
        let native = patched
            .effective_libraries(&mac())
            .into_iter()
            .filter_map(|library| library.file(&mac()))
            .find(|file| file.native && file.url.contains("mmachina"))
            .expect("an arm64 native for LWJGL");
        assert!(native.path.contains("3.3.1-mmachina.1"));
        // 求值完之后不再需要平台判断。
        assert!(
            patched
                .libraries
                .iter()
                .all(|library| library.rules.is_none())
        );
    }

    #[test]
    fn versions_that_already_ship_arm64_natives_are_left_alone() {
        let metadata = VersionMetadata {
            libraries: vec![Library {
                name: "org.lwjgl:lwjgl:3.3.1".to_owned(),
                ..Library::default()
            }],
            ..VersionMetadata::default()
        };
        assert!(apply(&metadata, &table(), "osx-arm64", "osx").is_none());
    }

    #[test]
    fn a_library_only_for_intel_macs_is_dropped_under_prism_rules() {
        // 1.12.2 给 macOS 单列的那份旧 LWJGL 只按 `osx` 放行。在 Prism 的语义里
        // 那是 Intel Mac；Apple Silicon 上要用的是补丁换上来的那一份。
        let intel_only = Library {
            name: "org.lwjgl.lwjgl:lwjgl:2.9.2-nightly-20140822".to_owned(),
            rules: Some(vec![Rule {
                action: RuleAction::Allow,
                os: Some(crate::OsRule {
                    name: Some("osx".to_owned()),
                    ..crate::OsRule::default()
                }),
                features: None,
            }]),
            ..Library::default()
        };
        assert!(settle(intel_only, "osx-arm64", "osx").is_none());
    }

    fn os_rule(action: RuleAction, name: Option<&str>) -> Rule {
        Rule {
            action,
            os: name.map(|name| crate::OsRule {
                name: Some(name.to_owned()),
                ..crate::OsRule::default()
            }),
            features: None,
        }
    }

    #[test]
    fn lwjgl_2_on_apple_silicon_keeps_the_newer_build_with_arm64_natives() {
        // 1.12.2 的写法：2.9.4 给除 macOS 以外的系统，2.9.2 单给 macOS。
        let everywhere_but_mac = || {
            Some(vec![
                os_rule(RuleAction::Allow, None),
                os_rule(RuleAction::Disallow, Some("osx")),
            ])
        };
        let platform = |version: &str, rules| Library {
            name: format!("org.lwjgl.lwjgl:lwjgl-platform:{version}"),
            downloads: Some(LibraryDownloads {
                artifact: None,
                classifiers: Some(HashMap::from([(
                    "natives-osx".to_owned(),
                    info(&format!(
                        "https://libraries.minecraft.net/{version}-natives-osx.jar"
                    )),
                )])),
            }),
            natives: Some(HashMap::from([(
                "osx".to_owned(),
                "natives-osx".to_owned(),
            )])),
            rules,
            ..Library::default()
        };
        let metadata = VersionMetadata {
            libraries: vec![
                Library {
                    name: "org.lwjgl.lwjgl:lwjgl:2.9.4-nightly-20150209".to_owned(),
                    rules: everywhere_but_mac(),
                    ..Library::default()
                },
                Library {
                    name: "org.lwjgl.lwjgl:lwjgl:2.9.2-nightly-20140822".to_owned(),
                    rules: Some(vec![os_rule(RuleAction::Allow, Some("osx"))]),
                    ..Library::default()
                },
                platform("2.9.4-nightly-20150209", everywhere_but_mac()),
                platform(
                    "2.9.2-nightly-20140822",
                    Some(vec![os_rule(RuleAction::Allow, Some("osx"))]),
                ),
            ],
            ..VersionMetadata::default()
        };
        let patched = apply(&metadata, &table(), "osx-arm64", "osx").expect("legacy LWJGL");
        let names: Vec<&str> = patched
            .libraries
            .iter()
            .map(|library| library.name.as_str())
            .collect();
        assert!(names.contains(&"org.lwjgl.lwjgl:lwjgl:2.9.4-nightly-20150209"));
        assert!(!names.iter().any(|name| name.contains("2.9.2-nightly")));

        let native = patched
            .effective_libraries(&mac())
            .into_iter()
            .filter_map(|library| library.file(&mac()))
            .find(|file| file.native)
            .expect("the platform natives");
        assert!(native.url.contains("MinecraftMachina"), "{}", native.url);
    }

    #[test]
    fn the_committed_table_only_points_at_https_with_hashes() {
        for patch in table() {
            for library in &patch.additional_libraries {
                let downloads = library.downloads.as_ref().expect("downloads");
                let files = downloads
                    .artifact
                    .iter()
                    .chain(downloads.classifiers.iter().flat_map(|map| map.values()));
                for file in files {
                    assert!(file.url.starts_with("https://"), "{}", file.url);
                    assert_eq!(file.sha1.len(), 40, "{}", file.url);
                }
            }
        }
    }
}
