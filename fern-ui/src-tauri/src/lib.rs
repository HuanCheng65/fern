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
    drop(events);
    let _ = forwarder.await;
    result
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            app_name,
            data_paths,
            list_instances,
            list_versions,
            create_instance,
            offline_account,
            prepare_instance
        ])
        .run(tauri::generate_context!())
        .expect("error while running Fern");
}
use tauri::Emitter;
