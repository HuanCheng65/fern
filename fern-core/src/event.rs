use serde::{Deserialize, Serialize};

use fern_download::DownloadEvent;

use crate::crash::CrashReport;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchStage {
    ResolvingVersion,
    CheckingFiles,
    PreparingJava,
    BuildingCommand,
    StartingProcess,
    Running,
    Exited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

/// Event envelope emitted by the core and forwarded unchanged by Tauri.
///
/// 一条规则：类型标签用 snake_case（`launch_stage`、`preparing_java`），数据
/// 字段用 camelCase（`instanceId`）。前者是判别用的常量，后者是 JS 里要当
/// 属性读的东西，两边各自随各自的习惯，比全局统一成一种更少出错。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "payload",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum LauncherEvent {
    Download(DownloadEvent),
    LaunchStage {
        instance_id: String,
        stage: LaunchStage,
    },
    GameLog {
        instance_id: String,
        level: LogLevel,
        message: String,
    },
    GameExited {
        instance_id: String,
        exit_code: Option<i32>,
    },
    /// 非正常退出。和 `GameExited` 分开发：正常关掉游戏不该在界面上留下任何
    /// 痕迹，崩了才需要说话。
    GameCrashed(CrashReport),
}

/// 把下载事件转成启动器事件的桥。
///
/// 下载器只认得 [`DownloadEvent`]，而界面只该订阅一条「启动器在干什么」的
/// 流。桥在这里搭一次，比让下载器去认识启动器的事件模型好。
pub fn download_bridge(
    events: &tokio::sync::mpsc::UnboundedSender<LauncherEvent>,
) -> tokio::sync::mpsc::UnboundedSender<DownloadEvent> {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = events.clone();
    tokio::spawn(async move {
        while let Some(event) = receiver.recv().await {
            if events.send(LauncherEvent::Download(event)).is_err() {
                break;
            }
        }
    });
    sender
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_are_tagged_for_frontend_dispatch() {
        let event = LauncherEvent::LaunchStage {
            instance_id: "cinder-valley".to_owned(),
            stage: LaunchStage::CheckingFiles,
        };
        let value = serde_json::to_value(event).expect("serialize launcher event");

        assert_eq!(value["type"], "launch_stage");
        assert_eq!(value["payload"]["stage"], "checking_files");
    }
}
