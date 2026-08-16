//! Построчный лог в stderr. Obsession ретранслирует эти строки в UI-лог с
//! префиксом `[tg]`, поэтому каждая строка самодостаточна и флашится сразу.

use std::io::Write;

fn emit(level: &str, message: &str) {
    let now = chrono::Local::now().format("%H:%M:%S");
    let mut stderr = std::io::stderr().lock();
    let _ = writeln!(stderr, "{now} [{level}] {message}");
    let _ = stderr.flush();
}

pub fn info(message: impl AsRef<str>) {
    emit("INFO", message.as_ref());
}

pub fn warn(message: impl AsRef<str>) {
    emit("WARN", message.as_ref());
}
