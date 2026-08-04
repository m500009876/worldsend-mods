use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModEntry {
    #[serde(rename = "fileName")]
    pub file_name: String,
    #[serde(rename = "modId")]
    pub mod_id: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    pub version: String,
    #[serde(rename = "downloadUrl")]
    pub download_url: String,
    #[serde(default)]
    pub sha256: String,
    #[serde(rename = "fileSizeBytes", default)]
    pub file_size_bytes: u64,
    #[serde(default = "default_true")]
    pub required: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub version: String,
    #[serde(rename = "mcVersion")]
    pub mc_version: String,
    pub loader: String,
    #[serde(rename = "loaderVersion")]
    pub loader_version: String,
    #[serde(rename = "neoForgeUrl", default)]
    pub loader_installer_url: String,
    #[serde(rename = "publishedAt", default)]
    pub published_at: String,
    pub mods: Vec<ModEntry>,
    #[serde(rename = "overridesUrl", default)]
    pub overrides_url: Option<String>,
    #[serde(rename = "overridesSha256", default)]
    pub overrides_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewsItem {
    pub id: String,
    pub title: String,
    pub content: String,
    #[serde(default)]
    pub tag: String,
    #[serde(rename = "createdAt", default)]
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerStatus {
    pub online: bool,
    #[serde(default)]
    pub players: Option<PlayersInfo>,
    #[serde(default)]
    pub ping: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayersInfo {
    pub online: u32,
    pub max: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchSettings {
    #[serde(default = "default_nickname")]
    pub nickname: String,
    #[serde(default = "default_ram")]
    pub ram_gb: u32,
}

fn default_nickname() -> String {
    "Player".to_string()
}

fn default_ram() -> u32 {
    6
}

impl Default for LaunchSettings {
    fn default() -> Self {
        Self {
            nickname: default_nickname(),
            ram_gb: default_ram(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "stage", content = "data")]
pub enum LaunchProgress {
    Checking { message: String },
    InstallingJava { message: String },
    InstallingLoader { message: String },
    InstallingOverrides { message: String },
    SyncingMods { current: usize, total: usize, name: String },
    DeletingMod { name: String },
    Ready,
    Launching,
    Error { message: String },
}
