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

#[tauri::command]
fn data_paths() -> Result<fern_core::DataPaths, String> {
    // 纯算路径，不碰磁盘，留在主线程上。
    paths()
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
    fern_core::DataPaths::for_current_user().map_err(|error| error.to_string())
}

#[tauri::command]
async fn list_instances() -> Result<Vec<fern_core::InstanceProfile>, String> {
    off_thread(|| fern_core::list_instances(&paths()?).map_err(|error| format!("{error:#}")))
        .await?
}

#[tauri::command]
async fn list_versions() -> Result<Vec<fern_core::VersionOption>, String> {
    fern_core::list_versions()
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
            fern_core::latest_loader_version(kind, &game_version)
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
    fern_core::list_loader_versions(kind, &game_version)
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

#[tauri::command]
fn offline_account(player_name: String) -> fern_core::Credentials {
    fern_core::offline_credentials(player_name)
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
        let mut directory = paths()?.game_directory(id.as_str());
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

#[tauri::command]
async fn launch_instance(
    app: tauri::AppHandle,
    instance_id: String,
    player_name: String,
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
        Ok(_) => fern_core::launch_instance(&paths, &instance_id, &player_name, &events, &job)
            .await
            .map_err(|error| format!("{error:#}")),
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

/// 装一个版本。模组会连同必需依赖一起装。
#[tauri::command]
async fn install_from_modrinth(
    app: tauri::AppHandle,
    instance_id: String,
    version_id: String,
    kind: fern_core::ResourceKind,
    title: String,
    subjects: Vec<String>,
) -> Result<Vec<String>, String> {
    let paths = paths()?;
    let events = launcher_events(&app);
    let job = fern_core::Job::begin(&events, title, subjects);
    let result = fern_core::install_from_modrinth(&paths, &instance_id, &version_id, kind, &job)
        .await
        .map_err(|error| format!("{error:#}"));
    job.finish(&result);
    result
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

/// 外置登录。密码只在这一次调用里存在，登录成功后进系统钥匙串的是令牌。
#[tauri::command]
async fn yggdrasil_login(
    api_root: String,
    username: String,
    password: String,
) -> Result<fern_core::AccountView, String> {
    let client_token =
        off_thread(|| fern_core::client_token().map_err(|error| format!("{error:#}"))).await??;
    let session = fern_core::authenticate(&api_root, &username, &password, &client_token)
        .await
        .map_err(|error| format!("{error:#}"))?;
    // 只把界面用得着的那部分交出去，令牌留在这一侧。
    let view = fern_core::AccountView::from(&session);
    off_thread(move || fern_core::store_session(&session).map_err(|error| format!("{error:#}")))
        .await??;
    Ok(view)
}

/// 当前登录的是谁。没登录过返回 null——那是正常状态，不是错误。
#[tauri::command]
async fn yggdrasil_session() -> Result<Option<fern_core::AccountView>, String> {
    // 钥匙串是一次 IPC，对方还可能弹窗问你要不要放行——绝不能在主线程上等。
    off_thread(|| {
        fern_core::load_session()
            .map(|session| session.as_ref().map(fern_core::AccountView::from))
            .map_err(|error| format!("{error:#}"))
    })
    .await?
}

#[tauri::command]
async fn yggdrasil_logout() -> Result<(), String> {
    off_thread(|| fern_core::clear_session().map_err(|error| format!("{error:#}"))).await?
}

/// 微软正版登录。
///
/// device code flow：先要一个八位码，把它发给界面显示，然后一直轮询直到
/// 用户在浏览器里点完。整个过程里密码和令牌都不经过 webview——界面拿到的
/// 只有那个念给人听的八位码。
#[tauri::command]
async fn microsoft_login(app: tauri::AppHandle) -> Result<fern_core::AccountView, String> {
    let challenge = fern_core::begin_microsoft_login()
        .await
        .map_err(|error| format!("{error:#}"))?;
    // DeviceCodeChallenge 序列化时会跳过 device_code，只带 user_code 出去。
    let _ = app.emit("microsoft-device-code", &challenge);

    let session = fern_core::finish_microsoft_login(&challenge)
        .await
        .map_err(|error| format!("{error:#}"))?;
    let view = fern_core::AccountView::from(&session);
    off_thread(move || {
        fern_core::store_microsoft_session(&session).map_err(|error| format!("{error:#}"))
    })
    .await??;
    Ok(view)
}

#[tauri::command]
async fn microsoft_session() -> Result<Option<fern_core::AccountView>, String> {
    off_thread(|| {
        fern_core::load_microsoft_session()
            .map(|session| session.as_ref().map(fern_core::AccountView::from))
            .map_err(|error| format!("{error:#}"))
    })
    .await?
}

#[tauri::command]
async fn microsoft_logout() -> Result<(), String> {
    off_thread(|| fern_core::clear_microsoft_session().map_err(|error| format!("{error:#}")))
        .await?
}

/// 这台机器上的 Java。设置页要能看见 Fern 到底会用哪一个。
#[tauri::command]
async fn list_java_runtimes() -> Result<Vec<fern_core::JavaRuntime>, String> {
    off_thread(|| Ok(fern_core::discover_java(Some(&paths()?)))).await?
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
            data_paths,
            list_instances,
            list_versions,
            create_instance,
            list_loader_versions,
            installable_loaders,
            offline_account,
            detect_java,
            microsoft_login,
            microsoft_session,
            microsoft_logout,
            yggdrasil_login,
            yggdrasil_session,
            yggdrasil_logout,
            list_java_runtimes,
            remove_java_runtime,
            instance_runtime,
            search_resources,
            project_detail,
            open_external,
            install_modpack,
            inspect_modpack,
            import_modpack,
            project_versions,
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
