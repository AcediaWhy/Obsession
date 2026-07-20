//! Менеджер сети (L2) — идентификация сети для привязки кэшей надёжности.
//!
//! Два ключа (из [[dpi-reliability-arch]]):
//! - **MAC шлюза** — ключ L1-кэша «что работало в ЭТОЙ сети» (оффлайн, мгновенно,
//!   стабилен пока роутер тот же). Берём из ARP-таблицы по IP шлюза.
//! - **ASN_region** — ключ L2-рейтинга (`AS12389_RU-MOSCOW`; ТСПУ привязан к МРФ
//!   провайдера, регион обязателен). Через `ipinfo.io/json` одним запросом.
//!
//! MAC **мемоизирует** ASN_region в `netid_cache.json` → ipinfo дёргается один раз
//! на новую сеть. Деградация без падений: нет MAC → синтетический ключ по IP
//! шлюза; оффлайн → `asn_region=None` (Мозг работает на одном L3).
//!
//! Парсеры чистые (юнит-тест на captured-строках); исполнение под `#[cfg(windows)]`.

use std::collections::HashMap;

#[cfg(windows)]
use std::io::Read;
#[cfg(windows)]
use std::process::Stdio;
#[cfg(windows)]
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::legacy_reliability::contracts::NetworkFingerprint;
use crate::paths::Paths;

/// Total synchronous-command budget for one read-only local identity probe.
/// Runtime waits for this bounded operation instead of detaching it behind an
/// async timeout.
#[cfg(windows)]
pub const LOCAL_READ_ONLY_BUDGET: Duration = Duration::from_millis(900);

#[cfg(windows)]
const SYSTEM_COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(10);
#[cfg(windows)]
const MAX_SYSTEM_COMMAND_OUTPUT_BYTES: u64 = 1024 * 1024;

/// Снимок сетевой идентичности для Мозга.
#[derive(Clone, Debug, Default)]
pub struct NetIdentity {
    /// MAC шлюза (нормализован в `aa:bb:cc:dd:ee:ff`) либо синтетический ключ.
    pub gateway_mac: Option<String>,
    /// Ключ рейтинга `AS<asn>_<COUNTRY>-<REGION>`, если удалось определить.
    pub asn_region: Option<String>,
    /// Человекочитаемое имя оператора (для UI/лога).
    pub org: Option<String>,
}

/// Read-only local network plane used by Legacy Environment Gate.
///
/// Unlike [`resolve`], collecting this snapshot never performs GeoIP I/O and
/// never reads or writes a cache.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalNetworkIdentity {
    pub online: bool,
    pub interface_up: bool,
    pub default_route_available: bool,
    pub gateway_reachable: bool,
    pub fingerprint: NetworkFingerprint,
}

impl Default for LocalNetworkIdentity {
    fn default() -> Self {
        Self {
            online: false,
            interface_up: false,
            default_route_available: false,
            gateway_reachable: false,
            fingerprint: NetworkFingerprint::Unknown,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefaultRoute {
    pub gateway_ip: String,
    pub interface_ip: String,
}

/// Одна запись кэша идентичности сети.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct CacheEntry {
    asn_region: Option<String>,
    org: Option<String>,
    fetched_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct NetIdCache {
    schema_version: u32,
    networks: HashMap<String, CacheEntry>,
}

impl Default for NetIdCache {
    fn default() -> Self {
        Self {
            schema_version: 1,
            networks: HashMap::new(),
        }
    }
}

// ─── Чистые парсеры ─────────────────────────────────────────────────────────

/// IP шлюза по умолчанию из вывода `route print` (IPv4). Строка маршрута
/// `0.0.0.0  0.0.0.0  <gateway>  <iface>  <metric>` — берём 3-й столбец.
pub fn parse_gateway_ip(route_print: &str) -> Option<String> {
    parse_default_route(route_print).map(|route| route.gateway_ip)
}

/// Exact IPv4 default route and interface address from `route print`.
pub fn parse_default_route(route_print: &str) -> Option<DefaultRoute> {
    let mut best: Option<(u32, DefaultRoute)> = None;
    for line in route_print.lines() {
        let cols: Vec<&str> = line.split_whitespace().collect();
        // Ровно IPv4-маршрут по умолчанию: dest и mask оба 0.0.0.0.
        if cols.len() >= 5 && cols[0] == "0.0.0.0" && cols[1] == "0.0.0.0" {
            let gw = cols[2];
            let interface = cols[3];
            let Some(metric) = cols[4].parse::<u32>().ok() else {
                continue;
            };
            if is_ipv4(gw) && gw != "0.0.0.0" && is_ipv4(interface) && interface != "0.0.0.0" {
                let candidate = DefaultRoute {
                    gateway_ip: gw.to_string(),
                    interface_ip: interface.to_string(),
                };
                if best
                    .as_ref()
                    .is_none_or(|(best_metric, _)| metric < *best_metric)
                {
                    best = Some((metric, candidate));
                }
            }
        }
    }
    best.map(|(_, route)| route)
}

/// IP шлюза из `ipconfig` (фолбэк): строка `Default Gateway . . . : <ip>`.
pub fn parse_gateway_ip_ipconfig(ipconfig: &str) -> Option<String> {
    for line in ipconfig.lines() {
        let low = line.to_ascii_lowercase();
        if low.contains("default gateway") || low.contains("основной шлюз") {
            if let Some(rhs) = line.split(':').nth(1) {
                let ip = rhs.trim();
                if is_ipv4(ip) && ip != "0.0.0.0" {
                    return Some(ip.to_string());
                }
            }
        }
    }
    None
}

/// MAC для заданного IP из вывода `arp -a`. Windows отдаёт MAC через дефисы
/// (`aa-bb-cc-dd-ee-ff`) — нормализуем в двоеточия и нижний регистр.
pub fn parse_mac_for_ip(arp_a: &str, ip: &str) -> Option<String> {
    for line in arp_a.lines() {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() >= 2 && cols[0] == ip {
            return normalize_mac(cols[1]);
        }
    }
    None
}

/// Разбор `ipinfo.io/json`: `(asn, region_code, org)`.
/// - `org` вида `"AS12389 PJSC Rostelecom"` → asn=`AS12389`, org=полная строка.
/// - `country`+`region` → `RU-MOSCOW` (внутренний ключ; при переходе на
///   ISO 3166-2 community-рейтинг добавить маппинг имени региона в код).
pub fn parse_ipinfo(json: &str) -> (Option<String>, Option<String>, Option<String>) {
    let v: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return (None, None, None),
    };
    let org = v.get("org").and_then(|x| x.as_str()).map(|s| s.to_string());
    let asn = org.as_deref().and_then(|s| {
        let first = s.split_whitespace().next()?;
        if first.starts_with("AS")
            && first.len() > 2
            && first[2..].chars().all(|c| c.is_ascii_digit())
        {
            Some(first.to_string())
        } else {
            None
        }
    });
    let country = v.get("country").and_then(|x| x.as_str());
    let region = v.get("region").and_then(|x| x.as_str());
    let region_code = match (country, region) {
        (Some(c), Some(r)) if !c.is_empty() && !r.is_empty() => {
            Some(format!("{}-{}", c.to_ascii_uppercase(), region_slug(r)))
        }
        _ => None,
    };
    (asn, region_code, org)
}

/// Полный ключ рейтинга из asn + region_code.
pub fn asn_region_key(asn: Option<&str>, region_code: Option<&str>) -> Option<String> {
    match (asn, region_code) {
        (Some(a), Some(r)) => Some(format!("{a}_{r}")),
        _ => None,
    }
}

/// Слаг региона: буквы/цифры в верхнем регистре, пробелы схлопнуты в один `_`.
fn region_slug(region: &str) -> String {
    let mut out = String::new();
    let mut prev_us = false;
    for ch in region.trim().chars() {
        if ch.is_alphanumeric() {
            out.extend(ch.to_uppercase());
            prev_us = false;
        } else if !prev_us && !out.is_empty() {
            out.push('_');
            prev_us = true;
        }
    }
    while out.ends_with('_') {
        out.pop();
    }
    out
}

fn is_ipv4(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() == 4
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.parse::<u8>().is_ok())
}

/// Нормализует MAC в `aa:bb:cc:dd:ee:ff`. Принимает разделители `-`/`:`.
/// Отбрасывает явно невалидные (нулевой/широковещательный) адреса.
fn normalize_mac(raw: &str) -> Option<String> {
    let hex: Vec<&str> = raw.split(['-', ':']).collect();
    if hex.len() != 6 {
        return None;
    }
    let mut bytes = Vec::with_capacity(6);
    for h in &hex {
        if h.len() != 2 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        bytes.push(h.to_ascii_lowercase());
    }
    let joined = bytes.join(":");
    if joined == "00:00:00:00:00:00" || joined == "ff:ff:ff:ff:ff:ff" {
        return None;
    }
    Some(joined)
}

/// Builds a privacy-safe stable key without exposing local interface, gateway,
/// or MAC values to status/log payloads.
fn local_fingerprint(route: &DefaultRoute, gateway_mac: Option<&str>) -> NetworkFingerprint {
    let Some(gateway_mac) = gateway_mac else {
        return NetworkFingerprint::Unstable {
            reason: "gateway_identity_unavailable".to_owned(),
        };
    };
    let mut hasher = Sha256::new();
    hasher.update(b"obsession:legacy-local-network:v1\0");
    for value in [
        route.interface_ip.as_bytes(),
        route.gateway_ip.as_bytes(),
        gateway_mac.as_bytes(),
    ] {
        hasher.update((value.len() as u64).to_be_bytes());
        hasher.update(value);
    }
    let digest = hasher.finalize();
    let mut key = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(key, "{byte:02x}");
    }
    NetworkFingerprint::Stable { key }
}

// ─── Исполнение ─────────────────────────────────────────────────────────────

#[cfg_attr(not(windows), allow(dead_code))]
fn load_cache(paths: &Paths) -> NetIdCache {
    match std::fs::read_to_string(paths.netid_cache_path()) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => NetIdCache::default(),
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
fn save_cache(paths: &Paths, cache: &NetIdCache) {
    let path = paths.netid_cache_path();
    let json = serde_json::to_string_pretty(cache).unwrap_or_default();
    let tmp = path.with_extension("json.tmp");
    // Атомарно: temp + rename. При неудаче НЕ пишем в целевой файл напрямую (это
    // оставило бы обрезанный JSON при краше) — кэш некритичен, убираем tmp.
    if std::fs::write(&tmp, &json)
        .and_then(|_| std::fs::rename(&tmp, &path))
        .is_err()
    {
        let _ = std::fs::remove_file(&tmp);
    }
}

#[cfg(windows)]
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Runs one hidden read-only system command within a caller-owned absolute
/// deadline. Timed-out children are killed and synchronously reaped before
/// returning, so cancellation of the surrounding async task cannot leave an
/// unbounded route/ARP/ping process behind.
#[cfg(windows)]
fn run_local_command_until(program: &str, args: &[&str], deadline: Instant) -> Option<Vec<u8>> {
    use crate::util::std_command;

    if Instant::now() >= deadline {
        return None;
    }

    let mut command = std_command(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn().ok()?;
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    };

    // Drain concurrently so an unexpectedly large route table cannot fill a
    // pipe and stall the child. The extra byte distinguishes exact-fit output
    // from an oversized stream without an unbounded allocation.
    let reader = match std::thread::Builder::new()
        .name("netid-command-output".to_owned())
        .spawn(move || -> std::io::Result<Vec<u8>> {
            let mut output = Vec::new();
            stdout
                .take(MAX_SYSTEM_COMMAND_OUTPUT_BYTES.saturating_add(1))
                .read_to_end(&mut output)?;
            if u64::try_from(output.len()).unwrap_or(u64::MAX) > MAX_SYSTEM_COMMAND_OUTPUT_BYTES {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "local network command output exceeded its limit",
                ));
            }
            Ok(output)
        }) {
        Ok(reader) => reader,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
    };

    let completed = loop {
        match child.try_wait() {
            Ok(Some(_)) => break true,
            Ok(None) => {}
            Err(_) => break false,
        }

        let now = Instant::now();
        if now >= deadline {
            break false;
        }
        std::thread::sleep(SYSTEM_COMMAND_POLL_INTERVAL.min(deadline.duration_since(now)));
    };

    if !completed {
        let _ = child.kill();
    }
    // Always reap, including the race where the child exits between try_wait
    // and kill. Dropping a timed-out Child would leak the process handle and
    // could leave the command running.
    let _ = child.wait();
    let output = reader.join().ok()?.ok()?;
    completed.then_some(output)
}

/// Собирает локальные (оффлайн) идентификаторы сети синхронными командами.
/// Возвращает `(gateway_ip, gateway_mac)`.
#[cfg(windows)]
fn gather_local() -> (Option<String>, Option<String>) {
    let deadline = Instant::now() + LOCAL_READ_ONLY_BUDGET;
    let route = run_local_command_until("route", &["print"], deadline)
        .map(|output| String::from_utf8_lossy(&output).into_owned())
        .unwrap_or_default();
    let gw = parse_gateway_ip(&route).or_else(|| {
        let ipcfg = run_local_command_until("ipconfig", &[], deadline)
            .map(|output| String::from_utf8_lossy(&output).into_owned())
            .unwrap_or_default();
        parse_gateway_ip_ipconfig(&ipcfg)
    });

    let mac = gw.as_ref().and_then(|ip| {
        let arp = run_local_command_until("arp", &["-a"], deadline)
            .map(|output| String::from_utf8_lossy(&output).into_owned())
            .unwrap_or_default();
        parse_mac_for_ip(&arp, ip)
    });

    (gw, mac)
}

/// Collects only the local route/interface/gateway identity. No external
/// request and no cache access is permitted on this path.
#[cfg(windows)]
pub async fn resolve_local_read_only() -> LocalNetworkIdentity {
    tokio::task::spawn_blocking(|| {
        let deadline = Instant::now() + LOCAL_READ_ONLY_BUDGET;
        let route_text = run_local_command_until("route", &["print"], deadline)
            .map(|output| String::from_utf8_lossy(&output).into_owned())
            .unwrap_or_default();
        let route = parse_default_route(&route_text);

        let gateway_mac = route.as_ref().and_then(|route| {
            let read_arp = || {
                let arp = run_local_command_until("arp", &["-a"], deadline)
                    .map(|output| String::from_utf8_lossy(&output).into_owned())
                    .unwrap_or_default();
                parse_mac_for_ip(&arp, &route.gateway_ip)
            };
            read_arp().or_else(|| {
                // Sending one bounded ping forces Windows to resolve the
                // gateway at L2 even when ICMP echo itself is disabled. This
                // avoids treating an expired ARP entry as a network change.
                let _ = run_local_command_until(
                    "ping",
                    &["-n", "1", "-w", "250", route.gateway_ip.as_str()],
                    deadline,
                );
                read_arp()
            })
        });

        let Some(route) = route else {
            return LocalNetworkIdentity::default();
        };
        LocalNetworkIdentity {
            online: true,
            interface_up: true,
            default_route_available: true,
            // The presence of a selected gateway proves routing configuration;
            // missing ARP identity makes the fingerprint unstable, not offline.
            gateway_reachable: true,
            fingerprint: local_fingerprint(&route, gateway_mac.as_deref()),
        }
    })
    .await
    .unwrap_or_default()
}

#[cfg(not(windows))]
pub async fn resolve_local_read_only() -> LocalNetworkIdentity {
    LocalNetworkIdentity::default()
}

/// Определяет идентичность сети: MAC шлюза (ключ L1) + ASN_region (ключ L2).
/// Читает/пишет `netid_cache.json`; ipinfo дёргается только для новой сети.
#[cfg(windows)]
pub async fn resolve(paths: &Paths) -> NetIdentity {
    let (gw_ip, mac) = tokio::task::spawn_blocking(gather_local)
        .await
        .unwrap_or((None, None));

    // Ключ кэша: MAC шлюза, иначе синтетика по IP, иначе "unknown".
    let key = mac
        .clone()
        .or_else(|| gw_ip.clone().map(|ip| format!("gw:{ip}")))
        .unwrap_or_else(|| "unknown".to_string());

    let mut cache = load_cache(paths);
    if let Some(entry) = cache.networks.get(&key) {
        return NetIdentity {
            gateway_mac: mac.or_else(|| Some(key.clone())),
            asn_region: entry.asn_region.clone(),
            org: entry.org.clone(),
        };
    }

    // Новая сеть → один запрос ipinfo.
    let (asn_region, org) = fetch_ipinfo().await;
    cache.networks.insert(
        key.clone(),
        CacheEntry {
            asn_region: asn_region.clone(),
            org: org.clone(),
            fetched_at: now_secs(),
        },
    );
    save_cache(paths, &cache);

    NetIdentity {
        gateway_mac: mac.or(Some(key)),
        asn_region,
        org,
    }
}

/// Определяет ASN_region через геоIP. Пробует ipinfo.io, при неудаче/недоборе —
/// фолбэк ip-api.com (ipinfo часто заблокирован ТСПУ — тем же, что мы обходим).
/// Возвращает `(asn_region_key, org)`.
#[cfg(windows)]
async fn fetch_ipinfo() -> (Option<String>, Option<String>) {
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(_) => return (None, None),
    };

    // Основной источник — ipinfo.io.
    if let Ok(resp) = client.get("https://ipinfo.io/json").send().await {
        if let Ok(body) = resp.text().await {
            let (asn, region, org) = parse_ipinfo(&body);
            let key = asn_region_key(asn.as_deref(), region.as_deref());
            if key.is_some() {
                return (key, org);
            }
        }
    }

    // Фолбэк — ip-api.com (иная схема, обычно доступен из РФ).
    if let Ok(resp) = client.get("http://ip-api.com/json/").send().await {
        if let Ok(body) = resp.text().await {
            let (asn, region, org) = parse_ipapi(&body);
            return (asn_region_key(asn.as_deref(), region.as_deref()), org);
        }
    }

    (None, None)
}

/// Разбор `ip-api.com/json` (фолбэк, другая схема): `(asn, region_code, org)`.
/// Поля: `countryCode`="RU", `region`="MOW" (уже ISO-код субъекта), `as`="AS12389 …".
#[cfg_attr(not(windows), allow(dead_code))]
pub fn parse_ipapi(json: &str) -> (Option<String>, Option<String>, Option<String>) {
    let v: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return (None, None, None),
    };
    // ip-api сигналит успех строкой status.
    if v.get("status").and_then(|s| s.as_str()) == Some("fail") {
        return (None, None, None);
    }
    let as_field = v.get("as").and_then(|x| x.as_str());
    let org = v
        .get("isp")
        .and_then(|x| x.as_str())
        .or(as_field)
        .map(|s| s.to_string());
    let asn = as_field.and_then(|s| {
        let first = s.split_whitespace().next()?;
        if first.starts_with("AS")
            && first.len() > 2
            && first[2..].chars().all(|c| c.is_ascii_digit())
        {
            Some(first.to_string())
        } else {
            None
        }
    });
    let country = v.get("countryCode").and_then(|x| x.as_str());
    let region = v.get("region").and_then(|x| x.as_str());
    let region_code = match (country, region) {
        (Some(c), Some(r)) if !c.is_empty() && !r.is_empty() => {
            Some(format!("{}-{}", c.to_ascii_uppercase(), region_slug(r)))
        }
        _ => None,
    };
    (asn, region_code, org)
}

/// Стаб для не-windows целей — сеть не определяется, Мозг работает на L3.
#[cfg(not(windows))]
pub async fn resolve(_paths: &Paths) -> NetIdentity {
    NetIdentity::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_gateway_from_route_print() {
        let out = "\
===========================================================================
IPv4 Route Table
===========================================================================
Active Routes:
Network Destination        Netmask          Gateway       Interface  Metric
          0.0.0.0          0.0.0.0      192.168.1.1     192.168.1.100     25
        127.0.0.0        255.0.0.0         On-link       127.0.0.1    331
";
        assert_eq!(parse_gateway_ip(out), Some("192.168.1.1".to_string()));
        assert_eq!(
            parse_default_route(out),
            Some(DefaultRoute {
                gateway_ip: "192.168.1.1".to_string(),
                interface_ip: "192.168.1.100".to_string(),
            })
        );
    }

    #[test]
    fn local_fingerprint_is_hashed_and_requires_gateway_identity() {
        let route = DefaultRoute {
            gateway_ip: "192.168.1.1".to_owned(),
            interface_ip: "192.168.1.100".to_owned(),
        };
        let stable = local_fingerprint(&route, Some("aa:bb:cc:dd:ee:ff"));
        let NetworkFingerprint::Stable { key } = stable else {
            panic!("complete local identity must be stable");
        };
        assert_eq!(key.len(), 64);
        assert!(!key.contains("192.168"));
        assert!(!key.contains("aa:bb"));
        assert_eq!(
            local_fingerprint(&route, None),
            NetworkFingerprint::Unstable {
                reason: "gateway_identity_unavailable".to_owned(),
            }
        );
    }

    #[test]
    fn default_route_uses_lowest_metric() {
        let routes = "\
0.0.0.0  0.0.0.0  10.8.0.1  10.8.0.2  90
0.0.0.0  0.0.0.0  192.168.1.1  192.168.1.100  25
";
        assert_eq!(
            parse_default_route(routes),
            Some(DefaultRoute {
                gateway_ip: "192.168.1.1".into(),
                interface_ip: "192.168.1.100".into(),
            })
        );
    }

    #[test]
    fn gateway_ipconfig_fallback() {
        let out = "\
   IPv4 Address. . . . . . . . . . . : 192.168.0.104
   Subnet Mask . . . . . . . . . . . : 255.255.255.0
   Default Gateway . . . . . . . . . : 192.168.0.1
";
        assert_eq!(
            parse_gateway_ip_ipconfig(out),
            Some("192.168.0.1".to_string())
        );
    }

    #[test]
    fn parses_mac_for_gateway_ip() {
        let out = "\
Interface: 192.168.1.100 --- 0x2
  Internet Address      Physical Address      Type
  192.168.1.1           aa-bb-cc-dd-ee-ff     dynamic
  192.168.1.255         ff-ff-ff-ff-ff-ff     static
";
        assert_eq!(
            parse_mac_for_ip(out, "192.168.1.1"),
            Some("aa:bb:cc:dd:ee:ff".to_string())
        );
        // Широковещательный MAC отбрасывается.
        assert_eq!(parse_mac_for_ip(out, "192.168.1.255"), None);
        // Нет такого IP.
        assert_eq!(parse_mac_for_ip(out, "10.0.0.1"), None);
    }

    #[test]
    fn parses_ipinfo_asn_region_org() {
        let json = r#"{"ip":"1.2.3.4","city":"Moscow","region":"Moscow","country":"RU","org":"AS12389 PJSC Rostelecom","timezone":"Europe/Moscow"}"#;
        let (asn, region, org) = parse_ipinfo(json);
        assert_eq!(asn.as_deref(), Some("AS12389"));
        assert_eq!(region.as_deref(), Some("RU-MOSCOW"));
        assert_eq!(org.as_deref(), Some("AS12389 PJSC Rostelecom"));
        assert_eq!(
            asn_region_key(asn.as_deref(), region.as_deref()).as_deref(),
            Some("AS12389_RU-MOSCOW")
        );
    }

    #[test]
    fn ipinfo_missing_fields_degrade() {
        let (asn, region, _) = parse_ipinfo(r#"{"ip":"1.2.3.4"}"#);
        assert!(asn.is_none());
        assert!(region.is_none());
        assert!(asn_region_key(asn.as_deref(), region.as_deref()).is_none());
        // Битый JSON — всё None, без паники.
        assert_eq!(parse_ipinfo("{not json"), (None, None, None));
    }

    #[test]
    fn parses_ipapi_fallback_schema() {
        let json = r#"{"status":"success","countryCode":"RU","region":"MOW","city":"Moscow","isp":"Rostelecom","as":"AS12389 PJSC Rostelecom"}"#;
        let (asn, region, org) = parse_ipapi(json);
        assert_eq!(asn.as_deref(), Some("AS12389"));
        assert_eq!(region.as_deref(), Some("RU-MOW"));
        assert_eq!(org.as_deref(), Some("Rostelecom"));
        assert_eq!(
            asn_region_key(asn.as_deref(), region.as_deref()).as_deref(),
            Some("AS12389_RU-MOW")
        );
        // status=fail → всё None.
        assert_eq!(
            parse_ipapi(r#"{"status":"fail","message":"private range"}"#),
            (None, None, None)
        );
    }

    #[test]
    fn region_slug_collapses_spaces() {
        assert_eq!(region_slug("Moscow"), "MOSCOW");
        assert_eq!(region_slug("  Nizhny Novgorod  "), "NIZHNY_NOVGOROD");
        assert_eq!(region_slug("Saint-Petersburg"), "SAINT_PETERSBURG");
    }

    #[test]
    fn normalize_mac_accepts_dashes_and_colons() {
        assert_eq!(
            normalize_mac("AA-BB-CC-DD-EE-FF"),
            Some("aa:bb:cc:dd:ee:ff".to_string())
        );
        assert_eq!(
            normalize_mac("aa:bb:cc:dd:ee:ff").as_deref(),
            Some("aa:bb:cc:dd:ee:ff")
        );
        assert_eq!(normalize_mac("garbage"), None);
        assert_eq!(normalize_mac("00-00-00-00-00-00"), None);
    }

    #[cfg(windows)]
    #[test]
    fn local_system_command_is_killed_and_reaped_at_shared_deadline() {
        let started = Instant::now();
        let deadline = started + Duration::from_millis(100);
        let output = run_local_command_until(
            "powershell",
            &[
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Start-Sleep -Seconds 30",
            ],
            deadline,
        );

        assert!(output.is_none());
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "timed-out child must be killed and synchronously reaped"
        );

        let expired_started = Instant::now();
        assert!(run_local_command_until("route", &["print"], deadline).is_none());
        assert!(expired_started.elapsed() < Duration::from_millis(100));
    }

    #[cfg(windows)]
    #[test]
    fn local_system_command_preserves_hidden_stdout_capture() {
        let output = run_local_command_until(
            "cmd",
            &["/D", "/C", "echo bounded"],
            Instant::now() + Duration::from_secs(2),
        )
        .expect("short hidden command must complete inside its budget");

        assert_eq!(String::from_utf8_lossy(&output).trim(), "bounded");
    }
}
