//! Общие утилиты: спавн команд без консольного окна и эмит событий в UI.

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
}
