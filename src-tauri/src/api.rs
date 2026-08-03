use crate::config::{manifest_url, news_url, SERVER_IP, SERVER_PORT};
use crate::mc_ping::ping_server;
use crate::models::{Manifest, NewsItem, PlayersInfo, ServerStatus};
use anyhow::{anyhow, Result};

pub async fn fetch_manifest() -> Result<Manifest> {
    let url = manifest_url();
    let res = reqwest::get(&url).await?;
    if !res.status().is_success() {
        return Err(anyhow!(
            "manifest.json не найден в репозитории ({}). Убедитесь, что репозиторий публичный и файл существует.",
            res.status()
        ));
    }
    let text = res.text().await?;
    serde_json::from_str::<Manifest>(&text)
        .map_err(|e| anyhow!("manifest.json повреждён или имеет неверный формат: {}", e))
}

// News is optional — if news.json doesn't exist in the repo, just show
// an empty list instead of erroring the whole app.
pub async fn fetch_news() -> Result<Vec<NewsItem>> {
    let url = news_url();
    let res = reqwest::get(&url).await?;
    if !res.status().is_success() {
        return Ok(vec![]);
    }
    let text = res.text().await?;
    Ok(serde_json::from_str::<Vec<NewsItem>>(&text).unwrap_or_default())
}

// Real Server List Ping — no backend involved.
pub async fn fetch_server_status() -> Result<ServerStatus> {
    let result = ping_server(SERVER_IP, SERVER_PORT).await;
    Ok(ServerStatus {
        online: result.online,
        players: match (result.players_online, result.players_max) {
            (Some(online), Some(max)) => Some(PlayersInfo { online, max }),
            _ => None,
        },
        ping: None,
    })
}
