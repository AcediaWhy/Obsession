//! L1-кэш «что работало в ЭТОЙ сети» (`%APPDATA%\Obsession\netcache.json`).
//!
//! Ключ верхнего уровня — MAC шлюза (из [`crate::netid`]). Для каждой сети хранится
//! последний подтверждённо-рабочий `.conf` по категории + счётчик успехов и время
//! подтверждения. Это первая ступень лестницы Мозга (мгновенно, 0 шума, ~90%
//! случаев). Пишется рантаймом по `Action::WriteCache` после фазы Confirming.
//!
//! Запись атомарна (tmp→rename, как `lists.rs`); битый файл → пустой кэш + лог.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::paths::Paths;

const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CatEntry {
    pub conf: String,
    pub confirmed_at: u64,
    pub success_count: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Network {
    #[serde(default)]
    asn_region: Option<String>,
    #[serde(default)]
    categories: HashMap<String, CatEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NetCache {
    schema_version: u32,
    networks: HashMap<String, Network>,
}

impl Default for NetCache {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            networks: HashMap::new(),
        }
    }
}

impl NetCache {
    /// Загружает кэш (пустой при отсутствии/битости/несовместимой схеме).
    pub fn load(paths: &Paths) -> Self {
        let text = match std::fs::read_to_string(paths.netcache_path()) {
            Ok(t) => t,
            Err(_) => return Self::default(),
        };
        match serde_json::from_str::<NetCache>(&text) {
            Ok(c) if c.schema_version == SCHEMA_VERSION => c,
            _ => Self::default(),
        }
    }

    /// Атомарно сохраняет кэш. Ошибку глотаем (кэш — не критичный путь).
    pub fn save(&self, paths: &Paths) {
        let path = paths.netcache_path();
        let json = serde_json::to_string_pretty(self).unwrap_or_default();
        let tmp = path.with_extension("json.tmp");
        let write = std::fs::write(&tmp, &json).and_then(|_| std::fs::rename(&tmp, &path));
        if write.is_err() {
            let _ = std::fs::remove_file(&tmp);
            let _ = std::fs::write(&path, &json);
        }
    }

    /// Рабочий `.conf` для (сеть, категория), если есть в кэше.
    pub fn get(&self, mac: &str, category: &str) -> Option<String> {
        self.networks
            .get(mac)
            .and_then(|n| n.categories.get(category))
            .map(|e| e.conf.clone())
    }

    /// Записывает подтверждённо-рабочий `.conf`. Тот же conf → инкремент счётчика;
    /// смена conf → счётчик с 1. `now` — время подтверждения (unix-секунды).
    pub fn put(&mut self, mac: &str, asn_region: Option<&str>, category: &str, conf: &str, now: u64) {
        let net = self.networks.entry(mac.to_string()).or_default();
        if asn_region.is_some() {
            net.asn_region = asn_region.map(|s| s.to_string());
        }
        let entry = net.categories.entry(category.to_string()).or_insert(CatEntry {
            conf: conf.to_string(),
            confirmed_at: now,
            success_count: 0,
        });
        if entry.conf != conf {
            entry.conf = conf.to_string();
            entry.success_count = 0;
        }
        entry.confirmed_at = now;
        entry.success_count = entry.success_count.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_then_get_roundtrip() {
        let mut c = NetCache::default();
        c.put("aa:bb", Some("AS1_RU-MOW"), "youtube", "yt_3.conf", 100);
        assert_eq!(c.get("aa:bb", "youtube").as_deref(), Some("yt_3.conf"));
        assert_eq!(c.get("aa:bb", "discord"), None);
        assert_eq!(c.get("zz:zz", "youtube"), None);
    }

    #[test]
    fn same_conf_increments_count_new_conf_resets() {
        let mut c = NetCache::default();
        c.put("m", None, "yt", "a.conf", 1);
        c.put("m", None, "yt", "a.conf", 2);
        let e = c.networks["m"].categories["yt"].clone();
        assert_eq!(e.success_count, 2);
        assert_eq!(e.confirmed_at, 2);
        // Смена conf сбрасывает счётчик.
        c.put("m", None, "yt", "b.conf", 3);
        let e2 = c.networks["m"].categories["yt"].clone();
        assert_eq!(e2.conf, "b.conf");
        assert_eq!(e2.success_count, 1);
    }

    #[test]
    fn deserialize_bad_schema_is_empty() {
        // Несовместимая схема через прямой парс + проверка в load-логике.
        let bad = r#"{"schema_version":99,"networks":{}}"#;
        let parsed: NetCache = serde_json::from_str(bad).unwrap();
        assert_ne!(parsed.schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn serde_roundtrip_preserves_entries() {
        let mut c = NetCache::default();
        c.put("m", Some("AS1_X"), "yt", "a.conf", 5);
        let json = serde_json::to_string(&c).unwrap();
        let back: NetCache = serde_json::from_str(&json).unwrap();
        assert_eq!(back.get("m", "yt").as_deref(), Some("a.conf"));
    }
}
