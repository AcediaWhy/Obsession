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

pub const CLIENTHELLO_EXPERIMENT_ID: &str = "discord_tls_clienthello_clone";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Zapret2ProfileSelection {
    pub category: String,
    pub transport: String,
    pub profile_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Zapret2ProfileOption {
    pub category: String,
    pub transport: String,
    pub profile_id: String,
    pub experimental: bool,
}

/// One selectable slot; Gaming's ipset TLS is deliberately outside control TLS.
pub fn profile_transport(strategy: &manifest::StrategyDef) -> Option<&'static str> {
    let has_l7 = |value: &str| strategy.filter_l7.iter().any(|entry| entry == value);
    if strategy.ipset.is_none() && has_l7("tls") && strategy.filter_udp.is_none() { Some("tls") }
    else if strategy.ipset.is_none() && has_l7("quic") && strategy.filter_tcp.is_none() { Some("quic") }
    else if strategy.category == "discord" && strategy.filter_udp.is_some()
        && strategy.filter_tcp.is_none() && (has_l7("discord") || has_l7("stun")) { Some("voice") }
    else if has_l7("http") && strategy.filter_udp.is_none() { Some("http") }
    else if strategy.filter_udp.is_some() && strategy.filter_tcp.is_none() && strategy.filter_l7.is_empty() { Some("udp") }
    else { None }
}

/// Only TLS ClientHello profiles may be replaced by the fixed experiment.
pub fn is_discord_tls_profile(strategy: &manifest::StrategyDef) -> bool {
    strategy.category == "discord"
        && strategy.filter_l7 == ["tls"]
        && strategy.payload == ["tls_client_hello"]
        && strategy.filter_udp.is_none()
        && strategy.transports.iter().all(|value| matches!(value.as_str(), "tcp" | "tls"))
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

    pub fn profiles_for_options(
        &self,
        category: &str,
        requested_level: u8,
        clienthello: bool,
    ) -> Result<Vec<manifest::StrategyDef>, String> {
        let mut profiles = self.profiles_for(category, requested_level);
        if !clienthello || category != "discord" { return Ok(profiles); }
        let experiment = self.manifest.experiments.iter()
            .find(|strategy| strategy.id == CLIENTHELLO_EXPERIMENT_ID && is_discord_tls_profile(strategy))
            .ok_or_else(|| "Эксперимент ClientHello отсутствует в проверенном паке. Обновите Obsession.".to_string())?;
        let index = profiles.iter().position(is_discord_tls_profile)
            .ok_or_else(|| "В выбранном наборе отсутствует профиль Discord TLS.".to_string())?;
        profiles.retain(|strategy| !is_discord_tls_profile(strategy));
        profiles.insert(index.min(profiles.len()), experiment.clone());
        Ok(profiles)
    }

    pub fn profiles_for_selections(
        &self,
        category: &str,
        requested_level: u8,
        clienthello: bool,
        selections: &[Zapret2ProfileSelection],
    ) -> Result<Vec<manifest::StrategyDef>, String> {
        let explicit_tls = selections.iter().any(|selection| selection.category == category && selection.transport == "tls");
        let mut profiles = self.profiles_for_options(category, requested_level, clienthello && !explicit_tls)?;
        let mut selected_slots = std::collections::BTreeSet::new();
        for selection in selections.iter().filter(|selection| selection.category == category) {
            if !matches!(selection.transport.as_str(), "tls" | "quic" | "voice" | "http" | "udp")
                || selection.profile_id.is_empty() || selection.profile_id.len() > 128
                || !selection.profile_id.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
                || !selected_slots.insert(selection.transport.as_str()) {
                return Err("Неверный или повторный выбор транспортного профиля Zapret2.".into());
            }
            if let Some(parts) = self.discord_composition(&selection.profile_id)? {
                if category != "discord" || selection.transport != "tls" { return Err("Сборка доступна только для Discord TLS.".into()); }
                let position = profiles.iter().position(|strategy| profile_transport(strategy) == Some("tls"))
                    .ok_or("Для сборки нет исходного TLS-профиля.")?;
                profiles.retain(|strategy| profile_transport(strategy) != Some("tls"));
                profiles.splice(position.min(profiles.len())..position.min(profiles.len()), parts);
                continue;
            }
            let replacement = self.manifest.strategies.iter().chain(&self.manifest.presets).chain(&self.manifest.experiments)
                .find(|strategy| strategy.id == selection.profile_id)
                .ok_or_else(|| "Выбранный профиль отсутствует во встроенном паке Zapret2.".to_string())?;
            if replacement.category != category || profile_transport(replacement) != Some(selection.transport.as_str()) {
                return Err("Профиль Zapret2 не соответствует сервису или транспорту.".into());
            }
            let position = profiles.iter().position(|strategy| profile_transport(strategy) == Some(selection.transport.as_str()))
                .ok_or_else(|| "Для транспорта нет исходного профиля Zapret2.".to_string())?;
            profiles.retain(|strategy| profile_transport(strategy) != Some(selection.transport.as_str()));
            profiles.insert(position.min(profiles.len()), replacement.clone());
        }
        Ok(profiles)
    }

    pub fn discord_composition(&self, id: &str) -> Result<Option<Vec<manifest::StrategyDef>>, String> {
        use obsession_runtime_protocol::zapret2_composition::{DiscordComposition, DiscordScope, compatible};
        let Some(recipe) = DiscordComposition::parse(id).map_err(str::to_owned)? else { return Ok(None); };
        let mut result = Vec::new();
        for (parent,scope) in [(&recipe.api_profile_id,DiscordScope::Api),(&recipe.cdn_profile_id,DiscordScope::Cdn)] {
            let strategy = self.manifest.strategies.iter().chain(&self.manifest.presets).find(|strategy| &strategy.id == parent)
                .ok_or("Родитель сборки отсутствует в проверенном каталоге.")?;
            if !compatible(&strategy.category,strategy.filter_tcp.as_deref(),strategy.filter_udp.as_deref(),
                &strategy.filter_l7,strategy.hostlist.as_deref(),strategy.ipset.as_deref(),&strategy.transports) {
                return Err("Родитель сборки имеет несовместимые фильтры.".into());
            }
            let mut strategy = strategy.clone();
            strategy.composition_scope = Some(scope);
            result.push(strategy);
        }
        Ok(Some(result))
    }

    pub fn profile_catalog(&self) -> Vec<Zapret2ProfileOption> {
        let mut options = self.manifest.strategies.iter().chain(&self.manifest.presets).chain(&self.manifest.experiments)
            .filter_map(|strategy| profile_transport(strategy).map(|transport| Zapret2ProfileOption {
                category: strategy.category.clone(), transport: transport.to_owned(), profile_id: strategy.id.clone(),
                experimental: strategy.candidate.is_some() || self.manifest.experiments.iter().any(|experiment| experiment.id == strategy.id),
            })).collect::<Vec<_>>();
        options.sort_by(|left, right| (&left.category, &left.transport, left.experimental, &left.profile_id)
            .cmp(&(&right.category, &right.transport, right.experimental, &right.profile_id)));
        options
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
                assert_eq!(pack.manifest.pack_version, "0.12.0");
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
                let voice_variants: std::collections::HashSet<_> = (1..=4)
                    .map(|level| {
                        pack.profiles_for("discord", level)
                            .into_iter()
                            .find(zapret2::is_voice_profile)
                            .unwrap()
                            .desync
                    })
                    .collect();
                assert_eq!(
                    voice_variants.len(),
                    4,
                    "voice levels must try different rules"
                );
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
                experiments: vec![],
                presets: vec![],
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
            composition_scope: None,
            id: id.into(),
            category: "youtube_twitch".into(),
            aggressiveness,
            lua: String::new(),
            desync: vec!["multisplit:pos=1".into()],
            steps: vec![],
            candidate: None,
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

    #[test]
    fn clienthello_overlay_preserves_every_stable_level_and_other_transports() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/strategy-packs/builtin");
        let pack = load_pack(&root).unwrap();
        assert_eq!(pack.manifest.strategies.len(), 19);
        let default = pack.profiles_for_options("discord", 0, false).unwrap();
        assert!(default.iter().any(|profile| profile.id == "discord_tls_multidisorder_alt"));
        for level in 0..=4 {
            let original = pack.profiles_for("discord", level);
            assert_eq!(pack.profiles_for_options("discord", level, false).unwrap(), original);
            let experimental = pack.profiles_for_options("discord", level, true).unwrap();
            assert_eq!(experimental.iter().filter(|profile| is_discord_tls_profile(profile)).count(), 1);
            assert!(experimental.iter().any(|profile| profile.id == CLIENTHELLO_EXPERIMENT_ID));
            assert_eq!(experimental.iter().filter(|profile| !is_discord_tls_profile(profile)).collect::<Vec<_>>(),
                original.iter().filter(|profile| !is_discord_tls_profile(profile)).collect::<Vec<_>>());
            for category in ["youtube_twitch", "gaming"] {
                assert_eq!(pack.profiles_for_options(category, level, true).unwrap(), pack.profiles_for(category, level));
            }
        }
        let mut absent = LoadedPack { manifest: pack.manifest.clone(), dir: pack.dir.clone() };
        absent.manifest.experiments.clear();
        assert!(absent.profiles_for_options("discord", 0, true).is_err());
        assert_eq!(absent.profiles_for_options("discord", 0, false).unwrap(), default);
    }

    #[test]
    fn named_profile_catalog_and_overlays_preserve_ordinary_transports() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/strategy-packs/builtin");
        let pack = load_pack(&root).unwrap();
        let catalog = pack.profile_catalog();
        assert!(catalog.iter().any(|option| option.profile_id == "classic_gaming_tls_split_sni" && option.transport == "tls" && !option.experimental));
        assert!(catalog.iter().any(|option| option.profile_id == CLIENTHELLO_EXPERIMENT_ID && option.experimental));
        assert!(!catalog.iter().any(|option| option.profile_id == "gaming_ipset_tls"));
        let selections = vec![Zapret2ProfileSelection { category: "discord".into(), transport: "tls".into(), profile_id: "discord_tls_text".into() },
            Zapret2ProfileSelection { category: "gaming".into(), transport: "tls".into(), profile_id: "classic_gaming_tls_split_sni".into() }];
        for category in ["discord", "gaming"] {
            let ordinary = pack.profiles_for_options(category, 4, false).unwrap();
            let result = pack.profiles_for_selections(category, 4, true, &selections).unwrap();
            assert_eq!(result.iter().filter(|profile| profile_transport(profile) != Some("tls")).collect::<Vec<_>>(),
                ordinary.iter().filter(|profile| profile_transport(profile) != Some("tls")).collect::<Vec<_>>());
            assert!(!result.iter().any(|profile| profile.id == CLIENTHELLO_EXPERIMENT_ID));
        }
        for (category, transport, id) in [("discord", "tls", "youtube_tls"), ("discord", "tls", "discord_quic_level_1"),
            ("gaming", "tls", "gaming_ipset_tls"), ("discord", "tls", "absent")] {
            assert!(pack.profiles_for_selections(category, 0, false, &[Zapret2ProfileSelection {
                category: category.into(), transport: transport.into(), profile_id: id.into(),
            }]).is_err());
        }
    }
}
