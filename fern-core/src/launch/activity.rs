//! 游戏这会儿在干什么：主菜单、哪个存档、哪个服务器。
//!
//! 两条通道，各说各能说的那一半：
//!
//! - **quickPlay 日志**（1.20 及以后）。命令行里给了 `--quickPlayPath`，游戏
//!   进世界或进服务器时就往那个文件写一条 JSON，字段是 `type` `id` `name`
//!   `lastPlayedTime` `gamemode`。**身份**是它给的——存档目录名、服务器地址、
//!   玩家自己起的服务器名，一个字都不用从日志里猜。它只记「进」，不记
//!   「回菜单」，也不记老版本。
//! - **stdout 的日志行**。所有版本都有，能给出**时刻**：集成服务器起停就是
//!   进出单人世界，`Connecting to` 那行就是进多人。代价是它是猜的——模组会
//!   改日志格式，服务器和玩家说的话也原样进日志。
//!
//! 合起来：日志给时刻，quickPlay 给身份，先到的先说，后到的补充。两边都没
//! 说过话就是 `Place::Unknown`——**宁可界面上空着，也不要一个编出来的位置**。
//!
//! 已知缺口：从服务器退回菜单只能靠 `Stopping worker threads`，那一行的来源
//! 是渲染线程的区块构建器，资源包重载也可能打出来。宁可漏报一次「回菜单」，
//! 也不要在人还在服务器里的时候说他在主菜单。

use std::{
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedSender;

use crate::LauncherEvent;

/// 人在哪一屏。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Place {
    /// 还没有任何一条通道说过话。
    #[default]
    Unknown,
    /// 主菜单，不在任何世界里。
    Menu,
    Singleplayer,
    Multiplayer,
    Realms,
}

/// 一次会话此刻的去向。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    pub place: Place,
    /// 存档目录名、服务器地址、或者 Realm 的 id。
    ///
    /// 多人那一路日志也给得出（`Connecting to` 那行带着主机和端口）；单人和
    /// Realms 只有 quickPlay 日志说得出来，说不出来就是 `None`。
    pub id: Option<String>,
    /// 显示名——存档名，或者服务器列表里那个自己起的名字。只有 quickPlay
    /// 日志给得出。
    pub name: Option<String>,
}

impl Activity {
    fn somewhere(place: Place) -> Self {
        Self {
            place,
            id: None,
            name: None,
        }
    }

    /// 人在世界里，不是在主菜单待着。
    fn in_world(&self) -> bool {
        matches!(
            self.place,
            Place::Singleplayer | Place::Multiplayer | Place::Realms
        )
    }
}

/// 命令行上交给游戏的那个相对路径（相对游戏目录，也就是进程的工作目录）。
///
/// 名字是我们自己的，不叫 `log.json`：每次启动前要把上一次那份删掉，而外部
/// 实例的游戏目录可能同时被官方启动器用着，删它的文件不合适。
pub(crate) const LOG_ARGUMENT: &str = "quickPlay/fern.json";

pub(crate) fn log_path(game_directory: &Path) -> PathBuf {
    game_directory.join("quickPlay").join("fern.json")
}

/// 开局先清场。
///
/// 这份文件只有一条记录，游戏每进一次世界就整个重写。上一次会话留下的那条
/// 长得和新的一模一样，清掉它，文件里剩下的东西就一定属于这一次。
pub(crate) fn reset(game_directory: &Path) {
    let path = log_path(game_directory);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // 不存在是常态：第一次启动，或者游戏版本还没有 quickPlay。
    let _ = std::fs::remove_file(&path);
}

/// 一次会话的去向，两条通道都往这里汇。
pub(crate) struct Tracker {
    instance_id: String,
    events: UnboundedSender<LauncherEvent>,
    quick_play_log: PathBuf,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    activity: Activity,
    /// 上一次读到的 quickPlay 日志原文。变了才算有新的一条——`lastPlayedTime`
    /// 精确到微秒，同一个世界进两次也是两份不同的原文。
    seen: Option<String>,
    /// 有没有任何一条通道说过话。
    ///
    /// 没说过 ≠ 没进过世界：老版本没有 quickPlay 日志，重度模组包的日志也可能
    /// 一条标志都对不上。那时候这一整套观察必须**弃权**，而不是报一个 0。
    heard: bool,
    /// 累计在世界里的时间。
    in_world: Duration,
    /// 当前这一段是什么时候进去的。`None` 表示这会儿不在世界里。
    since: Option<Instant>,
}

impl Tracker {
    pub(crate) fn new(
        instance_id: &str,
        game_directory: &Path,
        events: UnboundedSender<LauncherEvent>,
    ) -> Self {
        Self {
            instance_id: instance_id.to_owned(),
            events,
            quick_play_log: log_path(game_directory),
            state: Mutex::new(State::default()),
        }
    }

    /// 日志读线程每读到一行就喂进来。
    pub(crate) fn observe_log(&self, line: &str) {
        let Some(marker) = read_marker(line) else {
            return;
        };
        let mut state = self.state();
        let next = match marker {
            Marker::Left => Activity::somewhere(Place::Menu),
            Marker::Singleplayer => {
                // 是哪个存档要等 quickPlay 日志说。它已经说过了就别把名字
                // 洗掉——同一次进入，两条通道各说一次是常态。
                if state.activity.place == Place::Singleplayer {
                    return;
                }
                Activity::somewhere(Place::Singleplayer)
            }
            Marker::Multiplayer(address) => {
                if state.activity.place == Place::Multiplayer
                    && state.activity.id.as_deref() == Some(address.as_str())
                {
                    return;
                }
                Activity {
                    place: Place::Multiplayer,
                    id: Some(address),
                    name: None,
                }
            }
        };
        self.settle(&mut state, next);
    }

    /// 轮询线程每隔几秒调一次。
    pub(crate) fn poll_quick_play(&self) {
        // 读不到是常态：1.20 之前的版本根本不写这份文件。
        let Ok(text) = std::fs::read_to_string(&self.quick_play_log) else {
            return;
        };
        let mut state = self.state();
        if state.seen.as_deref() == Some(text.as_str()) {
            return;
        }
        state.seen = Some(text.clone());
        let Some(activity) = parse_log(&text) else {
            return;
        };
        self.settle(&mut state, activity);
    }

    /// 这一次会话，人真正待在世界里的分钟数。
    ///
    /// `None` 是**弃权**：两条通道一句话都没说过，这次会话说不出人在哪，判断
    /// 就该退回别的依据（见 `memory::history::Session::is_valid`）。报 0 会把
    /// 一次正常的游玩说成挂机。
    pub(crate) fn in_world_minutes(&self) -> Option<f64> {
        let state = self.state();
        if !state.heard {
            return None;
        }
        let mut total = state.in_world;
        // 退出游戏时人多半还在世界里，那一段没有「离开」事件来结算。
        if let Some(since) = state.since {
            total += since.elapsed();
        }
        Some(total.as_secs_f64() / 60.0)
    }

    /// 变了才说话。同一件事两条通道各报一次，界面不该收到两条。
    fn settle(&self, state: &mut State, next: Activity) {
        if state.activity == next {
            return;
        }
        // 只在状态真的变了的时候结算：没变就说明这一段还在继续。
        let now = Instant::now();
        if let Some(since) = state.since.take() {
            state.in_world += now.duration_since(since);
        }
        if next.in_world() {
            state.since = Some(now);
        }
        state.heard = true;
        state.activity = next.clone();
        super::running::mark_activity(&self.instance_id, &next);
        let _ = self.events.send(LauncherEvent::GameActivity {
            instance_id: self.instance_id.clone(),
            activity: next,
        });
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// quickPlay 日志里那一条。
///
/// 官方写的是「恰好一条记录的数组」，这里按数组读、取最后一条；顺手认一下
/// 光秃秃一个对象的写法，多几行而已。
fn parse_log(text: &str) -> Option<Activity> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let entry = match &value {
        serde_json::Value::Array(entries) => entries.last()?,
        other => other,
    };
    let place = match entry.get("type").and_then(serde_json::Value::as_str)? {
        "singleplayer" => Place::Singleplayer,
        "multiplayer" => Place::Multiplayer,
        "realms" => Place::Realms,
        // 认不出来的类型不猜。
        _ => return None,
    };
    let field = |key: &str| {
        entry
            .get(key)
            .and_then(serde_json::Value::as_str)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    };
    Some(Activity {
        place,
        id: field("id"),
        name: field("name"),
    })
}

/// 一行日志说明发生了什么。
#[derive(Debug, PartialEq, Eq)]
enum Marker {
    /// 集成服务器起来了——进单人世界，或者是别人进了自己开的局域网。
    Singleplayer,
    /// 正在连服务器。带着 `host` 或 `host:port`。
    Multiplayer(String),
    /// 世界卸了，回菜单。
    Left,
}

/// 离开世界的那几行。
///
/// `Stopping server` 是集成服务器停机，退出单人世界会打；`Stopping worker
/// threads` 是客户端把区块构建器停掉，退出任何世界都会打，也是多人那一路
/// 唯一等得到的信号。退出游戏时两行都会出现，但那之后进程就没了，多报一次
/// 「回菜单」不影响任何东西。
const LEFT: [&str; 2] = ["Stopping server", "Stopping worker threads"];

fn read_marker(line: &str) -> Option<Marker> {
    let body = body(line);
    // 聊天原样进日志。服务器的公告、别的玩家打的字里可以出现任何一个标志串，
    // 这一句是整张表的前提。
    if body.contains("[CHAT]") {
        return None;
    }
    if body.starts_with("Starting integrated minecraft server") {
        return Some(Marker::Singleplayer);
    }
    if LEFT.iter().any(|marker| body.starts_with(marker)) {
        return Some(Marker::Left);
    }
    connecting_to(body).map(Marker::Multiplayer)
}

/// `Connecting to mc.example.net, 25565`
///
/// 两道闸：末段必须是个端口号（挡掉「Connecting to 认证服务器」这类），主机
/// 不能是本机（局域网和单人的内部连接走的是 localhost，那不是多人游戏）。
fn connecting_to(body: &str) -> Option<String> {
    let target = body.strip_prefix("Connecting to ")?;
    let (host, port) = target.rsplit_once(", ")?;
    let port: u16 = port.trim().parse().ok()?;
    let host = host.trim();
    if host.is_empty()
        || host.eq_ignore_ascii_case("localhost")
        || host == "127.0.0.1"
        || host == "::1"
    {
        return None;
    }
    // 默认端口不写出来——服务器列表里存的就是光一个主机名，两边对得上。
    Some(if port == 25565 {
        host.to_owned()
    } else {
        format!("{host}:{port}")
    })
}

/// 剥掉 `[12:34:56] [Render thread/INFO]: ` 这一截，只留正文。
///
/// 认第一个 `]: `：原版到这里正好是等级那一节的末尾，Forge 多一节
/// `[minecraft/MinecraftServer]:`，也正好落在正文之前。剥不掉就原样用——
/// 下游全是 `starts_with`，多一截前缀只会少认，不会错认。
fn body(line: &str) -> &str {
    match line.find("]: ") {
        Some(at) => &line[at + 3..],
        None => line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_integrated_server_starting_means_a_singleplayer_world() {
        assert_eq!(
            read_marker(
                "[12:34:56] [Server thread/INFO]: Starting integrated minecraft server version 1.21.1"
            ),
            Some(Marker::Singleplayer)
        );
    }

    #[test]
    fn forges_extra_bracket_still_leaves_the_body_intact() {
        assert_eq!(
            read_marker(
                "[12:34:56] [Server thread/INFO] [minecraft/MinecraftServer]: Starting integrated minecraft server version 1.20.1"
            ),
            Some(Marker::Singleplayer)
        );
    }

    #[test]
    fn connecting_carries_the_address() {
        assert_eq!(
            read_marker("[12:34:56] [Render thread/INFO]: Connecting to mc.example.net, 25565"),
            Some(Marker::Multiplayer("mc.example.net".to_owned()))
        );
        assert_eq!(
            read_marker("[12:34:56] [Render thread/INFO]: Connecting to mc.example.net, 25577"),
            Some(Marker::Multiplayer("mc.example.net:25577".to_owned()))
        );
    }

    #[test]
    fn the_local_connection_is_not_multiplayer() {
        // 开局域网之后自己那一份连的是本机，那还是同一个单人世界。
        assert_eq!(
            read_marker("[12:34:56] [Render thread/INFO]: Connecting to localhost, 25565"),
            None
        );
    }

    #[test]
    fn a_connection_without_a_port_is_not_a_server() {
        assert_eq!(
            read_marker("[12:34:56] [Render thread/INFO]: Connecting to the session service"),
            None
        );
    }

    #[test]
    fn chat_cannot_move_the_player() {
        // 服务器公告和别人打的字原样进日志，里面可以出现任何一个标志串。
        for said in [
            "[12:34:56] [Render thread/INFO]: [CHAT] Connecting to evil.example.net, 25565",
            "[12:34:56] [Render thread/INFO]: [CHAT] Stopping server",
            "[12:34:56] [Render thread/INFO]: [CHAT] Starting integrated minecraft server version 1.21.1",
        ] {
            assert_eq!(read_marker(said), None, "{said}");
        }
    }

    #[test]
    fn leaving_a_world_goes_back_to_the_menu() {
        assert_eq!(
            read_marker("[12:34:56] [Server thread/INFO]: Stopping server"),
            Some(Marker::Left)
        );
        assert_eq!(
            read_marker("[12:34:56] [Render thread/INFO]: Stopping worker threads"),
            Some(Marker::Left)
        );
    }

    /// 一个跑着的会话：日志行喂进去，quickPlay 文件写出来，收事件。
    fn session(
        name: &str,
    ) -> (
        Tracker,
        PathBuf,
        tokio::sync::mpsc::UnboundedReceiver<LauncherEvent>,
    ) {
        let directory =
            std::env::temp_dir().join(format!("fern-activity-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(directory.join("quickPlay")).expect("make a game directory");
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        (
            Tracker::new("cinder-valley", &directory, sender),
            log_path(&directory),
            receiver,
        )
    }

    fn places(receiver: &mut tokio::sync::mpsc::UnboundedReceiver<LauncherEvent>) -> Vec<Activity> {
        let mut seen = Vec::new();
        while let Ok(LauncherEvent::GameActivity { activity, .. }) = receiver.try_recv() {
            seen.push(activity);
        }
        seen
    }

    #[test]
    fn the_log_gives_the_moment_and_the_quick_play_file_fills_in_the_name() {
        let (tracker, log, mut events) = session("join");

        // 连上服务器：日志先到，地址是从那一行读出来的，还没有名字。
        tracker.observe_log("[12:34:56] [Render thread/INFO]: Connecting to mc.example.net, 25565");
        // 游戏进服务器成功之后才写这份文件，名字是服务器列表里那个。
        std::fs::write(
            &log,
            r#"[{"type":"multiplayer","id":"mc.example.net","name":"朋友的服","lastPlayedTime":"2026-08-12T03:04:05.123456Z","gamemode":"survival"}]"#,
        )
        .expect("write the quick play log");
        tracker.poll_quick_play();
        // 文件没再变，第二次轮询不该再说一遍。
        tracker.poll_quick_play();

        assert_eq!(
            places(&mut events),
            [
                Activity {
                    place: Place::Multiplayer,
                    id: Some("mc.example.net".to_owned()),
                    name: None,
                },
                Activity {
                    place: Place::Multiplayer,
                    id: Some("mc.example.net".to_owned()),
                    name: Some("朋友的服".to_owned()),
                },
            ]
        );

        // 退回菜单。quickPlay 日志对这件事一个字都不说，只有日志行说得出。
        tracker.observe_log("[12:40:00] [Render thread/INFO]: Stopping worker threads");
        tracker.poll_quick_play();
        assert_eq!(places(&mut events), [Activity::somewhere(Place::Menu)]);
    }

    #[test]
    fn only_time_inside_a_world_counts_as_playing() {
        let (tracker, _log, _events) = session("played");

        // 一条通道都没说过话：这次会话说不出人在哪，弃权而不是报零——报零会
        // 把一次正常的游玩（老版本、认不出日志的模组包）说成挂机。
        assert_eq!(tracker.in_world_minutes(), None);

        // 说过话了，而且说的是「在主菜单」。开着游戏挂在这里多久都不算玩。
        tracker.observe_log("[12:00:00] [Render thread/INFO]: Stopping worker threads");
        assert_eq!(tracker.in_world_minutes(), Some(0.0));

        // 进服务器，再退回菜单：中间那一段算进去，退出来之后不再增长。
        tracker.observe_log("[12:00:01] [Render thread/INFO]: Connecting to mc.example.net, 25565");
        tracker.observe_log("[12:30:00] [Render thread/INFO]: Stopping worker threads");
        let played = tracker.in_world_minutes().expect("知道人在哪");
        assert!(played > 0.0);
        assert_eq!(tracker.in_world_minutes(), Some(played));
    }

    #[test]
    fn the_singleplayer_marker_does_not_wash_away_the_world_name() {
        let (tracker, log, mut events) = session("world");

        std::fs::write(
            &log,
            r#"[{"type":"singleplayer","id":"New World","name":"新的世界","gamemode":"survival"}]"#,
        )
        .expect("write the quick play log");
        tracker.poll_quick_play();
        // 两条通道说的是同一次进入，谁后到都不该把对方知道的东西抹掉。
        tracker.observe_log(
            "[12:34:56] [Server thread/INFO]: Starting integrated minecraft server version 1.21.1",
        );

        assert_eq!(
            places(&mut events),
            [Activity {
                place: Place::Singleplayer,
                id: Some("New World".to_owned()),
                name: Some("新的世界".to_owned()),
            }]
        );
    }

    #[test]
    fn the_quick_play_log_gives_the_identity() {
        let text = r#"[{"type":"multiplayer","id":"mc.example.net","name":"朋友的服","lastPlayedTime":"2026-08-12T03:04:05.123456Z","gamemode":"survival"}]"#;
        assert_eq!(
            parse_log(text),
            Some(Activity {
                place: Place::Multiplayer,
                id: Some("mc.example.net".to_owned()),
                name: Some("朋友的服".to_owned()),
            })
        );
    }

    #[test]
    fn a_singleplayer_entry_carries_the_folder_and_the_display_name() {
        let text =
            r#"[{"type":"singleplayer","id":"New World","name":"新的世界","gamemode":"creative"}]"#;
        assert_eq!(
            parse_log(text),
            Some(Activity {
                place: Place::Singleplayer,
                id: Some("New World".to_owned()),
                name: Some("新的世界".to_owned()),
            })
        );
    }

    #[test]
    fn a_log_we_cannot_read_says_nothing() {
        // 半截文件、认不出的类型、根本不是 JSON——一律当没看见，绝不猜。
        assert_eq!(parse_log("[{\"type\":\"multiplayer\","), None);
        assert_eq!(parse_log(r#"[{"type":"holodeck","id":"x"}]"#), None);
        assert_eq!(parse_log("[]"), None);
    }

    #[test]
    fn missing_fields_leave_holes_rather_than_guesses() {
        assert_eq!(
            parse_log(r#"[{"type":"realms","id":"1234","name":""}]"#),
            Some(Activity {
                place: Place::Realms,
                id: Some("1234".to_owned()),
                name: None,
            })
        );
    }

    #[test]
    fn the_interface_sees_a_place_and_two_optional_strings() {
        let value = serde_json::to_value(Activity {
            place: Place::Singleplayer,
            id: Some("New World".to_owned()),
            name: None,
        })
        .expect("serialize activity");
        assert_eq!(value["place"], "singleplayer");
        assert_eq!(value["id"], "New World");
        assert!(value["name"].is_null());
        assert_eq!(
            serde_json::to_value(Activity::default()).expect("serialize the default")["place"],
            "unknown"
        );
    }
}
