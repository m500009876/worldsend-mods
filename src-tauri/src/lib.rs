mod api;
mod config;
mod java;
mod launcher;
mod mc_ping;
mod models;
mod version_profile;

use models::{LaunchProgress, LaunchSettings, Manifest, NewsItem, ServerStatus};
use tauri::Emitter;
use tauri_plugin_store::StoreExt;

const SETTINGS_STORE: &str = "settings.json";

#[tauri::command]
async fn get_manifest() -> Result<Manifest, String> {
    api::fetch_manifest().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_news() -> Result<Vec<NewsItem>, String> {
    api::fetch_news().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_server_status() -> Result<ServerStatus, String> {
    api::fetch_server_status().await.map_err(|e| e.to_string())
}

#[tauri::command]
fn check_java() -> bool {
    java::find_java().is_some()
}

#[tauri::command]
fn get_settings(app: tauri::AppHandle) -> Result<LaunchSettings, String> {
    let store = app.store(SETTINGS_STORE).map_err(|e| e.to_string())?;
    let settings = store
        .get("settings")
        .and_then(|v| serde_json::from_value::<LaunchSettings>(v).ok())
        .unwrap_or_default();
    Ok(settings)
}

#[tauri::command]
fn save_settings(app: tauri::AppHandle, settings: LaunchSettings) -> Result<(), String> {
    let store = app.store(SETTINGS_STORE).map_err(|e| e.to_string())?;
    store.set(
        "settings",
        serde_json::to_value(&settings).map_err(|e| e.to_string())?,
    );
    store.save().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn start_launch(app: tauri::AppHandle, settings: LaunchSettings) -> Result<(), String> {
    let emit_err = |app: &tauri::AppHandle, msg: String| {
        let _ = app.emit(
            "launch-progress",
            LaunchProgress::Error { message: msg.clone() },
        );
        msg
    };

    let _ = app.emit(
        "launch-progress",
        LaunchProgress::Checking {
            message: "Проверка сборки...".into(),
        },
    );
    let manifest = api::fetch_manifest()
        .await
        .map_err(|e| emit_err(&app, format!("Не удалось получить сборку: {}", e)))?;

    if java::find_java().is_none() {
        return Err(emit_err(
            &app,
            format!("Java не найдена. Установите Java {}+", config::JAVA_MIN_VERSION),
        ));
    }

    launcher::ensure_loader_installed(&app, &manifest)
        .await
        .map_err(|e| emit_err(&app, e.to_string()))?;

    launcher::sync_mods(&app, &manifest)
        .await
        .map_err(|e| emit_err(&app, e.to_string()))?;

    let _ = app.emit("launch-progress", LaunchProgress::Ready);

    let dir = launcher::game_dir().map_err(|e| emit_err(&app, e.to_string()))?;
    launcher::launch_game(&dir, &manifest, &settings)
        .map_err(|e| emit_err(&app, e.to_string()))?;

    let _ = app.emit("launch-progress", LaunchProgress::Launching);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            get_manifest,
            get_news,
            get_server_status,
            check_java,
            get_settings,
            save_settings,
            start_launch,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
