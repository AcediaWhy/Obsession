//! Персистентные настройки в `%APPDATA%\Obsession\settings.json`.
//! Замена SharedPreferences (`settings_local_datasource.dart`).

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub minimize_to_tray: bool,
    pub start_minimized: bool,
    pub selected_categories: Vec<String>,
    pub selected_configs: HashMap<String, String>,
    pub proxy_port: u16,
    pub fake_tls_domain: String,
    pub ai_provider: String,
    pub has_completed_onboarding: bool,
    /// Авто-восстановление обхода (Мозг L3). Default false — пока не обкатано.
    pub auto_recovery: bool,
    /// Меньше анимаций: гасит canvas/WebGL-фон и Framer-циклы (a11y + экономия
    /// CPU/батареи). Default false; фронт также уважает `prefers-reduced-motion`.
    pub reduce_motion: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            minimize_to_tray: true,
            start_minimized: false,
            selected_categories: vec!["discord".to_string()],
            selected_configs: HashMap::new(),
            proxy_port: 1443,
            fake_tls_domain: String::new(),
            ai_provider: "malw".to_string(),
            has_completed_onboarding: false,
            auto_recovery: false,
            reduce_motion: false,
        }
    }
}

impl Settings {
    fn file(base_dir: &Path) -> std::path::PathBuf {
        base_dir.join("settings.json")
    }

    pub fn load(base_dir: &Path) -> Self {
        match std::fs::read_to_string(Self::file(base_dir)) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, base_dir: &Path) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self).unwrap_or_default();
        std::fs::write(Self::file(base_dir), json)
    }
}
