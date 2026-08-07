//! 启动器设置。
//!
//! 整份设置就是数据目录里的一个 `settings.json`。放在这里而不是 webview 的
//! localStorage：那是浏览器的缓存，清一次数据就没了，也不在用户能看见、能
//! 备份、能贴给别人的地方。文档里那条「一套配置就是几 KB 的文件，可分享、
//! 可回滚、可复现」要成立，它就得是一个真的文件。
//!
//! 分工：`appearance` 是不透明的 JSON，界面自己定义自己的长相——加一个圆角
//! 档位不该改 Rust。剩下的字段核心自己要用（下载源决定镜像顺序，玩家名进
//! 启动参数），所以是有类型的。
//!
//! 进程内缓存一份，读设置不该每次都打一次盘；写的时候一起更新缓存，所以
//! `source_order()` 这种在下载路径上被调用的函数拿到的永远是最新的选择。

use std::{
    fs,
    sync::{Arc, RwLock},
};

use anyhow::{Context, Result};
use fern_download::{BmclapiSource, DownloadSource, OfficialSource};
use serde::{Deserialize, Serialize};

use crate::DataPaths;

/// 先试哪个下载源。没有「自动」——文件里写的就是实际会发生的事，
/// 区域推荐是向导第一次替用户按下的那一下，不是一个运行时才解析的状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourcePreference {
    #[default]
    Official,
    Bmclapi,
}

/// 单账户时代留下的那一段。
///
/// 账户已经搬去 `accounts.json`（见 accounts.rs），这里只剩下**读**——第一次
/// 找不到名册时靠它把老用户的登录搬过去。字段留着是因为删了就迁不动了；
/// 没有任何地方再写它。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AccountSettings {
    pub kind: crate::accounts::AccountKind,
    pub player_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DownloadSettings {
    pub source: SourcePreference,
}

/// 所有实例的起点。
///
/// 实例设置回答「这一个要不要特别一点」，这里回答「一般情况下是什么样」。
/// 没有这一层的话，每建一个实例都要把同样的选择再做一遍，而人只会做一次，
/// 之后的实例全都带着一份自己没选过的默认值。
///
/// 只放**没有自动算法**的东西，加上那一条自动算法唯一需要人来定的量。
/// 内存不是「默认多少 MB」——那会和实例里的「自动」打架，两个都在说
/// 「我不管你决定」，答案却不一样。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GameDefaults {
    /// 这台机器上最多把多少内存交给游戏，MB。`None` 是物理内存的一半。
    ///
    /// 这是自动分配那套算法里唯一一个**只有用户知道答案**的量：机器上还跑着
    /// 什么，只有他清楚。其余的参数（基线取四分之一、大整合包抬到 8 G）是我们
    /// 的判断，摆出来只会变成一排没人敢动的开关。
    ///
    /// 它同时夹住自动算出来的值和实例里手填的值——一个数字一个意思。想给某个
    /// 实例更多，就是在决定多分一点机器给游戏，该抬的是这条线。
    pub memory_ceiling_mb: Option<u32>,
    /// 不填就是 G1。
    pub garbage_collector: Option<crate::GarbageCollector>,
    /// 实例没指定时的游戏窗口尺寸。
    pub resolution: Option<crate::Resolution>,
    /// 额外 JVM 参数，原样一行，按空白切开。
    ///
    /// 不做引号解析：真正的 shell 引号规则是一大片表面积，而这个框在实践中
    /// 装的是 `-XX:+Foo -Dbar=1`。带空格的值请写进实例设置以外的地方。
    pub jvm_arguments: String,
}

/// Java 相关的设置。
///
/// 只有一项，而且是「我们扫不到的地方」——自动下载不做开关：文档里这一层
/// 对用户是隐形的，一个能关掉它的开关等于给用户一个把游戏弄坏的按钮。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct JavaSettings {
    /// 用户手动登记的安装位置。扫描路径的并集之外的那些。
    pub extra_paths: Vec<std::path::PathBuf>,
}

/// 一个实例最终生效的那份。
///
/// 三层：实例说了算 → 全局默认 → 内置默认。求值只在这里做一次，`launch` 和
/// 设置界面读的是同一个结果——两边各算各的，界面上写着 G1 实际跑着 ZGC 这种
/// 事就是这么来的。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveSettings {
    /// 手动指定的堆。`None` 表示按自动算。
    pub max_memory_mb: Option<u32>,
    /// 交给游戏的内存上限，MB。已经算进物理内存的兜底。
    pub memory_ceiling_mb: u32,
    pub garbage_collector: crate::GarbageCollector,
    pub resolution: Option<crate::Resolution>,
    pub process_priority: crate::ProcessPriority,
    pub jvm_arguments: Vec<String>,
}

pub fn effective(
    instance: &crate::InstanceSettings,
    defaults: &GameDefaults,
    physical_bytes: Option<u64>,
) -> EffectiveSettings {
    EffectiveSettings {
        max_memory_mb: instance.max_memory_mb,
        memory_ceiling_mb: crate::heap_ceiling(physical_bytes, defaults.memory_ceiling_mb),
        garbage_collector: instance
            .garbage_collector
            .or(defaults.garbage_collector)
            .unwrap_or_default(),
        resolution: instance.resolution.or(defaults.resolution),
        process_priority: instance.process_priority.unwrap_or_default(),
        jvm_arguments: defaults
            .jvm_arguments
            .split_whitespace()
            .map(str::to_owned)
            .collect(),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// 界面外观。核心只负责存取，不解释里面有什么。
    pub appearance: serde_json::Value,
    pub account: AccountSettings,
    pub download: DownloadSettings,
    /// 所有实例的起点。实例设置只写它要偏离的那几项。
    pub game: GameDefaults,
    pub java: JavaSettings,
    /// 首次启动向导走完过一次。
    pub setup_done: bool,
    /// 游戏窗口开出来之后把启动器收起来（文档 §5.4 末句）。
    ///
    /// 这是行为不是长相，所以没有塞进不透明的 appearance 段。核心不执行它——
    /// 最小化是窗口的事，只有界面做得到——但它该和其余设置一样是有类型的。
    pub minimize_on_launch: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            appearance: serde_json::Value::Object(serde_json::Map::new()),
            account: AccountSettings::default(),
            download: DownloadSettings::default(),
            game: GameDefaults::default(),
            java: JavaSettings::default(),
            setup_done: false,
            minimize_on_launch: false,
        }
    }
}

static CACHE: RwLock<Option<Settings>> = RwLock::new(None);

/// 从磁盘读一份。文件不存在是正常的第一次启动；读坏了也不该让启动器打不开，
/// 退回默认值，下一次保存会把它写回成能读的样子。
pub fn load(paths: &DataPaths) -> Settings {
    let settings = fs::read(paths.settings_path())
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Settings>(&bytes).ok())
        .unwrap_or_default();
    *CACHE.write().expect("settings cache poisoned") = Some(settings.clone());
    settings
}

pub fn save(paths: &DataPaths, settings: &Settings) -> Result<()> {
    fs::create_dir_all(&paths.root).context("create data root")?;
    let bytes = serde_json::to_vec_pretty(settings).context("serialize settings")?;
    fs::write(paths.settings_path(), bytes).context("write settings")?;
    *CACHE.write().expect("settings cache poisoned") = Some(settings.clone());
    Ok(())
}

/// 当前设置。缓存是空的就顺手读一次——下载路径上的调用方不该被迫先初始化。
pub fn current() -> Settings {
    if let Some(settings) = CACHE.read().expect("settings cache poisoned").clone() {
        return settings;
    }
    match DataPaths::for_current_user() {
        Ok(paths) => load(&paths),
        Err(_) => Settings::default(),
    }
}

/// 按偏好排好的下载源。两个源始终都在列表里，偏好只决定顺序：选错了最坏是
/// 慢一点，不会下不动。
pub fn source_order() -> Vec<Arc<dyn DownloadSource>> {
    match current().download.source {
        SourcePreference::Bmclapi => vec![Arc::new(BmclapiSource), Arc::new(OfficialSource)],
        SourcePreference::Official => vec![Arc::new(OfficialSource), Arc::new(BmclapiSource)],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn round_trips_through_disk() {
        let root = env::temp_dir().join(format!("fern-settings-test-{}", std::process::id()));
        let paths = DataPaths::new(&root);

        let mut settings = Settings::default();
        settings.account.player_name = "Steve".to_owned();
        settings.download.source = SourcePreference::Bmclapi;
        settings.setup_done = true;
        settings.minimize_on_launch = true;
        settings.appearance = serde_json::json!({ "density": "compact" });
        save(&paths, &settings).expect("save settings");

        let read = load(&paths);
        assert_eq!(read, settings);
        assert_eq!(read.appearance["density"], "compact");

        fs::remove_dir_all(root).expect("remove test data root");
    }

    #[test]
    fn an_instance_only_writes_down_what_it_wants_to_differ_on() {
        use crate::{GarbageCollector, InstanceSettings, Resolution};

        let defaults = GameDefaults {
            memory_ceiling_mb: Some(6144),
            garbage_collector: Some(GarbageCollector::Z),
            resolution: Some(Resolution {
                width: 1600,
                height: 900,
            }),
            jvm_arguments: "-Dfoo=1  -XX:+Bar".to_owned(),
        };
        let physical = Some(32 * 1024 * 1024 * 1024u64);

        // 什么都没说的实例，整份跟着全局。
        let plain = crate::settings::effective(&InstanceSettings::default(), &defaults, physical);
        assert_eq!(plain.garbage_collector, GarbageCollector::Z);
        assert_eq!(plain.resolution.map(|r| r.width), Some(1600));
        assert_eq!(plain.memory_ceiling_mb, 6144);
        // 参数按空白切开，连续空白不该切出空串。
        assert_eq!(plain.jvm_arguments, vec!["-Dfoo=1", "-XX:+Bar"]);

        // 说了的那几项归实例。
        let special = crate::settings::effective(
            &InstanceSettings {
                garbage_collector: Some(GarbageCollector::G1),
                ..InstanceSettings::default()
            },
            &defaults,
            physical,
        );
        assert_eq!(special.garbage_collector, GarbageCollector::G1);
        // 没说的仍然跟全局，不会因为说了一项就整份脱钩。
        assert_eq!(special.resolution.map(|r| r.width), Some(1600));

        // 全局也没说时才落到内置默认。
        let bare = crate::settings::effective(
            &InstanceSettings::default(),
            &GameDefaults::default(),
            physical,
        );
        assert_eq!(bare.garbage_collector, GarbageCollector::G1);
        assert_eq!(bare.resolution, None);
        assert_eq!(bare.memory_ceiling_mb, 16384);
        assert!(bare.jvm_arguments.is_empty());
    }

    #[test]
    fn unreadable_file_falls_back_to_defaults() {
        let root = env::temp_dir().join(format!("fern-settings-bad-{}", std::process::id()));
        let paths = DataPaths::new(&root);
        fs::create_dir_all(&root).expect("create root");
        fs::write(paths.settings_path(), b"{ not json").expect("write junk");

        assert_eq!(load(&paths), Settings::default());

        fs::remove_dir_all(root).expect("remove test data root");
    }
}
