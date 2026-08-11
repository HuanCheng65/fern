use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use pearl_core::invite::Invite;
use pearl_core::probe::{self, ProbeOptions};
use pearl_core::session::{
    self, HostOptions, JoinOptions, MinecraftSource, SessionEvent, SessionOptions,
};
use pearl_core::settings::{self, Settings};
use pearl_core::sidecar::session_event_json;
use tauri::{Emitter, Manager, State};
use tokio::sync::Mutex;

const PEARL_SIGNAL: &str = "https://pearl.huanchengfly.top";
const PEARL_STOP_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Default)]
struct RunningPearl {
    task: Option<(tauri::async_runtime::JoinHandle<()>, session::StopHandle)>,
    share: Option<tokio::sync::watch::Sender<Option<u16>>>,
}

impl RunningPearl {
    async fn stop(&mut self) {
        self.share = None;
        if let Some((mut task, stop)) = self.task.take() {
            stop.stop();
            if tokio::time::timeout(PEARL_STOP_TIMEOUT, &mut task)
                .await
                .is_err()
            {
                tracing::warn!("Pearl session did not stop in time");
                task.abort();
            }
        }
    }
}

#[derive(Default)]
pub struct PearlSessions(Arc<Mutex<RunningPearl>>);

fn pearl_options(name: String) -> Result<SessionOptions, String> {
    let display_name = settings::validate_display_name(&name).map_err(|error| error.to_string())?;
    if let Err(error) = Settings::remember_display_name(&display_name) {
        tracing::debug!("could not remember Pearl display name: {error:#}");
    }
    Ok(SessionOptions {
        signal_base: std::env::var("PEARL_SIGNAL").unwrap_or_else(|_| PEARL_SIGNAL.to_owned()),
        identity_path: None,
        display_name,
        probe: ProbeOptions {
            port: probe::preferred_port(),
            timeout: Duration::from_millis(1_500),
            ..ProbeOptions::default()
        },
        relay_only: false,
    })
}

async fn run_pearl<F, Fut>(
    app: tauri::AppHandle,
    sessions: State<'_, PearlSessions>,
    share: Option<tokio::sync::watch::Sender<Option<u16>>>,
    start: F,
) -> Result<(), String>
where
    F: FnOnce(session::StopSignal, Box<dyn FnMut(SessionEvent) + Send>) -> Fut + Send + 'static,
    Fut: Future<Output = anyhow::Result<()>> + Send,
{
    let state = sessions.0.clone();
    let mut running = state.lock().await;
    running.stop().await;
    running.share = share;
    let emitter = app.clone();
    let emit = Box::new(move |event: SessionEvent| {
        let _ = emitter.emit("session", session_event_json(&event));
    }) as Box<dyn FnMut(SessionEvent) + Send>;
    let (stop, signal) = session::stop_channel();
    let task = tauri::async_runtime::spawn(async move {
        let detail = start(signal, emit)
            .await
            .err()
            .map(|error| format!("{error:#}"));
        let _ = app.emit(
            "session",
            serde_json::json!({ "event": "ended", "detail": detail }),
        );
    });
    running.task = Some((task, stop));
    Ok(())
}

#[tauri::command]
async fn pearl_host(
    app: tauri::AppHandle,
    sessions: State<'_, PearlSessions>,
    name: String,
) -> Result<(), String> {
    let session = pearl_options(name)?;
    let (share, shared_port) = tokio::sync::watch::channel(None);
    run_pearl(app, sessions, Some(share), move |stop, emit| async move {
        session::run_host(
            HostOptions {
                session,
                minecraft: MinecraftSource::Discovered,
                shared_port: Some(shared_port),
            },
            stop,
            emit,
        )
        .await
    })
    .await
}

#[tauri::command]
async fn pearl_join(
    app: tauri::AppHandle,
    sessions: State<'_, PearlSessions>,
    invite: String,
    name: String,
) -> Result<(), String> {
    let session = pearl_options(name)?;
    let invite: Invite = invite
        .trim()
        .parse()
        .map_err(|_| "这串邀请码看起来不对，应该是十二个数字或一条 pearl:// 链接".to_owned())?;
    run_pearl(app, sessions, None, move |stop, emit| async move {
        session::run_join(
            JoinOptions {
                session,
                invite,
                local_port: 0,
            },
            stop,
            emit,
        )
        .await
    })
    .await
}

#[tauri::command]
async fn pearl_stop(sessions: State<'_, PearlSessions>) -> Result<(), String> {
    sessions.0.lock().await.stop().await;
    Ok(())
}

#[tauri::command]
async fn pearl_share_port(
    sessions: State<'_, PearlSessions>,
    port: Option<u16>,
) -> Result<(), String> {
    if port == Some(0) {
        return Err("端口需要在 1 到 65535 之间".to_owned());
    }
    let running = sessions.0.lock().await;
    let Some(share) = &running.share else {
        return Err("没有正在运行的房间".to_owned());
    };
    share.send(port).map_err(|_| "会话已经结束".to_owned())
}

#[tauri::command]
fn pearl_remembered_name() -> Option<String> {
    Settings::load().display_name
}

#[tauri::command]
fn pearl_remember_name(name: String) -> Result<(), String> {
    Settings::remember_display_name(&name)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn app_name() -> &'static str {
    "Fern"
}

/// 关于页要说的：这是哪一份构建，跑在什么上面。
///
/// 版本号从构建来，不是界面里写死的字符串——写死的那个和 `tauri.conf.json`、
/// `Cargo.toml` 各写各的，迟早对不上。构建标识（短哈希与日期）由 build.rs 注入。
#[tauri::command]
fn about() -> About {
    About {
        version: env!("CARGO_PKG_VERSION"),
        // 源码包里没有 .git，那时候是空串，界面照实不显示。
        commit: env!("FERN_COMMIT"),
        built: env!("FERN_BUILD_DATE"),
        platform: fern_core::platform(),
        // 「只有我这台打不开」十有八九是它。用户自己查不到，我们查得到。
        webview: tauri::webview_version().unwrap_or_default(),
        // deb 装的那一份不自更新（见 docs/fern-update-design.md §4），界面要据此
        // 换一个按钮，而不是给出一个按下去会解释自己为什么不行的按钮。
        self_update: fern_core::update_install() != fern_core::UpdateInstall::SystemPackage,
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct About {
    version: &'static str,
    commit: &'static str,
    built: &'static str,
    platform: String,
    webview: String,
    self_update: bool,
}

/// 这个通道上有没有更新。
///
/// 版本号取 `package_info()`，**不是** `CARGO_PKG_VERSION`——虽然 build.rs 强制
/// 两者相等（见那里的 `the_two_version_numbers_must_agree`），但拿去比大小的
/// 应该是自更新真正认的那一个，而不是碰巧相等的另一个。
///
/// 这一步只回答「有没有」，用的是我们自己的清单（`fern-core` 的 `update`）。
/// 装是 `update_apply` 的事，那里才轮到更新器插件。分开的理由是清单里有插件
/// 不认识的字段（`rollout` / `critical` / `minVersion`），而它们决定要不要装。
#[tauri::command]
async fn check_update(app: tauri::AppHandle) -> Result<fern_core::UpdateDecision, String> {
    let version = app.package_info().version.to_string();
    // 不进 off_thread：这里的磁盘动作只是读一份几 KB 的 settings.json，
    // 真正花时间的是后面那一次网络请求，而它本来就是异步的。
    fern_core::check_for_update(&paths()?, &version)
        .await
        .map_err(|error| format!("{error:#}"))
}

/// 下载并装上新版本。**装完不重启**——重启由用户按（见 `update_restart`）。
///
/// 分工是查过插件源码之后定的（docs/fern-update-design.md §3）：下载和验签用它的，
/// 因为 `download` 在返回字节之前就验完签了；落盘只有 Windows 是我们自己做的，
/// 因为它在 Windows 上只认安装器，会把便携 exe 当安装器从临时目录跑起来，
/// 磁盘上什么都不会变。
#[tauri::command]
async fn update_apply(app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_updater::UpdaterExt;

    let install = fern_core::update_install();
    if install == fern_core::UpdateInstall::SystemPackage {
        // 界面本来就不该给出这个按钮。真到了这里，说清楚为什么而不是去弹提权框。
        return Err("这份 Fern 由系统包管理器安装，请通过包管理器更新。".to_owned());
    }
    // 便携版可能被放在只读目录、U 盘、网络盘里。**先试写再下载**：下了几十兆
    // 才发现写不进去，比一开始就说清楚糟得多。
    if install == fern_core::UpdateInstall::PortableExecutable {
        fern_core::writable_beside_executable().map_err(|error| format!("{error:#}"))?;
    }

    let version = app.package_info().version.to_string();
    let channel = fern_core::update_channel(&paths()?, &version);
    let endpoint = channel
        .manifest_url(fern_core::UPDATE_ENDPOINT)
        .parse()
        .map_err(|error| format!("更新地址无效：{error}"))?;

    // 端点在运行时给，不写死在 tauri.conf.json 里——通道是用户设置，
    // 而配置文件是编译期的。用 `app.updater_builder()` 而不是直接构造
    // `UpdaterBuilder`：后者在 Windows 上会因为 current_exe_args 为空而 panic。
    let updater = app
        .updater_builder()
        .endpoints(vec![endpoint])
        .map_err(|error| format!("{error}"))?
        .build()
        .map_err(|error| format!("{error}"))?;

    let Some(update) = updater.check().await.map_err(|error| format!("{error}"))? else {
        return Err("没有可以安装的更新。".to_owned());
    };

    let progress = app.clone();
    let mut downloaded: usize = 0;
    let bytes = update
        .download(
            move |chunk, total| {
                downloaded += chunk;
                let _ = progress.emit(
                    "update_progress",
                    serde_json::json!({ "downloaded": downloaded, "total": total }),
                );
            },
            || {},
        )
        .await
        .map_err(|error| format!("{error}"))?;

    match install {
        fern_core::UpdateInstall::PortableExecutable => {
            // 先落到临时文件：self_replace 要一个磁盘上的路径，而它在 Windows 上
            // 做的是「把当前 exe 挪开腾出文件名，再把新文件放到原路径」。
            let staged = std::env::temp_dir().join(format!("fern-{version}-update.tmp"));
            std::fs::write(&staged, &bytes).map_err(|error| format!("写入临时文件失败：{error}"))?;
            let replaced = self_replace::self_replace(&staged).map_err(|error| error.to_string());
            // 无论成败都清掉临时文件；失败时原来的可执行文件还在原地。
            let _ = std::fs::remove_file(&staged);
            replaced?;
        }
        fern_core::UpdateInstall::Bundle => {
            update.install(bytes).map_err(|error| format!("{error}"))?;
        }
        fern_core::UpdateInstall::SystemPackage => unreachable!("上面已经挡掉了"),
    }
    Ok(())
}

/// 重启，让刚装上的那一份生效。
///
/// 单独一个命令，因为**什么时候重启是用户的事**：更新装好的那一刻他可能正在
/// 游戏里。界面在有游戏运行时不给这个按钮。
#[tauri::command]
fn update_restart(app: tauri::AppHandle) {
    app.restart()
}

#[tauri::command]
fn data_paths() -> Result<DataLocation, String> {
    // 纯算路径，不碰磁盘，留在主线程上。
    let paths = paths()?;
    Ok(DataLocation {
        portable: paths.is_portable(),
        game: paths.shared_game_root(),
        root: paths.root,
        logs: paths.logs,
    })
}

/// 设置里「数据」那一节要说的话。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct DataLocation {
    root: std::path::PathBuf,
    /// 共享的资源、依赖库与版本描述。是一个标准的 `.minecraft` 布局。
    game: std::path::PathBuf,
    logs: std::path::PathBuf,
    /// 数据根跟着可执行文件走。旁边有 `fern-portable` 标记时就是这样。
    portable: bool,
}

/// 把一段同步的活挪出主线程。
///
/// Tauri 里不带 `async` 的命令在主线程上执行——那正是 webview 事件循环所在的
/// 线程。于是复制一个几 GB 的实例目录、扫一遍磁盘找 Java、读两百个 jar 的
/// 清单，这些事一发生，整个窗口连动画都停住，看起来就是「点了没反应，过一会
/// 自己好了」。
///
/// 不用 `#[tauri::command(async)]`：那只是挪到异步运行时上，而同一条运行时正
/// 在跑下载，拿阻塞占掉它一个 worker 等于把问题换了个地方。阻塞的活交给专门
/// 的阻塞线程池，它闲置时不占任何东西。
///
/// 规则：**碰磁盘、碰钥匙串、起进程的命令一律走这里。** 只算路径、不做 I/O 的
/// 留在原地——为它们付一次线程调度不值得。
async fn off_thread<T, F>(work: F) -> Result<T, String>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| format!("后台任务没能跑完：{error}"))
}

/// 拿到数据目录，顺带把错误变成界面能显示的字符串。
fn paths() -> Result<fern_core::DataPaths, String> {
    // 可执行文件旁边有 `fern-portable` 标记时跟着它走，否则用平台的用户数据
    // 目录。整个应用只在这一个函数里回答「数据根在哪」。
    fern_core::DataPaths::resolve().map_err(|error| error.to_string())
}

#[tauri::command]
async fn list_instances() -> Result<Vec<fern_core::InstanceProfile>, String> {
    off_thread(|| fern_core::list_instances(&paths()?).map_err(|error| format!("{error:#}")))
        .await?
}

/// 能建实例的所有版本。
///
/// `refresh` 是用户按下刷新的那一下；平时走缓存，六小时之内不再联网。
#[tauri::command]
async fn list_versions(refresh: bool) -> Result<Vec<fern_core::VersionOption>, String> {
    fern_core::list_versions(&paths()?, refresh)
        .await
        .map_err(|error| format!("{error:#}"))
}

/// 建实例。`loader` 是 `vanilla` / `fabric` / `quilt`。
///
/// 加载器版本可以不给——不给就取最新的稳定版，这是绝大多数人想要的那个，
/// 不该逼着每个人先去理解「loader 版本」是什么。
#[tauri::command]
async fn create_instance(
    name: String,
    game_version: String,
    loader: Option<String>,
    loader_version: Option<String>,
) -> Result<fern_core::InstanceProfile, String> {
    let paths = fern_core::DataPaths::for_current_user().map_err(|error| error.to_string())?;
    let kind = parse_loader(loader.as_deref())?;
    let version = match (kind, loader_version) {
        (fern_core::LoaderKind::Vanilla, _) => None,
        (_, Some(version)) if !version.is_empty() => Some(version),
        (kind, _) => Some(
            fern_core::latest_loader_version(&paths, kind, &game_version)
                .await
                .map_err(|error| format!("{error:#}"))?,
        ),
    };
    fern_core::create_instance_with_loader(&paths, &name, &game_version, kind, version.as_deref())
        .map_err(|error| format!("{error:#}"))
}

/// 这个游戏版本上装得了的加载器版本。
#[tauri::command]
async fn list_loader_versions(
    loader: String,
    game_version: String,
) -> Result<Vec<fern_core::LoaderVersion>, String> {
    let kind = parse_loader(Some(&loader))?;
    if kind == fern_core::LoaderKind::Vanilla {
        return Ok(Vec::new());
    }
    fern_core::list_loader_versions(&paths()?, kind, &game_version)
        .await
        .map_err(|error| format!("{error:#}"))
}

/// 现在装得上的加载器，给创建面板用。硬编码在界面里的话，加一种就要改两处。
#[tauri::command]
fn installable_loaders() -> Vec<fern_core::LoaderOption> {
    fern_core::installable_loaders()
}

fn parse_loader(loader: Option<&str>) -> Result<fern_core::LoaderKind, String> {
    let Some(loader) = loader.filter(|value| !value.is_empty()) else {
        return Ok(fern_core::LoaderKind::Vanilla);
    };
    serde_json::from_value(serde_json::Value::String(loader.to_owned()))
        .map_err(|_| format!("不认识的加载器：{loader}"))
}

/// Settings live in `settings.json` under the data root, not in webview
/// storage: they survive a cleared webview, and the user can open, back up, or
/// share the file.
#[tauri::command]
async fn get_settings() -> Result<fern_core::Settings, String> {
    off_thread(|| Ok(fern_core::load_settings(&paths()?))).await?
}

#[tauri::command]
async fn save_settings(settings: fern_core::Settings) -> Result<(), String> {
    off_thread(move || {
        fern_core::save_settings(&paths()?, &settings).map_err(|error| format!("{error:#}"))
    })
    .await?
}

/// Returns null when nothing usable was found, which is the only case the
/// setup wizard has anything to say about.
#[tauri::command]
async fn detect_java() -> Option<fern_core::JavaRuntime> {
    // 扫的是整个 PATH 加几个系统目录，慢起来是秒级的。
    off_thread(fern_core::detect_java).await.ok().flatten()
}

/// 打开实例的游戏目录，或者它下面的某个子目录（`mods`、`saves`……）。
#[tauri::command]
async fn open_instance_directory(instance_id: String, sub: Option<String>) -> Result<(), String> {
    off_thread(move || {
        let id = fern_core::InstanceId::parse(instance_id).map_err(|error| error.to_string())?;
        let paths = paths()?;
        // 外部实例的游戏目录在别人的目录树下，按 id 推导会打开一个空目录。
        let profile = fern_core::read_instance(&paths, id.as_str())
            .map_err(|error| format!("{error:#}"))?;
        let mut directory = fern_core::instance_paths(&paths, &profile).game_directory(id.as_str());
        if let Some(sub) = sub.filter(|sub| !sub.is_empty()) {
            // 子目录名来自界面，不能原样拼。
            if sub.contains(['/', '\\']) || sub.contains("..") {
                return Err(format!("非法的子目录：{sub}"));
            }
            directory.push(sub);
        }
        std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        hand_to_system(directory.as_os_str())
    })
    .await?
}

#[tauri::command]
async fn open_logs_directory() -> Result<(), String> {
    off_thread(|| {
        let logs = paths()?.logs;
        std::fs::create_dir_all(&logs).map_err(|error| error.to_string())?;
        hand_to_system(logs.as_os_str())
    })
    .await?
}

/// 把一个目录或者一条链接交给系统的打开程序。
///
/// 三个平台上都是同一个动作，只是程序名不同；目录和链接对这些程序来说也是
/// 同一种东西——一个参数。所以这里收 `OsStr` 而不是 `Path`，免得为了复用把
/// 一条 URL 说成是路径。
fn hand_to_system(target: &std::ffi::OsStr) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let mut command = std::process::Command::new("explorer");

    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");

    #[cfg(target_os = "linux")]
    let mut command = std::process::Command::new("xdg-open");

    command
        .arg(target)
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// 把核心的事件流接到前端。
///
/// 转发任务是脱手的，不等命令返回：游戏日志和退出事件是在 `launch_instance`
/// 返回之后才陆续到来的，命令一结束就关掉通道，界面就再也收不到游戏在说
/// 什么。发送端全部丢掉时（下载结束、游戏退出、读线程收摊）通道自己关，
/// 任务随之结束。
fn launcher_events(
    app: &tauri::AppHandle,
) -> tokio::sync::mpsc::UnboundedSender<fern_core::LauncherEvent> {
    let (events, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = receiver.recv().await {
            let _ = app.emit("launcher-event", event);
        }
    });
    events
}

/// 标题和 `subjects` 由界面给。
///
/// 后端负责宣告作业的存在和进展，不负责编一个显示用的名字——用户是在哪个页面
/// 上、对着哪个东西点的这一下，只有界面知道。`subjects` 是这件事干在谁身上
/// （实例 id、项目 id，可以都有），界面据此把作业挂回对应的页面，而不必去认识
/// 作业的种类。
#[tauri::command]
async fn prepare_instance(
    app: tauri::AppHandle,
    instance_id: String,
    title: String,
    subjects: Vec<String>,
) -> Result<fern_core::PrepareResult, String> {
    let paths = paths()?;
    let events = launcher_events(&app);
    let job = fern_core::Job::begin(&events, title, subjects);
    let result = fern_core::prepare_instance(&paths, &instance_id, &job)
        .await
        .map_err(|error| format!("{error:#}"));
    job.finish(&result);
    if let Err(error) = &result {
        let _ = paths.append_log(&format!("[prepare] instance={instance_id} error={error}"));
    }
    result
}

/// 这台机器上所有实例的存档与服务器。
///
/// 只取名字：命令面板要回答的是「有哪些世界」，而算体积要把每个世界的几万个
/// 区块文件都 stat 一遍。随开随取，不缓存——世界是在游戏里建的，服务器是在
/// 游戏里加的，任何缓存都会悄悄过期。
#[tauri::command]
async fn list_places() -> Result<Places, String> {
    off_thread(|| {
        let paths = paths()?;
        let instances =
            fern_core::list_instances(&paths).map_err(|error| format!("{error:#}"))?;
        let mut saves = Vec::new();
        let mut servers = Vec::new();
        for profile in &instances {
            let id = profile.id.as_str();
            for name in fern_core::save_names(&paths, id) {
                saves.push(PlaceEntry {
                    instance_id: id.to_owned(),
                    instance_name: profile.name.clone(),
                    name,
                    address: None,
                });
            }
            for server in fern_core::list_servers(&paths, id) {
                servers.push(PlaceEntry {
                    instance_id: id.to_owned(),
                    instance_name: profile.name.clone(),
                    name: server.name,
                    address: Some(server.address),
                });
            }
        }
        Ok(Places { saves, servers })
    })
    .await?
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PlaceEntry {
    instance_id: String,
    instance_name: String,
    name: String,
    address: Option<String>,
}

#[derive(serde::Serialize)]
struct Places {
    saves: Vec<PlaceEntry>,
    servers: Vec<PlaceEntry>,
}

/// 启动之前先看一眼这个实例。
///
/// 不阻止启动，只是把能提前看出来的问题说出来——缺前置、装了两份、加载器不对。
#[tauri::command]
async fn preflight(instance_id: String) -> Result<Vec<fern_core::Finding>, String> {
    off_thread(move || {
        let paths = paths()?;
        let profile = fern_core::read_instance(&paths, &instance_id)
            .map_err(|error| format!("{error:#}"))?;
        Ok(fern_core::preflight_instance(&paths, &profile))
    })
    .await?
}

/// 这个实例的文件和上次记录的对不对得上。没有话说时是空列表。
///
/// 和预检查分开：那边回答「这样点下去会不会起不来」，这边回答「这些文件还是
/// 上次那些吗」。两件事不在一条轴上，混进一个列表只会让两边都变模糊。
///
/// 读盘那一半走便宜的档（只重算大小或修改时间变过的），所以打开实例和点启动
/// 之前都调得起。彻底的那一遍在游戏退出之后跑，不占用户的时间。
#[tauri::command]
async fn integrity(instance_id: String) -> Result<Vec<fern_core::IntegrityNotice>, String> {
    let paths = paths()?;
    Ok(fern_core::check_integrity(&paths, &instance_id).await)
}

// ——— 快照 ———
//
// 全部走 `off_thread`：拍一张要读整个游戏目录，恢复要写回去，都是重活。

/// 这个实例有哪些快照，从新到旧。
#[tauri::command]
async fn list_snapshots(instance_id: String) -> Result<Vec<fern_core::Snapshot>, String> {
    off_thread(move || {
        fern_core::list_snapshots(&paths()?, &instance_id).map_err(|error| format!("{error:#}"))
    })
    .await?
}

/// 手动拍一张。带标签的永久保留。
#[tauri::command]
async fn take_snapshot(
    instance_id: String,
    label: Option<String>,
) -> Result<fern_core::Snapshot, String> {
    off_thread(move || {
        fern_core::take_snapshot(
            &paths()?,
            &instance_id,
            fern_core::SnapshotReason::Manual,
            label.filter(|text| !text.trim().is_empty()),
        )
        .map_err(|error| format!("{error:#}"))
    })
    .await?
}

/// 恢复。`scope` 与 `mode` 是带标签的枚举，形状由核心那边定。
#[tauri::command]
async fn restore_snapshot(
    instance_id: String,
    snapshot: String,
    scope: fern_core::RestoreScope,
    mode: fern_core::RestoreMode,
) -> Result<fern_core::Restored, String> {
    off_thread(move || {
        fern_core::restore_snapshot(&paths()?, &instance_id, &snapshot, &scope, &mode)
            .map_err(|error| format!("{error:#}"))
    })
    .await?
}

/// 删一张快照，顺手回收没人引用的内容。
#[tauri::command]
async fn delete_snapshot(instance_id: String, snapshot: String) -> Result<(), String> {
    off_thread(move || {
        let paths = paths()?;
        fern_core::remove_snapshot(&paths, &instance_id, &snapshot)
            .map_err(|error| format!("{error:#}"))?;
        // 回收失败不该让「删掉了吗」这个问题变得没有答案——快照确实已经删了。
        let _ = fern_core::collect_garbage(&paths);
        Ok(())
    })
    .await?
}

/// 贴标签或者取消标签。贴过标签的永久保留。
#[tauri::command]
async fn label_snapshot(
    instance_id: String,
    snapshot: String,
    label: Option<String>,
) -> Result<fern_core::Snapshot, String> {
    off_thread(move || {
        fern_core::label_snapshot(&paths()?, &instance_id, &snapshot, label)
            .map_err(|error| format!("{error:#}"))
    })
    .await?
}

/// 按保留策略剪一批。
#[tauri::command]
async fn prune_snapshots(instance_id: String) -> Result<Vec<String>, String> {
    off_thread(move || {
        fern_core::prune_snapshots(&paths()?, &instance_id).map_err(|error| format!("{error:#}"))
    })
    .await?
}

/// 快照一共占多少磁盘。
#[tauri::command]
async fn backup_usage() -> Result<fern_core::Usage, String> {
    off_thread(move || fern_core::backup_usage(&paths()?).map_err(|error| format!("{error:#}")))
        .await?
}

/// 把一个世界打成 zip。
#[tauri::command]
async fn export_world(
    instance_id: String,
    save: String,
    destination: String,
) -> Result<fern_core::Exported, String> {
    off_thread(move || {
        fern_core::export_world(
            &paths()?,
            &instance_id,
            &save,
            std::path::Path::new(&destination),
        )
        .map_err(|error| format!("{error:#}"))
    })
    .await?
}

/// 完整搬迁包。装得下就一定装得回去。
#[tauri::command]
async fn export_fernpack(
    instance_id: String,
    contents: fern_core::ExportContents,
    destination: String,
) -> Result<fern_core::Exported, String> {
    off_thread(move || {
        fern_core::export_fernpack(
            &paths()?,
            &instance_id,
            contents,
            std::path::Path::new(&destination),
        )
        .map_err(|error| format!("{error:#}"))
    })
    .await?
}

/// Modrinth 整合包。模组按哈希反查下载地址，所以这一条要联网。
#[tauri::command]
async fn export_mrpack(
    instance_id: String,
    contents: fern_core::ExportContents,
    destination: String,
) -> Result<fern_core::Exported, String> {
    fern_core::export_mrpack(
        &paths()?,
        &instance_id,
        contents,
        std::path::Path::new(&destination),
    )
    .await
    .map_err(|error| format!("{error:#}"))
}

/// 这个实例有什么可导。导出弹窗按它画勾选项，没有的分区不出现。
#[tauri::command]
async fn export_inventory(instance_id: String) -> Result<fern_core::ExportInventory, String> {
    off_thread(move || {
        fern_core::export_inventory(&paths()?, &instance_id).map_err(|error| format!("{error:#}"))
    })
    .await?
}

/// 现在有哪些游戏在跑。
///
/// 界面每次回到前台都要问一次：进程可能在启动器不知情的时候没了（用户自己
/// 关的、崩了、被系统收走），只靠事件会留下一个永远「运行中」的按钮。
#[tauri::command]
fn running_games() -> Vec<fern_core::RunningGame> {
    // 读一张内存里的表，不碰磁盘，留在主线程上。
    fern_core::running_games()
}

/// 强行结束一个游戏。没存的进度会丢，这句话要由界面说出来。
#[tauri::command]
fn stop_game(instance_id: String) -> Result<(), String> {
    fern_core::stop_game(&instance_id).map_err(|error| format!("{error:#}"))
}

/// 启动。
///
/// `world` / `server` 是「直接进去」：前者是存档目录名，后者是服务器地址。
/// 两个都给时以 `world` 为准——一次只能进一个地方。
#[tauri::command]
async fn launch_instance(
    app: tauri::AppHandle,
    instance_id: String,
    world: Option<String>,
    server: Option<String>,
    title: String,
    subjects: Vec<String>,
) -> Result<fern_core::LaunchResult, String> {
    let paths = paths()?;
    let events = launcher_events(&app);
    // 一次点击一个作业：补全和启动是同一件事的两段，各自往总步数里添自己那份。
    let job = fern_core::Job::begin(&events, title, subjects);
    job.expect(1);
    let prepared = fern_core::prepare_instance(&paths, &instance_id, &job)
        .await
        .map_err(|error| format!("{error:#}"));
    let result = match prepared {
        Ok(_) => {
            let quick = match (world, server) {
                (Some(name), _) if !name.is_empty() => Some(fern_core::QuickPlay::World(name)),
                (_, Some(address)) if !address.is_empty() => {
                    Some(fern_core::QuickPlay::Server(address))
                }
                _ => None,
            };
            fern_core::launch_instance(&paths, &instance_id, quick, &events, &job)
                .await
                .map_err(|error| format!("{error:#}"))
        }
        Err(error) => Err(error),
    };
    job.finish(&result);
    if let Err(error) = &result {
        let _ = paths.append_log(&format!("[launch] instance={instance_id} error={error}"));
    }
    result
}

/// 补给站搜索。
///
/// 条件全部由界面给出，不从「当前实例」推断——补给站是一个独立的地方，
/// 装不装得上是标注，不是过滤器。
#[tauri::command]
async fn search_resources(
    query: fern_core::SearchQuery,
) -> Result<fern_core::SearchResult, String> {
    fern_core::search_modrinth(&query)
        .await
        .map_err(|error| format!("{error:#}"))
}

/// 一个项目的详情。
#[tauri::command]
async fn project_detail(project: String) -> Result<fern_core::ProjectDetail, String> {
    fern_core::modrinth_project(&project)
        .await
        .map_err(|error| format!("{error:#}"))
}

/// 一个项目的全部版本，新的在前。兼容性由界面按目标实例标注。
#[tauri::command]
async fn project_versions(project: String) -> Result<Vec<fern_core::ProjectVersion>, String> {
    fern_core::modrinth_versions(&project)
        .await
        .map_err(|error| format!("{error:#}"))
}

/// 按下安装会发生什么：装哪些文件、哪些前置已经有了、哪些还缺。
///
/// 界面在装之前问这一个，装的时候后端再算一遍同样的计划——所以显示的和做的
/// 永远是同一件事。
#[tauri::command]
async fn install_plan(
    instance_id: String,
    version_id: String,
    kind: fern_core::ResourceKind,
) -> Result<fern_core::InstallPlan, String> {
    let paths = paths()?;
    fern_core::resolve_install_plan(&paths, &instance_id, &version_id, kind)
        .await
        .map_err(|error| format!("{error:#}"))
}

/// 装一个版本。模组会连同**还缺的**必需依赖一起装。
#[tauri::command]
async fn install_from_modrinth(
    app: tauri::AppHandle,
    instance_id: String,
    version_id: String,
    kind: fern_core::ResourceKind,
    title: String,
    subjects: Vec<String>,
) -> Result<fern_core::InstallOutcome, String> {
    let paths = paths()?;
    let events = launcher_events(&app);
    let job = fern_core::Job::begin(&events, title, subjects);
    let result = fern_core::install_from_modrinth(&paths, &instance_id, &version_id, kind, &job)
        .await
        .map_err(|error| format!("{error:#}"));
    job.finish(&result);
    result
}

/// 可执行文件旁边有没有一个现成的 `.minecraft`。首次启动时问一句用的。
#[tauri::command]
fn nearby_game_directory() -> Option<std::path::PathBuf> {
    // 只看一层目录是否存在，留在主线程上。
    fern_core::nearby_game_directory()
}

/// 看一眼一个外部 `.minecraft` 里有哪些版本。什么都不改。
#[tauri::command]
async fn scan_game_directory(path: String) -> Result<fern_core::ExternalScan, String> {
    off_thread(move || {
        fern_core::scan_external_directory(&paths()?, std::path::Path::new(&path))
            .map_err(|error| format!("{error:#}"))
    })
    .await?
}

/// 把其中一个版本添加为实例。不移动、不复制任何游戏文件。
#[tauri::command]
async fn attach_game_version(
    path: String,
    version_id: String,
    shared_libraries: bool,
) -> Result<fern_core::InstanceProfile, String> {
    off_thread(move || {
        fern_core::attach_external_version(
            &paths()?,
            std::path::Path::new(&path),
            &version_id,
            shared_libraries,
        )
        .map_err(|error| format!("{error:#}"))
    })
    .await?
}

/// 从 Modrinth 装一个整合包。它建的是一个**新实例**，不是装进已有的实例。
#[tauri::command]
async fn install_modpack(
    app: tauri::AppHandle,
    version_id: String,
    name: Option<String>,
    title: String,
    subjects: Vec<String>,
) -> Result<fern_core::InstanceProfile, String> {
    let paths = paths()?;
    let events = launcher_events(&app);
    let job = fern_core::Job::begin(&events, title, subjects);
    let result = fern_core::install_modpack_from_modrinth(
        &paths,
        &version_id,
        name.as_deref().filter(|value| !value.is_empty()),
        &job,
    )
    .await
    .map_err(|error| format!("{error:#}"));
    job.finish(&result);
    if let Err(error) = &result {
        let _ = paths.append_log(&format!("[modpack] version={version_id} error={error}"));
    }
    result
}

/// 先看一眼本地这个 .mrpack 里是什么，不动磁盘。
#[tauri::command]
async fn inspect_modpack(path: String) -> Result<fern_core::PackSummary, String> {
    off_thread(move || {
        fern_core::inspect_modpack(std::path::Path::new(&path))
            .map_err(|error| format!("{error:#}"))
    })
    .await?
}

/// 导入一个本地的 .mrpack。
#[tauri::command]
async fn import_modpack(
    app: tauri::AppHandle,
    path: String,
    name: Option<String>,
    title: String,
    subjects: Vec<String>,
) -> Result<fern_core::InstanceProfile, String> {
    let paths = paths()?;
    let events = launcher_events(&app);
    let job = fern_core::Job::begin(&events, title, subjects);
    let result = fern_core::install_modpack(
        &paths,
        std::path::Path::new(&path),
        name.as_deref().filter(|value| !value.is_empty()),
        &job,
    )
    .await
    .map_err(|error| format!("{error:#}"));
    job.finish(&result);
    result
}

/// 用系统浏览器打开一个链接。
///
/// 详情页上的链接是 Modrinth 给的字符串，会被原样递给系统的打开程序，所以
/// 只放行 https——`file://` 会打开本地文件，Windows 上还有一堆自定义协议。
#[tauri::command]
async fn open_external(url: String) -> Result<(), String> {
    if !fern_core::is_external_url(&url) {
        return Err("只能打开 https 链接".to_owned());
    }
    // 起浏览器要 fork 一个进程，在主线程上做就是一次可见的卡顿。
    off_thread(move || hand_to_system(url.as_ref())).await?
}

/// 这个实例装了哪些模组。
#[tauri::command]
async fn list_mods(instance_id: String) -> Result<Vec<fern_core::ModFile>, String> {
    // 每个 jar 都要开一次 zip 读清单，两百个模组的实例上这是实打实的活。
    off_thread(move || {
        fern_core::list_mods(&paths()?, &instance_id).map_err(|error| format!("{error:#}"))
    })
    .await?
}

/// 这个实例里的存档。只读——删世界交给文件管理器。
#[tauri::command]
async fn list_saves(instance_id: String) -> Result<Vec<fern_core::SaveEntry>, String> {
    off_thread(move || {
        fern_core::list_saves(&paths()?, &instance_id).map_err(|error| format!("{error:#}"))
    })
    .await?
}

/// 开或关一个模组。改的是扩展名，文件还在。
#[tauri::command]
async fn set_mod_enabled(
    instance_id: String,
    file_name: String,
    enabled: bool,
) -> Result<String, String> {
    off_thread(move || {
        fern_core::set_mod_enabled(&paths()?, &instance_id, &file_name, enabled)
            .map_err(|error| format!("{error:#}"))
    })
    .await?
}

#[tauri::command]
async fn remove_mod(instance_id: String, file_name: String) -> Result<(), String> {
    off_thread(move || {
        fern_core::remove_mod(&paths()?, &instance_id, &file_name)
            .map_err(|error| format!("{error:#}"))
    })
    .await?
}

/// 把本地的 jar 装进实例。界面上是拖进来或者选文件。
#[tauri::command]
async fn install_mods(
    instance_id: String,
    paths_to_install: Vec<String>,
) -> Result<Vec<fern_core::ModFile>, String> {
    off_thread(move || {
        let data = paths()?;
        let mut installed = Vec::new();
        for source in paths_to_install {
            installed.push(
                fern_core::install_mod(&data, &instance_id, std::path::Path::new(&source))
                    .map_err(|error| format!("{error:#}"))?,
            );
        }
        Ok(installed)
    })
    .await?
}

#[tauri::command]
async fn delete_instance(instance_id: String) -> Result<(), String> {
    // 删的是整棵游戏目录，可能上万个文件。
    off_thread(move || {
        fern_core::delete_instance(&paths()?, &instance_id).map_err(|error| format!("{error:#}"))
    })
    .await?
}

#[tauri::command]
async fn rename_instance(
    instance_id: String,
    name: String,
) -> Result<fern_core::InstanceProfile, String> {
    off_thread(move || {
        fern_core::rename_instance(&paths()?, &instance_id, &name)
            .map_err(|error| format!("{error:#}"))
    })
    .await?
}

/// 复制一个实例。整个游戏目录都要抄一份，几个 GB 是常态。
#[tauri::command]
async fn duplicate_instance(
    instance_id: String,
    name: String,
) -> Result<fern_core::InstanceProfile, String> {
    off_thread(move || {
        fern_core::duplicate_instance(&paths()?, &instance_id, &name)
            .map_err(|error| format!("{error:#}"))
    })
    .await?
}

/// 这个实例在这台机器上会得到什么：自动算出来的内存、会选中的 Java。
///
/// 设置面板要能回答「不改的话会怎样」——光写「自动」两个字什么都没解释。
#[tauri::command]
async fn instance_runtime(instance_id: String) -> Result<fern_core::InstanceRuntime, String> {
    // 它要读实例配置，还要挑一个 Java——两件都是磁盘上的事。
    off_thread(move || {
        fern_core::instance_runtime(&paths()?, &instance_id).map_err(|error| format!("{error:#}"))
    })
    .await?
}

#[tauri::command]
async fn update_instance_settings(
    instance_id: String,
    settings: fern_core::InstanceSettings,
) -> Result<fern_core::InstanceProfile, String> {
    off_thread(move || {
        fern_core::update_instance_settings(&paths()?, &instance_id, settings)
            .map_err(|error| format!("{error:#}"))
    })
    .await?
}

/// 名册。这里面没有任何令牌，只有「谁是谁」。
#[tauri::command]
async fn list_accounts() -> Result<Vec<fern_core::AccountRecord>, String> {
    off_thread(|| Ok(fern_core::list_accounts(&paths()?))).await?
}

#[tauri::command]
async fn active_account() -> Result<Option<fern_core::AccountRecord>, String> {
    off_thread(|| Ok(fern_core::active_account(&paths()?))).await?
}

/// 一个账户的皮肤，`data:` 地址，头部由界面去裁。
///
/// 拿不到就是 `None`，不是错误：离线号本来就没有皮肤，而皮肤站抽风不该让一份
/// 账户名单打不开。界面那边退回生成式色块。
#[tauri::command]
async fn account_skin(id: String) -> Result<Option<fern_core::AccountSkin>, String> {
    // 读名册要碰磁盘，取皮肤要联网：前者进阻塞线程池，后者留在这里 await。
    let found = off_thread(move || {
        let paths = paths()?;
        let record = fern_core::list_accounts(&paths)
            .into_iter()
            .find(|item| item.id == id);
        Ok::<_, String>(record.map(|record| (paths, record)))
    })
    .await??;
    let Some((paths, record)) = found else {
        return Ok(None);
    };
    Ok(fern_core::account_skin(&paths, &record).await)
}

#[tauri::command]
async fn set_active_account(id: String) -> Result<(), String> {
    off_thread(move || {
        fern_core::set_active_account(&paths()?, &id).map_err(|error| format!("{error:#}"))
    })
    .await?
}

/// 加一个离线账户。名字就是身份——UUID 由它算出来。
#[tauri::command]
async fn add_offline_account(player_name: String) -> Result<fern_core::AccountRecord, String> {
    off_thread(move || {
        fern_core::add_offline_account(&paths()?, &player_name)
            .map_err(|error| format!("{error:#}"))
    })
    .await?
}

#[tauri::command]
async fn rename_offline_account(
    id: String,
    player_name: String,
) -> Result<fern_core::AccountRecord, String> {
    off_thread(move || {
        fern_core::rename_offline_account(&paths()?, &id, &player_name)
            .map_err(|error| format!("{error:#}"))
    })
    .await?
}

/// 移除一个账户，连同它在钥匙串里的令牌。
#[tauri::command]
async fn remove_account(id: String) -> Result<(), String> {
    off_thread(move || {
        fern_core::remove_account(&paths()?, &id).map_err(|error| format!("{error:#}"))
    })
    .await?
}

/// 钉住某个实例用哪个账户。`account_id` 为 null 表示跟着当前账户走。
#[tauri::command]
async fn set_instance_account(
    instance_id: String,
    account_id: Option<String>,
) -> Result<fern_core::InstanceProfile, String> {
    off_thread(move || {
        fern_core::set_instance_account(&paths()?, &instance_id, account_id.as_deref())
            .map_err(|error| format!("{error:#}"))
    })
    .await?
}

/// 外置登录。密码只在这一次调用里存在，登录成功后进系统钥匙串的是令牌。
#[tauri::command]
async fn yggdrasil_login(
    api_root: String,
    username: String,
    password: String,
) -> Result<fern_core::AccountRecord, String> {
    let client_token =
        off_thread(|| fern_core::client_token().map_err(|error| format!("{error:#}"))).await??;
    let session = fern_core::authenticate(&api_root, &username, &password, &client_token)
        .await
        .map_err(|error| format!("{error:#}"))?;
    // 交给界面的是名册里那一条，令牌留在这一侧。
    off_thread(move || {
        fern_core::adopt_account(&paths()?, fern_core::Secret::Yggdrasil(session))
            .map_err(|error| format!("{error:#}"))
    })
    .await?
}

/// 那条正在轮询的登录的催促口。
///
/// 全局一个：同一时刻只该有一场登录在进行（界面那边也拦着）。没有人在等的
/// 时候催一下不会有任何后果。
fn login_nudge() -> &'static fern_core::Nudge {
    static NUDGE: std::sync::OnceLock<fern_core::Nudge> = std::sync::OnceLock::new();
    NUDGE.get_or_init(fern_core::Nudge::new)
}

/// 「我已经在浏览器里登完了」。
///
/// 轮询本来就会自己发现，这颗按钮省下的是那几秒的等待——而那几秒发生在用户
/// 已经做完自己那一半、正盯着启动器看的时候，是整条流程里最难熬的一段。
#[tauri::command]
fn check_microsoft_login() {
    login_nudge().poke();
}

/// 微软正版登录。
///
/// device code flow：先要一个八位码，把它发给界面显示，然后一直轮询直到
/// 用户在浏览器里点完。整个过程里密码和令牌都不经过 webview——界面拿到的
/// 只有那个念给人听的八位码。
#[tauri::command]
async fn microsoft_login(app: tauri::AppHandle) -> Result<fern_core::AccountRecord, String> {
    let challenge = fern_core::begin_microsoft_login()
        .await
        .map_err(|error| format!("{error:#}"))?;
    // DeviceCodeChallenge 序列化时会跳过 device_code，只带 user_code 出去。
    let _ = app.emit("microsoft-device-code", &challenge);

    let session = fern_core::finish_microsoft_login(&challenge, login_nudge())
        .await
        .map_err(|error| format!("{error:#}"))?;
    off_thread(move || {
        fern_core::adopt_account(&paths()?, fern_core::Secret::Microsoft(session))
            .map_err(|error| format!("{error:#}"))
    })
    .await?
}

/// 这台机器有多少内存，以及现在交给游戏的上限在哪。
#[tauri::command]
async fn memory_budget() -> Result<fern_core::MemoryBudget, String> {
    off_thread(|| {
        Ok(fern_core::memory_budget(
            fern_core::current_settings().game.memory_ceiling_mb,
        ))
    })
    .await?
}

/// 这台机器上的 Java。设置页要能看见 Fern 到底会用哪一个。
#[tauri::command]
async fn list_java_runtimes() -> Result<Vec<fern_core::JavaRuntime>, String> {
    off_thread(|| Ok(fern_core::discover_java(Some(&paths()?)))).await?
}

/// Java 按大版本分组的全貌：每一组由谁在用，装了没有。
#[tauri::command]
async fn java_overview() -> Result<Vec<fern_core::JavaGroup>, String> {
    off_thread(|| {
        let paths = paths()?;
        let instances =
            fern_core::list_instances(&paths).map_err(|error| format!("{error:#}"))?;
        Ok(fern_core::java_overview(&paths, &instances))
    })
    .await?
}

/// 主动装一个大版本。走作业，因为这是一次两百兆上下的下载。
#[tauri::command]
async fn install_java(
    app: tauri::AppHandle,
    major: u16,
    title: String,
    subjects: Vec<String>,
) -> Result<(), String> {
    let paths = paths()?;
    let events = launcher_events(&app);
    let job = fern_core::Job::begin(&events, title, subjects);
    job.expect(1);
    job.step(format!("下载 Java {major}"));
    let result = fern_core::install_java(&paths, major, &job.downloads())
        .await
        .map(|_| ())
        .map_err(|error| format!("{error:#}"));
    job.finish(&result);
    result
}

/// 手动登记一个安装位置。可以指到可执行文件，也可以指到根目录。
#[tauri::command]
async fn add_java_path(path: String) -> Result<fern_core::JavaRuntime, String> {
    off_thread(move || {
        fern_core::add_java_path(&paths()?, std::path::Path::new(&path))
            .map_err(|error| format!("{error:#}"))
    })
    .await?
}

/// 不再登记某个手动加进来的位置。磁盘上的东西一个字节都不动。
#[tauri::command]
async fn forget_java_path(home: String) -> Result<(), String> {
    off_thread(move || {
        fern_core::forget_java_path(&paths()?, std::path::Path::new(&home))
            .map_err(|error| format!("{error:#}"))
    })
    .await?
}

/// 删掉一份 Fern 自己下载的运行时。核心那边会拒绝 `runtimes/` 以外的路径。
#[tauri::command]
async fn remove_java_runtime(home: String) -> Result<(), String> {
    off_thread(move || {
        fern_core::remove_runtime(&paths()?, std::path::Path::new(&home))
            .map_err(|error| format!("{error:#}"))
    })
    .await?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // 必须是第一个插件（官方文档的要求），否则第二个进程会先把窗口建出来
        // 再退出，屏幕上闪一下。
        //
        // 两个 Fern 同时跑会各写各的 settings.json、accounts.json 和实例描述，
        // 后写的那一份赢，另一边的改动无声消失。所以第二次双击不是「再开一个」，
        // 而是把已经开着的那个叫到前面来。
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        // 只用它的检查、下载与验签。落盘在 `update_apply` 里按形态分开。
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(PearlSessions::default())
        .setup(|app| {
            #[cfg(not(target_os = "macos"))]
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_decorations(false);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event
                && let Some(sessions) = window.try_state::<PearlSessions>()
            {
                let state = sessions.0.clone();
                tauri::async_runtime::spawn(async move { state.lock().await.stop().await });
            }
        })
        .invoke_handler(tauri::generate_handler![
            app_name,
            about,
            check_update,
            update_apply,
            update_restart,
            data_paths,
            list_instances,
            list_versions,
            create_instance,
            list_loader_versions,
            installable_loaders,
            detect_java,
            list_accounts,
            active_account,
            account_skin,
            set_active_account,
            add_offline_account,
            rename_offline_account,
            remove_account,
            set_instance_account,
            microsoft_login,
            check_microsoft_login,
            yggdrasil_login,
            memory_budget,
            list_java_runtimes,
            java_overview,
            install_java,
            add_java_path,
            forget_java_path,
            remove_java_runtime,
            instance_runtime,
            search_resources,
            project_detail,
            open_external,
            nearby_game_directory,
            scan_game_directory,
            attach_game_version,
            install_modpack,
            inspect_modpack,
            import_modpack,
            project_versions,
            install_plan,
            install_from_modrinth,
            list_mods,
            list_saves,
            set_mod_enabled,
            remove_mod,
            install_mods,
            delete_instance,
            rename_instance,
            duplicate_instance,
            update_instance_settings,
            get_settings,
            save_settings,
            open_instance_directory,
            open_logs_directory,
            prepare_instance,
            launch_instance,
            running_games,
            stop_game,
            preflight,
            integrity,
            list_snapshots,
            take_snapshot,
            restore_snapshot,
            delete_snapshot,
            label_snapshot,
            prune_snapshots,
            backup_usage,
            export_world,
            export_fernpack,
            export_mrpack,
            export_inventory,
            list_places,
            pearl_host,
            pearl_join,
            pearl_stop,
            pearl_share_port,
            pearl_remembered_name,
            pearl_remember_name
        ])
        .run(tauri::generate_context!())
        .expect("error while running Fern");
}
