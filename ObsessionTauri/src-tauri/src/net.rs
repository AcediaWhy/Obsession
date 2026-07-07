//! Проверка сетевой доступности (для авто-подбора DPI-конфигов).
//! Порт из `network_tester.dart`.

use std::time::Duration;

use tokio::net::TcpStream;
use tokio::time::timeout;

/// Проверяет доступность хоста из URL.
/// Сначала быстрый TCP-connect на :443, затем fallback на HTTP GET.
/// Любой полученный ответ считается признаком работающего соединения.
pub async fn test_url(url: &str, timeout_secs: u64) -> bool {
    let dur = Duration::from_secs(timeout_secs);

    let host = match extract_host(url) {
        Some(h) => h,
        None => return false,
    };

    // 1. TCP-connect на :443 — работает даже если сервер отдаёт 403/429/капчу.
    if let Ok(Ok(stream)) = timeout(dur, TcpStream::connect((host.as_str(), 443))).await {
        drop(stream);
        return true;
    }

    // 2. Fallback: HTTP GET. Сертификаты валидируются штатно.
    let client = match reqwest::Client::builder().timeout(dur).build() {
        Ok(c) => c,
        Err(_) => return false,
    };
    client.get(url).send().await.is_ok()
}

fn extract_host(url: &str) -> Option<String> {
    let without_scheme = url.split("://").nth(1).unwrap_or(url);
    let host = without_scheme
        .split('/')
        .next()?
        .split(':')
        .next()?
        .to_string();
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}
