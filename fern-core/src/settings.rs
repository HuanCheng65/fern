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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccountKind {
    #[default]
    Offline,
    Microsoft,
    Authlib,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AccountSettings {
    pub kind: AccountKind,
    pub player_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DownloadSettings {
    pub source: SourcePreference,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// 界面外观。核心只负责存取，不解释里面有什么。
    pub appearance: serde_json::Value,
    pub account: AccountSettings,
    pub download: DownloadSettings,
    /// 首次启动向导走完过一次。
    pub setup_done: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            appearance: serde_json::Value::Object(serde_json::Map::new()),
            account: AccountSettings::default(),
            download: DownloadSettings::default(),
            setup_done: false,
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
        settings.appearance = serde_json::json!({ "density": "compact" });
        save(&paths, &settings).expect("save settings");

        let read = load(&paths);
        assert_eq!(read, settings);
        assert_eq!(read.appearance["density"], "compact");

        fs::remove_dir_all(root).expect("remove test data root");
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
