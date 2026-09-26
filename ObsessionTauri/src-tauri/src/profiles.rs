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
    let mut profiles: Vec<Profile> = match std::fs::read_to_string(file(base_dir)) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    for profile in &mut profiles {
        crate::settings::remove_retired_dpi_selection(
            &mut profile.selected_categories,
            &mut profile.selected_configs,
        );
    }
    profiles
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retired_category_is_removed_from_old_profiles_without_losing_other_choices() {
        let dir = std::env::temp_dir().join(format!(
            "obsession-profiles-retired-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(file(&dir), r#"[
            {"id":"mixed","name":"Игры","selected_categories":["atrisk","gaming"],
             "selected_configs":{"atrisk":"atrisk_1.conf","gaming":"gaming_2.conf"},"proxy_port":2443},
            {"id":"retired","name":"Старый","selected_categories":["at_risk"],
             "selected_configs":{"at_risk":"atrisk_2.conf"}},
            {"id":"empty","selected_categories":[]}
        ]"#).unwrap();

        let profiles = load(&dir);
        assert_eq!(profiles.len(), 3);
        assert_eq!(profiles[0].name, "Игры");
        assert_eq!(profiles[0].selected_categories, ["gaming"]);
        assert_eq!(profiles[0].selected_configs.len(), 1);
        assert_eq!(profiles[0].selected_configs["gaming"], "gaming_2.conf");
        assert_eq!(profiles[0].proxy_port, 2443);
        assert_eq!(profiles[1].selected_categories, ["discord"]);
        assert!(profiles[1].selected_configs.is_empty());
        assert!(profiles[2].selected_categories.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
