//! 实例层：一个实例是什么，以及它的目录里有什么。
//!
//! `mod.rs` 是模型——`InstanceProfile` 落盘长什么样由它说了算；`catalog.rs`
//! 是对这些模型的增删改查（建、删、改名、复制、读运行时状况）。其余三个各读
//! 游戏目录里的一类东西：`mods.rs` 还管启用与禁用，`saves.rs` 和 `servers.rs`
//! 只读——删存档、改服务器列表这种事交给文件管理器和游戏自己。
//!
//! `origin.rs` 横穿这几条：谁往游戏目录里放了文件，谁就在那里记一笔。
//! `integrity.rs` 是它的另一半——磁盘上现在是什么样，和记下来的对不对得上。
//! `class.rs` 与 `capability.rs` 又是对账的一条依据：一个文件被静默改写时，
//! 那份代码里多出了哪些原本没有的调用。

pub(crate) mod capability;
pub(crate) mod catalog;
pub(crate) mod class;
pub(crate) mod external;
pub(crate) mod hashes;
pub(crate) mod integrity;
pub(crate) mod jar;
pub(crate) mod mods;
pub(crate) mod origin;
pub(crate) mod prism;
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

/// 叠在游戏本体上的一层。
///
/// **一个实例是一摞有序的层，不是「一个版本 + 一个加载器」。** 这个形状不是
/// 为了让人给一个实例装两个加载器（那只是顺带），是为了**别的启动器建出来的
/// 实例**：Prism 的 `mmc-pack.json` 加 `patches/*.json` 本身就是一份有序的层
/// 表。压成一份合并好的 JSON 也能启动，但从此改不动——换加载器版本、加一层、
/// 删一层，全都做不了。
///
/// 一层只记「是什么、哪个版本、磁盘上那份描述叫什么」。层里到底改了什么
/// （主类、库、tweaker、参数）写在它自己那份版本描述里，由
/// [`crate::launch::version::resolve`] 按顺序合并——这也是 Prism 的做法，而
/// 且免得我们再发明一套表达同一件事的结构。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Component {
    /// 这一层是哪个加载器。[`LoaderKind::Vanilla`] 表示它不是加载器，只是一
    /// 份附加的版本描述（别人的实例里常有）。
    pub kind: LoaderKind,
    /// 这一层自己的版本号，界面上显示的那个（`0.16.5`）。
    pub version: String,
    /// 这一层那份版本描述的 id（`fabric-loader-0.16.5-1.21.1`）。
    ///
    /// 和上面分开存：命名规则是上游的约定，不是我们能保证的东西，而启动时
    /// 要读的是这个 id 对应的文件。装完之后以这里写的为准。
    #[serde(default)]
    pub version_id: String,
    /// 这一层要叠进 client jar 的那些文件，按顺序（jar mod）。
    ///
    /// 1.6 之前的模组就是这么装的：把 class 覆盖进游戏本体，再删掉
    /// `META-INF/`。它没有加载器，所以也没有别的地方能表达它——只能是一层。
    /// 路径指向 Fern 自己实例目录下的副本，不是用户原来那一份。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub jar_mods: Vec<std::path::PathBuf>,
}

/// 旧名字。外面还这么叫的地方留着，省得一次改动横跨太多文件。
pub type LoaderProfile = Component;

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
    /// 主加载器。
    ///
    /// **这是从 [`Self::components`] 算出来的**，不是另一份事实：写盘前由
    /// [`Self::normalized`] 重算。它留在结构里是因为界面、崩溃规则的守卫、
    /// Java 区间都只关心「这个实例是 Forge 还是 Fabric」，而那个问题不该让
    /// 每个调用方都自己去遍历一遍层表。
    #[serde(default)]
    pub loader: LoaderKind,
    /// 叠在游戏本体上的那些层，按顺序。游戏本体自己不在表里——它是
    /// [`Self::game_version`]，换掉它就是换一个实例。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<Component>,
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
    /// 主加载器那一层。多层叠着时是最外面那一个加载器。
    ///
    /// 「最外面」而不是「第一个」：Prism 的层表按依赖顺序排，越靠后越接近
    /// 成品，而决定主类的正是最后那一层。
    pub fn loader_component(&self) -> Option<&Component> {
        self.components
            .iter()
            .rev()
            .find(|component| component.kind != LoaderKind::Vanilla)
    }

    /// 把算得出来的那些字段算一遍。**写盘前必须过这一道。**
    pub fn normalized(mut self) -> Self {
        self.loader = self
            .loader_component()
            .map(|component| component.kind)
            .unwrap_or(LoaderKind::Vanilla);
        self
    }

    /// 从旧形状迁移过来：`loaderProfile` 那一个变成层表里的第一层。
    ///
    /// 只认「层表是空的、而旧字段有值」这一种情况——迁移过一次之后旧字段就
    /// 不再写盘了，所以它不会反复发生。
    pub(crate) fn migrate(raw: &mut serde_json::Value) {
        let Some(object) = raw.as_object_mut() else {
            return;
        };
        let has_components = object
            .get("components")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|components| !components.is_empty());
        if has_components {
            return;
        }
        if let Some(legacy) = object
            .remove("loaderProfile")
            .filter(|value| !value.is_null())
        {
            object.insert(
                "components".to_owned(),
                serde_json::Value::Array(vec![legacy]),
            );
        }
    }

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
            components: Vec::new(),
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

    /// 旧实例读进来就是新形状：那一个加载器变成层表里唯一的一层。
    ///
    /// 迁移只发生在读这一侧，写出去的永远是新形状——所以它不会反复发生，也
    /// 不必在磁盘上留一个「已迁移」的标记。
    #[test]
    fn an_instance_from_before_the_component_list_reads_as_one_layer() {
        let mut raw = serde_json::json!({
            "schemaVersion": 1,
            "id": "moss",
            "name": "Moss",
            "gameVersion": "1.20.1",
            "loader": "fabric",
            "loaderProfile": {
                "kind": "fabric",
                "version": "0.16.5",
                "versionId": "fabric-loader-0.16.5-1.20.1"
            },
            "cover": { "identity": "moss", "growth": 0 }
        });
        InstanceProfile::migrate(&mut raw);
        let profile: InstanceProfile = serde_json::from_value(raw).expect("迁移之后要读得出来");
        assert_eq!(profile.components.len(), 1);
        assert_eq!(
            profile.loader_component().map(|one| one.kind),
            Some(LoaderKind::Fabric)
        );
        assert_eq!(
            profile
                .loader_component()
                .map(|one| one.version_id.as_str()),
            Some("fabric-loader-0.16.5-1.20.1")
        );
        // 旧字段不再写出去。
        let written = serde_json::to_value(&profile).expect("serialize");
        assert!(written.get("loaderProfile").is_none());

        // 原版实例没有那个字段，迁移之后层表是空的，不该凭空多出一层。
        let mut vanilla = serde_json::json!({
            "schemaVersion": 1, "id": "bare", "name": "Bare", "gameVersion": "1.21.1",
            "cover": { "identity": "bare", "growth": 0 }
        });
        InstanceProfile::migrate(&mut vanilla);
        let bare: InstanceProfile = serde_json::from_value(vanilla).expect("read");
        assert!(bare.components.is_empty());
        assert!(bare.loader_component().is_none());
    }

    /// 主加载器是算出来的，不是另存一份。叠了两层时算的是最外面那一层。
    #[test]
    fn the_primary_loader_comes_from_the_outermost_layer() {
        let mut profile =
            InstanceProfile::vanilla(InstanceId::parse("stack").expect("id"), "Stack", "1.7.10");
        profile.components.push(Component {
            kind: LoaderKind::Forge,
            version: "10.13.4.1614".to_owned(),
            version_id: "1.7.10-Forge10.13.4.1614".to_owned(),
            jar_mods: Vec::new(),
        });
        // 一份没有加载器身份的附加描述不该顶替主加载器。
        profile.components.push(Component {
            kind: LoaderKind::Vanilla,
            version: "1".to_owned(),
            version_id: "extra".to_owned(),
            jar_mods: Vec::new(),
        });
        let profile = profile.normalized();
        assert_eq!(profile.loader, LoaderKind::Forge);
        assert_eq!(
            profile.loader_component().map(|one| one.version.as_str()),
            Some("10.13.4.1614")
        );
    }

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
