// ── Static configuration ─────────────────────────────────────────────
//
// No backend website — the launcher reads everything it needs straight
// from a public GitHub repo. Only PUBLIC read endpoints are used
// (raw.githubusercontent.com + release asset downloads), so no token is
// ever embedded in the built .exe. Never put a write-scoped token here —
// anyone can extract strings from a compiled binary.
//
// Override at build time if needed:
//   WORLDSEND_GH_OWNER=m500009876 WORLDSEND_GH_REPO=worldsend-mods cargo tauri build
pub fn github_owner() -> String {
    option_env!("WORLDSEND_GH_OWNER").unwrap_or("m500009876").to_string()
}

pub fn github_repo() -> String {
    option_env!("WORLDSEND_GH_REPO").unwrap_or("worldsend-mods").to_string()
}

pub fn github_branch() -> String {
    option_env!("WORLDSEND_GH_BRANCH").unwrap_or("main").to_string()
}

/// Raw URL of manifest.json in the repo — this is what admins edit to
/// push updates to every installed launcher (version, loader, mods list).
pub fn manifest_url() -> String {
    format!(
        "https://raw.githubusercontent.com/{}/{}/{}/manifest.json",
        github_owner(),
        github_repo(),
        github_branch()
    )
}

/// Optional news.json in the same repo — if it doesn't exist, the News
/// tab just shows "no news" instead of erroring.
pub fn news_url() -> String {
    format!(
        "https://raw.githubusercontent.com/{}/{}/{}/news.json",
        github_owner(),
        github_repo(),
        github_branch()
    )
}

// The Minecraft server players connect to. Change here if it ever moves.
pub const SERVER_IP: &str = "185.219.84.148";
pub const SERVER_PORT: u16 = 30909;

pub const JAVA_MIN_VERSION: u32 = 21;
