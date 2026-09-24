//! Проверка доступности ИИ-сервисов после применения `hosts`.
//! Проба проверяет DNS, TCP, TLS-сертификат и HTTP-ответ без учётных данных.
//! Редиректы не выполняются; успешного TCP-соединения недостаточно.

use std::time::Duration;

use serde::Serialize;

/// Результат пробы одного сервиса (сериализуется во фронт — WS1.7).
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiProbeResult {
    pub host: String,
    pub ok: bool,
    /// Core-сервис влияет на решение об откате; опциональный — только отображается.
    pub core: bool,
    pub detail: String,
}

/// Цель пробы: хост + признак core.
pub struct ProbeTarget {
    pub host: &'static str,
    pub core: bool,
}

/// Курируемый набор AI-сервисов для проверки. Core — те, по которым судим об
/// успехе обхода (их провал ВСЕХ разом → откат). Держим маленьким и стабильным.
pub fn targets_for(_provider: &str) -> Vec<ProbeTarget> {
    vec![
        ProbeTarget {
            host: "chatgpt.com",
            core: true,
        },
        ProbeTarget {
            host: "claude.ai",
            core: true,
        },
        ProbeTarget {
            host: "gemini.google.com",
            core: false,
        },
    ]
}

/// Пробит один хост: HTTPS HEAD с системной проверкой сертификата, без cookies/токенов.
async fn probe_host(host: &str, core: bool, timeout_secs: u64) -> AiProbeResult {
    let mk = |ok: bool, detail: String| AiProbeResult {
        host: host.to_string(),
        ok,
        core,
        detail,
    };
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(c) => c,
        Err(e) => return mk(false, format!("client: {e}")),
    };
    match client.head(format!("https://{host}/")).send().await {
        // Любой HTTP-ответ = TLS-рукопожатие прошло = домен поднят.
        Ok(resp) => mk(true, format!("HTTP {}", resp.status().as_u16())),
        Err(e) => {
            let stage = if e.is_timeout() {
                "timeout"
            } else if e.is_connect() {
                "connect/TLS"
            } else {
                "request"
            };
            mk(false, format!("{stage}: {e}"))
        }
    }
}

/// Пробит все цели провайдера ПАРАЛЛЕЛЬНО (JoinSet). Порядок результатов не
/// гарантирован (сортировку — на стороне UI).
pub async fn probe_provider(provider: &str, timeout_secs: u64) -> Vec<AiProbeResult> {
    let mut set = tokio::task::JoinSet::new();
    for t in targets_for(provider) {
        set.spawn(async move { probe_host(t.host, t.core, timeout_secs).await });
    }
    let mut out = Vec::new();
    while let Some(res) = set.join_next().await {
        if let Ok(r) = res {
            out.push(r);
        }
    }
    out
}

/// Решение об откате: есть core-цели и ВСЕ они провалились. Один упавший
/// опциональный (или один из нескольких core) сервис откат НЕ вызывает —
/// защита от ложного отката при временной недоступности отдельного сервиса.
pub fn all_core_failed(results: &[AiProbeResult]) -> bool {
    let mut has_core = false;
    let mut any_core_ok = false;
    for r in results.iter().filter(|r| r.core) {
        has_core = true;
        if r.ok {
            any_core_ok = true;
        }
    }
    has_core && !any_core_ok
}

#[cfg(test)]
mod tests {
    use super::*;

    fn res(host: &str, ok: bool, core: bool) -> AiProbeResult {
        AiProbeResult {
            host: host.into(),
            ok,
            core,
            detail: String::new(),
        }
    }

    #[test]
    fn all_core_failed_only_when_every_core_down() {
        // Оба core упали → откат.
        assert!(all_core_failed(&[
            res("a", false, true),
            res("b", false, true)
        ]));
        // Один core жив → не откат.
        assert!(!all_core_failed(&[
            res("a", true, true),
            res("b", false, true)
        ]));
        // Упал только опциональный → не откат.
        assert!(!all_core_failed(&[
            res("a", true, true),
            res("opt", false, false)
        ]));
        // Нет core-целей вовсе → не откат (нечем судить).
        assert!(!all_core_failed(&[res("opt", false, false)]));
        assert!(!all_core_failed(&[]));
    }

    #[tokio::test]
    async fn plain_tcp_listener_fails_probe_no_tls() {
        // Мирроринг net::test — голый TCP без TLS не должен считаться успехом.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let peer = tokio::spawn(async move {
            let _ = listener.accept().await;
            tokio::time::sleep(Duration::from_millis(100)).await;
        });
        let host = format!("{}:{}", addr.ip(), addr.port());
        let r = probe_host(&host, true, 1).await;
        assert!(!r.ok, "голый TCP без TLS — не успех: {}", r.detail);
        let _ = peer.await;
    }
}
