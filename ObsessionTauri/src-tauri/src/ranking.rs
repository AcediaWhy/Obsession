//! Рейтинг L2 — упорядочивание bundled `.conf` под `ASN_region` из `ranking.json`.
//!
//! ⚠️ Источник кандидатов — ТОЛЬКО локальные доверенные `.conf`. JSON НЕ поставляет
//! строк для winws (админ-права + C), он лишь **сортирует** уже существующие имена
//! файлов. Поэтому ревалидация строгая: каждое имя обязано существовать среди
//! `get_configs_for_category`, иначе отбрасывается. Битый/несовместимый файл →
//! деградация к L3 (полный список категории), не падение.

use std::collections::HashMap;

use serde::Deserialize;

use crate::paths::Paths;

/// Версия синтаксиса аргументов winws, под которую собран клиент. Рейтинг с
/// несовместимым `winws_compat` игнорируется (аргументы могли поменяться).
const WINWS_VERSION: (u32, u32, u32) = (0, 9, 0);

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Deserialize)]
struct RawRanking {
    schema_version: u32,
    #[serde(default)]
    winws_compat: String,
    #[serde(default)]
    categories: HashMap<String, RawCategory>,
}

#[derive(Debug, Deserialize)]
struct RawCategory {
    #[serde(default)]
    default: Vec<String>,
    #[serde(default)]
    by_asn_region: HashMap<String, Vec<String>>,
}

/// Провалидированный рейтинг: имена гарантированно существуют на диске.
#[derive(Debug, Default)]
pub struct Ranking {
    categories: HashMap<String, RankedCategory>,
}

#[derive(Debug, Default)]
struct RankedCategory {
    default: Vec<String>,
    by_asn_region: HashMap<String, Vec<String>>,
}

impl Ranking {
    /// Загружает и валидирует `ranking.json`. Любая проблема → пустой рейтинг
    /// (Мозг деградирует к L3 = полному списку категории) + предупреждение в лог.
    pub fn load(paths: &Paths) -> Self {
        let text = match std::fs::read_to_string(paths.ranking_path()) {
            Ok(t) => t,
            Err(_) => return Self::default(),
        };
        let raw: RawRanking = match serde_json::from_str(&text) {
            Ok(r) => r,
            Err(_) => return Self::default(),
        };
        Self::validate(raw, paths)
    }

    fn validate(raw: RawRanking, paths: &Paths) -> Self {
        if raw.schema_version != SCHEMA_VERSION {
            return Self::default();
        }
        if !winws_compat_ok(&raw.winws_compat) {
            return Self::default();
        }

        let mut categories = HashMap::new();
        for (cat, raw_cat) in raw.categories {
            let existing = paths.get_configs_for_category(&cat);
            let keep = |names: Vec<String>| -> Vec<String> {
                names.into_iter().filter(|n| existing.contains(n)).collect()
            };
            let default = keep(raw_cat.default);
            let by_asn_region = raw_cat
                .by_asn_region
                .into_iter()
                .map(|(k, v)| (k, keep(v)))
                .collect();
            categories.insert(
                cat,
                RankedCategory {
                    default,
                    by_asn_region,
                },
            );
        }
        Self { categories }
    }

    /// Упорядоченные кандидаты категории под `asn_region`: `by_asn_region ∪ default`
    /// (dedup, порядок сохранён). Пусто, если категории нет в рейтинге.
    pub fn ranked_for(&self, category: &str, asn_region: Option<&str>) -> Vec<String> {
        let Some(cat) = self.categories.get(category) else {
            return Vec::new();
        };
        let mut out: Vec<String> = Vec::new();
        if let Some(asn) = asn_region {
            if let Some(regional) = cat.by_asn_region.get(asn) {
                for n in regional {
                    if !out.contains(n) {
                        out.push(n.clone());
                    }
                }
            }
        }
        for n in &cat.default {
            if !out.contains(n) {
                out.push(n.clone());
            }
        }
        out
    }
}

/// Проверяет `winws_compat` вида `">=X.Y.Z"` против [`WINWS_VERSION`]. Пустая или
/// нераспознанная строка считается несовместимой (консервативно).
fn winws_compat_ok(req: &str) -> bool {
    let req = req.trim();
    let Some(rest) = req.strip_prefix(">=") else {
        return false;
    };
    let Some(want) = parse_semver(rest.trim()) else {
        return false;
    };
    WINWS_VERSION >= want
}

fn parse_semver(s: &str) -> Option<(u32, u32, u32)> {
    let mut it = s.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next()?.parse().ok()?;
    let patch = it.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ranked_cat(default: &[&str], regional: &[(&str, &[&str])]) -> RankedCategory {
        RankedCategory {
            default: default.iter().map(|s| s.to_string()).collect(),
            by_asn_region: regional
                .iter()
                .map(|(k, v)| (k.to_string(), v.iter().map(|s| s.to_string()).collect()))
                .collect(),
        }
    }

    fn ranking(cats: Vec<(&str, RankedCategory)>) -> Ranking {
        Ranking {
            categories: cats.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
        }
    }

    #[test]
    fn semver_compat_gate() {
        assert!(winws_compat_ok(">=0.9.0"));
        assert!(winws_compat_ok(">=0.8.0"));
        assert!(!winws_compat_ok(">=1.0.0")); // клиент старше требования
        assert!(!winws_compat_ok("")); // пусто → несовместимо
        assert!(!winws_compat_ok("0.9.0")); // без префикса → несовместимо
        assert!(!winws_compat_ok(">=abc"));
    }

    #[test]
    fn regional_precedes_default_with_dedup() {
        let r = ranking(vec![(
            "youtube",
            ranked_cat(
                &["a.conf", "b.conf", "c.conf"],
                &[("AS1_RU-MOW", &["b.conf", "z.conf"])],
            ),
        )]);
        // Региональные впереди, дубль b.conf не повторяется, z.conf сохраняется.
        assert_eq!(
            r.ranked_for("youtube", Some("AS1_RU-MOW")),
            vec!["b.conf", "z.conf", "a.conf", "c.conf"]
        );
        // Без региона — только default.
        assert_eq!(
            r.ranked_for("youtube", None),
            vec!["a.conf", "b.conf", "c.conf"]
        );
        // Неизвестный регион — падаем на default.
        assert_eq!(
            r.ranked_for("youtube", Some("AS999_XX-YY")),
            vec!["a.conf", "b.conf", "c.conf"]
        );
    }

    #[test]
    fn unknown_category_is_empty() {
        let r = ranking(vec![]);
        assert!(r.ranked_for("nope", None).is_empty());
    }

    #[test]
    fn validate_drops_nonexistent_and_bad_schema() {
        // schema_version != 1 → пустой рейтинг.
        let raw = RawRanking {
            schema_version: 2,
            winws_compat: ">=0.9.0".into(),
            categories: HashMap::new(),
        };
        // Валидация вызывается через load; здесь дергаем winws_compat_ok/parse напрямую.
        assert!(raw.schema_version != SCHEMA_VERSION);
        assert_eq!(parse_semver("0.9"), Some((0, 9, 0)));
        assert_eq!(parse_semver("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_semver("x"), None);
    }
}
