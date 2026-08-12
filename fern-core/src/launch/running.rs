//! 跑着的游戏。
//!
//! 进程一旦拉起来就没人再管它了，这是上一版的样子：`launch_instance` 把
//! `Child` 交给一个等待线程，界面手上只剩一个「有没有游戏在跑」的布尔。三件
//! 事因此做不到——
//!
//! - **停不掉。** 游戏卡死只能去任务管理器。
//! - **不知道是谁在跑。** 一个布尔说不出跑的是哪个实例，也就撑不起多开。
//! - **从点下启动到窗口出现之间是一段空白。** 那一段可以有十几秒（我们等的是
//!   日志里的窗口标志，等不到还有十五秒兜底），期间按钮显示的是「启动」，
//!   像是刚才那一下没生效。
//!
//! 所以这里存的是**进程**，不是「状态」：谁在跑、pid 多少、什么时候起的、窗口
//! 出来没有。停止就是对着表里那个 `Child` 调 kill。
//!
//! **同一份游戏目录不允许跑两个进程**，判据是目录而不是实例 id：两个外部实例
//! 可以指着同一个 `.minecraft`，那时候两份进程写同一批存档，后果和把一个实例
//! 开两遍完全一样——存档互相覆盖，而且是静默的。
//!
//! 等待用轮询而不是 `Child::wait`。`wait` 要独占 `&mut Child`，握着它就没法再
//! 去 kill 同一个进程；两百毫秒一次的 `try_wait` 对一局游戏来说什么都不是。

use std::{
    path::{Path, PathBuf},
    process::Child,
    sync::{Arc, Mutex, MutexGuard, OnceLock},
    time::Duration,
};

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

use super::activity::Activity;

/// 轮询进程是否结束的间隔。
const POLL: Duration = Duration::from_millis(200);

/// 界面看到的一条「正在跑」。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunningGame {
    pub instance_id: String,
    pub process_id: u32,
    /// Unix 秒。界面上那个「已运行 12 分钟」由它算。
    pub started_at: u64,
    /// 游戏窗口已经开出来了。false 是「进程起来了，还在加载」。
    pub ready: bool,
    /// 这会儿在哪一屏（见 `launch::activity`）。事件是变化时才发的，晚订阅
    /// 的人从这里补上当前值。
    pub activity: Activity,
}

struct Session {
    instance_id: String,
    game_directory: PathBuf,
    process_id: u32,
    started_at: u64,
    ready: bool,
    activity: Activity,
    child: Arc<Mutex<Child>>,
}

fn sessions() -> MutexGuard<'static, Vec<Session>> {
    static SESSIONS: OnceLock<Mutex<Vec<Session>>> = OnceLock::new();
    SESSIONS
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 现在有哪些游戏在跑。
pub fn list() -> Vec<RunningGame> {
    sessions()
        .iter()
        .map(|session| RunningGame {
            instance_id: session.instance_id.clone(),
            process_id: session.process_id,
            started_at: session.started_at,
            ready: session.ready,
            activity: session.activity.clone(),
        })
        .collect()
}

/// 这个游戏目录已经被谁占着了。
pub fn occupant(game_directory: &Path) -> Option<String> {
    sessions()
        .iter()
        .find(|session| session.game_directory == game_directory)
        .map(|session| session.instance_id.clone())
}

/// 强行结束一个游戏。
///
/// 是 kill 不是「关闭」：标准库只给得出这一种，而这个按钮存在的理由本来就是
/// 游戏已经不响应了。**没存的进度会丢**，界面上要照这个说。
pub fn stop(instance_id: &str) -> Result<()> {
    let child = {
        let sessions = sessions();
        sessions
            .iter()
            .find(|session| session.instance_id == instance_id)
            .map(|session| session.child.clone())
            .ok_or_else(|| anyhow!("这个实例没有在运行"))?
    };
    // 锁的是那一个进程，不是整张表：kill 期间别的实例照常能启动、能查询。
    let mut child = child
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    child.kill()?;
    // 收尸在等待线程那边，这里只管发信号。
    Ok(())
}

pub(crate) fn register(
    instance_id: &str,
    game_directory: &Path,
    process_id: u32,
    started_at: u64,
    child: Arc<Mutex<Child>>,
) {
    sessions().push(Session {
        instance_id: instance_id.to_owned(),
        game_directory: game_directory.to_path_buf(),
        process_id,
        started_at,
        ready: false,
        activity: Activity::default(),
        child,
    });
}

/// 窗口开出来了。
pub(crate) fn mark_ready(instance_id: &str) {
    if let Some(session) = sessions()
        .iter_mut()
        .find(|session| session.instance_id == instance_id)
    {
        session.ready = true;
    }
}

/// 人换地方了。
pub(crate) fn mark_activity(instance_id: &str, activity: &Activity) {
    if let Some(session) = sessions()
        .iter_mut()
        .find(|session| session.instance_id == instance_id)
    {
        session.activity = activity.clone();
    }
}

pub(crate) fn unregister(instance_id: &str) {
    sessions().retain(|session| session.instance_id != instance_id);
}

/// 等它结束，期间不占着 `Child` 不放。
pub(crate) fn wait(child: &Arc<Mutex<Child>>) -> Result<Option<i32>, std::io::Error> {
    loop {
        {
            let mut guard = child
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match guard.try_wait() {
                Ok(Some(status)) => return Ok(status.code()),
                Ok(None) => {}
                Err(error) => return Err(error),
            }
        }
        std::thread::sleep(POLL);
    }
}

/// 给别的模块的测试用。
///
/// 「游戏跑着的时候不许拍快照」这条规则的测试需要一个「正在跑」的状态，而它
/// 真正依赖的只有这张表里有没有那一条记录——不需要真的有个进程。
#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    pub(crate) struct Occupied(String);

    impl Drop for Occupied {
        fn drop(&mut self) {
            unregister(&self.0);
        }
    }

    /// 假装某个实例正占着这个游戏目录，直到返回值被丢弃。
    pub(crate) fn occupy(instance_id: &str, game_directory: &Path) -> Occupied {
        sessions().push(Session {
            instance_id: instance_id.to_owned(),
            game_directory: game_directory.to_path_buf(),
            process_id: 0,
            started_at: 0,
            ready: false,
            activity: Activity::default(),
            child: Arc::new(Mutex::new(
                std::process::Command::new(if cfg!(windows) { "cmd" } else { "true" })
                    .args(if cfg!(windows) {
                        vec!["/C", "exit"]
                    } else {
                        vec![]
                    })
                    .spawn()
                    .expect("spawn a placeholder process"),
            )),
        });
        Occupied(instance_id.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 起一个一定会自己结束的进程，用来占位。
    fn sleeper(seconds: &str) -> Child {
        std::process::Command::new(if cfg!(windows) { "cmd" } else { "sleep" })
            .args(if cfg!(windows) {
                vec!["/C", "timeout", seconds]
            } else {
                vec![seconds]
            })
            .spawn()
            .expect("spawn a placeholder process")
    }

    #[test]
    fn a_running_game_can_be_found_and_stopped() {
        let directory = std::env::temp_dir().join("fern-running-test");
        let child = Arc::new(Mutex::new(sleeper("30")));
        let pid = child.lock().expect("lock").id();
        register("stop-me", &directory, pid, 0, child.clone());

        assert!(list().iter().any(|game| game.instance_id == "stop-me"));
        // 同一个游戏目录已经被占着，这是拒绝第二次启动的依据。
        assert_eq!(occupant(&directory).as_deref(), Some("stop-me"));
        assert!(!list()[0].ready);
        mark_ready("stop-me");
        assert!(
            list()
                .iter()
                .find(|g| g.instance_id == "stop-me")
                .expect("still there")
                .ready
        );

        stop("stop-me").expect("stop");
        // kill 之后等待线程收得到尸，不会一直挂着。
        wait(&child).expect("wait");
        unregister("stop-me");
        assert!(occupant(&directory).is_none());
        assert!(stop("stop-me").is_err());
    }
}
