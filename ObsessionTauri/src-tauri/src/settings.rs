//! Персистентные настройки в `%APPDATA%\Obsession\settings.json`.
//! Запись выполняется через temp + atomic replace, чтобы crash не оставлял
//! обрезанный JSON. Частичные обновления применяются к актуальному снимку под
//! единым backend lock (см. `commands::update_settings`).

use std::collections::HashMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

static SAVE_SEQUENCE: AtomicU64 = AtomicU64::new(1);
const LEGACY_RELIABILITY_MIGRATION_VERSION: u8 = 1;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub minimize_to_tray: bool,
    pub start_minimized: bool,
    /// Legacy categories stay independent from the Zapret2 service selection.
    pub selected_categories: Vec<String>,
    pub zapret2_selected_categories: Vec<String>,
    pub selected_configs: HashMap<String, String>,
    pub proxy_port: u16,
    pub fake_tls_domain: String,
    pub ai_provider: String,
    pub has_completed_onboarding: bool,
    /// Compatibility flag for the quarantined L3 Brain. It is independent from
    /// Legacy Reliability and can never opt in to the new Automatic mode.
    pub auto_recovery: bool,
    /// One-way migration marker. Field-level serde default intentionally maps
    /// old files without this property to zero, while new defaults start current.
    #[serde(default)]
    pub legacy_reliability_migration_version: u8,
    /// Legacy Reliability rollout mode. Automatic remains a separate explicit
    /// opt-in and is never inferred from the compatibility `auto_recovery` flag.
    pub legacy_reliability_mode: String,
    /// Master capability switch. When false, Eyes/Manager are not started and
    /// recovery settings are retained only for a future explicit re-enable.
    pub legacy_reliability_enabled: bool,
    /// Persisted Phase 4 kill switch. Missing/old settings deserialize to the
    /// safe engaged state; explicitly selecting Automatic disarms it atomically.
    pub legacy_automatic_paused: bool,
    /// Categories excluded from future Assisted/Automatic attempts. Active
    /// stop/start transactions still finish to candidate or exact rollback.
    pub legacy_reliability_frozen_categories: Vec<String>,
    /// Меньше анимаций: гасит canvas/WebGL-фон и Framer-циклы.
    pub reduce_motion: bool,
    /// Глобальный хоткей вкл/выкл защиты. Пустая строка = выключен.
    pub hotkey_toggle: String,
    /// Таймаут публикации прокси в LAN (сек). По истечении снимаются forwarder
    /// 0.0.0.0 и firewall-правило (локальный 127.0.0.1 прокси продолжает жить).
    /// 0 = без авто-закрытия (публикация до ручного стопа).
    pub lan_publish_secs: u16,
    /// Выбранный DPI-движок: "legacy" (Zapret1, default) или "zapret2" (Beta).
    /// Мозг никогда не выставляет zapret2 автоматически.
    pub dpi_engine: String,
    /// Уровень агрессивности стратегии Zapret2 (1=мягко … 3=жёстко). Выбирает
    /// стратегию из лестницы пака по aggressiveness. 0 = авто (максимальная).
    pub zapret2_level: u8,
    /// Локальный подбор Safe Strategy DSL для Zapret2. Отдельный feature flag.
    pub adaptive_strategy_enabled: bool,
    /// Adaptive search budget: balanced (default), fast or deep.
    pub adaptive_search_mode: String,
}

/// Частичное обновление настроек. `Option` отличает «поле не меняется» от false,
/// пустой строки или пустого списка, которые являются валидными значениями.
#[derive(Default, Deserialize)]
#[serde(default)]
pub struct SettingsPatch {
    pub minimize_to_tray: Option<bool>,
    pub start_minimized: Option<bool>,
    pub selected_categories: Option<Vec<String>>,
    pub zapret2_selected_categories: Option<Vec<String>>,
    pub selected_configs: Option<HashMap<String, String>>,
    pub proxy_port: Option<u16>,
    pub fake_tls_domain: Option<String>,
    pub ai_provider: Option<String>,
    pub has_completed_onboarding: Option<bool>,
    pub auto_recovery: Option<bool>,
    pub legacy_reliability_enabled: Option<bool>,
    pub legacy_reliability_mode: Option<String>,
    pub legacy_automatic_paused: Option<bool>,
    pub legacy_reliability_frozen_categories: Option<Vec<String>>,
    pub reduce_motion: Option<bool>,
    pub hotkey_toggle: Option<String>,
    pub lan_publish_secs: Option<u16>,
    pub dpi_engine: Option<String>,
    pub zapret2_level: Option<u8>,
    pub adaptive_strategy_enabled: Option<bool>,
    pub adaptive_search_mode: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            minimize_to_tray: true,
            start_minimized: false,
            selected_categories: vec!["discord".to_string()],
            zapret2_selected_categories: vec!["discord".to_string()],
            selected_configs: HashMap::new(),
            proxy_port: 1443,
            fake_tls_domain: String::new(),
            ai_provider: "malw".to_string(),
            has_completed_onboarding: false,
            auto_recovery: false,
            legacy_reliability_migration_version: LEGACY_RELIABILITY_MIGRATION_VERSION,
            legacy_reliability_enabled: true,
            legacy_reliability_mode: "observe_only".to_string(),
            legacy_automatic_paused: true,
            legacy_reliability_frozen_categories: Vec::new(),
            reduce_motion: false,
            hotkey_toggle: "Ctrl+Shift+KeyO".to_string(),
            lan_publish_secs: 0,
            dpi_engine: "legacy".to_string(),
            zapret2_level: 0, // 0 = авто (максимальная агрессивность)
            adaptive_strategy_enabled: false,
            adaptive_search_mode: "balanced".to_string(),
        }
    }
}

impl Settings {
    fn file(base_dir: &Path) -> PathBuf {
        base_dir.join("settings.json")
    }

    pub fn load(base_dir: &Path) -> Self {
        let mut settings = match std::fs::read_to_string(Self::file(base_dir)) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => Self::default(),
        };
        // Migrate the old global Brain flag exactly once. A user may explicitly
        // re-enable that compatibility feature later, but it never arms Legacy
        // Automatic because the two controls remain independent.
        if settings.legacy_reliability_migration_version < LEGACY_RELIABILITY_MIGRATION_VERSION {
            settings.legacy_reliability_migration_version = LEGACY_RELIABILITY_MIGRATION_VERSION;
            if settings.auto_recovery {
                settings.auto_recovery = false;
                settings.legacy_reliability_mode = "observe_only".to_string();
                settings.legacy_automatic_paused = true;
            }
        }
        if !matches!(
            settings.legacy_reliability_mode.as_str(),
            "observe_only" | "assisted" | "automatic"
        ) {
            settings.legacy_reliability_mode = "observe_only".to_string();
            settings.legacy_automatic_paused = true;
        } else if settings.legacy_reliability_mode != "automatic" {
            settings.legacy_automatic_paused = true;
        }
        sanitize_frozen_categories(&mut settings.legacy_reliability_frozen_categories);
        settings
    }

    pub fn apply_patch(&mut self, patch: SettingsPatch) {
        macro_rules! apply {
            ($field:ident) => {
                if let Some(value) = patch.$field {
                    self.$field = value;
                }
            };
        }
        apply!(minimize_to_tray);
        apply!(start_minimized);
        apply!(selected_categories);
        apply!(zapret2_selected_categories);
        apply!(selected_configs);
        apply!(proxy_port);
        apply!(fake_tls_domain);
        apply!(ai_provider);
        apply!(has_completed_onboarding);
        if patch.auto_recovery.is_some() {
            self.auto_recovery = false;
        }
        apply!(legacy_reliability_enabled);
        let requested_pause = patch.legacy_automatic_paused;
        if let Some(mode) = patch.legacy_reliability_mode {
            match mode.as_str() {
                "automatic" => {
                    self.legacy_reliability_mode = mode;
                    self.legacy_automatic_paused = requested_pause.unwrap_or(false);
                }
                "assisted" => {
                    self.legacy_reliability_mode = mode;
                    self.legacy_automatic_paused = true;
                }
                _ => {
                    self.legacy_reliability_mode = "observe_only".to_string();
                    self.legacy_automatic_paused = true;
                }
            }
        } else if let Some(paused) = requested_pause {
            self.legacy_automatic_paused = if self.legacy_reliability_mode == "automatic" {
                paused
            } else {
                true
            };
        }
        if let Some(mut categories) = patch.legacy_reliability_frozen_categories {
            sanitize_frozen_categories(&mut categories);
            self.legacy_reliability_frozen_categories = categories;
        }
        apply!(reduce_motion);
        apply!(hotkey_toggle);
        apply!(lan_publish_secs);
        apply!(dpi_engine);
        apply!(zapret2_level);
        apply!(adaptive_strategy_enabled);
        if let Some(mode) = patch.adaptive_search_mode {
            self.adaptive_search_mode = match mode.as_str() {
                "fast" | "deep" => mode,
                _ => "balanced".to_string(),
            };
        }
    }

    /// Durable temp write + atomic replace в том же каталоге.
    pub fn save(&self, base_dir: &Path) -> io::Result<()> {
        std::fs::create_dir_all(base_dir)?;
        let destination = Self::file(base_dir);
        let sequence = SAVE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary = base_dir.join(format!(
            ".settings.json.tmp.{}.{}",
            std::process::id(),
            sequence
        ));
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        let result = (|| {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            atomic_replace_with_retry(&temporary, &destination)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
    }
}

fn sanitize_frozen_categories(categories: &mut Vec<String>) {
    for category in categories.iter_mut() {
        category.make_ascii_lowercase();
    }
    categories.retain(|category| {
        !category.is_empty()
            && category.len() <= 64
            && category
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    });
    categories.sort_unstable();
    categories.dedup();
    categories.truncate(64);
}

fn atomic_replace_with_retry(source: &Path, destination: &Path) -> io::Result<()> {
    let mut last_error = None;
    for attempt in 0..50 {
        match atomic_replace(source, destination) {
            Ok(()) => return Ok(()),
            Err(error)
                if attempt < 49
                    && (matches!(
                        error.kind(),
                        io::ErrorKind::PermissionDenied | io::ErrorKind::WouldBlock
                    ) || matches!(error.raw_os_error(), Some(5 | 32))) =>
            {
                last_error = Some(error);
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            Err(error) => return Err(error),
        }
    }
    // 50 попыток исчерпаны: возвращаем последнюю ошибку вместо unreachable!() —
    // при panic=abort это была бы гарантированная остановка приложения.
    Err(last_error.unwrap_or_else(|| {
        io::Error::other("atomic_replace: 50 attempts exhausted")
    }))
}

#[cfg(windows)]
fn atomic_replace(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;

    #[link(name = "Kernel32")]
    extern "system" {
        fn MoveFileExW(existing: *const u16, new_name: *const u16, flags: u32) -> i32;
    }

    let source_w: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination_w: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let ok = unsafe {
        MoveFileExW(
            source_w.as_ptr(),
            destination_w.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn atomic_replace(source: &Path, destination: &Path) -> io::Result<()> {
    std::fs::rename(source, destination)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "obsession-settings-{name}-{}-{}",
            std::process::id(),
            SAVE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn patch_changes_only_present_fields() {
        let mut settings = Settings {
            fake_tls_domain: "old.example".into(),
            ..Default::default()
        };
        settings.apply_patch(SettingsPatch {
            proxy_port: Some(2443),
            reduce_motion: Some(true),
            ..Default::default()
        });
        assert_eq!(settings.proxy_port, 2443);
        assert!(settings.reduce_motion);
        assert_eq!(settings.fake_tls_domain, "old.example");
        assert_eq!(settings.selected_categories, vec!["discord"]);
    }

    #[test]
    fn adaptive_search_mode_defaults_and_normalizes() {
        let mut settings = Settings::default();
        assert_eq!(settings.adaptive_search_mode, "balanced");
        settings.apply_patch(SettingsPatch {
            adaptive_search_mode: Some("fast".into()),
            ..Default::default()
        });
        assert_eq!(settings.adaptive_search_mode, "fast");
        settings.apply_patch(SettingsPatch {
            adaptive_search_mode: Some("unsupported".into()),
            ..Default::default()
        });
        assert_eq!(settings.adaptive_search_mode, "balanced");
    }

    #[test]
    fn legacy_automatic_mode_is_explicit_and_old_flag_migrates_safe() {
        let mut settings = Settings::default();
        assert!(settings.legacy_reliability_enabled);
        assert_eq!(settings.legacy_reliability_mode, "observe_only");
        assert!(settings.legacy_automatic_paused);
        settings.apply_patch(SettingsPatch {
            legacy_reliability_enabled: Some(false),
            ..Default::default()
        });
        assert!(!settings.legacy_reliability_enabled);
        settings.apply_patch(SettingsPatch {
            legacy_reliability_enabled: Some(true),
            ..Default::default()
        });
        assert!(settings.legacy_reliability_enabled);
        settings.apply_patch(SettingsPatch {
            legacy_reliability_mode: Some("assisted".into()),
            ..Default::default()
        });
        assert_eq!(settings.legacy_reliability_mode, "assisted");
        assert!(settings.legacy_automatic_paused);
        settings.apply_patch(SettingsPatch {
            legacy_reliability_mode: Some("automatic".into()),
            ..Default::default()
        });
        assert_eq!(settings.legacy_reliability_mode, "automatic");
        assert!(!settings.legacy_automatic_paused);
        settings.apply_patch(SettingsPatch {
            legacy_automatic_paused: Some(true),
            legacy_reliability_frozen_categories: Some(vec![
                "youtube_twitch".into(),
                "bad category".into(),
                "Discord".into(),
                "discord".into(),
            ]),
            ..Default::default()
        });
        assert!(settings.legacy_automatic_paused);
        assert_eq!(
            settings.legacy_reliability_frozen_categories,
            vec!["discord", "youtube_twitch"]
        );
        settings.apply_patch(SettingsPatch {
            legacy_reliability_mode: Some("unsupported".into()),
            legacy_automatic_paused: Some(false),
            ..Default::default()
        });
        assert_eq!(settings.legacy_reliability_mode, "observe_only");
        assert!(settings.legacy_automatic_paused);
        assert!(!settings.auto_recovery);

        let dir = test_dir("legacy-mode-migration");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            Settings::file(&dir),
            r#"{"auto_recovery":true,"legacy_reliability_mode":"assisted"}"#,
        )
        .unwrap();
        let migrated = Settings::load(&dir);
        assert!(!migrated.auto_recovery);
        assert!(migrated.legacy_reliability_enabled);
        assert_eq!(migrated.legacy_reliability_mode, "observe_only");
        assert!(migrated.legacy_automatic_paused);
        assert_eq!(
            migrated.legacy_reliability_migration_version,
            LEGACY_RELIABILITY_MIGRATION_VERSION
        );

        let explicitly_reenabled = Settings {
            auto_recovery: true,
            legacy_reliability_mode: "automatic".into(),
            legacy_automatic_paused: false,
            ..migrated
        };
        explicitly_reenabled.save(&dir).unwrap();
        let reloaded = Settings::load(&dir);
        assert!(reloaded.auto_recovery);
        assert_eq!(reloaded.legacy_reliability_mode, "automatic");
        assert!(!reloaded.legacy_automatic_paused);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn atomic_save_never_leaves_partial_json_under_concurrency() {
        let dir = test_dir("atomic");
        std::fs::create_dir_all(&dir).unwrap();
        let mut threads = Vec::new();
        for i in 0..8u16 {
            let dir = dir.clone();
            threads.push(std::thread::spawn(move || {
                for n in 0..25u16 {
                    let settings = Settings {
                        proxy_port: 2000 + i * 25 + n,
                        fake_tls_domain: format!("{i}-{n}.example"),
                        ..Default::default()
                    };
                    settings.save(&dir).unwrap();
                }
            }));
        }
        for thread in threads {
            thread.join().unwrap();
        }
        let raw = std::fs::read_to_string(Settings::file(&dir)).unwrap();
        let parsed: Settings = serde_json::from_str(&raw).unwrap();
        assert!((2000..2200).contains(&parsed.proxy_port));
        assert!(parsed.fake_tls_domain.ends_with(".example"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
