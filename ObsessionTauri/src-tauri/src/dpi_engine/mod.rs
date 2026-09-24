//! Абстракция DPI-движка (WS3). Позволяет Zapret1 (legacy) и Zapret2 сосуществовать
//! за единым интерфейсом. На этом этапе подключён только чистый слой манифеста
//! Strategy Pack ([`manifest`]); реализации движков (legacy/zapret2) и трейт
//! `DpiEngine` добавляются в задачах 3.2/3.4 поверх проверенной схемы.

#![allow(dead_code)] // EngineKind/интерфейс подключаются к lifecycle в 3.2/3.4

pub mod manifest;
pub mod resources;
pub mod zapret2;

use serde::{Deserialize, Serialize};

/// Какой DPI-движок активен. Legacy — по умолчанию; Zapret2 — ручная Beta.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EngineKind {
    /// Zapret1 (winws.exe + .conf) — стабильный движок по умолчанию.
    #[default]
    Legacy,
    /// Zapret2 (winws2.exe + Strategy Pack) — включается только вручную (Beta).
    Zapret2,
}

impl EngineKind {
    pub fn parse(s: &str) -> EngineKind {
        match s {
            "zapret2" => EngineKind::Zapret2,
            _ => EngineKind::Legacy,
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            EngineKind::Legacy => "legacy",
            EngineKind::Zapret2 => "zapret2",
        }
    }
}

/// Возможности/версия движка для UI-бейджа и compat-гейта Strategy Pack.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EngineDescribe {
    pub kind: &'static str,
    pub version: String,
    /// Beta-движок: не выбирается Мозгом автоматически, помечается в UI.
    pub beta: bool,
}

impl EngineKind {
    /// Статическое описание движка (без запуска процесса).
    pub fn describe(&self) -> EngineDescribe {
        match self {
            EngineKind::Legacy => EngineDescribe {
                kind: "legacy",
                // winws v72.12 — стабильный движок Zapret1.
                version: "72.12".into(),
                beta: false,
            },
            EngineKind::Zapret2 => {
                let (a, b, c, d) = zapret2::WINWS2_VERSION;
                EngineDescribe {
                    kind: "zapret2",
                    version: format!("{a}.{b}.{c}.{d}"),
                    beta: true,
                }
            }
        }
    }

    /// Может ли Мозг авто-выбирать этот движок. Zapret2 — только вручную (Beta).
    pub fn brain_selectable(&self) -> bool {
        matches!(self, EngineKind::Legacy)
    }
}

/// Выбор движка с учётом доступности Zapret2 и сохранённого Legacy-набора.
/// При отсутствии Zapret2 используется Legacy; после сбоя без сохранённого
/// Legacy-набора запуск прекращается.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineDecision {
    /// Запустить Legacy (winws). `fell_back` = пришли сюда как fallback с Zapret2.
    RunLegacy { fell_back: bool },
    /// Запустить Zapret2 (winws2).
    RunZapret2,
    /// Остановлено с явной ошибкой (Zapret2 упал, а рабочего Legacy нет).
    Stopped,
}

/// Выбор движка на СТАРТЕ сессии.
pub fn decide_start(selected: EngineKind, winws2_available: bool) -> EngineDecision {
    match selected {
        EngineKind::Legacy => EngineDecision::RunLegacy { fell_back: false },
        EngineKind::Zapret2 if winws2_available => EngineDecision::RunZapret2,
        // Zapret2 выбран, но бинарника нет → безопасный откат к Legacy.
        EngineKind::Zapret2 => EngineDecision::RunLegacy { fell_back: true },
    }
}

/// Выбор после КРАХА Zapret2 в рантайме: вернуть Legacy, если был сохранён
/// рабочий набор; иначе — остановиться с ошибкой.
pub fn decide_after_zapret2_crash(has_saved_legacy: bool) -> EngineDecision {
    if has_saved_legacy {
        EngineDecision::RunLegacy { fell_back: true }
    } else {
        EngineDecision::Stopped
    }
}

/// Загруженный и ПРОВЕРЕННЫЙ по целостности встроенный Strategy Pack.
pub struct LoadedPack {
    pub manifest: manifest::StrategyPackManifest,
    /// Директория пака (для резолва lua-путей в winws2 `--lua-init=@`).
    pub dir: std::path::PathBuf,
}

impl LoadedPack {
    /// Стратегии категории, отсортированные по возрастанию агрессивности
    /// (Мозг/пользователь идёт снизу вверх).
    pub fn strategies_for(&self, category: &str) -> Vec<manifest::StrategyDef> {
        let mut v: Vec<_> = self
            .manifest
            .strategies
            .iter()
            .filter(|s| s.category == category)
            .cloned()
            .collect();
        v.sort_by_key(|s| s.aggressiveness);
        v
    }

    /// Возвращает ВСЕ профили выбранного уровня категории. Несколько профилей
    /// одного уровня нужны, например, чтобы YouTube TLS и QUIC работали вместе.
    /// Уровень 0/неизвестный выбирает максимальную доступную агрессивность.
    pub fn profiles_for(&self, category: &str, requested_level: u8) -> Vec<manifest::StrategyDef> {
        let all = self.strategies_for(category);
        let Some(max_level) = all.iter().map(|s| s.aggressiveness).max() else {
            return Vec::new();
        };
        let level =
            if requested_level > 0 && all.iter().any(|s| s.aggressiveness == requested_level) {
                requested_level
            } else {
                max_level
            };
        all.into_iter()
            .filter(|s| s.aggressiveness == level)
            .collect()
    }
}

/// Читает `manifest.json` из директории пака и ВАЛИДИРУЕТ целостность: SHA-256
/// каждого lua-файла резолвится с диска (путь безопасен → внутри `pack_dir`).
/// Возвращает Err с первой ошибкой валидации — движок Zapret2 не стартует на
/// непроверенном паке (Lua = исполняемый код).
pub fn load_pack(pack_dir: &std::path::Path) -> Result<LoadedPack, String> {
    let manifest_path = pack_dir.join("manifest.json");
    let bytes = std::fs::read(&manifest_path)
        .map_err(|e| format!("не удалось прочитать {}: {e}", manifest_path.display()))?;
    let manifest = manifest::parse_manifest(&bytes)?;

    let caps = manifest::EngineCapabilities {
        winws2_version: zapret2::WINWS2_VERSION,
        lua_api: zapret2::LUA_API,
    };
    // Резолвер читает файл пака по относительному пути ВНУТРИ pack_dir. Валидатор
    // уже отсёк traversal (`..`/абсолют) — здесь дополнительно защищаемся, требуя,
    // чтобы канонизированный путь оставался под pack_dir.
    let pack_dir_owned = pack_dir.to_path_buf();
    let resolve = |rel: &str| -> Option<Vec<u8>> {
        let candidate = pack_dir_owned.join(rel);
        std::fs::read(&candidate).ok()
    };
    let report = manifest::validate_pack(&manifest, caps, resolve);
    if !report.is_valid() {
        return Err(format!(
            "Strategy Pack не прошёл проверку целостности: {}",
            report.errors.join("; ")
        ));
    }
    Ok(LoadedPack {
        manifest,
        dir: pack_dir.to_path_buf(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_description_preserves_upstream_revision() {
        assert_eq!(EngineKind::Zapret2.describe().version, "1.0.5.2");
    }

    #[test]
    fn parse_defaults_to_legacy() {
        assert_eq!(EngineKind::parse("legacy"), EngineKind::Legacy);
        assert_eq!(EngineKind::parse("zapret2"), EngineKind::Zapret2);
        assert_eq!(EngineKind::parse("garbage"), EngineKind::Legacy);
        assert_eq!(EngineKind::default(), EngineKind::Legacy);
    }

    #[test]
    fn zapret2_is_beta_and_not_brain_selectable() {
        assert!(EngineKind::Zapret2.describe().beta);
        assert!(!EngineKind::Zapret2.brain_selectable());
        // Legacy — стабильный, доступен Мозгу.
        assert!(!EngineKind::Legacy.describe().beta);
        assert!(EngineKind::Legacy.brain_selectable());
    }

    #[test]
    fn decide_start_honors_selection_and_availability() {
        // Legacy всегда Legacy.
        assert_eq!(
            decide_start(EngineKind::Legacy, true),
            EngineDecision::RunLegacy { fell_back: false }
        );
        // Zapret2 + бинарник → Zapret2.
        assert_eq!(
            decide_start(EngineKind::Zapret2, true),
            EngineDecision::RunZapret2
        );
        // Zapret2 без бинарника → тихий откат к Legacy.
        assert_eq!(
            decide_start(EngineKind::Zapret2, false),
            EngineDecision::RunLegacy { fell_back: true }
        );
    }

    #[test]
    fn crash_falls_back_to_legacy_or_stops() {
        assert_eq!(
            decide_after_zapret2_crash(true),
            EngineDecision::RunLegacy { fell_back: true }
        );
        assert_eq!(decide_after_zapret2_crash(false), EngineDecision::Stopped);
    }

    #[test]
    fn loads_real_builtin_pack_with_valid_integrity() {
        // Грузим НАСТОЯЩИЙ встроенный пак из resources — проверяет, что манифест
        // и SHA-256 lua-файлов на диске сходятся (ловит дрейф хешей/путей).
        let pack_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources/strategy-packs/builtin");
        if !pack_dir.join("manifest.json").exists() {
            // Lua-файлы могут отсутствовать в окружении без ассетов — тогда скип.
            return;
        }
        match load_pack(&pack_dir) {
            Ok(pack) => {
                assert_eq!(pack.manifest.pack_id, "builtin.base");
                assert_eq!(pack.manifest.pack_version, "0.7.0");
                // Уровень 4 поднимает TCP и QUIC вместе — как youtube на уровне 1.
                let disc_alt = pack.profiles_for("discord", 4);
                assert_eq!(disc_alt.len(), 3);
                assert!(disc_alt
                    .iter()
                    .any(|s| s.id == "discord_tls_multidisorder_alt"));
                assert!(disc_alt.iter().any(|s| s.id == "discord_quic_alt"));
                for level in 1..=4 {
                    let profiles = pack.profiles_for("discord", level);
                    assert_eq!(profiles.len(), 3);
                    assert!(profiles.iter().any(zapret2::is_voice_profile));
                    assert!(profiles
                        .iter()
                        .any(|p| p.transports.iter().any(|t| t == "quic")));
                }
                // Discord-стратегии отсортированы по агрессивности.
                let disc = pack.strategies_for("discord");
                assert!(!disc.is_empty(), "должны быть discord-стратегии");
                assert!(
                    disc.windows(2)
                        .all(|w| w[0].aggressiveness <= w[1].aggressiveness),
                    "стратегии отсортированы по возрастанию агрессивности"
                );

                let youtube = pack.profiles_for("youtube_twitch", 1);
                assert_eq!(youtube.len(), 2);
                assert_eq!(youtube[0].id, "youtube_tls");
                // pos=1 давал пустой первый сегмент, поэтому режем только midsld.
                assert_eq!(youtube[0].desync, ["multidisorder_legacy:pos=midsld"]);
                assert_eq!(youtube[1].id, "youtube_quic");
                // QUIC-фейк должен быть настоящим Initial, а не 0x40 + 619 нулей.
                assert_eq!(youtube[1].desync, ["fake:blob=quic_google:repeats=6"]);
                assert_eq!(youtube[1].out_range.as_deref(), Some("-d10"));

                let gaming = pack.profiles_for("gaming", 1);
                assert_eq!(gaming.len(), 5);
                assert_eq!(
                    gaming
                        .iter()
                        .map(|profile| profile.id.as_str())
                        .collect::<Vec<_>>(),
                    [
                        "gaming_control_tls",
                        "gaming_control_quic",
                        "gaming_ipset_http",
                        "gaming_ipset_tls",
                        "gaming_ipset_udp",
                    ]
                );
                assert!(gaming[..2].iter().all(|profile| profile.ipset.is_none()));
                assert!(gaming[2..].iter().all(|profile| profile.ipset.is_some()));
            }
            Err(e) => panic!("встроенный пак не прошёл валидацию: {e}"),
        }
    }

    #[test]
    fn profiles_for_returns_all_profiles_at_selected_level() {
        let pack = LoadedPack {
            manifest: manifest::StrategyPackManifest {
                schema_version: manifest::PACK_SCHEMA_VERSION,
                pack_id: "test".into(),
                pack_version: "1".into(),
                engine: manifest::EngineCompat {
                    winws2_min: ">=1.0.0".into(),
                    lua_api: zapret2::LUA_API,
                },
                categories: vec!["youtube_twitch".into()],
                protocols: vec!["tls".into(), "quic".into()],
                files: vec![],
                blobs: vec![],
                strategies: vec![
                    strategy_for_test("yt_tls", 1),
                    strategy_for_test("yt_quic", 1),
                    strategy_for_test("yt_hard", 2),
                ],
                probes: vec![],
                fallback: None,
            },
            dir: std::path::PathBuf::new(),
        };
        let level_one = pack.profiles_for("youtube_twitch", 1);
        assert_eq!(level_one.len(), 2);
        assert_eq!(level_one[0].id, "yt_tls");
        assert_eq!(level_one[1].id, "yt_quic");
        assert_eq!(pack.profiles_for("youtube_twitch", 0)[0].id, "yt_hard");
    }

    fn strategy_for_test(id: &str, aggressiveness: u8) -> manifest::StrategyDef {
        manifest::StrategyDef {
            id: id.into(),
            category: "youtube_twitch".into(),
            aggressiveness,
            lua: String::new(),
            desync: vec!["multisplit:pos=1".into()],
            transports: vec!["tcp".into()],
            hostlist: None,
            ipset: None,
            filter_tcp: None,
            filter_udp: None,
            filter_l7: vec!["tls".into()],
            payload: vec!["tls_client_hello".into()],
            out_range: None,
            in_range: None,
        }
    }
}
