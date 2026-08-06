#[tauri::command]
fn app_name() -> &'static str {
    "Fern"
}

#[tauri::command]
fn data_paths() -> Result<fern_core::DataPaths, String> {
    fern_core::DataPaths::for_current_user().map_err(|error| error.to_string())
}

#[tauri::command]
fn default_instances() -> Vec<fern_core::InstanceProfile> {
    vec![
        fern_core::InstanceProfile::vanilla(
            fern_core::InstanceId::parse("cinder-valley").expect("static instance id"),
            "余烬谷",
            "1.21.1",
        ),
        fern_core::InstanceProfile {
            schema_version: 1,
            id: fern_core::InstanceId::parse("moss-archive").expect("static instance id"),
            name: "苔痕档案".to_owned(),
            game_version: "1.20.4".to_owned(),
            loader: fern_core::LoaderKind::NeoForge,
            loader_profile: Some(fern_core::LoaderProfile {
                kind: fern_core::LoaderKind::NeoForge,
                version: "20.4.237".to_owned(),
            }),
            cover: fern_core::CoverSeed {
                identity: "moss-archive".to_owned(),
                growth: 21,
            },
            settings: fern_core::InstanceSettings::default(),
        },
    ]
}

#[tauri::command]
fn offline_account(player_name: String) -> fern_core::Credentials {
    fern_core::offline_credentials(player_name)
}

#[tauri::command]
async fn prepare_instance(
    app: tauri::AppHandle,
    version_id: String,
) -> Result<fern_core::PrepareResult, String> {
    let paths = fern_core::DataPaths::for_current_user().map_err(|error| error.to_string())?;
    let (events, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let event_app = app.clone();
    let forwarder = tauri::async_runtime::spawn(async move {
        while let Some(event) = receiver.recv().await {
            let _ = event_app.emit("download-event", event);
        }
    });
    let result = fern_core::prepare_instance(&paths, &version_id, &events)
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
            default_instances,
            offline_account,
            prepare_instance
        ])
        .run(tauri::generate_context!())
        .expect("error while running Fern");
}
use tauri::Emitter;
