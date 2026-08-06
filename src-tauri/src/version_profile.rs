// Parses Minecraft/Forge/NeoForge version JSON profiles (the files the
// loader installer drops into `versions/<id>/<id>.json`) and merges the
// `inheritsFrom` chain (modded profile -> vanilla profile), the same way
// real launchers (official launcher, MultiMC/PrismLauncher) do it.
//
// This is what makes the actual `java` invocation correct: modern
// Forge/NeoForge don't just add a classpath entry, they run through a
// bootstrap main-class with module-path / --add-opens JVM args that are
// only known from this JSON — hardcoding them would break on version
// bumps.

use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Clone)]
struct RawVersionJson {
    id: Option<String>,
    #[serde(rename = "inheritsFrom")]
    inherits_from: Option<String>,
    #[serde(rename = "mainClass")]
    main_class: Option<String>,
    arguments: Option<RawArguments>,
    #[serde(rename = "minecraftArguments")]
    minecraft_arguments: Option<String>,
    libraries: Option<Vec<RawLibrary>>,
    #[serde(rename = "assetIndex")]
    asset_index: Option<RawAssetIndex>,
    assets: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct RawAssetIndex {
    id: String,
}

#[derive(Debug, Deserialize, Clone)]
struct RawArguments {
    #[serde(default)]
    game: Vec<Value>,
    #[serde(default)]
    jvm: Vec<Value>,
}

#[derive(Debug, Deserialize, Clone)]
struct RawLibrary {
    name: String,
    #[serde(default)]
    downloads: Option<RawLibDownloads>,
    #[serde(default)]
    rules: Option<Vec<Value>>,
}

#[derive(Debug, Deserialize, Clone)]
struct RawLibDownloads {
    artifact: Option<RawArtifact>,
}

#[derive(Debug, Deserialize, Clone)]
struct RawArtifact {
    path: Option<String>,
}

pub struct MergedProfile {
    pub main_class: String,
    pub jvm_args: Vec<String>,
    pub game_args: Vec<String>,
    pub classpath_entries: Vec<PathBuf>,
    pub asset_index: String,
}

fn current_os_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    }
}

/// Evaluates a Mojang-style `rules` array (used by both libraries and
/// argument entries). We don't support optional "features" (custom
/// resolution, demo mode, quick play) — any rule requiring a feature is
/// treated as not applicable, matching how most non-official launchers
/// behave since those features aren't used here.
fn rules_allow(rules: &Option<Vec<Value>>) -> bool {
    let rules = match rules {
        None => return true,
        Some(r) if r.is_empty() => return true,
        Some(r) => r,
    };

    let mut allowed = false;
    for rule in rules {
        let action = rule.get("action").and_then(|v| v.as_str()).unwrap_or("allow");
        if rule.get("features").is_some() {
            // Skip feature-gated rules entirely (e.g. is_demo_user).
            continue;
        }
        let os_matches = match rule.get("os").and_then(|o| o.get("name")).and_then(|n| n.as_str()) {
            Some(name) => name == current_os_name(),
            None => true,
        };
        if os_matches {
            allowed = action == "allow";
        }
    }
    allowed
}

fn load_json(versions_dir: &Path, id: &str) -> Result<RawVersionJson> {
    let path = versions_dir.join(id).join(format!("{}.json", id));
    let text = std::fs::read_to_string(&path)
        .map_err(|e| anyhow!("Не найден профиль версии {}: {} ({})", id, path.display(), e))?;
    let parsed: RawVersionJson = serde_json::from_str(&text)
        .map_err(|e| anyhow!("Некорректный JSON профиля версии {}: {}", id, e))?;
    Ok(parsed)
}

/// Loads `id` and, if it inherits from another version, that parent too
/// (vanilla), returning [parent, child] in application order (parent
/// first, so the child's values take precedence when merged).
fn load_chain(versions_dir: &Path, id: &str) -> Result<Vec<RawVersionJson>> {
    let leaf = load_json(versions_dir, id)?;
    let mut chain = vec![leaf.clone()];
    if let Some(parent_id) = &leaf.inherits_from {
        let parent = load_json(versions_dir, parent_id)?;
        chain.insert(0, parent);
    }
    Ok(chain)
}

fn arg_value_to_strings(value: &Value) -> Vec<String> {
    match value {
        Value::String(s) => vec![s.clone()],
        Value::Array(arr) => arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect(),
        _ => vec![],
    }
}

fn maven_name_to_path(name: &str) -> Option<PathBuf> {
    // group:artifact:version[:classifier][@ext]
    let (coords, ext) = match name.split_once('@') {
        Some((c, e)) => (c, e),
        None => (name, "jar"),
    };
    let parts: Vec<&str> = coords.split(':').collect();
    if parts.len() < 3 {
        return None;
    }
    let group = parts[0].replace('.', "/");
    let artifact = parts[1];
    let version = parts[2];
    let classifier = parts.get(3);
    let file_name = match classifier {
        Some(c) => format!("{}-{}-{}.{}", artifact, version, c, ext),
        None => format!("{}-{}.{}", artifact, version, ext),
    };
    Some(PathBuf::from(group).join(artifact).join(version).join(file_name))
}

/// After the loader installer runs, it creates a new version folder whose
/// exact id (e.g. "neoforge-21.4.0-beta" or "1.21.1-neoforge-21.4.0-beta")
/// isn't standardized across loaders/versions. Instead of hardcoding a
/// naming pattern, we scan `versions/*` for a profile that inherits from
/// the target MC version — that's the modded profile we want to launch.
/// Falls back to the plain MC version (vanilla) if no such profile exists.
pub fn find_profile_id(dir: &Path, mc_version: &str) -> Result<String> {
    let versions_dir = dir.join("versions");
    if !versions_dir.exists() {
        return Err(anyhow!("Папка versions не найдена — ядро не установлено"));
    }

    let mut best: Option<String> = None;
    for entry in std::fs::read_dir(&versions_dir)? {
        let entry = entry?;
        if !entry.path().is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().to_string();
        if id == mc_version {
            continue; // that's the vanilla parent, keep looking for a child
        }
        if let Ok(json) = load_json(&versions_dir, &id) {
            if json.inherits_from.as_deref() == Some(mc_version) {
                best = Some(id);
                break;
            }
        }
    }

    if let Some(id) = best {
        return Ok(id);
    }

    // No modded profile found — fall back to vanilla if it exists.
    if versions_dir.join(mc_version).join(format!("{}.json", mc_version)).exists() {
        return Ok(mc_version.to_string());
    }

    Err(anyhow!(
        "Профиль версии для {} не найден в {}",
        mc_version,
        versions_dir.display()
    ))
}

/// ready-to-use launch profile: resolved classpath (in dependency order,
/// deduplicated by Maven group:artifact so the child/modded version wins
/// over the vanilla parent for shared libs), main class, and raw JVM/game
/// argument templates (still containing `${placeholders}` — substituted
/// separately once we know their values).
pub fn merge_profile(dir: &Path, profile_id: &str) -> Result<MergedProfile> {
    let versions_dir = dir.join("versions");
    let libraries_dir = dir.join("libraries");
    let chain = load_chain(&versions_dir, profile_id)?;

    let mut main_class = String::new();
    let mut jvm_args: Vec<String> = Vec::new();
    let mut game_args: Vec<String> = Vec::new();
    let mut asset_index = String::new();

    // group:artifact -> resolved path, insertion-ordered but later
    // entries (child profile) overwrite earlier ones (parent/vanilla).
    let mut libs: Vec<(String, PathBuf)> = Vec::new();
    let mut lib_index: HashMap<String, usize> = HashMap::new();

    for version in &chain {
        if let Some(mc) = &version.main_class {
            main_class = mc.clone();
        }
        if let Some(ai) = &version.asset_index {
            asset_index = ai.id.clone();
        } else if let Some(assets) = &version.assets {
            asset_index = assets.clone();
        }

        if let Some(args) = &version.arguments {
            for v in &args.jvm {
                if let Some(obj) = v.as_object() {
                    if !rules_allow(&obj.get("rules").cloned().map(|r| r.as_array().cloned().unwrap_or_default())) {
                        continue;
                    }
                    if let Some(val) = obj.get("value") {
                        jvm_args.extend(arg_value_to_strings(val));
                    }
                } else {
                    jvm_args.extend(arg_value_to_strings(v));
                }
            }
            for v in &args.game {
                if let Some(obj) = v.as_object() {
                    if !rules_allow(&obj.get("rules").cloned().map(|r| r.as_array().cloned().unwrap_or_default())) {
                        continue;
                    }
                    if let Some(val) = obj.get("value") {
                        game_args.extend(arg_value_to_strings(val));
                    }
                } else {
                    game_args.extend(arg_value_to_strings(v));
                }
            }
        } else if let Some(legacy) = &version.minecraft_arguments {
            game_args.extend(legacy.split_whitespace().map(|s| s.to_string()));
        }

        if let Some(libraries) = &version.libraries {
            for lib in libraries {
                if !rules_allow(&lib.rules) {
                    continue;
                }
                let name_parts: Vec<&str> = lib.name.split(':').collect();
                let key = if name_parts.len() >= 4 {
                    format!("{}:{}:{}", name_parts[0], name_parts[1], name_parts[3])
                } else if name_parts.len() >= 2 {
                    format!("{}:{}", name_parts[0], name_parts[1])
                } else {
                    lib.name.clone()
                };

                let resolved = lib
                    .downloads
                    .as_ref()
                    .and_then(|d| d.artifact.as_ref())
                    .and_then(|a| a.path.as_ref())
                    .map(|p| libraries_dir.join(p))
                    .or_else(|| maven_name_to_path(&lib.name).map(|p| libraries_dir.join(p)));

                if let Some(path) = resolved {
                    if let Some(&idx) = lib_index.get(&key) {
                        libs[idx] = (key, path);
                    } else {
                        lib_index.insert(key.clone(), libs.len());
                        libs.push((key, path));
                    }
                }
            }
        }
    }

    if main_class.is_empty() {
        return Err(anyhow!("В профиле версии {} не указан mainClass", profile_id));
    }

    let classpath_entries: Vec<PathBuf> = libs
        .into_iter()
        .map(|(_, p)| p)
        .filter(|p| p.exists())
        .collect();

    if asset_index.is_empty() {
        asset_index = profile_id.to_string();
    }

    Ok(MergedProfile {
        main_class,
        jvm_args,
        game_args,
        classpath_entries,
        asset_index,
    })
}
