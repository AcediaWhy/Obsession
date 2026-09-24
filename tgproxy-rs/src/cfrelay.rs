//! Резервное подключение через публичные Cloudflare relay.
//! Встроенный список доменов хранится открытым текстом. Кэш `--cfproxy-cache`
//! обновляет порядок доменов после успешного соединения.

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
const FALLBACK_BUDGET: std::time::Duration = std::time::Duration::from_secs(25);
const FAILED_RELAY_COOLDOWN: Duration = Duration::from_secs(5 * 60);
const MAX_WORKER_ATTEMPTS: usize = 3;
const MAX_CACHE_BYTES: u64 = 64 * 1024;
const CACHE_SCHEMA_VERSION: u8 = 2;
static CACHE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
static CACHE_SERIAL: AtomicU64 = AtomicU64::new(0);

/// Состояние фолбэка: порядок доменов (кэш вперёд) и путь кэша.
pub struct CfRelay {
    domains: Vec<String>,
    cache_path: Option<PathBuf>,
    active_domains: Mutex<HashMap<u16, String>>,
    failed_domains: Mutex<HashMap<(u16, String), Instant>>,
    attempt_serial: AtomicU64,
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
            active_domains: Mutex::new(HashMap::new()),
            failed_domains: Mutex::new(HashMap::new()),
            attempt_serial: AtomicU64::new(0),
        }
    }

    /// Уже сработавший relay идёт первым отдельно для каждого DC. Пока
    /// такого нет, параллельные подключения начинают с разных мест списка,
    /// чтобы не устраивать stampede на одном недоступном домене.
    fn attempt_order(&self, dc: u16) -> Vec<String> {
        let mut order = self.domains.clone();
        let cooled = {
            let now = Instant::now();
            let mut failed = self
                .failed_domains
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            failed.retain(|_, until| *until > now);
            failed
                .iter()
                .filter(|((failed_dc, _), _)| *failed_dc == dc)
                .map(|((_, domain), _)| domain.clone())
                .collect::<Vec<_>>()
        };
        order.sort_by_key(|domain| cooled.contains(domain));
        let active = self
            .active_domains
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&dc)
            .cloned();
        if let Some(active) = active.filter(|domain| !cooled.contains(domain)) {
            if let Some(index) = order.iter().position(|domain| domain == &active) {
                order.swap(0, index);
            }
            return order;
        }

        let available_len = order
            .iter()
            .position(|domain| cooled.contains(domain))
            .unwrap_or(order.len());
        let rotation_len = if available_len == 0 {
            order.len()
        } else {
            available_len
        };
        if rotation_len > 1 {
            let serial = self.attempt_serial.fetch_add(1, Ordering::Relaxed) as usize;
            order[..rotation_len].rotate_left(serial % rotation_len);
        }
        order
    }

    /// Подключается к `wss://kws{dc}.{relay}/apiws`.
    /// Признак media передаётся в relay-init, а номер DC в имени не меняется.
    /// Ответ HTTP 101 подтверждает только доступность relay; рабочим он
    /// считается после подтверждения MTProto-сессии.
    pub async fn connect(
        &self,
        dc: u16,
        is_media: bool,
        connector: &TlsConnector,
        peer: &str,
    ) -> Option<(ws::WsReader, Arc<ws::WsWriter>, String)> {
        let media_tag = if is_media { " media" } else { "" };
        let attempts = crate::connect_race::first_ready(
            self.attempt_order(dc),
            Duration::from_millis(200),
            3,
            |base| {
                let connector = connector.clone();
                let peer = peer.to_owned();
                async move {
                    let host = public_relay_host(dc, is_media, &base);
                    let attempt = async {
                        let ip = match dns::resolve(&host).await {
                            Ok(ip) => ip,
                            Err(error) => {
                                logger::warn(format!(
                                    "[{peer}] DC{dc}{media_tag} relay {host}: dns: {error}"
                                ));
                                return None;
                            }
                        };
                        logger::info(format!(
                            "[{peer}] DC{dc}{media_tag} -> wss://{host}/apiws via {ip} (cf relay)"
                        ));
                        match ws::connect(
                            &ip.to_string(),
                            &host,
                            "/apiws",
                            CONNECT_TIMEOUT,
                            &connector,
                        )
                        .await
                        {
                            Ok((reader, writer)) => return Some((reader, writer, base)),
                            Err(error) => {
                                logger::warn(format!(
                                    "[{peer}] DC{dc}{media_tag} relay {host} failed: {error}"
                                ));
                            }
                        }
                        None
                    };
                    // DNS and TLS share one deadline, so neither can occupy a
                    // race slot indefinitely. A cancelled loser isn't demoted.
                    match tokio::time::timeout(CONNECT_TIMEOUT, attempt).await {
                        Ok(result) => result,
                        Err(_) => {
                            logger::warn(format!(
                                "[{peer}] DC{dc}{media_tag} relay attempt timed out"
                            ));
                            None
                        }
                    }
                }
            },
            |base| self.forget_failed_domain(dc, &base),
        );
        match tokio::time::timeout(FALLBACK_BUDGET, attempts).await {
            Ok(result) => result,
            Err(_) => {
                logger::warn(format!(
                    "[{peer}] DC{dc}{media_tag} public relay fallback timed out"
                ));
                None
            }
        }
    }

    /// Продвинуть сработавший домен в начало и сохранить кэш (best-effort).
    fn remember_working_domain(&self, dc: u16, domain: &str) {
        self.failed_domains
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&(dc, domain.to_string()));
        if is_default_domain(domain) {
            self.active_domains
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(dc, domain.to_string());
        }

        let Some(path) = &self.cache_path else {
            return;
        };
        if !is_default_domain(domain) {
            return;
        }
        let _guard = CACHE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut order: Vec<String> = vec![domain.to_string()];
        if let Some(cached) = load_cached_domains_unlocked(path) {
            order.extend(cached.into_iter().filter(|existing| existing != domain));
        }
        order.extend(
            self.domains
                .iter()
                .filter(|existing| *existing != domain)
                .cloned(),
        );
        let mut unique = Vec::with_capacity(order.len());
        for candidate in order {
            if !unique.contains(&candidate) {
                unique.push(candidate);
            }
        }
        let _ = save_cached_domains_unlocked(path, &unique);
    }

    fn forget_failed_domain(&self, dc: u16, domain: &str) {
        let mut active = self
            .active_domains
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if active.get(&dc).is_some_and(|current| current == domain) {
            active.remove(&dc);
        }
        drop(active);
        if is_default_domain(domain) {
            self.failed_domains
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(
                    (dc, domain.to_string()),
                    Instant::now() + FAILED_RELAY_COOLDOWN,
                );
        }
    }

    pub(crate) fn report_session_success(&self, dc: u16, domain: &str) {
        self.remember_working_domain(dc, domain);
    }

    pub(crate) fn report_session_failure(&self, dc: u16, domain: &str) {
        self.forget_failed_domain(dc, domain);

        let Some(path) = &self.cache_path else {
            return;
        };
        if !is_default_domain(domain) {
            return;
        }
        let _guard = CACHE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut order = load_cached_domains_unlocked(path).unwrap_or_else(|| self.domains.clone());
        order.retain(|candidate| candidate != domain);
        order.push(domain.to_string());
        let _ = save_cached_domains_unlocked(path, &order);
    }
}

fn public_relay_host(dc: u16, _is_media: bool, base: &str) -> String {
    format!("kws{dc}.{base}")
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
    let attempts = async {
        for domain in domains.iter().take(MAX_WORKER_ATTEMPTS) {
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
                    logger::warn(format!("[{peer}] DC{dc} worker {domain} failed: {error}"));
                }
            }
        }
        None
    };
    match tokio::time::timeout(FALLBACK_BUDGET, attempts).await {
        Ok(result) => result,
        Err(_) => {
            logger::warn(format!("[{peer}] DC{dc} worker fallback timed out"));
            None
        }
    }
}

fn load_cached_domains(path: &Path) -> Option<Vec<String>> {
    let _guard = CACHE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    load_cached_domains_unlocked(path)
}

fn load_cached_domains_unlocked(path: &Path) -> Option<Vec<String>> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > MAX_CACHE_BYTES
    {
        return None;
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    std::fs::File::open(path)
        .ok()?
        .take(MAX_CACHE_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_CACHE_BYTES {
        return None;
    }
    let text = String::from_utf8(bytes).ok()?;
    let parsed: Vec<String> = serde_lite(&text)?;
    let mut valid = Vec::new();
    for domain in parsed {
        if is_default_domain(&domain) && !valid.contains(&domain) {
            valid.push(domain);
        }
        if valid.len() == CFPROXY_DEFAULT_DOMAINS.len() {
            break;
        }
    }
    if valid.is_empty() {
        None
    } else {
        Some(valid)
    }
}

#[cfg(test)]
fn save_cached_domains(path: &Path, domains: &[String]) -> std::io::Result<()> {
    let _guard = CACHE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    save_cached_domains_unlocked(path, domains)
}

fn save_cached_domains_unlocked(path: &Path, domains: &[String]) -> std::io::Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let mut allowed = Vec::new();
    for domain in domains {
        if is_default_domain(domain) && !allowed.contains(&domain) {
            allowed.push(domain);
        }
        if allowed.len() == CFPROXY_DEFAULT_DOMAINS.len() {
            break;
        }
    }
    let json = format!(
        "{{\"version\":{CACHE_SCHEMA_VERSION},\"domains\":[{}]}}",
        allowed
            .iter()
            .map(|d| format!("\"{d}\""))
            .collect::<Vec<_>>()
            .join(",")
    );
    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
    let serial = CACHE_SERIAL.fetch_add(1, Ordering::Relaxed);
    let tmp = path.with_file_name(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        serial
    ));
    let write_result = (|| {
        let mut file = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        file.write_all(json.as_bytes())?;
        file.sync_all()
    })();
    if let Err(error) = write_result {
        let _ = std::fs::remove_file(&tmp);
        return Err(error);
    }

    #[cfg(windows)]
    let replace_result = {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                let _ = std::fs::remove_file(&tmp);
                return Err(error);
            }
        }
        std::fs::rename(&tmp, path)
    };
    #[cfg(not(windows))]
    let replace_result = std::fs::rename(&tmp, path);

    if replace_result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    replace_result
}

fn is_default_domain(domain: &str) -> bool {
    CFPROXY_DEFAULT_DOMAINS.contains(&domain)
}

/// Мини-парсер `"domains": ["a", "b"]` без зависимости от serde_json.
fn serde_lite(text: &str) -> Option<Vec<String>> {
    let version_start = text.find("\"version\"")? + "\"version\"".len();
    let version_value = text[version_start..].split_once(':')?.1.trim_start();
    let version_len = version_value
        .bytes()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    let version = version_value[..version_len].parse::<u8>().ok()?;
    if version != CACHE_SCHEMA_VERSION {
        return None;
    }
    let start = text.find("\"domains\"")?;
    let open = text[start..].find('[')? + start;
    let close = text[open + 1..].find(']')? + open + 1;
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

    fn req_pq_multi_packet(rng: &mut impl rand::RngCore) -> Vec<u8> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before Unix epoch");
        let fraction = ((u64::from(now.subsec_nanos())) << 32) / 1_000_000_000;
        let message_id = ((now.as_secs() << 32) | fraction) & !3;

        let mut packet = Vec::with_capacity(41);
        packet.push(0x0a); // abridged length: 40 bytes / 4
        packet.extend_from_slice(&0u64.to_le_bytes()); // auth_key_id: unencrypted
        packet.extend_from_slice(&message_id.to_le_bytes());
        packet.extend_from_slice(&20u32.to_le_bytes());
        packet.extend_from_slice(&0xbe7e8ef1u32.to_le_bytes());
        let mut nonce = [0u8; 16];
        rng.fill_bytes(&mut nonce);
        packet.extend_from_slice(&nonce);
        packet
    }

    #[test]
    fn req_pq_multi_has_unencrypted_mtproto_envelope() {
        let packet = req_pq_multi_packet(&mut rand::rngs::OsRng);
        assert_eq!(packet.len(), 41);
        assert_eq!(packet[0], 0x0a);
        assert_eq!(&packet[1..9], &[0; 8]);
        assert_eq!(u32::from_le_bytes(packet[17..21].try_into().unwrap()), 20);
        assert_eq!(
            u32::from_le_bytes(packet[21..25].try_into().unwrap()),
            0xbe7e8ef1
        );
    }

    #[test]
    fn public_relay_hostname_never_encodes_media() {
        let base = "relay.example";
        assert_eq!(public_relay_host(2, false, base), "kws2.relay.example");
        assert_eq!(public_relay_host(2, true, base), "kws2.relay.example");
        assert_eq!(public_relay_host(203, false, base), "kws203.relay.example");
        assert_eq!(public_relay_host(203, true, base), "kws203.relay.example");
    }

    #[test]
    fn default_list_has_no_duplicates() {
        let mut sorted = CFPROXY_DEFAULT_DOMAINS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), CFPROXY_DEFAULT_DOMAINS.len());
    }

    #[test]
    fn attempt_order_spreads_unlearned_connections_across_relays() {
        let relay = CfRelay::new(None);
        let first = relay.attempt_order(2);
        let second = relay.attempt_order(2);
        assert_eq!(first[0], CFPROXY_DEFAULT_DOMAINS[0]);
        assert_eq!(second[0], CFPROXY_DEFAULT_DOMAINS[1]);
        assert_ne!(first, second);
    }

    #[test]
    fn working_domain_is_remembered_per_dc_in_memory() {
        let relay = CfRelay::new(None);
        relay.remember_working_domain(2, "sadnews.co.uk");
        relay.remember_working_domain(4, "fixtelega.co.uk");

        assert_eq!(relay.attempt_order(2)[0], "sadnews.co.uk");
        assert_eq!(relay.attempt_order(4)[0], "fixtelega.co.uk");
    }

    #[test]
    fn failed_working_domain_is_invalidated_in_memory() {
        let relay = CfRelay::new(None);
        relay.remember_working_domain(2, "sadnews.co.uk");
        assert_eq!(relay.attempt_order(2)[0], "sadnews.co.uk");

        relay.forget_failed_domain(2, "sadnews.co.uk");
        assert_ne!(relay.attempt_order(2)[0], "sadnews.co.uk");
        assert_eq!(relay.attempt_order(2).last().unwrap(), "sadnews.co.uk");
    }

    #[test]
    fn unstable_session_demotes_domain_in_persistent_cache() {
        let dir = std::env::temp_dir().join("tgproxy_rs_test_session_failure_cache");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cfproxy_cache.json");
        save_cached_domains(
            &path,
            &["sadnews.co.uk".to_string(), "pclead.co.uk".to_string()],
        )
        .unwrap();

        let relay = CfRelay::new(Some(&path));
        relay.report_session_success(2, "sadnews.co.uk");
        relay.report_session_failure(2, "sadnews.co.uk");

        let cached = load_cached_domains(&path).unwrap();
        assert_ne!(cached[0], "sadnews.co.uk");
        assert_eq!(cached.last().unwrap(), "sadnews.co.uk");
        assert_ne!(relay.attempt_order(2)[0], "sadnews.co.uk");
        let _ = std::fs::remove_dir_all(&dir);
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

        // Повторная запись заменяет существующий файл и на Windows.
        save_cached_domains(&path, &["sadnews.co.uk".to_string()]).unwrap();
        assert_eq!(
            load_cached_domains(&path),
            Some(vec!["sadnews.co.uk".to_string()])
        );

        // Битый кэш игнорируется.
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(load_cached_domains(&path), None);

        // Слишком большой файл не читается целиком.
        std::fs::write(&path, vec![b'x'; MAX_CACHE_BYTES as usize + 1]).unwrap();
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
    fn legacy_connection_only_cache_is_ignored() {
        let dir = std::env::temp_dir().join("tgproxy_rs_test_legacy_cache");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cfproxy_cache.json");
        std::fs::write(&path, r#"{"domains":["sadnews.co.uk"]}"#).unwrap();

        let relay = CfRelay::new(Some(&path));
        assert_eq!(relay.domains[0], CFPROXY_DEFAULT_DOMAINS[0]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn single_cached_domain_is_honored() {
        let dir = std::env::temp_dir().join("tgproxy_rs_test_single");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cfproxy_cache.json");
        // Кэш не является источником доверия: произвольный домен отбрасывается.
        std::fs::write(&path, r#"{"domains":["only-one.co.uk"]}"#).unwrap();

        let relay = CfRelay::new(Some(&path));
        assert_eq!(relay.domains[0], CFPROXY_DEFAULT_DOMAINS[0]);
        assert_eq!(relay.domains.len(), CFPROXY_DEFAULT_DOMAINS.len());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Живая сквозная проверка: наш DNS-резолв -> верифицированный TLS ->
    /// WS-upgrade -> obfuscated2 relay init + req_pq_multi -> настоящий
    /// Telegram DC за relay возвращает непустой ответ. Запускать явно:
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
            let (mut reader, writer) = match crate::ws::connect(
                &ip.to_string(),
                &host,
                "/apiws",
                std::time::Duration::from_secs(8),
                &connector,
            )
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
            let mut packet = req_pq_multi_packet(&mut rand::rngs::OsRng);
            let key: [u8; 32] = init[8..40].try_into().unwrap();
            let iv: [u8; 16] = init[40..56].try_into().unwrap();
            let mut tg_enc = crate::crypto::CtrCipher::new(&key, &iv);
            tg_enc.skip_64();
            tg_enc.apply(&mut packet);
            if let Err(error) = writer.send(&packet).await {
                println!("{host}: req_pq_multi send failed: {error}");
                continue;
            }
            match tokio::time::timeout(std::time::Duration::from_secs(8), reader.recv()).await {
                Ok(Some(response)) => {
                    println!("{host}: RESPONSE {} bytes", response.len());
                    if !response.is_empty() {
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
        assert!(any_ok, "no relay returned data for req_pq_multi");
    }

    /// Синтетический obfuscated2-клиент для внешнего прокси: прокси должен
    /// быть заранее запущен (по умолчанию 127.0.0.1:24445). Адрес можно
    /// переопределить через TGPROXY_PROBE_ADDR, а DC — через TGPROXY_PROBE_DC;
    /// отрицательный DC моделирует media-соединение. Проба отправляет
    /// клиентский init, затем первый настоящий MTProto-пакет (req_pq,
    /// abridged) и ждёт ответных байтов — так проверяется путь до DC
    /// целиком, как это делает реальный клиент.
    /// `cargo test synthetic_client -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "requires a proxy listening on 127.0.0.1:24445"]
    async fn synthetic_client_probe() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let secret_hex = "5a5a1133445566778899aabbccddeeff";
        let mut secret = [0u8; 16];
        for i in 0..16 {
            secret[i] = u8::from_str_radix(&secret_hex[i * 2..i * 2 + 2], 16).expect("secret hex");
        }
        let target =
            std::env::var("TGPROXY_PROBE_ADDR").unwrap_or_else(|_| "127.0.0.1:24445".to_string());
        let probe_dc = std::env::var("TGPROXY_PROBE_DC")
            .ok()
            .map(|raw| raw.parse::<i16>().expect("TGPROXY_PROBE_DC must be i16"))
            .unwrap_or(2);

        let mut client = tokio::net::TcpStream::connect(&target)
            .await
            .expect("connect to reference proxy");
        println!("connected to {target}, sending synthetic client init for DC{probe_dc}");

        let mut rng = rand::rngs::OsRng;
        let init = crate::obf2::build_client_init(
            probe_dc,
            crate::obf2::ProtoTag::Abridged,
            &secret,
            &mut rng,
        );
        client.write_all(&init).await.expect("send client init");

        // Первый MTProto-пакет настоящего клиента: req_pq_multi (abridged).
        let mut req_pq = req_pq_multi_packet(&mut rng);

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
        let received = tokio::time::timeout(
            std::time::Duration::from_secs(20),
            client.read(&mut response),
        )
        .await
        .expect("silence: no data in 20s")
        .expect("read response from proxy");
        assert!(received > 0, "connection closed by proxy without data");
        println!(
            "SUCCESS: {received} bytes from DC (first byte 0x{:02x})",
            response[0]
        );
    }

    /// Та же сквозная проба, но с padded-intermediate транспортом, который
    /// используют мобильные клиенты Telegram. Нужна отдельно от abridged:
    /// именно transport framing определяет границы WebSocket messages.
    #[tokio::test]
    #[ignore = "requires a proxy listening on 127.0.0.1:24445"]
    async fn synthetic_padded_client_probe() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let secret_hex = "5a5a1133445566778899aabbccddeeff";
        let mut secret = [0u8; 16];
        for i in 0..16 {
            secret[i] = u8::from_str_radix(&secret_hex[i * 2..i * 2 + 2], 16).expect("secret hex");
        }
        let target =
            std::env::var("TGPROXY_PROBE_ADDR").unwrap_or_else(|_| "127.0.0.1:24445".to_string());
        let probe_dc = std::env::var("TGPROXY_PROBE_DC")
            .ok()
            .map(|raw| raw.parse::<i16>().expect("TGPROXY_PROBE_DC must be i16"))
            .unwrap_or(2);

        let mut client = tokio::net::TcpStream::connect(&target)
            .await
            .expect("connect to reference proxy");
        println!("connected to {target}, sending padded client init for DC{probe_dc}");

        let mut rng = rand::rngs::OsRng;
        let init = crate::obf2::build_client_init(
            probe_dc,
            crate::obf2::ProtoTag::Padded,
            &secret,
            &mut rng,
        );
        client.write_all(&init).await.expect("send client init");

        let abridged = req_pq_multi_packet(&mut rng);
        let payload = &abridged[1..];
        let padding_len = 12usize;
        let mut req_pq = Vec::with_capacity(4 + payload.len() + padding_len);
        req_pq.extend_from_slice(&((payload.len() + padding_len) as u32).to_le_bytes());
        req_pq.extend_from_slice(payload);
        let mut padding = vec![0u8; padding_len];
        rand::RngCore::fill_bytes(&mut rng, &mut padding);
        req_pq.extend_from_slice(&padding);

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
        client.write_all(&req_pq).await.expect("send padded req_pq");
        println!("padded req_pq sent ({} bytes)", req_pq.len());

        let mut response = vec![0u8; 4096];
        let received = tokio::time::timeout(
            std::time::Duration::from_secs(20),
            client.read(&mut response),
        )
        .await
        .expect("silence: no data in 20s")
        .expect("read response from proxy");
        assert!(received > 0, "connection closed by proxy without data");
        response.truncate(received);

        let reversed: Vec<u8> = init[8..56].iter().rev().copied().collect();
        let server_key = {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(&reversed[..32]);
            hasher.update(secret);
            <[u8; 32]>::from(hasher.finalize())
        };
        let server_iv: [u8; 16] = reversed[32..48].try_into().unwrap();
        let mut s2c = CtrCipher::new(&server_key, &server_iv);
        s2c.apply(&mut response);

        assert!(
            response.len() >= 28,
            "padded response is shorter than MTProto envelope"
        );
        let outer_len =
            u32::from_le_bytes(response[..4].try_into().unwrap()) as usize & 0x7fff_ffff;
        assert_eq!(outer_len + 4, response.len());
        assert_eq!(&response[4..12], &[0; 8], "resPQ must be unencrypted");
        assert_eq!(
            u32::from_le_bytes(response[24..28].try_into().unwrap()),
            0x0516_2463,
            "unexpected first Telegram constructor"
        );
        println!(
            "SUCCESS PADDED: {received} bytes, valid resPQ constructor, outer_len={outer_len}"
        );
    }

    /// Печать hex пары init+packet (Rust-генерация) для скармливания
    /// воркер-эндпоинту /probe3 — контроль JS-крипты.
    #[test]
    fn print_probe_hex() {
        let mut rng = rand::rngs::OsRng;
        let init = crate::obf2::make_relay_init(crate::obf2::ProtoTag::Abridged, 2, &mut rng);
        let mut packet = req_pq_multi_packet(&mut rng);

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
        let domain =
            std::env::var("TGPROXY_WORKER").expect("set TGPROXY_WORKER=<your>.workers.dev");
        let connector = crate::ws::tls_connector();
        let path = "/apiws?dc=2&m=0".to_string();

        let ip = crate::dns::resolve(&domain)
            .await
            .expect("resolve worker domain");
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

        // Первый MTProto-пакет (req_pq_multi) продолжением tg-потока — как
        // настоящий клиент.
        let mut packet = req_pq_multi_packet(&mut rand::rngs::OsRng);
        let key: [u8; 32] = init[8..40].try_into().unwrap();
        let iv: [u8; 16] = init[40..56].try_into().unwrap();
        let mut tg_enc = crate::crypto::CtrCipher::new(&key, &iv);
        tg_enc.skip_64();
        tg_enc.apply(&mut packet);
        writer.send(&packet).await.expect("send req_pq_multi");

        let data = tokio::time::timeout(std::time::Duration::from_secs(10), reader.recv())
            .await
            .expect("worker: silence")
            .expect("worker: closed without data");
        assert!(!data.is_empty(), "worker returned an empty response");
        println!(
            "worker SUCCESS: {} bytes back (first byte 0x{:02x})",
            data.len(),
            data[0]
        );
        writer.close().await;
    }
}
