//! Именованные профили-пресеты в `%APPDATA%\Obsession\profiles.json`.
//! Профиль хранит выбор DPI (категории+конфиги), параметры Telegram-прокси и
//! ИИ-провайдера, чтобы применять всё разом в один клик.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub selected_categories: Vec<String>,
    pub selected_configs: HashMap<String, String>,
    pub proxy_port: u16,
    pub fake_tls_domain: String,
    pub ai_provider: String,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            selected_categories: Vec::new(),
            selected_configs: HashMap::new(),
            proxy_port: 1443,
            fake_tls_domain: String::new(),
            ai_provider: "malw".to_string(),
        }
    }
}

fn file(base_dir: &Path) -> std::path::PathBuf {
    base_dir.join("profiles.json")
}

/// Загружает список профилей (пустой, если файла нет/битый).
pub fn load(base_dir: &Path) -> Vec<Profile> {
    match std::fs::read_to_string(file(base_dir)) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

fn save(base_dir: &Path, profiles: &[Profile]) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(profiles).unwrap_or_default();
    std::fs::write(file(base_dir), json)
}

/// Вставляет или обновляет профиль по id. Пустой id — не сохраняем.
pub fn upsert(base_dir: &Path, profile: Profile) -> std::io::Result<Vec<Profile>> {
    let mut list = load(base_dir);
    match list.iter_mut().find(|p| p.id == profile.id) {
        Some(existing) => *existing = profile,
        None => list.push(profile),
    }
    save(base_dir, &list)?;
    Ok(list)
}

/// Удаляет профиль по id. Возвращает актуальный список.
pub fn delete(base_dir: &Path, id: &str) -> std::io::Result<Vec<Profile>> {
    let mut list = load(base_dir);
    list.retain(|p| p.id != id);
    save(base_dir, &list)?;
    Ok(list)
}
