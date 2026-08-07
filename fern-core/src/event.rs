use serde::{Deserialize, Serialize};

use crate::job::JobEvent;
use crate::launch::crash::CrashReport;

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
    /// 一件耗时的事的进展。带 id，所以同时跑好几件也分得开。
    Job(JobEvent),
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
    /// 游戏跑着时的堆压力，几秒一条。
    ///
    /// 数据来自我们自己注入的 GC 日志，和自适应分配读的是同一条流——所以这条
    /// 事件是免费的。读不到就不发：岛上没有那条线，好过一条编出来的线。
    GameMemory {
        instance_id: String,
        /// 最近一次回收之后的堆水位，MB。
        used_mb: u32,
        /// 这次会话到目前为止的峰值，MB。
        peak_mb: u32,
        /// 这次给了多少堆，MB。分母。
        xmx_mb: u32,
    },
    /// 非正常退出。和 `GameExited` 分开发：正常关掉游戏不该在界面上留下任何
    /// 痕迹，崩了才需要说话。
    GameCrashed(CrashReport),
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
