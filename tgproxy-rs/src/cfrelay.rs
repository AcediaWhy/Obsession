//! Фолбэк через публичные Cloudflare-relay домены (порт `_cfproxy_fallback`
//! + Obsession-патч дискового кэша last-good доменов).
//!
//! Отличия от апстрима: список доменов встроен открытым текстом — без
//! Цезарь-обфускации; кэш `--cfproxy-cache` хранит порядок последних
//! рабочих доменов и обновляется при успешном соединении.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio_rustls::TlsConnector;

use crate::dns;
use crate::logger;
use crate::ws;

/// Публичные relay-домена апстрима (декодированы из
/// `.github/cfproxy-domains.txt`, релиз 1.10.0).
pub const CFPROXY_DEFAULT_DOMAINS: [&str; 20] = [
    "pclead.co.uk",
    "offshor.co.uk",
    "cakeisalie.co.uk",
    "noskomnadzor.co.uk",
    "lovetrue.co.uk",
    "sorokdva.co.uk",
    "pyatdesyatdva.co.uk",
    "kartoshka.co.uk",
    "sorokodin.co.uk",
    "pyatdesyatodin.co.uk",
    "notelega.co.uk",
    "ebally.co.uk",
    "nebally.co.uk",
    "havegreatday.co.uk",
    "pomogite.co.uk",
    "fixtelega.co.uk",
    "sadnews.co.uk",
    "onedaychamp.co.uk",
    "stopblocking.co.uk",
    "nothingthere.co.uk",
];

const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Состояние фолбэка: порядок доменов (кэш вперёд) и путь кэша.
pub struct CfRelay {
    domains: Vec<String>,
    cache_path: Option<PathBuf>,
}

impl CfRelay {
    /// Домены из кэша (если файл читается) идут первыми, затем встроенные.
    pub fn new(cache_path: Option<&Path>) -> Self {
        let mut domains = Vec::new();
        if let Some(path) = cache_path {
            if let Some(cached) = load_cached_domains(path) {
                domains.extend(cached);
            }
        }
        for domain in CFPROXY_DEFAULT_DOMAINS {
            if !domains.iter().any(|existing| existing == domain) {
                domains.push(domain.to_string());
            }
        }
        CfRelay {
            domains,
            cache_path: cache_path.map(Path::to_path_buf),
        }
    }

    /// Порядок попыток: начинаем со смещения по номеру DC, чтобы разные
    /// клиенты расходились по разным relay (упрощённый balancer апстрима).
    fn attempt_order(&self, dc: u16) -> Vec<String> {
        let len = self.domains.len();
        let start = dc as usize % len;
        self.domains
            .iter()
            .cycle()
            .skip(start)
            .take(len)
            .cloned()
            .collect()
    }

    /// Подключение к `wss://kws{dc}.{relay}/apiws` (для media-DC —
    /// `kws{dc}-1`; DC203 мапится на kws2, как в прямом пути).
    /// Первый успешный домен поднимается в кэш.
    pub async fn connect(
        &self,
        dc: u16,
        is_media: bool,
        connector: &TlsConnector,
        peer: &str,
    ) -> Option<(ws::WsReader, Arc<ws::WsWriter>, String)> {
        let sub_dc = if dc == 203 { 2 } else { dc };
        let subdomain = if is_media {
            format!("kws{sub_dc}-1")
        } else {
            format!("kws{sub_dc}")
        };
        for base in self.attempt_order(dc) {
            let host = format!("{subdomain}.{base}");
            let media_tag = if is_media { " media" } else { "" };
            let ip = match dns::resolve(&host).await {
                Ok(ip) => ip,
                Err(error) => {
                    logger::warn(format!(
                        "[{peer}] DC{dc}{media_tag} relay {host}: dns: {error}"
                    ));
                    continue;
                }
            };
            logger::info(format!(
                "[{peer}] DC{dc}{media_tag} -> wss://{host}/apiws via {ip} (cf relay)"
            ));
            match ws::connect(&ip.to_string(), &host, "/apiws", CONNECT_TIMEOUT, connector).await {
                Ok((reader, writer)) => {
                    self.remember_working_domain(&base);
                    return Some((reader, writer, host));
                }
                Err(error) => {
                    logger::warn(format!(
                        "[{peer}] DC{dc}{media_tag} relay {host} failed: {error}"
                    ));
                }
            }
        }
        None
    }

    /// Продвинуть сработавший домен в начало и сохранить кэш (best-effort).
    fn remember_working_domain(&self, domain: &str) {
        let Some(path) = &self.cache_path else {
            return;
        };
        let mut order: Vec<String> = vec![domain.to_string()];
        order.extend(
            self.domains
                .iter()
                .filter(|existing| *existing != domain)
                .cloned(),
        );
        let _ = save_cached_domains(path, &order);
    }
}

/// Фолбэк через собственный Cloudflare Worker (WS-to-WS мост):
/// воркер принимает WS на `/apiws?dc=<n>&m=<0|1>` и связывает его с
/// `wss://kws{dc}[-1].web.telegram.org/apiws` — тем же эндпоинтом, что и
/// прямой путь. Публичные relay домены не участвуют — путь зависит
/// только от владельца.
pub async fn connect_worker(
    domains: &[String],
    dc: u16,
    is_media: bool,
    connector: &TlsConnector,
    peer: &str,
) -> Option<(ws::WsReader, Arc<ws::WsWriter>, String)> {
    let path = format!("/apiws?dc={dc}&m={}", if is_media { 1 } else { 0 });
    for domain in domains {
        let ip = match dns::resolve(domain).await {
            Ok(ip) => ip,
            Err(error) => {
                logger::warn(format!("[{peer}] DC{dc} worker {domain}: dns: {error}"));
                continue;
            }
        };
        logger::info(format!(
            "[{peer}] DC{dc} -> wss://{domain}{path} via {ip} (cf worker)"
        ));
        match ws::connect(&ip.to_string(), domain, &path, CONNECT_TIMEOUT, connector).await {
            Ok((reader, writer)) => return Some((reader, writer, domain.to_string())),
            Err(error) => {
                logger::warn(format!(
                    "[{peer}] DC{dc} worker {domain} failed: {error}"
                ));
            }
        }
    }
    None
}

fn load_cached_domains(path: &Path) -> Option<Vec<String>> {
    let text = std::fs::read_to_string(path).ok()?;
    let parsed: Vec<String> = serde_lite(&text)?;
    let valid: Vec<String> = parsed
        .into_iter()
        .filter(|domain| {
            !domain.is_empty()
                && domain.len() <= 253
                && domain
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'))
        })
        .take(64)
        .collect();
    // Кэшу достаточно одного валидного домена: он лишь задаёт порядок,
    // итоговый список всегда объединяется со встроенным.
    if valid.is_empty() {
        None
    } else {
        Some(valid)
    }
}

fn save_cached_domains(path: &Path, domains: &[String]) -> std::io::Result<()> {
    let json = format!(
        "{{\"domains\":[{}]}}",
        domains
            .iter()
            .map(|d| format!("\"{d}\""))
            .collect::<Vec<_>>()
            .join(",")
    );
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

/// Мини-парсер `"domains": ["a", "b"]` без зависимости от serde_json.
fn serde_lite(text: &str) -> Option<Vec<String>> {
    let start = text.find("\"domains\"")?;
    let open = text[start..].find('[')? + start;
    let close = text.find(']')?;
    if close <= open {
        return None;
    }
    let items = text[open + 1..close]
        .split(',')
        .filter_map(|item| {
            let trimmed = item.trim().trim_matches(|c| c == '"' || c == ' ');
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        })
        .collect();
    Some(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_list_has_no_duplicates() {
        let mut sorted = CFPROXY_DEFAULT_DOMAINS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), CFPROXY_DEFAULT_DOMAINS.len());
    }

    #[test]
    fn attempt_order_rotates_by_dc() {
        let relay = CfRelay::new(None);
        let order2 = relay.attempt_order(2);
        let order4 = relay.attempt_order(4);
        assert_eq!(order2[0], "cakeisalie.co.uk"); // индекс 2
        assert_eq!(order4[0], "lovetrue.co.uk"); // индекс 4
        // Полный набор сохраняется при любом смещении.
        let mut sorted = order2.clone();
        sorted.sort_unstable();
        let mut expected = CFPROXY_DEFAULT_DOMAINS.to_vec();
        expected.sort_unstable();
        assert_eq!(sorted, expected);
        assert_ne!(order2, order4);
    }

    #[test]
    fn cache_json_roundtrip() {
        let dir = std::env::temp_dir().join("tgproxy_rs_test_cache");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cfproxy_cache.json");

        save_cached_domains(
            &path,
            &["fixtelega.co.uk".to_string(), "pclead.co.uk".to_string()],
        )
        .unwrap();
        assert_eq!(
            load_cached_domains(&path),
            Some(vec![
                "fixtelega.co.uk".to_string(),
                "pclead.co.uk".to_string()
            ])
        );

        // Битый кэш игнорируется.
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(load_cached_domains(&path), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cached_domains_come_first() {
        let dir = std::env::temp_dir().join("tgproxy_rs_test_order");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cfproxy_cache.json");
        save_cached_domains(&path, &["sadnews.co.uk".to_string()]).unwrap();

        let relay = CfRelay::new(Some(&path));
        assert_eq!(relay.domains[0], "sadnews.co.uk");
        assert_eq!(relay.domains.len(), CFPROXY_DEFAULT_DOMAINS.len());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn single_cached_domain_is_honored() {
        let dir = std::env::temp_dir().join("tgproxy_rs_test_single");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cfproxy_cache.json");
        save_cached_domains(&path, &["only-one.co.uk".to_string()]).unwrap();

        let relay = CfRelay::new(Some(&path));
        assert_eq!(relay.domains[0], "only-one.co.uk");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Живая сквозная проверка: наш DNS-резолв -> верифицированный TLS ->
    /// WS-upgrade -> obfuscated2 relay init -> настоящий Telegram DC за
    /// relay отвечает 64-байтным init. Запускать явно:
    /// `cargo test live_cf -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "requires network access to CF relays"]
    async fn live_cf_relay_handshake() {
        let connector = crate::ws::tls_connector();
        let mut any_ok = false;
        for base in CFPROXY_DEFAULT_DOMAINS {
            let host = format!("kws2.{base}");
            let ip = match tokio::time::timeout(
                std::time::Duration::from_secs(6),
                crate::dns::resolve(&host),
            )
            .await
            {
                Ok(Ok(ip)) => ip,
                Ok(Err(error)) => {
                    println!("{host}: dns failed: {error}");
                    continue;
                }
                Err(_) => {
                    println!("{host}: dns timeout");
                    continue;
                }
            };
            let (mut reader, writer) =
                match crate::ws::connect(&ip.to_string(), &host, "/apiws", std::time::Duration::from_secs(8), &connector)
                    .await
                {
                    Ok(pair) => pair,
                    Err(error) => {
                        println!("{host}: ws connect failed: {error}");
                        continue;
                    }
                };

            let init = crate::obf2::make_relay_init(
                crate::obf2::ProtoTag::Abridged,
                2,
                &mut rand::rngs::OsRng,
            );
            if let Err(error) = writer.send(&init).await {
                println!("{host}: send failed: {error}");
                continue;
            }
            match tokio::time::timeout(std::time::Duration::from_secs(8), reader.recv()).await {
                Ok(Some(response)) => {
                    println!("{host}: RESPONSE {} bytes", response.len());
                    if response.len() == 64 {
                        any_ok = true;
                        writer.close().await;
                        break;
                    }
                }
                Ok(None) => println!("{host}: closed without response"),
                Err(_) => println!("{host}: silence after init"),
            }
            writer.close().await;
        }
        assert!(any_ok, "no relay answered with a 64-byte DC init");
    }

    /// Синтетический obfuscated2-клиент для внешнего прокси: прокси должен
    /// быть заранее запущен (по умолчанию 127.0.0.1:24445, адрес можно
    /// переопределить переменной TGPROXY_PROBE_ADDR). Проба отправляет
    /// клиентский init, затем первый настоящий MTProto-пакет (req_pq,
    /// abridged) и ждёт ответных байтов — так проверяется путь до DC
    /// целиком, как это делает реальный клиент.
    /// `cargo test synthetic_client -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "requires a proxy listening on 127.0.0.1:24445"]
    async fn synthetic_client_probe() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use rand::RngCore as _;

        let secret_hex = "5a5a1133445566778899aabbccddeeff";
        let mut secret = [0u8; 16];
        for i in 0..16 {
            secret[i] =
                u8::from_str_radix(&secret_hex[i * 2..i * 2 + 2], 16).expect("secret hex");
        }
        let target = std::env::var("TGPROXY_PROBE_ADDR")
            .unwrap_or_else(|_| "127.0.0.1:24445".to_string());

        let mut client = tokio::net::TcpStream::connect(&target)
            .await
            .expect("connect to reference proxy");
        println!("connected to {target}, sending synthetic client init");

        let mut rng = rand::rngs::OsRng;
        let init = crate::obf2::build_client_init(
            2,
            crate::obf2::ProtoTag::Abridged,
            &secret,
            &mut rng,
        );
        client.write_all(&init).await.expect("send client init");

        // Первый MTProto-пакет настоящего клиента: req_pq (abridged).
        // payload = конструктор 0x60469778 (LE) + 16 байт nonce,
        // abridged-заголовок = payload_len / 4 (20 -> 0x05).
        let mut req_pq = Vec::with_capacity(21);
        req_pq.push(0x05);
        req_pq.extend_from_slice(&0x60469778u32.to_le_bytes());
        let mut nonce = [0u8; 16];
        rng.fill_bytes(&mut nonce);
        req_pq.extend_from_slice(&nonce);

        // Шифруем продолжением клиентского c2s-потока (после init).
        use crate::crypto::CtrCipher;
        let prekey: [u8; 32] = init[8..40].try_into().unwrap();
        let iv: [u8; 16] = init[40..56].try_into().unwrap();
        let key = {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(prekey);
            hasher.update(secret);
            <[u8; 32]>::from(hasher.finalize())
        };
        let mut c2s = CtrCipher::new(&key, &iv);
        c2s.skip_64();
        c2s.apply(&mut req_pq);
        client.write_all(&req_pq).await.expect("send req_pq");
        println!("req_pq sent ({} bytes)", req_pq.len());

        let mut response = vec![0u8; 4096];
        match tokio::time::timeout(
            std::time::Duration::from_secs(20),
            client.read(&mut response),
        )
        .await
        {
            Ok(Ok(0)) => println!("connection closed by proxy without data"),
            Ok(Ok(n)) => println!("SUCCESS: {n} bytes from DC (first byte 0x{:02x})", response[0]),
            Ok(Err(error)) => println!("read error: {error}"),
            Err(_) => println!("silence: no data in 20s"),
        }
    }

    /// Печать hex пары init+packet (Rust-генерация) для скармливания
    /// воркер-эндпоинту /probe3 — контроль JS-крипты.
    #[test]
    fn print_probe_hex() {
        use rand::RngCore as _;

        let mut rng = rand::rngs::OsRng;
        let init = crate::obf2::make_relay_init(
            crate::obf2::ProtoTag::Abridged,
            2,
            &mut rng,
        );
        let mut packet = vec![0x05u8];
        packet.extend_from_slice(&0x60469778u32.to_le_bytes());
        let mut nonce = [0u8; 16];
        rng.fill_bytes(&mut nonce);
        packet.extend_from_slice(&nonce);

        let key: [u8; 32] = init[8..40].try_into().unwrap();
        let iv: [u8; 16] = init[40..56].try_into().unwrap();
        let mut tg_enc = crate::crypto::CtrCipher::new(&key, &iv);
        tg_enc.skip_64();
        tg_enc.apply(&mut packet);

        let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
        println!("INIT_HEX={}", hex(&init));
        println!("PACKET_HEX={}", hex(&packet));
    }

    /// Живая проба собственного CF worker'а: TGPROXY_WORKER=<домен>
    /// cargo test live_worker -- --ignored --nocapture
    /// Полный сценарий: WS-upgrade к воркеру, relay init, req_pq — и
    /// ожидание байтов от DC через воркер.
    #[tokio::test]
    #[ignore = "requires TGPROXY_WORKER env var with deployed worker domain"]
    async fn live_worker_handshake() {
        use rand::RngCore as _;

        let domain =
            std::env::var("TGPROXY_WORKER").expect("set TGPROXY_WORKER=<your>.workers.dev");
        let connector = crate::ws::tls_connector();
        let path = "/apiws?dc=2&m=0".to_string();

        let ip = crate::dns::resolve(&domain).await.expect("resolve worker domain");
        let (mut reader, writer) = crate::ws::connect(
            &ip.to_string(),
            &domain,
            &path,
            std::time::Duration::from_secs(10),
            &connector,
        )
        .await
        .expect("connect worker");
        println!("worker {domain} ({ip}): connected, sending relay init");

        let init = crate::obf2::make_relay_init(
            crate::obf2::ProtoTag::Abridged,
            2,
            &mut rand::rngs::OsRng,
        );
        writer.send(&init).await.expect("send relay init");

        // Первый MTProto-пакет (req_pq) продолжением tg-потока — как
        // настоящий клиент.
        let mut packet = vec![0x05u8];
        packet.extend_from_slice(&0x60469778u32.to_le_bytes());
        let mut nonce = [0u8; 16];
        rand::rngs::OsRng.fill_bytes(&mut nonce);
        packet.extend_from_slice(&nonce);
        let key: [u8; 32] = init[8..40].try_into().unwrap();
        let iv: [u8; 16] = init[40..56].try_into().unwrap();
        let mut tg_enc = crate::crypto::CtrCipher::new(&key, &iv);
        tg_enc.skip_64();
        tg_enc.apply(&mut packet);
        writer.send(&packet).await.expect("send req_pq");

        match tokio::time::timeout(std::time::Duration::from_secs(10), reader.recv()).await {
            Ok(Some(data)) => {
                println!(
                    "worker SUCCESS: {} bytes back (first byte 0x{:02x})",
                    data.len(),
                    data[0]
                );
            }
            Ok(None) => println!("worker: closed without data"),
            Err(_) => println!("worker: silence"),
        }
        writer.close().await;
    }
}
