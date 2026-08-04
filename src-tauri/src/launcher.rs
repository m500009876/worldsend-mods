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

async fn download_to_file(client: &reqwest::Client, url: &str, dest: &Path) -> Result<()> {
    let res = client.get(url).send().await?;
    if !res.status().is_success() {
        return Err(anyhow!("HTTP {} при скачивании {}", res.status(), url));
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp_dest = dest.with_extension("part");
    let mut file = tokio::fs::File::create(&tmp_dest).await?;
    let mut stream = res.bytes_stream();
    use tokio::io::AsyncWriteExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
    }
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
            download_to_file(&client, &m.download_url, &dest).await.map_err(|e| {
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
        .build()?;
    let zip_path = dir.join("overrides.zip");
    download_to_file(&client, &url, &zip_path).await?;

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
        .build()?;
    let installer_path = dir.join("loader-installer.jar");
    download_to_file(&client, &manifest.loader_installer_url, &installer_path).await?;

    let output = Command::new(java_console)
        .arg("-jar")
        .arg(&installer_path)
        .arg("--installClient")
        .arg(&dir)
        .current_dir(&dir)
        .output()
        .map_err(|e| anyhow!("Не удалось запустить установщик ядра: {}", e))?;

    let _ = fs::remove_file(&installer_path);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let combined = format!("{stdout}\n{stderr}");
        let tail: String = combined
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

pub fn launch_game(
    dir: &Path,
    manifest: &Manifest,
    settings: &LaunchSettings,
    java: &Path,
) -> Result<()> {
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
        .current_dir(dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    cmd.spawn()
        .map_err(|e| anyhow!("Не удалось запустить Minecraft: {}", e))?;

    Ok(())
}
