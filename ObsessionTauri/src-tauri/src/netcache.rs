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

const SCHEMA_VERSION: u32 = 2;

fn default_engine() -> String {
    "legacy".to_string()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CatEntry {
    pub conf: String,
    pub confirmed_at: u64,
    pub success_count: u64,
    /// Движок, на котором подтверждён conf (WS5). Старые файлы (schema v1) без
    /// поля мигрируют в "legacy" — единственный движок, что реально работал.
    #[serde(default = "default_engine")]
    pub engine: String,
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
    /// Загружает кэш с миграцией вперёд. Раньше любое несовпадение схемы
    /// ВЫБРАСЫВАЛО обученные данные (потеря L1-кэша при апгрейде) — теперь старые,
    /// но совместимо-читаемые версии сохраняются и апгрейдятся до текущей схемы.
    /// Пустой кэш только при отсутствии файла, битом JSON или версии ИЗ БУДУЩЕГО
    /// (её структуру мы знать не можем — консервативно сбрасываем).
    pub fn load(paths: &Paths) -> Self {
        let text = match std::fs::read_to_string(paths.netcache_path()) {
            Ok(t) => t,
            Err(_) => return Self::default(),
        };
        match serde_json::from_str::<NetCache>(&text) {
            // Текущая или более старая схема: serde-default'ы новых полей уже
            // применились при парсе (engine→"legacy"), поднимаем версию до текущей.
            Ok(mut c) if c.schema_version <= SCHEMA_VERSION => {
                c.schema_version = SCHEMA_VERSION;
                c
            }
            // Версия из будущего или битый файл → пустой кэш.
            _ => Self::default(),
        }
    }

    /// Атомарно сохраняет кэш. Ошибку глотаем (кэш — не критичный путь).
    pub fn save(&self, paths: &Paths) {
        let path = paths.netcache_path();
        let json = serde_json::to_string_pretty(self).unwrap_or_default();
        let tmp = path.with_extension("json.tmp");
        // Атомарно: пишем во временный и переименовываем. При неудаче НЕ пишем в
        // целевой файл напрямую (это оставило бы обрезанный JSON при краше) —
        // кэш некритичен, просто убираем tmp и попробуем при следующем save.
        if std::fs::write(&tmp, &json)
            .and_then(|_| std::fs::rename(&tmp, &path))
            .is_err()
        {
            let _ = std::fs::remove_file(&tmp);
        }
    }

    /// Все записи надёжности для сети (по категориям). Для UI-дашборда.
    pub fn network_entries(&self, mac: &str) -> HashMap<String, CatEntry> {
        self.networks
            .get(mac)
            .map(|n| n.categories.clone())
            .unwrap_or_default()
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
    pub fn put(
        &mut self,
        mac: &str,
        asn_region: Option<&str>,
        category: &str,
        conf: &str,
        now: u64,
    ) {
        let net = self.networks.entry(mac.to_string()).or_default();
        if asn_region.is_some() {
            net.asn_region = asn_region.map(|s| s.to_string());
        }
        let entry = net
            .categories
            .entry(category.to_string())
            .or_insert(CatEntry {
                conf: conf.to_string(),
                confirmed_at: now,
                success_count: 0,
                engine: default_engine(),
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
    fn migrates_v1_entry_without_engine_to_legacy() {
        // Старый файл schema v1: у CatEntry НЕТ поля engine — должен читаться и
        // получить engine="legacy" (миграция вперёд, данные НЕ теряются).
        let v1 = r#"{
            "schema_version": 1,
            "networks": {
                "aa:bb": {
                    "asn_region": "AS1_RU",
                    "categories": {
                        "yt": {"conf": "yt_3.conf", "confirmed_at": 100, "success_count": 5}
                    }
                }
            }
        }"#;
        let c: NetCache = serde_json::from_str(v1).unwrap();
        // Данные сохранились.
        assert_eq!(c.get("aa:bb", "yt").as_deref(), Some("yt_3.conf"));
        let entry = c.networks["aa:bb"].categories["yt"].clone();
        assert_eq!(entry.success_count, 5);
        // Отсутствующее поле engine → legacy.
        assert_eq!(entry.engine, "legacy");
    }

    #[test]
    fn future_schema_version_resets_but_old_preserved() {
        // Версия из будущего структурно неизвестна — но парс сам по себе валиден;
        // именно load() (не парс) решает сбросить. Проверяем инвариант версии.
        let future = r#"{"schema_version":5,"networks":{}}"#;
        let parsed: NetCache = serde_json::from_str(future).unwrap();
        assert!(parsed.schema_version > SCHEMA_VERSION);
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
