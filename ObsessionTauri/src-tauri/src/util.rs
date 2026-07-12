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

/// Минимальный интервал между всплывающими уведомлениями одного типа. Во время
/// шторма ТСПУ авто-восстановление щёлкает стратегиями пачками — без троттла
/// Windows заваливает пользователя тостами быстрее, чем он успевает их закрыть.
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

/// Показывает нативное уведомление Windows с троттлингом и схлопыванием.
///
/// В пределах [`NOTIFY_COOLDOWN`] повторные уведомления с тем же `key` НЕ
/// всплывают, а копятся счётчиком; когда окно остынет, ближайшее уведомление
/// добавит к тексту «(+N за это время)» — так пачка сотни переключений даёт
/// одно-два тоста вместо спама. Внутри-приложение лог/тосты этот троттл не
/// трогает — там полная хронология.
///
/// `key` группирует однотипные события (напр. `"recover"`, `"down"`); разные
/// ключи троттлятся независимо.
pub fn notify_throttled(app: &AppHandle, key: &str, title: &str, body: &str) {
    let now = Instant::now();
    let suppressed_before = {
        let mut gates = notify_gates().lock().unwrap();
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
    let _ = app
        .notification()
        .builder()
        .title(title)
        .body(body)
        .show();
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
}

/// Выбирает IPv4, по которому телефон в общей Wi-Fi достучится до этого ПК.
///
/// `route_source_ip()` отдаёт адрес ВЫХОДА В ИНТЕРНЕТ — но у аудитории обхода
/// DPI он сплошь уходит через VPN/виртуальный адаптер (Hyper-V, WSL, VirtualBox),
/// которого нет в сети телефона → QR с ним ведёт в никуда. Поэтому перебираем
/// реальные адаптеры и берём приватный LAN-адрес (192.168 / 10 / 172.16-31) на
/// «живой» физической карте, отсекая туннели и виртуальные интерфейсы по имени.
/// Идеал — когда адрес выхода в интернет сам есть среди реальных LAN-карт
/// (обычный дом без VPN): его и берём.
pub fn lan_ip_for_phone() -> LanIpPick {
    let route_ip = route_source_ip();

    let mut candidates: Vec<Ipv4Addr> = Vec::new();
    if let Ok(ifaces) = if_addrs::get_if_addrs() {
        for iface in ifaces {
            let IfAddr::V4(v4) = &iface.addr else { continue };
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
        }
    }
    candidates.sort_by_key(|ip| private_rank(*ip));

    // Берём ЛУЧШИЙ по рангу приватный LAN (192.168 раньше 10/172). Адрес выхода
    // в интернет используем лишь как tie-break среди равно-приоритетных — так
    // даже VPN, проскочивший фильтр по имени (обычно раздаёт 10.x), не перебьёт
    // реальный домашний 192.168.x, к которому подключён телефон.
    let chosen = match candidates.first() {
        Some(&best) => {
            let best_rank = private_rank(best);
            route_ip
                .filter(|rip| private_rank(*rip) == best_rank && candidates.contains(rip))
                .unwrap_or(best)
                .to_string()
        }
        // Ни одного приличного LAN — крайний случай: egress, иначе loopback
        // (тогда LAN-ссылка/форвардер не создаются, QR только для этого ПК).
        None => route_ip
            .map(|ip| ip.to_string())
            .unwrap_or_else(|| "127.0.0.1".to_string()),
    };

    LanIpPick {
        ip: chosen,
        candidates: candidates.iter().map(|ip| ip.to_string()).collect(),
        route_ip: route_ip.map(|ip| ip.to_string()),
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

/// Виртуальные/туннельные адаптеры по подстроке в имени — их адреса телефону
/// недоступны. Это ВТОРИЧная защита: даже если что-то проскочит, выбор по рангу
/// (192.168 раньше 10/172) обычно всё равно вернёт реальный LAN. Поэтому список
/// можно держать широким без риска — лишняя фильтрация лишь откатывает к egress.
/// tap/tun/outline ловят VPN-адаптеры (в т.ч. outline-tap0 у этого пользователя).
fn is_virtual_adapter(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    const NEEDLES: &[&str] = &[
        "vethernet", "hyper-v", "virtualbox", "vmware", "vmnet", "docker", "wsl",
        "tailscale", "wireguard", "zerotier", "hamachi", "radmin", "loopback", "virtual",
        "vpn", "tap", "tun", "outline", "openvpn", "nordvpn", "mullvad", "proton",
        "surfshark", "expressvpn", "anyconnect", "forti", "globalprotect",
    ];
    NEEDLES.iter().any(|needle| n.contains(needle))
}

/// Дописывает строку лога в файл на диске. Ошибки глушим — лог не критичен.
fn append_log_file(app: &AppHandle, ts: &str, level: &str, source: &str, message: &str) {
    use std::io::Write;
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let file = state.paths.logs_dir().join("app.log");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)
    {
        let _ = writeln!(f, "{ts} [{level}] {source}: {message}");
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
}

#[derive(Clone, Serialize)]
pub struct ProxyStatusPayload {
    pub running: bool,
    pub link: String,
    pub lan_link: Option<String>,
}
