use tauri::{Emitter, Manager};

#[tauri::command]
fn app_name() -> &'static str {
    "Fern"
}

#[tauri::command]
fn data_paths() -> Result<fern_core::DataPaths, String> {
    fern_core::DataPaths::for_current_user().map_err(|error| error.to_string())
}

#[tauri::command]
fn list_instances() -> Result<Vec<fern_core::InstanceProfile>, String> {
    let paths = fern_core::DataPaths::for_current_user().map_err(|error| error.to_string())?;
    fern_core::list_instances(&paths).map_err(|error| format!("{error:#}"))
}

#[tauri::command]
async fn list_versions() -> Result<Vec<fern_core::VersionOption>, String> {
    fern_core::list_versions().await.map_err(|error| format!("{error:#}"))
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
fn get_settings() -> Result<fern_core::Settings, String> {
    let paths = fern_core::DataPaths::for_current_user().map_err(|error| error.to_string())?;
    Ok(fern_core::load_settings(&paths))
}

#[tauri::command]
fn save_settings(settings: fern_core::Settings) -> Result<(), String> {
    let paths = fern_core::DataPaths::for_current_user().map_err(|error| error.to_string())?;
    fern_core::save_settings(&paths, &settings).map_err(|error| format!("{error:#}"))
}

/// Returns null when nothing usable was found, which is the only case the
/// setup wizard has anything to say about.
#[tauri::command]
fn detect_java() -> Option<fern_core::JavaRuntime> {
    fern_core::detect_java()
}

/// 打开实例的游戏目录，或者它下面的某个子目录（`mods`、`saves`……）。
#[tauri::command]
fn open_instance_directory(instance_id: String, sub: Option<String>) -> Result<(), String> {
    let id = fern_core::InstanceId::parse(instance_id).map_err(|error| error.to_string())?;
    let paths = fern_core::DataPaths::for_current_user().map_err(|error| error.to_string())?;
    let mut directory = paths.game_directory(id.as_str());
    if let Some(sub) = sub.filter(|sub| !sub.is_empty()) {
        // 子目录名来自界面，不能原样拼。
        if sub.contains(['/', '\\']) || sub.contains("..") {
            return Err(format!("非法的子目录：{sub}"));
        }
        directory.push(sub);
    }
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;

    #[cfg(target_os = "windows")]
    std::process::Command::new("explorer")
        .arg(&directory)
        .spawn()
        .map_err(|error| error.to_string())?;

    #[cfg(target_os = "macos")]
    std::process::Command::new("open")
        .arg(&directory)
        .spawn()
        .map_err(|error| error.to_string())?;

    #[cfg(target_os = "linux")]
    std::process::Command::new("xdg-open")
        .arg(&directory)
        .spawn()
        .map_err(|error| error.to_string())?;

    Ok(())
}

#[tauri::command]
fn open_logs_directory() -> Result<(), String> {
    let paths = fern_core::DataPaths::for_current_user().map_err(|error| error.to_string())?;
    std::fs::create_dir_all(&paths.logs).map_err(|error| error.to_string())?;

    #[cfg(target_os = "windows")]
    std::process::Command::new("explorer")
        .arg(&paths.logs)
        .spawn()
        .map_err(|error| error.to_string())?;

    #[cfg(target_os = "macos")]
    std::process::Command::new("open")
        .arg(&paths.logs)
        .spawn()
        .map_err(|error| error.to_string())?;

    #[cfg(target_os = "linux")]
    std::process::Command::new("xdg-open")
        .arg(&paths.logs)
        .spawn()
        .map_err(|error| error.to_string())?;

    Ok(())
}

/// 把核心的事件流接到前端。
///
/// 转发任务是脱手的，不等命令返回：游戏日志和退出事件是在 `launch_instance`
/// 返回之后才陆续到来的，命令一结束就关掉通道，界面就再也收不到游戏在说
/// 什么。发送端全部丢掉时（下载结束、游戏退出、读线程收摊）通道自己关，
/// 任务随之结束。
fn launcher_events(app: &tauri::AppHandle) -> tokio::sync::mpsc::UnboundedSender<fern_core::LauncherEvent> {
    let (events, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = receiver.recv().await {
            let _ = app.emit("launcher-event", event);
        }
    });
    events
}

#[tauri::command]
async fn prepare_instance(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<fern_core::PrepareResult, String> {
    let paths = fern_core::DataPaths::for_current_user().map_err(|error| error.to_string())?;
    let events = launcher_events(&app);
    let result = fern_core::prepare_instance(&paths, &instance_id, &events)
        .await
        .map_err(|error| format!("{error:#}"));
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
) -> Result<fern_core::LaunchResult, String> {
    let paths = fern_core::DataPaths::for_current_user().map_err(|error| error.to_string())?;
    let events = launcher_events(&app);
    let prepared = fern_core::prepare_instance(&paths, &instance_id, &events)
        .await
        .map_err(|error| format!("{error:#}"));
    let result = match prepared {
        Ok(_) => fern_core::launch_instance(&paths, &instance_id, &player_name, &events)
            .await
            .map_err(|error| format!("{error:#}")),
        Err(error) => Err(error),
    };
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
async fn search_resources(query: fern_core::SearchQuery) -> Result<fern_core::SearchResult, String> {
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
) -> Result<Vec<String>, String> {
    let paths = fern_core::DataPaths::for_current_user().map_err(|error| error.to_string())?;
    let events = launcher_events(&app);
    let downloads = fern_core::download_bridge(&events);
    fern_core::install_from_modrinth(&paths, &instance_id, &version_id, kind, &downloads)
        .await
        .map_err(|error| format!("{error:#}"))
}

/// 用系统浏览器打开一个链接。
///
/// 详情页上的链接是 Modrinth 给的字符串，会被原样递给系统的打开程序，所以
/// 只放行 https——`file://` 会打开本地文件，Windows 上还有一堆自定义协议。
#[tauri::command]
fn open_external(url: String) -> Result<(), String> {
    if !fern_core::is_external_url(&url) {
        return Err("只能打开 https 链接".to_owned());
    }

    #[cfg(target_os = "windows")]
    let mut command = std::process::Command::new("explorer");

    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");

    #[cfg(target_os = "linux")]
    let mut command = std::process::Command::new("xdg-open");

    command
        .arg(&url)
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// 这个实例装了哪些模组。
#[tauri::command]
fn list_mods(instance_id: String) -> Result<Vec<fern_core::ModFile>, String> {
    let paths = fern_core::DataPaths::for_current_user().map_err(|error| error.to_string())?;
    fern_core::list_mods(&paths, &instance_id).map_err(|error| format!("{error:#}"))
}

/// 这个实例里的存档。只读——删世界交给文件管理器。
#[tauri::command]
fn list_saves(instance_id: String) -> Result<Vec<fern_core::SaveEntry>, String> {
    let paths = fern_core::DataPaths::for_current_user().map_err(|error| error.to_string())?;
    fern_core::list_saves(&paths, &instance_id).map_err(|error| format!("{error:#}"))
}

/// 开或关一个模组。改的是扩展名，文件还在。
#[tauri::command]
fn set_mod_enabled(
    instance_id: String,
    file_name: String,
    enabled: bool,
) -> Result<String, String> {
    let paths = fern_core::DataPaths::for_current_user().map_err(|error| error.to_string())?;
    fern_core::set_mod_enabled(&paths, &instance_id, &file_name, enabled)
        .map_err(|error| format!("{error:#}"))
}

#[tauri::command]
fn remove_mod(instance_id: String, file_name: String) -> Result<(), String> {
    let paths = fern_core::DataPaths::for_current_user().map_err(|error| error.to_string())?;
    fern_core::remove_mod(&paths, &instance_id, &file_name).map_err(|error| format!("{error:#}"))
}

/// 把本地的 jar 装进实例。界面上是拖进来或者选文件。
#[tauri::command]
fn install_mods(
    instance_id: String,
    paths_to_install: Vec<String>,
) -> Result<Vec<fern_core::ModFile>, String> {
    let paths = fern_core::DataPaths::for_current_user().map_err(|error| error.to_string())?;
    let mut installed = Vec::new();
    for source in paths_to_install {
        installed.push(
            fern_core::install_mod(&paths, &instance_id, std::path::Path::new(&source))
                .map_err(|error| format!("{error:#}"))?,
        );
    }
    Ok(installed)
}

#[tauri::command]
fn delete_instance(instance_id: String) -> Result<(), String> {
    let paths = fern_core::DataPaths::for_current_user().map_err(|error| error.to_string())?;
    fern_core::delete_instance(&paths, &instance_id).map_err(|error| format!("{error:#}"))
}

#[tauri::command]
fn rename_instance(
    instance_id: String,
    name: String,
) -> Result<fern_core::InstanceProfile, String> {
    let paths = fern_core::DataPaths::for_current_user().map_err(|error| error.to_string())?;
    fern_core::rename_instance(&paths, &instance_id, &name).map_err(|error| format!("{error:#}"))
}

#[tauri::command]
fn duplicate_instance(
    instance_id: String,
    name: String,
) -> Result<fern_core::InstanceProfile, String> {
    let paths = fern_core::DataPaths::for_current_user().map_err(|error| error.to_string())?;
    fern_core::duplicate_instance(&paths, &instance_id, &name)
        .map_err(|error| format!("{error:#}"))
}

/// 这个实例在这台机器上会得到什么：自动算出来的内存、会选中的 Java。
///
/// 设置面板要能回答「不改的话会怎样」——光写「自动」两个字什么都没解释。
#[tauri::command]
fn instance_runtime(instance_id: String) -> Result<fern_core::InstanceRuntime, String> {
    let paths = fern_core::DataPaths::for_current_user().map_err(|error| error.to_string())?;
    fern_core::instance_runtime(&paths, &instance_id).map_err(|error| format!("{error:#}"))
}

#[tauri::command]
fn update_instance_settings(
    instance_id: String,
    settings: fern_core::InstanceSettings,
) -> Result<fern_core::InstanceProfile, String> {
    let paths = fern_core::DataPaths::for_current_user().map_err(|error| error.to_string())?;
    fern_core::update_instance_settings(&paths, &instance_id, settings)
        .map_err(|error| format!("{error:#}"))
}

/// 外置登录。密码只在这一次调用里存在，登录成功后进系统钥匙串的是令牌。
#[tauri::command]
async fn yggdrasil_login(
    api_root: String,
    username: String,
    password: String,
) -> Result<fern_core::AccountView, String> {
    let client_token = fern_core::client_token().map_err(|error| format!("{error:#}"))?;
    let session = fern_core::authenticate(&api_root, &username, &password, &client_token)
        .await
        .map_err(|error| format!("{error:#}"))?;
    fern_core::store_session(&session).map_err(|error| format!("{error:#}"))?;
    // 只把界面用得着的那部分交出去，令牌留在这一侧。
    Ok(fern_core::AccountView::from(&session))
}

/// 当前登录的是谁。没登录过返回 null——那是正常状态，不是错误。
#[tauri::command]
fn yggdrasil_session() -> Result<Option<fern_core::AccountView>, String> {
    fern_core::load_session()
        .map(|session| session.as_ref().map(fern_core::AccountView::from))
        .map_err(|error| format!("{error:#}"))
}

#[tauri::command]
fn yggdrasil_logout() -> Result<(), String> {
    fern_core::clear_session().map_err(|error| format!("{error:#}"))
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
    fern_core::store_microsoft_session(&session).map_err(|error| format!("{error:#}"))?;
    Ok(fern_core::AccountView::from(&session))
}

#[tauri::command]
fn microsoft_session() -> Result<Option<fern_core::AccountView>, String> {
    fern_core::load_microsoft_session()
        .map(|session| session.as_ref().map(fern_core::AccountView::from))
        .map_err(|error| format!("{error:#}"))
}

#[tauri::command]
fn microsoft_logout() -> Result<(), String> {
    fern_core::clear_microsoft_session().map_err(|error| format!("{error:#}"))
}

/// 这台机器上的 Java。设置页要能看见 Fern 到底会用哪一个。
#[tauri::command]
fn list_java_runtimes() -> Result<Vec<fern_core::JavaRuntime>, String> {
    let paths = fern_core::DataPaths::for_current_user().map_err(|error| error.to_string())?;
    Ok(fern_core::discover_java(Some(&paths)))
}

/// 删掉一份 Fern 自己下载的运行时。核心那边会拒绝 `runtimes/` 以外的路径。
#[tauri::command]
fn remove_java_runtime(home: String) -> Result<(), String> {
    let paths = fern_core::DataPaths::for_current_user().map_err(|error| error.to_string())?;
    fern_core::remove_runtime(&paths, std::path::Path::new(&home))
        .map_err(|error| format!("{error:#}"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            #[cfg(not(target_os = "macos"))]
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_decorations(false);
            }
            Ok(())
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
            launch_instance
        ])
        .run(tauri::generate_context!())
        .expect("error while running Fern");
}
