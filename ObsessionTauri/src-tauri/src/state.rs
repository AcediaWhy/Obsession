//! Глобальное состояние приложения, управляемое Tauri (`app.state`).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicBool;
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
    pub lan_link: Option<String>,
    pub stopping: bool,
    pub forwarder: Option<tokio::task::JoinHandle<()>>,
}

pub struct AppState {
    pub paths: Paths,
    pub dpi: Mutex<DpiState>,
    /// Сериализует DPI-операции (start_many/stop_all/test) между собой. Без него
    /// частые клики по «Старт» запускают параллельные start/stop, которые топчут
    /// общий `procs` и поднимают Глаза на уже убитый winws → обход не детектится.
    /// Async-мьютекс (держится через `.await`), НЕ std — операции асинхронные.
    pub dpi_gate: tokio::sync::Mutex<()>,
    /// Флаг отмены текущего теста конфигов. Ставится командой `dpi_test_cancel`
    /// (без ворот, чтобы сработать, пока тест их держит); `dpi::test` проверяет
    /// его между этапами и обрывается досрочно, освобождая обход.
    pub test_cancel: AtomicBool,
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
            dpi_gate: tokio::sync::Mutex::new(()),
            test_cancel: AtomicBool::new(false),
            proxy: Mutex::new(ProxyState::default()),
            settings: Mutex::new(settings),
            #[cfg(windows)]
            eyes: Mutex::new(None),
            brain: Mutex::new(None),
            netid: Mutex::new(None),
        }
    }
}
