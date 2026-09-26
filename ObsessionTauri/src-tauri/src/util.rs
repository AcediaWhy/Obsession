//! Общие утилиты: спавн команд без консольного окна и эмит событий в UI.

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use if_addrs::IfAddr;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;

/// Флаг CREATE_NO_WINDOW — дочерние процессы не открывают консоль.
#[cfg(windows)]
pub fn std_command<S: AsRef<std::ffi::OsStr>>(program: S) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut c = std::process::Command::new(program);
    c.creation_flags(CREATE_NO_WINDOW);
    c
}

#[cfg(not(windows))]
pub fn std_command<S: AsRef<std::ffi::OsStr>>(program: S) -> std::process::Command {
    std::process::Command::new(program)
}

/// Расширение std `Mutex`: берёт лок, восстанавливаясь после отравления. Паника
/// в чужой критсекции не должна каскадно ронять этот путь — особенно горячие
/// toggle/close/tray-пути и обработчик закрытия на главном event-loop потоке
/// (где падение `.unwrap()` на отравленном мьютексе повесило бы выход).
pub trait LockExt<T> {
    fn lock_recover(&self) -> std::sync::MutexGuard<'_, T>;
}

impl<T> LockExt<T> for Mutex<T> {
    fn lock_recover(&self) -> std::sync::MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Уровень лог-сообщения для UI.
#[derive(Clone, Serialize)]
pub struct LogPayload {
    pub level: String,
    pub source: String,
    pub message: String,
    pub ts: String,
}

fn now_hms() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}

/// Текущее Unix-время в секундах (0 при сбое системных часов).
pub fn unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Отправляет строку лога во фронтенд (событие `log`) И дублирует её на диск
/// в `%APPDATA%\Obsession\logs\app.log` — чтобы логи можно было прочитать
/// после закрытия окна (боковая панель UI не копируется).
pub fn emit_log(app: &AppHandle, level: &str, source: &str, message: &str) {
    let ts = now_hms();
    let _ = app.emit(
        "log",
        LogPayload {
            level: level.to_string(),
            source: source.to_string(),
            message: message.to_string(),
            ts: ts.clone(),
        },
    );
    append_log_file(app, &ts, level, source, message);
}

/// Сохраняет техническую диагностику на диск без события для журнала интерфейса.
pub(crate) fn write_diagnostic_log(app: &AppHandle, level: &str, source: &str, message: &str) {
    append_log_file(app, &now_hms(), level, source, message);
}

/// Минимальный интервал между однотипными нативными уведомлениями Windows.
const NOTIFY_COOLDOWN: Duration = Duration::from_secs(45);

/// Троттл-состояние на ключ (обычно = заголовок класса события).
struct NotifyGate {
    last: Option<Instant>,
    /// Сколько уведомлений подавлено с момента последнего показанного.
    suppressed: u32,
}

fn notify_gates() -> &'static Mutex<HashMap<String, NotifyGate>> {
    static G: OnceLock<Mutex<HashMap<String, NotifyGate>>> = OnceLock::new();
    G.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Показывает нативное уведомление Windows. Повторы с тем же `key` в пределах
/// [`NOTIFY_COOLDOWN`] подавляются; следующее уведомление сообщает их число.
/// Ключи обрабатываются независимо. Лог и уведомления внутри приложения не затронуты.
pub fn notify_throttled(app: &AppHandle, key: &str, title: &str, body: &str) {
    let now = Instant::now();
    let suppressed_before = {
        let mut gates = notify_gates().lock_recover();
        let gate = gates.entry(key.to_string()).or_insert(NotifyGate {
            last: None,
            suppressed: 0,
        });
        let cool = gate
            .last
            .map(|t| now.duration_since(t) < NOTIFY_COOLDOWN)
            .unwrap_or(false);
        if cool {
            // Ещё остываем — подавляем, но считаем.
            gate.suppressed = gate.suppressed.saturating_add(1);
            return;
        }
        // Показываем: фиксируем время и забираем накопленный счётчик.
        gate.last = Some(now);
        std::mem::take(&mut gate.suppressed)
    };

    let body = if suppressed_before > 0 {
        format!("{body} (+{suppressed_before} за это время)")
    } else {
        body.to_string()
    };
    notify_now(app, title, &body);
}

/// Показывает нативное уведомление Windows без троттла. Ошибки глушим —
/// уведомление не критично (например, если пользователь отключил их в системе).
pub(crate) fn notify_now(app: &AppHandle, title: &str, body: &str) {
    use tauri_plugin_notification::NotificationExt;
    let _ = app.notification().builder().title(title).body(body).show();
}

/// Адрес выхода в интернет через «трюк с UDP»: пакет не шлётся, ядро лишь
/// выбирает исходный интерфейс маршрута к 8.8.8.8. Это адрес ДЕФОЛТНОГО
/// маршрута — при активном VPN/виртуальном адаптере он может быть НЕ-LAN.
fn route_source_ip() -> Option<Ipv4Addr> {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("8.8.8.8:80").ok()?;
    match s.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(ip) if !ip.is_unspecified() => Some(ip),
        _ => None,
    }
}

/// Результат выбора LAN-адреса для QR: сам адрес + диагностика для лога.
pub struct LanIpPick {
    /// Адрес для QR/ссылки телефона, либо "127.0.0.1", если LAN не найден.
    pub ip: String,
    /// Все подходящие приватные LAN-адреса (для лога/диагностики).
    pub candidates: Vec<String>,
    /// Адрес выхода в интернет (если ≠ ip — вероятен VPN/вирт. адаптер).
    pub route_ip: Option<String>,
    /// CIDR-подсеть выбранного интерфейса (напр. "192.168.1.0/24") для строгого
    /// firewall-правила `remoteip=`. `None`, если подсеть определить не удалось —
    /// тогда вызывающий берёт безопасный фолбэк `LocalSubnet`.
    pub subnet: Option<String>,
}

/// CIDR-подсеть по IP и маске: `network/prefix` (напр. 192.168.1.0/24).
pub fn ipv4_cidr(ip: Ipv4Addr, netmask: Ipv4Addr) -> String {
    let net = u32::from(ip) & u32::from(netmask);
    let prefix = u32::from(netmask).count_ones();
    format!("{}/{prefix}", Ipv4Addr::from(net))
}

/// Выбирает IPv4, по которому телефон в общей Wi-Fi достучится до этого ПК.
///
/// Выбирает приватный LAN-адрес, доступный телефону. Адрес маршрута в интернет
/// может принадлежать VPN, поэтому сначала проверяются активные адаптеры,
/// затем применяется приоритет адресов и адрес маршрута как дополнительный критерий.
pub fn lan_ip_for_phone() -> LanIpPick {
    let route_ip = route_source_ip();

    let mut candidates: Vec<Ipv4Addr> = Vec::new();
    // Маска выбранного IP — для вычисления CIDR-подсети firewall-правила.
    let mut netmasks: std::collections::HashMap<Ipv4Addr, Ipv4Addr> =
        std::collections::HashMap::new();
    if let Ok(ifaces) = if_addrs::get_if_addrs() {
        for iface in ifaces {
            let IfAddr::V4(v4) = &iface.addr else {
                continue;
            };
            let ip = v4.ip;
            if !ip.is_private() {
                continue; // только 10/8, 172.16/12, 192.168/16 (отсекает 127/169.254/публичные)
            }
            if !iface.is_oper_up() {
                continue; // адаптер реально поднят
            }
            if is_virtual_adapter(&iface.name) {
                continue; // туннели/виртуалки — их адрес телефону недоступен
            }
            if !candidates.contains(&ip) {
                candidates.push(ip);
            }
            netmasks.entry(ip).or_insert(v4.netmask);
        }
    }
    candidates.sort_by_key(|ip| private_rank(*ip));

    // Приоритет: 192.168, затем 10 и 172. Адрес маршрута выбираем только
    // среди кандидатов с одинаковым приоритетом.
    let chosen = match candidates.first() {
        Some(&best) => {
            let best_rank = private_rank(best);
            route_ip
                .filter(|rip| private_rank(*rip) == best_rank && candidates.contains(rip))
                .unwrap_or(best)
                .to_string()
        }
        // Без LAN-кандидата используем адрес маршрута или loopback.
        None => route_ip
            .map(|ip| ip.to_string())
            .unwrap_or_else(|| "127.0.0.1".to_string()),
    };

    // Подсеть вычисляется только для выбранного адреса из активного LAN-адаптера.
    let subnet = chosen
        .parse::<Ipv4Addr>()
        .ok()
        .filter(|ip| ip.is_private())
        .and_then(|ip| netmasks.get(&ip).map(|mask| ipv4_cidr(ip, *mask)));

    LanIpPick {
        ip: chosen,
        candidates: candidates.iter().map(|ip| ip.to_string()).collect(),
        route_ip: route_ip.map(|ip| ip.to_string()),
        subnet,
    }
}

/// Приоритет приватных диапазонов для домашней Wi-Fi: 192.168 → 10 → 172.16-31.
/// Дефолтную host-only сеть VirtualBox (192.168.56.x) двигаем в самый низ как
/// tie-break на случай, если фильтр по имени её не поймал.
fn private_rank(ip: Ipv4Addr) -> u8 {
    match ip.octets() {
        [192, 168, 56, _] => 5,
        [192, 168, ..] => 0,
        [10, ..] => 1,
        _ => 2, // 172.16-31
    }
}

/// Исключает известные виртуальные и туннельные адаптеры по имени.
/// Проверка дополняет приоритет приватных LAN-адресов.
fn is_virtual_adapter(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    const NEEDLES: &[&str] = &[
        "vethernet",
        "hyper-v",
        "virtualbox",
        "vmware",
        "vmnet",
        "docker",
        "wsl",
        "tailscale",
        "wireguard",
        "zerotier",
        "hamachi",
        "radmin",
        "loopback",
        "virtual",
        "vpn",
        "tap",
        "tun",
        "outline",
        "openvpn",
        "nordvpn",
        "mullvad",
        "proton",
        "surfshark",
        "expressvpn",
        "anyconnect",
        "forti",
        "globalprotect",
    ];
    NEEDLES.iter().any(|needle| n.contains(needle))
}

/// Порог буфера, после которого сбрасываем строки на диск.
const LOG_FLUSH_BYTES: usize = 4096;
/// Максимальное время хранения строк в буфере перед записью на диск.
const LOG_FLUSH_INTERVAL: Duration = Duration::from_secs(2);

/// Уровни событий, требующие немедленной записи на диск.
fn log_level_is_important(level: &str) -> bool {
    matches!(level, "error" | "warn" | "success")
}

fn should_flush_log(level: &str, buffered: usize, since_flush: Duration) -> bool {
    log_level_is_important(level)
        || buffered >= LOG_FLUSH_BYTES
        || since_flush >= LOG_FLUSH_INTERVAL
}

struct LogSink {
    writer: std::io::BufWriter<std::fs::File>,
    last_flush: Instant,
}

fn log_sink() -> &'static Mutex<Option<LogSink>> {
    static SINK: Mutex<Option<LogSink>> = Mutex::new(None);
    &SINK
}

/// Сбрасывает буфер лога при завершении приложения: статический `LogSink`
/// не освобождается автоматически.
pub fn flush_log_file() {
    use std::io::Write;
    let mut guard = log_sink().lock_recover();
    if let Some(sink) = guard.as_mut() {
        let _ = sink.writer.flush();
        sink.last_flush = Instant::now();
    }
}

/// Дописывает строку в общий буфер лога и сбрасывает его по уровню, размеру
/// или времени. Ошибки записи не прерывают работу приложения.
fn append_log_file(app: &AppHandle, ts: &str, level: &str, source: &str, message: &str) {
    use std::io::Write;
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let file = state.paths.logs_dir().join("app.log");
    // Мьютекс не даёт строкам из разных потоков перемешаться в файле.
    let mut guard = log_sink().lock_recover();
    if guard.is_none() {
        match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file)
        {
            Ok(f) => {
                *guard = Some(LogSink {
                    writer: std::io::BufWriter::with_capacity(8192, f),
                    last_flush: Instant::now(),
                })
            }
            Err(_) => return,
        }
    }
    if let Some(sink) = guard.as_mut() {
        let _ = writeln!(sink.writer, "{ts} [{level}] {source}: {message}");
        if should_flush_log(level, sink.writer.buffer().len(), sink.last_flush.elapsed()) {
            let _ = sink.writer.flush();
            sink.last_flush = Instant::now();
        }
    }
}

#[cfg(test)]
mod log_flush_tests {
    use super::{should_flush_log, LOG_FLUSH_BYTES, LOG_FLUSH_INTERVAL};
    use std::time::Duration;

    #[test]
    fn important_levels_flush_immediately() {
        for level in ["error", "warn", "success"] {
            assert!(should_flush_log(level, 0, Duration::ZERO));
        }
    }

    #[test]
    fn quiet_info_waits_for_the_interval_or_the_buffer() {
        assert!(!should_flush_log("info", 0, Duration::ZERO));
        assert!(!should_flush_log(
            "info",
            LOG_FLUSH_BYTES - 1,
            LOG_FLUSH_INTERVAL - Duration::from_millis(1)
        ));
        assert!(should_flush_log("info", LOG_FLUSH_BYTES, Duration::ZERO));
        assert!(should_flush_log("info", 0, LOG_FLUSH_INTERVAL));
    }

    #[test]
    fn debug_is_not_treated_as_important() {
        assert!(!should_flush_log("debug", 1, Duration::ZERO));
    }
}

/// Значение subsystem вместе с независимой monotonic revision.
#[derive(Clone, Debug, Serialize)]
pub struct VersionedSection<T> {
    pub revision: u64,
    pub value: T,
}

impl<T> VersionedSection<T> {
    pub fn new(revision: u64, value: T) -> Self {
        Self { revision, value }
    }
}

#[derive(Clone, Serialize)]
pub struct DpiProcPublic {
    pub pid: u32,
    pub category: String,
    pub config_file: String,
}

#[derive(Clone, Serialize)]
pub struct DpiStatusPayload {
    pub active: bool,
    pub processes: Vec<DpiProcPublic>,
    /// Unix-время (сек) старта текущей сессии обхода; null = обход выключен.
    /// UI считает аптайм от него (переживает смену вкладок / resume из трея).
    pub started_at: Option<u64>,
}

#[derive(Clone, Serialize)]
pub struct ProxyStatusPayload {
    pub running: bool,
    pub link: String,
    pub lan_link: Option<String>,
    /// Активна ли LAN-публикация (0.0.0.0 forwarder + firewall).
    pub lan_published: bool,
    /// Unix-время (сек) авто-закрытия LAN-публикации; null = без авто-закрытия.
    pub lan_expiry_unix: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipv4_cidr_computes_network_and_prefix() {
        assert_eq!(
            ipv4_cidr(
                Ipv4Addr::new(192, 168, 1, 37),
                Ipv4Addr::new(255, 255, 255, 0)
            ),
            "192.168.1.0/24"
        );
        assert_eq!(
            ipv4_cidr(Ipv4Addr::new(10, 5, 6, 7), Ipv4Addr::new(255, 0, 0, 0)),
            "10.0.0.0/8"
        );
        assert_eq!(
            ipv4_cidr(
                Ipv4Addr::new(172, 20, 130, 5),
                Ipv4Addr::new(255, 255, 0, 0)
            ),
            "172.20.0.0/16"
        );
    }
}
