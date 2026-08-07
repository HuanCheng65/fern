//! 实例层：一个实例是什么，以及它的目录里有什么。
//!
//! `mod.rs` 是模型——`InstanceProfile` 落盘长什么样由它说了算；`catalog.rs`
//! 是对这些模型的增删改查（建、删、改名、复制、读运行时状况）。其余三个各读
//! 游戏目录里的一类东西：`mods.rs` 还管启用与禁用，`saves.rs` 和 `servers.rs`
//! 只读——删存档、改服务器列表这种事交给文件管理器和游戏自己。

pub(crate) mod catalog;
pub(crate) mod external;
pub(crate) mod mods;
pub(crate) mod saves;
pub(crate) mod servers;

use std::{fmt, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::DataPaths;

/// 这个实例实际用的那套目录。
///
/// **每条会碰文件的链路都要从这里开始。** 私有实例拿到的就是那份全局布局；
/// 外部实例拿到的是一份指向它自己那个 `.minecraft` 的副本，于是下游那三十
/// 来个 `paths.versions` / `paths.game_directory(...)` 一个字都不用改。
///
/// 判断只发生一次，就在入口处——散在下游各处判断「这个实例是不是外部的」，
/// 迟早会漏掉一处，而漏掉的那一处会把文件写进错误的目录。
pub fn paths_for(paths: &DataPaths, profile: &InstanceProfile) -> DataPaths {
    paths.scoped(
        profile.external.as_ref(),
        &crate::launch::version::effective_id(profile),
    )
}

/// 只有 id 的调用方用这一个。读不到描述就退回全局布局。
pub fn paths_by_id(paths: &DataPaths, instance_id: &str) -> DataPaths {
    match crate::read_instance(paths, instance_id) {
        Ok(profile) => paths_for(paths, &profile),
        Err(_) => paths.clone(),
    }
}

pub const INSTANCE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InstanceId(String);

impl InstanceId {
    pub fn parse(value: impl Into<String>) -> Result<Self, InstanceIdError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
        if valid {
            Ok(Self(value))
        } else {
            Err(InstanceIdError)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstanceIdError;

impl fmt::Display for InstanceIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("instance id must contain 1-64 ASCII letters, numbers, '-' or '_'")
    }
}

impl std::error::Error for InstanceIdError {}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoaderKind {
    #[default]
    Vanilla,
    Fabric,
    Quilt,
    NeoForge,
    Forge,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoaderProfile {
    pub kind: LoaderKind,
    /// 加载器自己的版本号，界面上显示的那个（`0.16.5`）。
    pub version: String,
    /// 加载器生成的那份版本描述的 id（`fabric-loader-0.16.5-1.21.1`）。
    ///
    /// 和上面分开存：命名规则是上游的约定，不是我们能保证的东西，而启动时
    /// 要读的是这个 id 对应的文件。装完之后以 profile 里写的为准。
    #[serde(default)]
    pub version_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverSeed {
    pub identity: String,
    #[serde(default)]
    pub growth: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

/// 垃圾回收器（文档 §6.2）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GarbageCollector {
    /// 由启动器按 Java 大版本、版本世代和实例内容决定。
    ///
    /// 这是默认值，而且是唯一一个会随时间变好的选项：Java 21 以上给分代 ZGC，
    /// 更老的给 G1，26.1 起的原版则完全不插手——Mojang 自己已经调好了。
    /// 上一版的默认是写死的 G1，那等于把这个判断永久冻结在 2024 年。
    #[default]
    Auto,
    /// G1 加一组客户端向的参数。
    G1,
    /// 分代 ZGC。停顿更短，但要更多堆外空间。
    Z,
}

/// 进程优先级（文档 §6.3）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProcessPriority {
    /// 后台跑着别的活的时候用，游戏让路。
    Low,
    #[default]
    Normal,
    /// 游戏优先。多数情况下没必要——调度器本来就偏向前台进程。
    High,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub java_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_memory_mb: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<Resolution>,
    /// 不填就是 G1。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub garbage_collector: Option<GarbageCollector>,
    /// 不填就是 Normal。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_priority: Option<ProcessPriority>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceProfile {
    pub schema_version: u32,
    pub id: InstanceId,
    pub name: String,
    pub game_version: String,
    #[serde(default)]
    pub loader: LoaderKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loader_profile: Option<LoaderProfile>,
    pub cover: CoverSeed,
    #[serde(default)]
    pub settings: InstanceSettings,
    /// 这个实例用哪个账户。None 表示跟着当前账户走。
    ///
    /// 放在这里而不是 `settings` 里，因为 `settings` 是被整份替换的（实例
    /// 设置面板一次提交一整屏），一个它不认识的字段会被顺手抹掉。
    ///
    /// 第一次成功启动会把当时用的那一个记下来，所以「绑定」不需要一个绑定
    /// 界面——它是「记住上次」的副产品。下周再打开这个整合包，它还是用小号，
    /// 哪怕这期间你用大号玩过别的。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    /// 这个实例的游戏文件不在 Fern 的目录里。
    ///
    /// 有值就意味着：**那些文件不归我们所有**。删掉这个实例只会删掉这一份
    /// 描述，一个游戏文件都不动；复制它也没有意义——两个实例指着同一个游戏
    /// 目录，等于两份存档互相覆盖。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external: Option<crate::data::ExternalGame>,
    /// 上次真的把游戏跑起来的时刻，Unix 秒。从没玩过就是 None。
    ///
    /// 曲库默认按它排序——「上次玩的那个」几乎总是「这次要玩的那个」。存
    /// 时刻而不是次数：次数会让一个玩过一次就弃掉的实例永远排在前面。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_played: Option<u64>,
}

impl InstanceProfile {
    pub fn vanilla(
        id: InstanceId,
        name: impl Into<String>,
        game_version: impl Into<String>,
    ) -> Self {
        let cover_identity = id.as_str().to_owned();
        Self {
            schema_version: INSTANCE_SCHEMA_VERSION,
            id,
            name: name.into(),
            game_version: game_version.into(),
            loader: LoaderKind::Vanilla,
            loader_profile: None,
            account_id: None,
            cover: CoverSeed {
                identity: cover_identity,
                growth: 0,
            },
            settings: InstanceSettings::default(),
            external: None,
            last_played: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_profile_has_a_stable_json_shape() {
        let profile = InstanceProfile::vanilla(
            InstanceId::parse("cinder-valley").expect("valid id"),
            "Cinder Valley",
            "1.21.1",
        );
        let value = serde_json::to_value(&profile).expect("serialize profile");

        assert_eq!(value["schemaVersion"], 1);
        assert_eq!(value["gameVersion"], "1.21.1");
        assert_eq!(value["loader"], "vanilla");
        assert_eq!(value["cover"]["identity"], "cinder-valley");
    }

    #[test]
    fn instance_ids_are_safe_as_directory_names() {
        assert!(InstanceId::parse("moss_archive-2").is_ok());
        assert!(InstanceId::parse("../other").is_err());
        assert!(InstanceId::parse("contains spaces").is_err());
    }
}
