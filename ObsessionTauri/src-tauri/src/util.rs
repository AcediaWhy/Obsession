//! Общие утилиты: спавн команд без консольного окна и эмит событий в UI.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

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

/// Определяет локальный IPv4, используемый для выхода в интернет.
/// Фолбэк на 127.0.0.1, если сети нет.
pub fn local_ip() -> String {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0");
    if let Ok(s) = socket {
        if s.connect("8.8.8.8:80").is_ok() {
            if let Ok(addr) = s.local_addr() {
                let ip = addr.ip().to_string();
                if ip != "0.0.0.0" {
                    return ip;
                }
            }
        }
    }
    "127.0.0.1".to_string()
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
