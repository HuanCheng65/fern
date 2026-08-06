use serde::{Deserialize, Serialize};

use fern_download::DownloadEvent;

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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
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
