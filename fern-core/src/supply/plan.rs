//! 安装计划：按下「安装」之前，先把会发生什么算清楚。
//!
//! 上一版直接边解析边下载，于是有两个问题连在一起：
//!
//! 1. **前置是隐形的。** 装一个模组顺带下来四个文件，界面只在装完之后报一句
//!    「已安装 5 个文件」——用户既不知道会发生这件事，也不知道那四个是什么。
//! 2. **前置不去重。** 解析只在这一次调用内部去重，完全不看实例里已经有什么。
//!    于是 Fabric API 被一装再装，每装一个模组就多一份。
//!
//! 两个问题的答案是同一个：**先算出一份计划，再照着执行。** 界面显示的和实际
//! 下载的是同一份数据，不存在「显示一套、做另一套」的空间。

use std::collections::{HashMap, HashSet};

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

use super::ResourceKind;
use crate::DataPaths;

/// 依赖的种类。原样来自 Modrinth。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DependencyKind {
    /// 没有它跑不起来。只有这一种会被自动装上。
    #[default]
    Required,
    /// 作者的建议。装不装是用户的事，替他决定不是我们的事。
    Optional,
    /// 和它装在一起会出问题。
    Incompatible,
    /// 已经打包在这个文件里了，不用另外装。
    Embedded,
}

impl DependencyKind {
    pub(super) fn from_api(value: &str) -> Self {
        match value {
            "optional" => Self::Optional,
            "incompatible" => Self::Incompatible,
            "embedded" => Self::Embedded,
            _ => Self::Required,
        }
    }
}

/// 一个前置现在是什么状况。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RequirementState {
    /// 实例里已经有了，而且用得上。这次不会再下一份。
    Satisfied,
    /// 有，但被禁用了。文件在，加载器不读它——所以游戏仍然起不来。
    Disabled,
    /// 有，但装的那个版本不适用于这个实例。
    Mismatched,
    /// 没有，这次会一起装上。
    Planned,
    /// 没有，而且找不到适用于这个实例的版本。
    Unavailable,
    /// 装了一个作者声明不兼容的东西。
    Conflicting,
}

/// 一条前置，连同它的状态。界面直接照这个渲染。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Requirement {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
    pub kind: DependencyKind,
    pub state: RequirementState,
    /// 已经装着的那个版本号，或者将要装的那个。两者都没有时为空。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_number: Option<String>,
}

/// 这次会落盘的一个文件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedFile {
    pub project_id: String,
    pub version_id: String,
    pub title: String,
    pub version_number: String,
    pub file_name: String,
    pub bytes: u64,
    /// 这是用户点的那一个，不是被牵出来的依赖。
    pub primary: bool,
    #[serde(skip)]
    pub url: String,
    #[serde(skip)]
    pub sha1: String,
}

/// 按下安装会发生什么。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallPlan {
    /// 会下载的文件，第一个是用户点的那一个。
    pub files: Vec<PlannedFile>,
    /// 全部前置，含已经满足的——**满足的那些也要显示**。「已经有了」是这一屏
    /// 最有用的一句话，把它藏起来，用户就只能靠文件数猜发生了什么。
    pub requirements: Vec<Requirement>,
    /// 总字节数。为零表示上游没报大小，不是没东西可下。
    pub bytes: u64,
}

impl InstallPlan {
    /// 有没有哪一条**必需**前置注定装不上。
    ///
    /// 可选的前置找不到不算数：那是作者的建议，本来就可以没有。
    pub fn blocked(&self) -> bool {
        self.requirements.iter().any(|item| {
            item.kind == DependencyKind::Required && item.state == RequirementState::Unavailable
        })
    }
}

/// 算一份计划。不下载任何东西。
pub async fn resolve(
    paths: &DataPaths,
    instance_id: &str,
    version_id: &str,
    kind: ResourceKind,
) -> Result<InstallPlan> {
    let profile = crate::read_instance(paths, instance_id)?;
    let game_version = profile.game_version.as_str();
    let loader = profile.loader;
    let game_directory = crate::instance::paths_for(paths, &profile).game_directory(instance_id);

    // 只有模组有依赖图，也只有模组需要知道实例里已经有什么。
    let installed = if kind.has_dependencies() {
        super::survey::installed(paths, &game_directory).await?
    } else {
        HashMap::new()
    };

    let mut files = Vec::new();
    let mut requirements: Vec<Requirement> = Vec::new();
    let mut resolved_projects = HashSet::new();
    let mut queue = vec![(version_id.to_owned(), 0usize, true)];

    while let Some((id, depth, primary)) = queue.pop() {
        if depth > super::MAX_DEPTH {
            continue;
        }
        let version = super::raw_version(&id).await?;
        if !resolved_projects.insert(version.project_id.clone()) {
            continue;
        }
        let file = version
            .primary_file()
            .ok_or_else(|| anyhow!("{} 这个版本没有可下载的文件", version.name))?;
        files.push(PlannedFile {
            project_id: version.project_id.clone(),
            version_id: version.id.clone(),
            // 项目标题稍后一次性补齐：这里只有 id，为每一个单独发一次请求，
            // 一个五层依赖链就是五次串行往返。
            title: String::new(),
            version_number: version.version_number.clone(),
            file_name: file.filename.clone(),
            bytes: file.size,
            primary,
            url: file.url.clone(),
            sha1: file.hashes.sha1.clone(),
        });

        if !kind.has_dependencies() {
            continue;
        }
        for dependency in &version.dependencies {
            let kind = DependencyKind::from_api(&dependency.dependency_type);
            let Some(project) = dependency.project_id.clone().filter(|id| !id.is_empty()) else {
                // 只给了 version_id 的必需依赖仍然要装，只是它没有项目 id 可
                // 供去重——那就照上游说的装。
                if kind == DependencyKind::Required
                    && let Some(exact) = dependency.version_id.clone().filter(|id| !id.is_empty())
                {
                    queue.push((exact, depth + 1, false));
                }
                continue;
            };
            if requirements.iter().any(|item| item.project_id == project) {
                continue;
            }
            // 打包在里面的东西不用管：它已经在那个 jar 里了。
            if kind == DependencyKind::Embedded {
                continue;
            }

            let (state, version_number) = match installed.get(&project) {
                // 装了一个作者声明不兼容的东西。这句话必须说，而且只有在真的
                // 装了的时候才说——没装的「不兼容」是一条噪声。
                Some(existing) if kind == DependencyKind::Incompatible => (
                    RequirementState::Conflicting,
                    Some(existing.version_number.clone()),
                ),
                // 文件在，但加载器不读它——所以游戏照样起不来。这和「已经有了」
                // 是两回事。
                Some(existing) if !existing.enabled => (
                    RequirementState::Disabled,
                    Some(existing.version_number.clone()),
                ),
                Some(existing) if !existing.fits(game_version, loader) => (
                    RequirementState::Mismatched,
                    Some(existing.version_number.clone()),
                ),
                Some(existing) => (
                    RequirementState::Satisfied,
                    Some(existing.version_number.clone()),
                ),
                // 不兼容的那一项没装，那就什么也不用说。
                None if kind == DependencyKind::Incompatible => continue,
                // 这一次已经把它排进计划里了（另一个模组也依赖它）。
                None if resolved_projects.contains(&project) => (RequirementState::Planned, None),
                None => {
                    // 没装。挑一个装得上的版本；只有必需的才真的排进下载。
                    match pick_candidate(&project, game_version, loader).await? {
                        Some(chosen) => {
                            let number = chosen.version_number.clone();
                            if kind == DependencyKind::Required {
                                queue.push((chosen.id, depth + 1, false));
                            }
                            (RequirementState::Planned, Some(number))
                        }
                        None => (RequirementState::Unavailable, None),
                    }
                }
            };

            requirements.push(Requirement {
                project_id: project,
                slug: String::new(),
                title: String::new(),
                icon_url: None,
                kind,
                state,
                version_number,
            });
        }
    }

    // 主项目排在最前：它是用户点的那一个，剩下的是它牵出来的。
    files.sort_by_key(|file| !file.primary);
    let bytes = files.iter().map(|file| file.bytes).sum();

    // 名字一次性补齐。id 对用户没有任何意义，而「fabric-api」和「Fabric API」
    // 之间的差别就是这一屏读不读得下去。
    let mut wanted: Vec<String> = requirements
        .iter()
        .map(|item| item.project_id.clone())
        .collect();
    wanted.extend(files.iter().map(|file| file.project_id.clone()));
    let names = super::project_names(&wanted).await.unwrap_or_default();
    for item in &mut requirements {
        if let Some(named) = names.get(&item.project_id) {
            item.slug = named.slug.clone();
            item.title = named.title.clone();
            item.icon_url = named.icon_url.clone();
        } else {
            item.title = item.project_id.clone();
        }
    }
    for file in &mut files {
        file.title = names
            .get(&file.project_id)
            .map(|named| named.title.clone())
            .unwrap_or_else(|| file.file_name.clone());
    }

    Ok(InstallPlan {
        files,
        requirements,
        bytes,
    })
}

/// 依赖只给了项目时，挑一个装得上的版本：优先正式版，没有就用最新的。
async fn pick_candidate(
    project: &str,
    game_version: &str,
    loader: crate::LoaderKind,
) -> Result<Option<super::RawVersion>> {
    let mut candidates = super::raw_versions(project, game_version, loader).await?;
    let chosen = candidates
        .iter()
        .position(|version| version.version_type == "release")
        .or(if candidates.is_empty() { None } else { Some(0) });
    Ok(chosen.map(|index| candidates.swap_remove(index)))
}

/// 一个项目在界面上叫什么。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Named {
    pub slug: String,
    pub title: String,
    pub icon_url: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn requirement(kind: DependencyKind, state: RequirementState) -> Requirement {
        Requirement {
            project_id: "P".to_owned(),
            slug: "p".to_owned(),
            title: "P".to_owned(),
            icon_url: None,
            kind,
            state,
            version_number: None,
        }
    }

    #[test]
    fn the_plan_reaches_the_interface_in_the_shape_it_expects() {
        // 字段名的约定在 AGENTS.md：类型标签 snake_case，数据字段 camelCase。
        // 这里对的是后者——写错了不会有编译错误，只会在界面上变成 undefined。
        let plan = InstallPlan {
            files: vec![PlannedFile {
                project_id: "AANobbMI".to_owned(),
                version_id: "abcd".to_owned(),
                title: "Sodium".to_owned(),
                version_number: "0.6.13".to_owned(),
                file_name: "sodium.jar".to_owned(),
                bytes: 1024,
                primary: true,
                url: "https://example.invalid/sodium.jar".to_owned(),
                sha1: "deadbeef".to_owned(),
            }],
            requirements: vec![requirement(
                DependencyKind::Required,
                RequirementState::Satisfied,
            )],
            bytes: 1024,
        };
        let value = serde_json::to_value(&plan).expect("serialize plan");
        assert_eq!(value["files"][0]["fileName"], "sodium.jar");
        assert_eq!(value["files"][0]["versionNumber"], "0.6.13");
        assert_eq!(value["requirements"][0]["state"], "satisfied");
        assert_eq!(value["requirements"][0]["kind"], "required");
        // 下载地址和校验和不出这道门：界面用不上，而它们是可以被拼进请求的
        // 网络来源字符串。
        assert!(value["files"][0].get("url").is_none());
        assert!(value["files"][0].get("sha1").is_none());
    }

    #[test]
    fn dependency_kinds_default_to_required() {
        assert_eq!(
            DependencyKind::from_api("required"),
            DependencyKind::Required
        );
        assert_eq!(
            DependencyKind::from_api("optional"),
            DependencyKind::Optional
        );
        assert_eq!(
            DependencyKind::from_api("incompatible"),
            DependencyKind::Incompatible
        );
        assert_eq!(
            DependencyKind::from_api("embedded"),
            DependencyKind::Embedded
        );
        // 认不出来的当必需：漏装一个前置的代价（游戏起不来）比多装一个大。
        assert_eq!(
            DependencyKind::from_api("brand-new"),
            DependencyKind::Required
        );
    }

    #[test]
    fn a_plan_is_blocked_only_by_a_required_dependency_with_nowhere_to_get_it() {
        let plan = |items: Vec<Requirement>| InstallPlan {
            files: Vec::new(),
            requirements: items,
            bytes: 0,
        };
        assert!(
            plan(vec![requirement(
                DependencyKind::Required,
                RequirementState::Unavailable
            )])
            .blocked()
        );
        assert!(
            !plan(vec![requirement(
                DependencyKind::Required,
                RequirementState::Satisfied
            )])
            .blocked()
        );
        // 可选的东西找不到不该拦住安装。
        assert!(
            !plan(vec![requirement(
                DependencyKind::Optional,
                RequirementState::Unavailable
            )])
            .blocked()
        );
    }
}
