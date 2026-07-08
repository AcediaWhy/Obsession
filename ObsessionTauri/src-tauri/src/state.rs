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
    /// Активный наблюдатель трафика («Глаза»), пока запущен winws.
    #[cfg(windows)]
    pub eyes: Mutex<Option<crate::eyes::EyesHandle>>,
    /// Задача «Мозга» (L3), пока включено авто-восстановление.
    pub brain: Mutex<Option<crate::brain::runtime::BrainHandle>>,
    /// Последний снимок сетевой идентичности (Менеджер сети, L2).
    pub netid: Mutex<Option<crate::netid::NetIdentity>>,
}

impl AppState {
    pub fn new(paths: Paths, settings: Settings) -> Self {
        Self {
            paths,
            dpi: Mutex::new(DpiState::default()),
            proxy: Mutex::new(ProxyState::default()),
            settings: Mutex::new(settings),
            #[cfg(windows)]
            eyes: Mutex::new(None),
            brain: Mutex::new(None),
            netid: Mutex::new(None),
        }
    }
}
