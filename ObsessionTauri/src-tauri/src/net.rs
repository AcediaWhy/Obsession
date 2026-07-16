//! Проверка сетевой доступности (для авто-подбора DPI-конфигов).
//! Положительный результат требует реального TLS/SNI + HTTP response headers.

use std::time::Duration;

/// Проверяет доступность URL через HTTPS. Любой HTTP status (включая 403/429)
/// считается успехом, но один лишь TCP-connect больше не даёт false positive.
pub async fn test_url(url: &str, timeout_secs: u64) -> bool {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .build()
    {
        Ok(client) => client,
        Err(_) => return false,
    };
    client.get(url).send().await.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn plain_tcp_listener_is_not_a_successful_https_probe() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let peer = tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_millis(100)).await;
        });

        assert!(!test_url(&format!("https://{addr}/"), 1).await);
        let _ = peer.await;
    }
}
