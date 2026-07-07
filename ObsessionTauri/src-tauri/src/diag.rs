//! Диагностика «работает ли обход»: полноценный HTTPS-GET к известным
//! заблокированным ресурсам. Именно TLS-хендшейк с SNI режется DPI, поэтому
//! GET — честный сигнал (в отличие от TCP-connect на :443, который проходит и
//! без обхода). Проверки идут параллельно; каждая отдаёт статус и задержку.

use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::task::JoinSet;

#[derive(Serialize, Clone)]
pub struct DiagResult {
    pub name: String,
    pub url: String,
    pub ok: bool,
    pub ms: u64,
}

/// Набор целей: типично блокируемые в РФ ресурсы + Google как контроль.
const TARGETS: &[(&str, &str)] = &[
    ("Discord", "https://discord.com/"),
    ("YouTube", "https://www.youtube.com/"),
    ("Twitch", "https://www.twitch.tv/"),
    ("Instagram", "https://www.instagram.com/"),
    ("Google", "https://www.google.com/"),
];

pub async fn run() -> Vec<DiagResult> {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(6))
        .build()
    {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut set = JoinSet::new();
    for (name, url) in TARGETS {
        let client = client.clone();
        let name = name.to_string();
        let url = url.to_string();
        set.spawn(async move {
            let start = Instant::now();
            // Любой валидный HTTP-ответ (даже 4xx/429) = соединение прошло.
            // Ошибка (reset при TLS, таймаут) = ресурс недоступен.
            let ok = client
                .get(&url)
                .send()
                .await
                .map(|r| r.status().as_u16() < 500)
                .unwrap_or(false);
            DiagResult {
                name,
                url,
                ok,
                ms: start.elapsed().as_millis() as u64,
            }
        });
    }

    let mut out = Vec::new();
    while let Some(res) = set.join_next().await {
        if let Ok(r) = res {
            out.push(r);
        }
    }
    // Стабильный порядок для UI (как в TARGETS).
    out.sort_by_key(|r| {
        TARGETS
            .iter()
            .position(|(n, _)| *n == r.name)
            .unwrap_or(usize::MAX)
    });
    out
}
