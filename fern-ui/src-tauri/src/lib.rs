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

#[tauri::command]
fn create_instance(name: String, game_version: String) -> Result<fern_core::InstanceProfile, String> {
    let paths = fern_core::DataPaths::for_current_user().map_err(|error| error.to_string())?;
    fern_core::create_instance(&paths, &name, &game_version).map_err(|error| format!("{error:#}"))
}

#[tauri::command]
fn offline_account(player_name: String) -> fern_core::Credentials {
    fern_core::offline_credentials(player_name)
}

#[tauri::command]
fn open_instance_directory(instance_id: String) -> Result<(), String> {
    let id = fern_core::InstanceId::parse(instance_id).map_err(|error| error.to_string())?;
    let paths = fern_core::DataPaths::for_current_user().map_err(|error| error.to_string())?;
    let directory = paths.game_directory(id.as_str());
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

#[tauri::command]
async fn prepare_instance(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<fern_core::PrepareResult, String> {
    let paths = fern_core::DataPaths::for_current_user().map_err(|error| error.to_string())?;
    let (events, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let event_app = app.clone();
    let forwarder = tauri::async_runtime::spawn(async move {
        while let Some(event) = receiver.recv().await {
            let _ = event_app.emit("download-event", event);
        }
    });
    let result = fern_core::prepare_instance(&paths, &instance_id, &events)
        .await
        .map_err(|error| format!("{error:#}"));
    if let Err(error) = &result {
        let _ = paths.append_log(&format!("[prepare] instance={instance_id} error={error}"));
    }
    drop(events);
    let _ = forwarder.await;
    result
}

#[tauri::command]
async fn launch_instance(
    app: tauri::AppHandle,
    instance_id: String,
    player_name: String,
) -> Result<fern_core::LaunchResult, String> {
    let paths = fern_core::DataPaths::for_current_user().map_err(|error| error.to_string())?;
    let (events, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let event_app = app.clone();
    let forwarder = tauri::async_runtime::spawn(async move {
        while let Some(event) = receiver.recv().await {
            let _ = event_app.emit("download-event", event);
        }
    });
    let prepared = fern_core::prepare_instance(&paths, &instance_id, &events)
        .await
        .map_err(|error| format!("{error:#}"));
    let result = match prepared {
        Ok(_) => fern_core::launch_instance(&paths, &instance_id, &player_name)
            .await
            .map_err(|error| format!("{error:#}")),
        Err(error) => Err(error),
    };
    if let Err(error) = &result {
        let _ = paths.append_log(&format!("[launch] instance={instance_id} error={error}"));
    }
    drop(events);
    let _ = forwarder.await;
    result
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
            offline_account,
            open_instance_directory,
            open_logs_directory,
            prepare_instance,
            launch_instance
        ])
        .run(tauri::generate_context!())
        .expect("error while running Fern");
}
