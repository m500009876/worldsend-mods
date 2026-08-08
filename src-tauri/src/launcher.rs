use crate::config::{SERVER_IP, SERVER_PORT};
use crate::models::{LaunchProgress, LaunchSettings, Manifest};
use anyhow::{anyhow, Result};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tauri::{AppHandle, Emitter};
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct VanillaArtifact {
    path: String,
    url: String,
}

#[derive(Deserialize)]
struct VanillaLibDownloads {
    artifact: Option<VanillaArtifact>,
}

#[derive(Deserialize)]
struct VanillaLibrary {
    name: String,
    downloads: Option<VanillaLibDownloads>,
    #[serde(default)]
    rules: Option<Vec<Value>>,
}

#[derive(Deserialize)]
struct VanillaAssetIndexRef {
    id: String,
    url: String,
}

#[derive(Deserialize)]
struct VanillaVersionJson {
    #[serde(default)]
    libraries: Vec<VanillaLibrary>,
    #[serde(rename = "assetIndex")]
    asset_index: Option<VanillaAssetIndexRef>,
}

#[derive(Deserialize)]
struct AssetObject {
    hash: String,
    #[allow(dead_code)]
    size: u64,
}

#[derive(Deserialize)]
struct AssetIndexJson {
    objects: HashMap<String, AssetObject>,
}

pub fn game_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow!("Не удалось определить домашнюю папку"))?;
    let dir = home.join(".worldsend");
    fs::create_dir_all(&dir)?;
    fs::create_dir_all(dir.join("mods"))?;
    Ok(dir)
}

fn emit(app: &AppHandle, progress: LaunchProgress) {
    let _ = app.emit("launch-progress", progress);
}

async fn download_to_file(
    app: &AppHandle,
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    label: &str,
) -> Result<()> {
    const STALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
    const EMIT_INTERVAL: std::time::Duration = std::time::Duration::from_millis(150);

    let res = client
        .get(url)
        .send()
        .await
        .map_err(|e| anyhow!("Не удалось подключиться к {}: {}", url, e))?;
    if !res.status().is_success() {
        return Err(anyhow!("HTTP {} при скачивании {}", res.status(), url));
    }
    let total = res.content_length();
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp_dest = dest.with_extension("part");
    let mut file = tokio::fs::File::create(&tmp_dest).await?;
    let mut stream = res.bytes_stream();
    use tokio::io::AsyncWriteExt;

    let mut downloaded: u64 = 0;
    let mut last_emit = tokio::time::Instant::now() - EMIT_INTERVAL;

    loop {
        let next = tokio::time::timeout(STALL_TIMEOUT, stream.next())
            .await
            .map_err(|_| {
                anyhow!(
                    "Скачивание зависло (нет данных {}с) — проверьте интернет-соединение и попробуйте снова",
                    STALL_TIMEOUT.as_secs()
                )
            })?;
        let Some(chunk) = next else { break };
        let chunk = chunk?;
        downloaded += chunk.len() as u64;
        file.write_all(&chunk).await?;

        let now = tokio::time::Instant::now();
        if now.duration_since(last_emit) >= EMIT_INTERVAL {
            last_emit = now;
            emit(
                app,
                LaunchProgress::Downloading {
                    name: label.to_string(),
                    downloaded,
                    total,
                },
            );
        }
    }

    emit(
        app,
        LaunchProgress::Downloading {
            name: label.to_string(),
            downloaded,
            total,
        },
    );

    file.flush().await?;
    drop(file);
    tokio::fs::rename(&tmp_dest, dest).await?;
    Ok(())
}

fn sha256_of_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(hex::encode(hasher.finalize()))
}

pub async fn sync_mods(app: &AppHandle, manifest: &Manifest) -> Result<()> {
    let dir = game_dir()?;
    let mods_dir = dir.join("mods");
    fs::create_dir_all(&mods_dir)?;

    let mut local: HashMap<String, PathBuf> = HashMap::new();
    for entry in fs::read_dir(&mods_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("jar") {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                local.insert(name.to_string(), path.clone());
            }
        }
    }

    let server_names: std::collections::HashSet<&str> =
        manifest.mods.iter().map(|m| m.file_name.as_str()).collect();

    for (name, path) in local.iter() {
        if !server_names.contains(name.as_str()) {
            emit(
                app,
                LaunchProgress::DeletingMod {
                    name: name.clone(),
                },
            );
            let _ = fs::remove_file(path);
        }
    }

    let client = reqwest::Client::builder()
        .user_agent("worldsend-launcher")
        .connect_timeout(std::time::Duration::from_secs(15))
        .build()?;

    let total = manifest.mods.len();
    for (i, m) in manifest.mods.iter().enumerate() {
        let dest = mods_dir.join(&m.file_name);
        let needs_download = match local.get(&m.file_name) {
            None => true,
            Some(existing_path) => {
                if m.sha256.is_empty() {
                    false
                } else {
                    match sha256_of_file(existing_path) {
                        Ok(hash) => !hash.eq_ignore_ascii_case(&m.sha256),
                        Err(_) => true,
                    }
                }
            }
        };

        emit(
            app,
            LaunchProgress::SyncingMods {
                current: i + 1,
                total,
                name: m.display_name.clone(),
            },
        );

        if needs_download {
            download_to_file(app, &client, &m.download_url, &dest, &m.display_name).await.map_err(|e| {
                anyhow!("Не удалось скачать мод {}: {}", m.display_name, e)
            })?;
        }
    }

    Ok(())
}

pub async fn ensure_overrides_installed(app: &AppHandle, manifest: &Manifest) -> Result<()> {
    let (url, expected_sha) = match (&manifest.overrides_url, &manifest.overrides_sha256) {
        (Some(u), Some(s)) if !u.trim().is_empty() => (u.clone(), s.clone()),
        _ => return Ok(()),
    };

    let dir = game_dir()?;
    let marker = dir.join(format!(".overrides-{}", &expected_sha[..expected_sha.len().min(16)]));
    if marker.exists() {
        return Ok(());
    }

    emit(
        app,
        LaunchProgress::InstallingOverrides {
            message: "Установка дополнительных файлов сборки...".into(),
        },
    );

    let client = reqwest::Client::builder()
        .user_agent("worldsend-launcher")
        .connect_timeout(std::time::Duration::from_secs(15))
        .build()?;
    let zip_path = dir.join("overrides.zip");
    download_to_file(app, &client, &url, &zip_path, "Дополнительные файлы сборки").await?;

    let actual_sha = sha256_of_file(&zip_path)?;
    if !actual_sha.eq_ignore_ascii_case(&expected_sha) {
        let _ = fs::remove_file(&zip_path);
        return Err(anyhow!(
            "Контрольная сумма overrides.zip не совпадает — файл повреждён или подменён"
        ));
    }

    let file = std::fs::File::open(&zip_path)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| anyhow!("overrides.zip повреждён: {}", e))?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let Some(rel_path) = entry.enclosed_name() else {
            continue;
        };
        let out_path = dir.join(rel_path);

        if entry.is_dir() {
            fs::create_dir_all(&out_path)?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out_file = fs::File::create(&out_path)?;
        std::io::copy(&mut entry, &mut out_file)?;
    }

    let _ = fs::remove_file(&zip_path);

    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(".overrides-") {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    fs::write(&marker, b"ok")?;

    Ok(())
}

/// The NeoForge/Forge installer's --installClient only fetches the
/// client.jar and NeoForge's own libraries — it does NOT walk the vanilla
/// version JSON's own "libraries" list (LWJGL, JOML, oshi, natives, ...).
/// A real launcher (like the official one) is expected to do that part
/// itself, so we do it here explicitly. Safe to call every launch — it
/// skips anything already on disk, so it's a no-op after the first run
/// (and self-heals if a previous run left something missing, e.g. from
/// a firewall hiccup).
pub async fn ensure_vanilla_libraries_installed(
    app: &AppHandle,
    dir: &Path,
    mc_version: &str,
) -> Result<()> {
    let json_path = dir
        .join("versions")
        .join(mc_version)
        .join(format!("{}.json", mc_version));
    if !json_path.exists() {
        return Ok(());
    }

    let text = fs::read_to_string(&json_path)?;
    let parsed: VanillaVersionJson = serde_json::from_str(&text)
        .map_err(|e| anyhow!("Не удалось разобрать {}: {}", json_path.display(), e))?;

    let client = reqwest::Client::builder()
        .user_agent("worldsend-launcher")
        .connect_timeout(std::time::Duration::from_secs(15))
        .build()?;
    let libraries_dir = dir.join("libraries");

    let total = parsed.libraries.len();
    for (i, lib) in parsed.libraries.iter().enumerate() {
        if !crate::version_profile::rules_allow(&lib.rules) {
            continue;
        }
        let Some(artifact) = lib.downloads.as_ref().and_then(|d| d.artifact.as_ref()) else {
            continue;
        };
        let dest = libraries_dir.join(&artifact.path);
        if dest.exists() {
            continue;
        }

        emit(
            app,
            LaunchProgress::Downloading {
                name: format!("Библиотеки Minecraft ({}/{})", i + 1, total),
                downloaded: 0,
                total: None,
            },
        );

        download_to_file(app, &client, &artifact.url, &dest, &lib.name)
            .await
            .map_err(|e| anyhow!("Не удалось скачать библиотеку {}: {}", lib.name, e))?;
    }

    Ok(())
}

/// Downloads the Minecraft asset index + every referenced asset object
/// (textures, sounds, lang files, ...) into `assets/indexes` and
/// `assets/objects`. Without this, `--assetsDir` points at a directory
/// that either doesn't exist or is empty, and the game fails to boot
/// immediately with "Directory [...assets] does not exist" (jopt-simple
/// validates the path before Minecraft's own code even runs) or crashes
/// shortly after opening a window trying to load missing resources.
/// Safe to call every launch — skips anything already on disk.
pub async fn ensure_assets_installed(
    app: &AppHandle,
    dir: &Path,
    mc_version: &str,
) -> Result<()> {
    let assets_dir = dir.join("assets");
    fs::create_dir_all(&assets_dir)?;
    fs::create_dir_all(assets_dir.join("indexes"))?;
    fs::create_dir_all(assets_dir.join("objects"))?;

    let json_path = dir
        .join("versions")
        .join(mc_version)
        .join(format!("{}.json", mc_version));
    if !json_path.exists() {
        return Ok(());
    }
    let text = fs::read_to_string(&json_path)?;
    let parsed: VanillaVersionJson = serde_json::from_str(&text)
        .map_err(|e| anyhow!("Не удалось разобрать {}: {}", json_path.display(), e))?;
    let Some(asset_index_ref) = parsed.asset_index else {
        return Ok(());
    };

    let client = reqwest::Client::builder()
        .user_agent("worldsend-launcher")
        .connect_timeout(std::time::Duration::from_secs(15))
        .build()?;

    let index_path = assets_dir
        .join("indexes")
        .join(format!("{}.json", asset_index_ref.id));
    if !index_path.exists() {
        emit(
            app,
            LaunchProgress::Downloading {
                name: "Индекс ассетов".into(),
                downloaded: 0,
                total: None,
            },
        );
        download_to_file(app, &client, &asset_index_ref.url, &index_path, "Индекс ассетов").await?;
    }

    let index_text = fs::read_to_string(&index_path)?;
    let index: AssetIndexJson = serde_json::from_str(&index_text)
        .map_err(|e| anyhow!("Не удалось разобрать индекс ассетов: {}", e))?;

    let objects_dir = assets_dir.join("objects");
    let total = index.objects.len();
    let mut done = 0usize;
    for (name, obj) in index.objects.iter() {
        done += 1;
        let hash = &obj.hash;
        if hash.len() < 2 {
            continue;
        }
        let dest = objects_dir.join(&hash[0..2]).join(hash);
        if dest.exists() {
            continue;
        }
        if done % 25 == 0 || done == total {
            emit(
                app,
                LaunchProgress::Downloading {
                    name: format!("Ассеты игры ({}/{})", done, total),
                    downloaded: 0,
                    total: None,
                },
            );
        }
        let url = format!(
            "https://resources.download.minecraft.net/{}/{}",
            &hash[0..2],
            hash
        );
        download_to_file(app, &client, &url, &dest, name)
            .await
            .map_err(|e| anyhow!("Не удалось скачать ассет {}: {}", name, e))?;
    }

    Ok(())
}

pub async fn ensure_loader_installed(
    app: &AppHandle,
    manifest: &Manifest,
    java_console: &Path,
) -> Result<()> {
    let dir = game_dir()?;
    let marker = dir.join(format!(
        ".loader-{}-{}-{}",
        manifest.loader, manifest.loader_version, manifest.mc_version
    ));
    if marker.exists() {
        return Ok(());
    }

    let profiles_path = dir.join("launcher_profiles.json");
    if !profiles_path.exists() {
        fs::write(
            &profiles_path,
            br#"{"profiles":{},"selectedProfile":"","clientToken":"","authenticationDatabase":{},"settings":{},"version":3}"#,
        )?;
    }

    if manifest.loader_installer_url.trim().is_empty() {
        fs::write(&marker, b"manual")?;
        return Ok(());
    }

    emit(
        app,
        LaunchProgress::InstallingLoader {
            message: format!(
                "Установка {} {}...",
                manifest.loader, manifest.loader_version
            ),
        },
    );

    let client = reqwest::Client::builder()
        .user_agent("worldsend-launcher")
        .connect_timeout(std::time::Duration::from_secs(15))
        .build()?;
    let installer_path = dir.join("loader-installer.jar");
    download_to_file(
        app,
        &client,
        &manifest.loader_installer_url,
        &installer_path,
        &format!("Установщик {}", manifest.loader),
    )
    .await?;

    let output = Command::new(java_console)
        .arg("-jar")
        .arg(&installer_path)
        .arg("--installClient")
        .arg(&dir)
        .current_dir(&dir)
        .output()
        .map_err(|e| anyhow!("Не удалось запустить установщик ядра: {}", e))?;

    let _ = fs::remove_file(&installer_path);

    let installer_log_path = dir.join("logs").join("installer-latest.log");
    if let Some(parent) = installer_log_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let full_log = format!(
        "=== stdout ===\n{}\n=== stderr ===\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = fs::write(&installer_log_path, &full_log);

    if !output.status.success() {
        let tail: String = full_log
            .chars()
            .rev()
            .take(600)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        return Err(anyhow!(
            "Установщик {} завершился с ошибкой (код {:?}).\n{}",
            manifest.loader,
            output.status.code(),
            tail.trim()
        ));
    }

    fs::write(&marker, b"ok")?;
    Ok(())
}

fn substitute(template: &str, vars: &HashMap<String, String>) -> String {
    let mut out = template.to_string();
    for (k, v) in vars {
        out = out.replace(&format!("${{{}}}", k), v);
    }
    out
}

pub fn launch_log_path(dir: &Path) -> Result<PathBuf> {
    let logs_dir = dir.join("logs");
    fs::create_dir_all(&logs_dir)?;
    Ok(logs_dir.join("launcher-latest.log"))
}

pub fn launch_game(
    dir: &Path,
    manifest: &Manifest,
    settings: &LaunchSettings,
    java: &Path,
) -> Result<std::process::Child> {
    let profile_id = crate::version_profile::find_profile_id(dir, &manifest.mc_version)?;
    let profile = crate::version_profile::merge_profile(dir, &profile_id)?;

    let ram_mb = settings.ram_gb * 1024;
    let ram_min = (ram_mb / 2).max(1024);
    let classpath_sep = if cfg!(target_os = "windows") { ";" } else { ":" };
    let classpath = profile
        .classpath_entries
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(classpath_sep);

    let assets_dir = dir.join("assets");
    let _ = fs::create_dir_all(&assets_dir);
    let natives_dir = dir.join("versions").join(&profile_id).join("natives");
    let _ = fs::create_dir_all(&natives_dir);

    let mut vars: HashMap<String, String> = HashMap::new();
    vars.insert("auth_player_name".into(), settings.nickname.clone());
    vars.insert("version_name".into(), profile_id.clone());
    vars.insert("game_directory".into(), dir.display().to_string());
    vars.insert("assets_root".into(), assets_dir.display().to_string());
    vars.insert("game_assets".into(), assets_dir.display().to_string());
    vars.insert("assets_index_name".into(), profile.asset_index.clone());
    vars.insert(
        "auth_uuid".into(),
        "00000000-0000-0000-0000-000000000000".into(),
    );
    vars.insert("auth_access_token".into(), "0".into());
    vars.insert("auth_xuid".into(), "0".into());
    vars.insert("user_type".into(), "legacy".into());
    vars.insert("user_properties".into(), "{}".into());
    vars.insert(
        "version_type".into(),
        format!("WorldsEnd v{}", manifest.version),
    );
    vars.insert("natives_directory".into(), natives_dir.display().to_string());
    vars.insert("launcher_name".into(), "worldsend-launcher".into());
    vars.insert("launcher_version".into(), "1.0.0".into());
    vars.insert("classpath".into(), classpath.clone());
    vars.insert("library_directory".into(), dir.join("libraries").display().to_string());
    vars.insert("classpath_separator".into(), classpath_sep.into());

    let mut cmd = Command::new(java);
    cmd.arg(format!("-Xmx{}M", ram_mb))
        .arg(format!("-Xms{}M", ram_min))
        .arg("-XX:+UseG1GC")
        .arg("-XX:+ParallelRefProcEnabled")
        .arg("-XX:MaxGCPauseMillis=200")
        .arg("-XX:+UnlockExperimentalVMOptions")
        .arg(format!("-Dminecraft.applet.TargetDirectory={}", dir.display()))
        .arg("-Duser.language=ru")
        .arg("-Duser.country=RU");

    if profile.jvm_args.is_empty() {
        cmd.arg("-cp").arg(&classpath);
    } else {
        for arg in &profile.jvm_args {
            cmd.arg(substitute(arg, &vars));
        }
    }

    cmd.arg(&profile.main_class);

    if profile.game_args.is_empty() {
        cmd.arg("--username")
            .arg(&settings.nickname)
            .arg("--version")
            .arg(&profile_id)
            .arg("--gameDir")
            .arg(dir)
            .arg("--assetsDir")
            .arg(&assets_dir)
            .arg("--assetIndex")
            .arg(&profile.asset_index)
            .arg("--uuid")
            .arg("00000000-0000-0000-0000-000000000000")
            .arg("--accessToken")
            .arg("0")
            .arg("--userType")
            .arg("legacy")
            .arg("--versionType")
            .arg(format!("WorldsEnd v{}", manifest.version));
    } else {
        for arg in &profile.game_args {
            cmd.arg(substitute(arg, &vars));
        }
    }

    cmd.arg("--server")
        .arg(SERVER_IP)
        .arg("--port")
        .arg(SERVER_PORT.to_string())
        .current_dir(dir);

    let log_path = launch_log_path(dir)?;
    let log_out = fs::File::create(&log_path)
        .map_err(|e| anyhow!("Не удалось создать файл лога: {}", e))?;
    let log_err = log_out
        .try_clone()
        .map_err(|e| anyhow!("Не удалось создать файл лога: {}", e))?;
    cmd.stdout(Stdio::from(log_out)).stderr(Stdio::from(log_err));

    let child = cmd
        .spawn()
        .map_err(|e| anyhow!("Не удалось запустить Minecraft: {}", e))?;

    Ok(child)
}
