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
pub(crate) mod discover;
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

/// 实例描述的形状版本。
///
/// 2：游戏本体进了层表（见 [`InstanceProfile::migrate`]）。
pub const INSTANCE_SCHEMA_VERSION: u32 = 2;

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
    /// 只存在于 1.5.2–1.12.2，而且**能叠在 Forge 上**——它是第一个真正意义上
    /// 的附加层，见 [`crate::launch::liteloader`]。
    LiteLoader,
}

impl LoaderKind {
    /// 它是叠在别人上面的一层，不是「主加载器」。
    ///
    /// 这个区别有实际后果：主加载器决定 Java 区间、崩溃规则的守卫、模组在
    /// 补给站按什么标签筛。一个 Forge + LiteLoader 的实例，主加载器是
    /// Forge——把最外面那一层当成主加载器的话，Java 上限那条就没人给了。
    pub fn stackable(self) -> bool {
        matches!(self, Self::LiteLoader)
    }
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
///
/// **游戏本体是第 0 层，不是层表之外的东西。** 它一度不在表里——「实例记着
/// 游戏版本号，那就是它那份描述的名字」——而那个等号只在我们自己建的实例里
/// 成立。别人的目录里，一份合并好的描述叫着别人起的名字（`1.16.5-Fabric
/// 0.14.11`、`Simply Craftmine`），版本号只是从 client jar 里认出来的一个标
/// 签。等号不成立时，凡是从标签去拼路径的代码都指向一个不存在的文件，而它每
/// 次都以不同的面目回来：客户端 jar 找不到、版本描述读不到、存档被写进另一个
/// 目录。所以这里不留那个等号——**要读哪份文件，只能从层表里问出来**。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Component {
    /// 这一层是哪个加载器。[`LoaderKind::Vanilla`] 表示它不是加载器：游戏本
    /// 体那一层，或者一份附加的版本描述（别人的实例里常有）。
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
    /// 不填就跟随全局默认。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_priority: Option<ProcessPriority>,
    /// 额外 JVM 参数，原样一行。不填就跟随全局那一行。
    ///
    /// 填了是**整段换掉**，不是接在全局后面——老整合包要的那几个 flag 常常和
    /// 全局那行冲突，接在后面就没有任何实例摆脱得了它。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jvm_arguments: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceProfile {
    pub schema_version: u32,
    pub id: InstanceId,
    pub name: String,
    /// 这是哪个 Minecraft 正式版。**一个标签，不是一个文件名。**
    ///
    /// 它回答的是「这个实例算 1.16.5」——Java 区间、模组按什么版本筛、崩溃
    /// 往哪个版本上归因、导出的整合包声明依赖哪个版本，都只要这一个答案。要
    /// 读哪份版本描述则要问 [`Self::components`]，两件事在别人的目录里对不上
    /// （见 [`Component`]）。
    pub game_version: String,
    /// 主加载器。
    ///
    /// **这是从 [`Self::components`] 算出来的**，不是另一份事实：写盘前由
    /// [`Self::normalized`] 重算。它留在结构里是因为界面、崩溃规则的守卫、
    /// Java 区间都只关心「这个实例是 Forge 还是 Fabric」，而那个问题不该让
    /// 每个调用方都自己去遍历一遍层表。
    #[serde(default)]
    pub loader: LoaderKind,
    /// 这个实例的那一摞层，从游戏本体开始，按顺序。
    ///
    /// **游戏本体也在表里**（第 0 层，`kind` 是 Vanilla），因为只有它知道自己
    /// 那份描述在磁盘上叫什么。启动、补全、找 client jar、算游戏目录读的都是
    /// 这张表，没有一处再从 [`Self::game_version`] 去拼路径。
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
    /// 这个实例累计跑了多少秒。
    ///
    /// 只累计**窗口真的开出来过**的那些次：起不来的那几次每次也占十几秒，
    /// 记进去就成了「玩了半小时」，而那半小时里一次游戏都没进过。
    ///
    /// 存秒不存分：一次会话不足一分钟的很常见（进去看一眼就退），按分钟累加
    /// 每次都归零，一天下来还是零。
    #[serde(default, skip_serializing_if = "is_zero")]
    pub play_seconds: u64,
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

impl InstanceProfile {
    /// 主加载器那一层。
    ///
    /// 「最外面那一个**不可叠加**的加载器」：层表按依赖顺序排，越靠后越接近
    /// 成品，所以从后往前找；但附加层（LiteLoader）要跳过——它排在最后，却
    /// 不是这个实例的主加载器，认错了的话 Java 区间、崩溃守卫、模组筛选全都
    /// 会按错的那个来。
    pub fn loader_component(&self) -> Option<&Component> {
        self.components
            .iter()
            .rev()
            .find(|component| component.kind != LoaderKind::Vanilla && !component.kind.stackable())
    }

    /// 叠在主加载器之上的那些层。
    pub fn addon_components(&self) -> impl Iterator<Item = &Component> {
        self.components
            .iter()
            .filter(|component| component.kind.stackable())
    }

    /// 把算得出来的那些字段算一遍。**写盘前必须过这一道。**
    pub fn normalized(mut self) -> Self {
        self.loader = self
            .loader_component()
            .map(|component| component.kind)
            .unwrap_or(LoaderKind::Vanilla);
        self
    }

    /// 从旧形状迁移过来。**只在读这一侧做**，写出去的永远是新形状，所以每一
    /// 条都不会反复发生，磁盘上也不必留「已迁移」的标记。
    ///
    /// 两条：
    ///
    /// 1. 一个实例一个加载器的年代——`loaderProfile` 那一个变成层表里的一层。
    ///    只认「层表是空的、而旧字段有值」这一种情况。
    /// 2. 游戏本体不在层表里的年代（`schemaVersion` 1）——补上第 0 层。它那份
    ///    描述叫什么，那时候只有一个来源可用：版本号。这对我们自己建的实例是
    ///    对的（那份 JSON 正是按版本号下下来的）。
    ///
    ///    **已经添加过的外部实例里，有加载器层的一个字也不补**：那一层写着真
    ///    正的目录名，再补一个按版本号猜的进去，只会让一份不属于这个实例的描
    ///    述被合进来。
    ///
    ///    没有加载器层的那些（原版目录、OptiFine 目录）仍然补——不是因为猜得
    ///    准，而是因为那个猜法正是它们此前一直在用的：目录名恰好等于版本号时
    ///    它们能启动，不等时本来就不能。不补的话，前一种会跟着一起坏掉。后一
    ///    种只能重新添加一次，那时目录名才会被真正记下来。
    pub(crate) fn migrate(raw: &mut serde_json::Value) {
        let Some(object) = raw.as_object_mut() else {
            return;
        };
        let components = |object: &serde_json::Map<String, serde_json::Value>| {
            object
                .get("components")
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default()
        };
        if components(object).is_empty()
            && let Some(legacy) = object
                .remove("loaderProfile")
                .filter(|value| !value.is_null())
        {
            object.insert(
                "components".to_owned(),
                serde_json::Value::Array(vec![legacy]),
            );
        }

        let schema = object
            .get("schemaVersion")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(1);
        let attached_with_layers = object.get("external").is_some_and(|value| !value.is_null())
            && !components(object).is_empty();
        if schema < 2 && !attached_with_layers {
            let game_version = object
                .get("gameVersion")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let mut stack = vec![serde_json::json!({
                "kind": "vanilla",
                "version": game_version,
                "versionId": game_version,
            })];
            stack.extend(components(object));
            object.insert("components".to_owned(), serde_json::Value::Array(stack));
        }
        object.insert(
            "schemaVersion".to_owned(),
            serde_json::json!(INSTANCE_SCHEMA_VERSION),
        );
    }

    pub fn vanilla(
        id: InstanceId,
        name: impl Into<String>,
        game_version: impl Into<String>,
    ) -> Self {
        let cover_identity = id.as_str().to_owned();
        let game_version = game_version.into();
        Self {
            schema_version: INSTANCE_SCHEMA_VERSION,
            id,
            name: name.into(),
            loader: LoaderKind::Vanilla,
            // 游戏本体那一层。我们自己建的实例里它那份描述就叫版本号——因为
            // 那份 JSON 正是按这个名字下下来的，不是因为两者天然相同。
            components: vec![Component {
                kind: LoaderKind::Vanilla,
                version: game_version.clone(),
                version_id: game_version.clone(),
                jar_mods: Vec::new(),
            }],
            game_version,
            account_id: None,
            cover: CoverSeed {
                identity: cover_identity,
                growth: 0,
            },
            settings: InstanceSettings::default(),
            external: None,
            last_played: None,
            play_seconds: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 旧实例读进来就是新形状：那一个加载器变成层表里的一层，游戏本体补成第
    /// 0 层。
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
        // 游戏本体 + 加载器。第 0 层那份描述叫版本号——我们自己建的实例正是
        // 按这个名字把它下下来的。
        assert_eq!(profile.schema_version, INSTANCE_SCHEMA_VERSION);
        assert_eq!(
            crate::launch::version::layers(&profile),
            vec!["1.20.1", "fabric-loader-0.16.5-1.20.1"]
        );
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

        // 原版实例没有那个字段，层表里就只有游戏本体那一层，而且它不是加载器。
        let mut vanilla = serde_json::json!({
            "schemaVersion": 1, "id": "bare", "name": "Bare", "gameVersion": "1.21.1",
            "cover": { "identity": "bare", "growth": 0 }
        });
        InstanceProfile::migrate(&mut vanilla);
        let bare: InstanceProfile = serde_json::from_value(vanilla).expect("read");
        assert_eq!(crate::launch::version::layers(&bare), vec!["1.21.1"]);
        assert!(bare.loader_component().is_none());

        // 外部实例一个字也不补：它那份描述叫什么，只有添加它的时候知道，而
        // 版本号是从 client jar 里认出来的标签。猜一个填进去，等于把这个
        // bug 写进磁盘。
        let mut attached = serde_json::json!({
            "schemaVersion": 1, "id": "theirs", "name": "1.16.5-Fabric 0.14.11",
            "gameVersion": "1.16.5",
            "loader": "fabric",
            "components": [{
                "kind": "fabric", "version": "0.14.11",
                "versionId": "1.16.5-Fabric 0.14.11"
            }],
            "cover": { "identity": "theirs", "growth": 0 },
            "external": { "root": "/games/.minecraft", "isolation": "perVersion" }
        });
        InstanceProfile::migrate(&mut attached);
        let attached: InstanceProfile = serde_json::from_value(attached).expect("read");
        assert_eq!(
            crate::launch::version::layers(&attached),
            vec!["1.16.5-Fabric 0.14.11"]
        );

        // 但没有任何一层的那些（层表建立之前添加的原版目录）还是要补上：目录
        // 名等于版本号时它们一直是能启动的，不补就跟着一起坏掉。
        let mut bare_attach = serde_json::json!({
            "schemaVersion": 1, "id": "plain", "name": "1.21.1（现有目录）",
            "gameVersion": "1.21.1",
            "cover": { "identity": "plain", "growth": 0 },
            "external": { "root": "/games/.minecraft", "isolation": "shared" }
        });
        InstanceProfile::migrate(&mut bare_attach);
        let bare_attach: InstanceProfile = serde_json::from_value(bare_attach).expect("read");
        assert_eq!(crate::launch::version::layers(&bare_attach), vec!["1.21.1"]);
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

    /// 附加层排在最后，但它不是主加载器。认错了的话 Java 区间、崩溃守卫、
    /// 模组筛选全都会按错的那个来。
    #[test]
    fn a_stacked_addon_does_not_become_the_primary_loader() {
        let mut profile =
            InstanceProfile::vanilla(InstanceId::parse("both").expect("id"), "都要", "1.7.10");
        profile.components.push(Component {
            kind: LoaderKind::Forge,
            version: "10.13.4.1614".to_owned(),
            version_id: "1.7.10-Forge10.13.4.1614".to_owned(),
            jar_mods: Vec::new(),
        });
        profile.components.push(Component {
            kind: LoaderKind::LiteLoader,
            version: "1.7.10_04".to_owned(),
            version_id: "1.7.10-LiteLoader1.7.10_04".to_owned(),
            jar_mods: Vec::new(),
        });
        let profile = profile.normalized();

        assert_eq!(profile.loader, LoaderKind::Forge);
        assert_eq!(
            profile.loader_component().map(|one| one.kind),
            Some(LoaderKind::Forge)
        );
        assert_eq!(
            profile
                .addon_components()
                .map(|one| one.kind)
                .collect::<Vec<_>>(),
            vec![LoaderKind::LiteLoader]
        );
        // 但要启动的仍然是最外面那一层——主类和参数由它给。
        assert_eq!(
            crate::launch::version::layers(&profile)
                .last()
                .map(String::as_str),
            Some("1.7.10-LiteLoader1.7.10_04")
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

        assert_eq!(value["schemaVersion"], 2);
        assert_eq!(value["gameVersion"], "1.21.1");
        assert_eq!(value["loader"], "vanilla");
        assert_eq!(value["cover"]["identity"], "cinder-valley");
        // 游戏本体那一层，连同它那份描述在磁盘上的名字。
        assert_eq!(value["components"][0]["kind"], "vanilla");
        assert_eq!(value["components"][0]["versionId"], "1.21.1");
    }

    #[test]
    fn instance_ids_are_safe_as_directory_names() {
        assert!(InstanceId::parse("moss_archive-2").is_ok());
        assert!(InstanceId::parse("../other").is_err());
        assert!(InstanceId::parse("contains spaces").is_err());
    }
}
