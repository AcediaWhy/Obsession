//! Глобальное состояние приложения, управляемое Tauri (`app.state`).

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use crate::paths::Paths;
use crate::settings::Settings;

/// Отслеживаемый DPI-процесс winws.
pub struct DpiProc {
    pub pid: u32,
    pub category: String,
    pub config_file: String,
}

#[derive(Default)]
pub struct DpiState {
    /// Активные (свои) процессы winws по PID.
    pub procs: HashMap<u32, DpiProc>,
    /// PID, которые останавливаем намеренно — чтобы монитор не считал это крахом.
    pub stopping: HashSet<u32>,
}

#[derive(Default)]
pub struct ProxyState {
    pub pid: Option<u32>,
    pub link: String,
    pub stopping: bool,
}

pub struct AppState {
    pub paths: Paths,
    pub dpi: Mutex<DpiState>,
    pub proxy: Mutex<ProxyState>,
    pub settings: Mutex<Settings>,
}

impl AppState {
    pub fn new(paths: Paths, settings: Settings) -> Self {
        Self {
            paths,
            dpi: Mutex::new(DpiState::default()),
            proxy: Mutex::new(ProxyState::default()),
            settings: Mutex::new(settings),
        }
    }
}
