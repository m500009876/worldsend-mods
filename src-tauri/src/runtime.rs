// Downloads and installs a matching Java runtime automatically (Eclipse
// Temurin / Adoptium — official, free, no license restrictions), so
// players never need to install Java themselves or worry about having
// the wrong version. Cached in the game directory after the first run.

use crate::config::JAVA_MIN_VERSION;
use crate::launcher::game_dir;
use crate::models::LaunchProgress;
use anyhow::{anyhow, Result};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter};

fn adoptium_os() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "mac"
    } else {
        "linux"
    }
}

fn adoptium_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "aarch64",
        "x86" => "x86-32",
        other => other,
    }
}

fn archive_ext() -> &'static str {
    if cfg!(target_os = "windows") {
        "zip"
    } else {
        "tar.gz"
    }
}

fn adoptium_download_url() -> String {
    format!(
        "https://api.adoptium.net/v3/binary/latest/{}/ga/{}/{}/jre/hotspot/normal/eclipse?project=jdk",
        JAVA_MIN_VERSION,
        adoptium_os(),
        adoptium_arch()
    )
}

fn runtime_root(dir: &Path) -> PathBuf {
    dir.join("runtime")
}

fn installed_marker(dir: &Path) -> PathBuf {
    runtime_root(dir).join(format!("jdk-{}", JAVA_MIN_VERSION))
}

fn java_exe_path(dir: &Path) -> PathBuf {
    let base = installed_marker(dir);
    if cfg!(target_os = "windows") {
        base.join("bin").join("javaw.exe")
    } else {
        base.join("bin").join("java")
    }
}

fn java_console_exe_path(dir: &Path) -> PathBuf {
    let base = installed_marker(dir);
    if cfg!(target_os = "windows") {
        base.join("bin").join("java.exe")
    } else {
        base.join("bin").join("java")
    }
}

fn emit(app: &AppHandle, progress: LaunchProgress) {
    let _ = app.emit("launch-progress", progress);
}

pub async fn ensure_java_installed(app: &AppHandle) -> Result<(PathBuf, PathBuf)> {
    let dir = game_dir()?;
    let windowed = java_exe_path(&dir);
    let console = java_console_exe_path(&dir);

    if windowed.exists() && console.exists() {
        return Ok((windowed, console));
    }

    emit(
        app,
        LaunchProgress::InstallingJava {
            message: format!("Загрузка Java {} (один раз, ~40-60 МБ)...", JAVA_MIN_VERSION),
        },
    );

    let root = runtime_root(&dir);
    fs::create_dir_all(&root)?;

    let url = adoptium_download_url();
    let client = reqwest::Client::builder()
        .user_agent("worldsend-launcher")
        .connect_timeout(std::time::Duration::from_secs(15))
        .build()?;
    let res = client.get(&url).send().await?;
    if !res.status().is_success() {
        return Err(anyhow!(
            "Не удалось скачать Java (HTTP {}). Проверьте интернет-соединение.",
            res.status()
        ));
    }
    let total = res.content_length();
    let bytes = {
        use futures_util::StreamExt;
        const STALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
        const EMIT_INTERVAL: std::time::Duration = std::time::Duration::from_millis(150);

        let mut stream = res.bytes_stream();
        let mut buf: Vec<u8> = Vec::new();
        let mut last_emit = tokio::time::Instant::now() - EMIT_INTERVAL;

        loop {
            let next = tokio::time::timeout(STALL_TIMEOUT, stream.next())
                .await
                .map_err(|_| {
                    anyhow!("Загрузка Java зависла (нет данных 30с) — проверьте интернет-соединение")
                })?;
            let Some(chunk) = next else { break };
            let chunk = chunk?;
            buf.extend_from_slice(&chunk);

            let now = tokio::time::Instant::now();
            if now.duration_since(last_emit) >= EMIT_INTERVAL {
                last_emit = now;
                emit(
                    app,
                    LaunchProgress::Downloading {
                        name: format!("Java {}", JAVA_MIN_VERSION),
                        downloaded: buf.len() as u64,
                        total,
                    },
                );
            }
        }
        emit(
            app,
            LaunchProgress::Downloading {
                name: format!("Java {}", JAVA_MIN_VERSION),
                downloaded: buf.len() as u64,
                total,
            },
        );
        buf
    };

    let extract_tmp = root.join("_extracting");
    if extract_tmp.exists() {
        let _ = fs::remove_dir_all(&extract_tmp);
    }
    fs::create_dir_all(&extract_tmp)?;

    if archive_ext() == "zip" {
        extract_zip(&bytes, &extract_tmp)?;
    } else {
        extract_tar_gz(&bytes, &extract_tmp)?;
    }

    let top_level = fs::read_dir(&extract_tmp)?
        .filter_map(|e| e.ok())
        .find(|e| e.path().is_dir())
        .ok_or_else(|| anyhow!("Архив Java пуст или имеет неожиданную структуру"))?
        .path();

    let target = installed_marker(&dir);
    if target.exists() {
        let _ = fs::remove_dir_all(&target);
    }
    fs::rename(&top_level, &target)?;
    let _ = fs::remove_dir_all(&extract_tmp);

    if !java_console_exe_path(&dir).exists() {
        return Err(anyhow!(
            "Java скачалась, но исполняемый файл не найден по ожидаемому пути. Попробуйте перезапустить лаунчер."
        ));
    }

    Ok((java_exe_path(&dir), java_console_exe_path(&dir)))
}

fn extract_zip(bytes: &[u8], dest: &Path) -> Result<()> {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor)
        .map_err(|e| anyhow!("Не удалось открыть архив Java: {}", e))?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let Some(rel_path) = entry.enclosed_name() else {
            continue;
        };
        let out_path = dest.join(rel_path);

        if entry.is_dir() {
            fs::create_dir_all(&out_path)?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out_file = fs::File::create(&out_path)?;
        std::io::copy(&mut entry, &mut out_file)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Some(mode) = entry.unix_mode() {
                let _ = fs::set_permissions(&out_path, fs::Permissions::from_mode(mode));
            }
        }
    }
    Ok(())
}

fn extract_tar_gz(bytes: &[u8], dest: &Path) -> Result<()> {
    let mut decoder = flate2::read::GzDecoder::new(bytes);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)?;
    let mut archive = tar::Archive::new(std::io::Cursor::new(decompressed));
    archive.unpack(dest)?;
    Ok(())
}
