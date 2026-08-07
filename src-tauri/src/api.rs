use crate::config::{manifest_url, news_url, SERVER_IP, SERVER_PORT};
use crate::mc_ping::ping_server;
use crate::models::{Manifest, NewsItem, PlayersInfo, ServerStatus};
use anyhow::{anyhow, Result};
use std::time::Duration;

// reqwest::get() uses a default client with NO timeout at all — if
// raw.githubusercontent.com is blocked/throttled for a player's ISP (this
// happens a lot), the request just hangs forever with no error, and the
// launcher UI is stuck on "Идёт запуск..." indefinitely. Every request
// here gets an explicit connect + total timeout so it fails loudly
// instead of hanging silently.
fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent("worldsend-launcher")
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| anyhow!("Не удалось создать HTTP-клиент: {}", e))
}

fn friendly_network_error(e: reqwest::Error, url: &str) -> anyhow::Error {
    if e.is_timeout() {
        anyhow!(
            "Не удалось подключиться к {} — сервер не отвечает (таймаут). \
             Возможно, соединение блокируется провайдером или VPN/антивирус мешает подключению. \
             Попробуйте включить VPN и повторить попытку.",
            url
        )
    } else if e.is_connect() {
        anyhow!(
            "Не удалось подключиться к {} — проверьте интернет-соединение или попробуйте VPN.",
            url
        )
    } else {
        anyhow!("Ошибка сети при обращении к {}: {}", url, e)
    }
}

pub async fn fetch_manifest() -> Result<Manifest> {
    let url = manifest_url();
    let client = http_client()?;
    let res = client
        .get(&url)
        .send()
        .await
        .map_err(|e| friendly_network_error(e, &url))?;
    if !res.status().is_success() {
        return Err(anyhow!(
            "manifest.json не найден в репозитории ({}). Убедитесь, что репозиторий публичный и файл существует.",
            res.status()
        ));
    }
    let text = res
        .text()
        .await
        .map_err(|e| friendly_network_error(e, &url))?;
    serde_json::from_str::<Manifest>(&text)
        .map_err(|e| anyhow!("manifest.json повреждён или имеет неверный формат: {}", e))
}

// News is optional — if news.json doesn't exist in the repo, or the
// request fails/times out, just show an empty list instead of erroring
// the whole app (news must never be able to block the Play button).
pub async fn fetch_news() -> Result<Vec<NewsItem>> {
    let url = news_url();
    let client = http_client()?;
    let res = match client.get(&url).send().await {
        Ok(res) => res,
        Err(_) => return Ok(vec![]),
    };
    if !res.status().is_success() {
        return Ok(vec![]);
    }
    let text = res.text().await.unwrap_or_default();
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
