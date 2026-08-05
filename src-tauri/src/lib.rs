mod api;
mod config;
mod java;
mod launcher;
mod mc_ping;
mod models;
mod runtime;
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
    if java::find_java().is_some() {
        return true;
    }
    match launcher::game_dir() {
        Ok(dir) => dir
            .join("runtime")
            .join(format!("jdk-{}", config::JAVA_MIN_VERSION))
            .exists(),
        Err(_) => false,
    }
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

    let (java_windowed, java_console) = runtime::ensure_java_installed(&app)
        .await
        .map_err(|e| emit_err(&app, e.to_string()))?;

    launcher::ensure_loader_installed(&app, &manifest, &java_console)
        .await
        .map_err(|e| emit_err(&app, e.to_string()))?;

    launcher::ensure_overrides_installed(&app, &manifest)
        .await
        .map_err(|e| emit_err(&app, e.to_string()))?;

    launcher::sync_mods(&app, &manifest)
        .await
        .map_err(|e| emit_err(&app, e.to_string()))?;

    let _ = app.emit("launch-progress", LaunchProgress::Ready);

    let dir = launcher::game_dir().map_err(|e| emit_err(&app, e.to_string()))?;
    let mut child = launcher::launch_game(&dir, &manifest, &settings, &java_windowed)
        .map_err(|e| emit_err(&app, e.to_string()))?;

    // Give the JVM a few seconds — if it crashes immediately (bad
    // classpath, missing library, etc.) we can catch that and show the
    // real reason instead of silently sitting on "Идёт запуск...".
    tokio::time::sleep(std::time::Duration::from_secs(4)).await;
    if let Ok(Some(status)) = child.try_wait() {
        let log_path = launcher::launch_log_path(&dir).ok();
        let tail = log_path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|s| {
                s.chars()
                    .rev()
                    .take(1500)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<String>()
            })
            .unwrap_or_default();
        return Err(emit_err(
            &app,
            format!(
                "Minecraft закрылся сразу после запуска (код {:?}).\n{}",
                status.code(),
                tail.trim()
            ),
        ));
    }

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
