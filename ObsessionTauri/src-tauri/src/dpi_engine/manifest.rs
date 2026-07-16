//! Схема и валидатор Strategy Pack манифеста (Zapret2, WS3 задача 3.3).
//!
//! Lua-стратегии = исполняемый код: пак не применяется без проверки целостности
//! (SHA-256 каждого файла) и совместимости движка/Lua API. Чистый слой — резолвер
//! байтов файла инъектируется (F.3), поэтому валидатор тестируется без FS и без
//! запуска winws2. Первые паки только встроенные; удалённый Lua запрещён (WS6).

#![allow(dead_code)] // подключается к engine lifecycle в WS3 задачи 3.2/3.4

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Версия схемы манифеста, которую понимает этот код.
pub const PACK_SCHEMA_VERSION: u32 = 1;

/// Возможности хоста (движка), с которыми сверяется пак.
#[derive(Clone, Copy, Debug)]
pub struct EngineCapabilities {
    /// Версия winws2 (major, minor, patch).
    pub winws2_version: (u32, u32, u32),
    /// Максимальная версия Lua API, которую поддерживает движок.
    pub lua_api: u32,
}

/// Требования пака к совместимости.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct EngineCompat {
    /// Минимальная версия winws2 вида `">=X.Y.Z"`.
    pub winws2_min: String,
    /// Требуемая версия Lua API (движок должен поддерживать >= этой).
    pub lua_api: u32,
}

/// Файл пака: относительный путь + ожидаемый SHA-256 (hex, lowercase).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackFile {
    pub path: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlobDef {
    pub name: String,
    pub path: String,
}

/// Определение одной стратегии (тройка engine+strategy+category для Мозга — WS5).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StrategyDef {
    pub id: String,
    pub category: String,
    /// Позиция на лестнице агрессивности (меньше = мягче). Мозг идёт снизу вверх.
    pub aggressiveness: u8,
    /// Lua entry-файл стратегии (относительный путь; должен быть среди `files`).
    pub lua: String,
    /// Упорядоченная цепочка `--lua-desync=<arg>` инстансов десинка. Порядок
    /// критичен (winws2 применяет в порядке указания). Напр.
    /// ["fake:blob=fake_default_tls:badsum", "multisplit:pos=1,midsld"].
    #[serde(default)]
    pub desync: Vec<String>,
    /// Транспорт(ы) профиля: включает `--filter-tcp=443`/`--filter-udp=443`.
    /// По умолчанию только TCP (пусто → tcp).
    #[serde(default)]
    pub transports: Vec<String>,
    /// Optional runtime list bindings. Values are safe `.txt` basenames
    /// resolved inside `Paths::lists_dir()` by the launcher.
    #[serde(default)]
    pub hostlist: Option<String>,
    #[serde(default)]
    pub ipset: Option<String>,
    /// Explicit profile port filters. When absent, the Zapret2 builder keeps
    /// the legacy transport-derived TCP/UDP 443 behavior.
    #[serde(default)]
    pub filter_tcp: Option<String>,
    #[serde(default)]
    pub filter_udp: Option<String>,
    #[serde(default)]
    pub filter_l7: Vec<String>,
    #[serde(default)]
    pub payload: Vec<String>,
    #[serde(default)]
    pub out_range: Option<String>,
    #[serde(default)]
    pub in_range: Option<String>,
}

/// Разобранный манифест пака (соответствует `manifest.json`).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StrategyPackManifest {
    pub schema_version: u32,
    pub pack_id: String,
    pub pack_version: String,
    pub engine: EngineCompat,
    pub categories: Vec<String>,
    pub protocols: Vec<String>,
    pub files: Vec<PackFile>,
    #[serde(default)]
    pub blobs: Vec<BlobDef>,
    pub strategies: Vec<StrategyDef>,
    /// Хосты для probe-проверки стратегий (без пользовательских данных).
    #[serde(default)]
    pub probes: Vec<String>,
    /// id стратегии fallback (должен существовать среди `strategies`).
    #[serde(default)]
    pub fallback: Option<String>,
}

/// Отчёт валидации. `errors` пуст ⇔ пак можно активировать.
#[derive(Debug, Default)]
pub struct PackValidationReport {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl PackValidationReport {
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }
}

/// SHA-256 в hex (нижний регистр).
fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn parse_semver(s: &str) -> Option<(u32, u32, u32)> {
    let mut it = s.trim().split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next()?.parse().ok()?;
    let patch = it.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

/// `have >= req` для строки вида `">=X.Y.Z"`. Пустое/битое → несовместимо.
fn winws2_min_satisfied(have: (u32, u32, u32), req: &str) -> bool {
    let req = req.trim();
    let Some(rest) = req.strip_prefix(">=") else {
        return false;
    };
    match parse_semver(rest) {
        Some(want) => have >= want,
        None => false,
    }
}

/// Путь безопасен для распаковки/чтения внутри пака: относительный, без `..`,
/// без корня/буквы диска, без backslash-экранов и NUL.
fn is_safe_rel_path(p: &str) -> bool {
    if p.is_empty() || p.contains('\0') || p.contains('\\') {
        return false;
    }
    if p.starts_with('/') || p.contains(':') {
        return false; // абсолютный путь или диск (C:)
    }
    // Ни один сегмент не равен ".." и не пуст.
    p.split('/').all(|seg| !seg.is_empty() && seg != "..")
}

fn is_safe_identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn is_safe_cli_value(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && !value
            .bytes()
            .any(|b| b == 0 || b.is_ascii_control() || b.is_ascii_whitespace())
}

fn is_safe_list_basename(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.ends_with(".txt")
        && !value.contains("..")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn parse_port_filter(value: &str) -> Option<Vec<(u16, u16)>> {
    if value.is_empty() || value.len() > 128 || value.bytes().any(|b| b.is_ascii_whitespace()) {
        return None;
    }
    value
        .split(',')
        .map(|part| {
            let (start, end) = match part.split_once('-') {
                Some((start, end)) => (start.parse::<u16>().ok()?, end.parse::<u16>().ok()?),
                None => {
                    let port = part.parse::<u16>().ok()?;
                    (port, port)
                }
            };
            (start > 0 && start <= end).then_some((start, end))
        })
        .collect()
}

fn has_high_ports(value: &str) -> bool {
    parse_port_filter(value).is_some_and(|ranges| ranges.iter().any(|(_, end)| *end > 1023))
}

/// Валидирует манифест пака. `caps` — возможности движка; `resolve` отдаёт байты
/// файла пака по относительному пути (`None` = файла нет). Чистая функция.
pub fn validate_pack<F>(
    manifest: &StrategyPackManifest,
    caps: EngineCapabilities,
    resolve: F,
) -> PackValidationReport
where
    F: Fn(&str) -> Option<Vec<u8>>,
{
    let mut report = PackValidationReport::default();
    let mut err = |m: String| report.errors.push(m);

    // 1. Версия схемы.
    if manifest.schema_version != PACK_SCHEMA_VERSION {
        err(format!(
            "несовместимая версия схемы пака: {} (ожидалась {PACK_SCHEMA_VERSION})",
            manifest.schema_version
        ));
    }

    // 2. Совместимость движка: версия winws2 + Lua API.
    if !winws2_min_satisfied(caps.winws2_version, &manifest.engine.winws2_min) {
        err(format!(
            "движок winws2 {:?} не удовлетворяет требованию {:?}",
            caps.winws2_version, manifest.engine.winws2_min
        ));
    }
    if manifest.engine.lua_api > caps.lua_api {
        err(format!(
            "пак требует Lua API {}, движок поддерживает только {}",
            manifest.engine.lua_api, caps.lua_api
        ));
    }

    // 3. Файлы: безопасный путь, наличие, точный SHA-256.
    let mut file_paths: BTreeSet<&str> = BTreeSet::new();
    for f in &manifest.files {
        if !is_safe_rel_path(&f.path) {
            err(format!("небезопасный путь файла в манифесте: {:?}", f.path));
            continue;
        }
        file_paths.insert(f.path.as_str());
        match resolve(&f.path) {
            None => err(format!("файл пака отсутствует: {}", f.path)),
            Some(bytes) => {
                let actual = sha256_hex(&bytes);
                if !actual.eq_ignore_ascii_case(&f.sha256) {
                    err(format!(
                        "хеш не совпал для {}: {actual} ≠ {}",
                        f.path, f.sha256
                    ));
                }
            }
        }
    }

    // 4. Глобальные blobs: безопасное имя, безопасный путь и файл из `files`.
    let mut blob_names: BTreeSet<&str> = BTreeSet::new();
    for blob in &manifest.blobs {
        if !is_safe_identifier(&blob.name) {
            err(format!("небезопасное имя blob: {:?}", blob.name));
        }
        if !blob_names.insert(blob.name.as_str()) {
            err(format!("дублирующееся имя blob: {}", blob.name));
        }
        if !is_safe_rel_path(&blob.path) || !file_paths.contains(blob.path.as_str()) {
            err(format!(
                "blob {} ссылается на необъявленный или небезопасный файл {}",
                blob.name, blob.path
            ));
        }
    }

    // 5. Стратегии: id/category/files + типизированные L7/payload/ranges/desync.
    let mut seen_ids: BTreeSet<&str> = BTreeSet::new();
    let known_cats: BTreeSet<&str> = manifest.categories.iter().map(String::as_str).collect();
    let allowed_l7 = ["http", "tls", "quic", "discord", "stun", "wireguard"];
    let allowed_payload = [
        "http_req",
        "tls_client_hello",
        "quic_initial",
        "discord_ip_discovery",
        "stun",
        "wireguard_initiation",
        "wireguard_cookie",
    ];
    for st in &manifest.strategies {
        if !seen_ids.insert(st.id.as_str()) {
            err(format!("дублирующийся id стратегии: {}", st.id));
        }
        if !known_cats.contains(st.category.as_str()) {
            err(format!(
                "стратегия {} ссылается на неизвестную категорию {}",
                st.id, st.category
            ));
        }
        if !st.lua.is_empty() && !file_paths.contains(st.lua.as_str()) {
            err(format!(
                "стратегия {} ссылается на lua-файл {} вне манифеста files",
                st.id, st.lua
            ));
        }
        for (label, list) in [("hostlist", &st.hostlist), ("ipset", &st.ipset)] {
            if list
                .as_deref()
                .is_some_and(|value| !is_safe_list_basename(value))
            {
                err(format!("стратегия {} содержит небезопасный {label}", st.id));
            }
        }
        for (label, filter) in [
            ("filter_tcp", &st.filter_tcp),
            ("filter_udp", &st.filter_udp),
        ] {
            if filter
                .as_deref()
                .is_some_and(|value| parse_port_filter(value).is_none())
            {
                err(format!("стратегия {} содержит невалидный {label}", st.id));
            }
        }
        let has_high_port_filter = st.filter_tcp.as_deref().is_some_and(has_high_ports)
            || st.filter_udp.as_deref().is_some_and(has_high_ports);
        if has_high_port_filter && st.ipset.is_none() {
            err(format!(
                "стратегия {} использует high-port filter без ipset",
                st.id
            ));
        }
        for l7 in &st.filter_l7 {
            if !allowed_l7.contains(&l7.as_str()) {
                err(format!(
                    "стратегия {} содержит неизвестный filter_l7 {l7}",
                    st.id
                ));
            }
        }
        for payload in &st.payload {
            if !allowed_payload.contains(&payload.as_str()) {
                err(format!(
                    "стратегия {} содержит неизвестный payload {payload}",
                    st.id
                ));
            }
        }
        for (label, range) in [("out_range", &st.out_range), ("in_range", &st.in_range)] {
            if let Some(range) = range {
                if !is_safe_cli_value(range) {
                    err(format!("стратегия {} содержит небезопасный {label}", st.id));
                }
            }
        }
        if st.desync.is_empty() {
            err(format!("стратегия {} не содержит lua-desync", st.id));
        }
        for desync in &st.desync {
            if !is_safe_cli_value(desync) {
                err(format!(
                    "стратегия {} содержит небезопасный lua-desync",
                    st.id
                ));
                continue;
            }
            for part in desync.split(':') {
                let Some(name) = part.strip_prefix("blob=") else {
                    continue;
                };
                if !name.starts_with("fake_default_")
                    && !name.starts_with("0x")
                    && !blob_names.contains(name)
                {
                    err(format!(
                        "стратегия {} ссылается на необъявленный blob {name}",
                        st.id
                    ));
                }
            }
        }
    }

    // 6. Fallback указывает на существующую стратегию.
    if let Some(fb) = &manifest.fallback {
        if !manifest.strategies.iter().any(|s| &s.id == fb) {
            err(format!(
                "fallback ссылается на несуществующую стратегию: {fb}"
            ));
        }
    }

    // 7. Пак без стратегий бесполезен.
    if manifest.strategies.is_empty() {
        err("пак не содержит ни одной стратегии".into());
    }

    report
}

/// Разбирает манифест из JSON-байтов (строгий serde).
pub fn parse_manifest(bytes: &[u8]) -> Result<StrategyPackManifest, String> {
    serde_json::from_slice(bytes).map_err(|e| format!("не удалось разобрать manifest.json: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps() -> EngineCapabilities {
        EngineCapabilities {
            winws2_version: (1, 0, 2),
            lua_api: 2,
        }
    }

    fn file_with(path: &str, bytes: &[u8]) -> PackFile {
        PackFile {
            path: path.into(),
            sha256: sha256_hex(bytes),
        }
    }

    /// Валидный пак с одним lua-файлом и одной стратегией.
    fn valid_manifest() -> (StrategyPackManifest, Vec<(String, Vec<u8>)>) {
        let lua_bytes = b"-- discord tls split\n".to_vec();
        let files = vec![("lua/discord_tls_split.lua".to_string(), lua_bytes.clone())];
        let manifest = StrategyPackManifest {
            schema_version: PACK_SCHEMA_VERSION,
            pack_id: "builtin.discord".into(),
            pack_version: "1.0.0".into(),
            engine: EngineCompat {
                winws2_min: ">=1.0.0".into(),
                lua_api: 2,
            },
            categories: vec!["discord".into()],
            protocols: vec!["tcp".into(), "tls".into()],
            files: vec![file_with("lua/discord_tls_split.lua", &lua_bytes)],
            blobs: vec![],
            strategies: vec![StrategyDef {
                id: "discord_tls_split".into(),
                category: "discord".into(),
                aggressiveness: 1,
                lua: "lua/discord_tls_split.lua".into(),
                desync: vec![
                    "fake:blob=fake_default_tls:badsum".into(),
                    "multisplit:pos=1,midsld".into(),
                ],
                transports: vec!["tcp".into()],
                hostlist: None,
                ipset: None,
                filter_tcp: None,
                filter_udp: None,
                filter_l7: vec!["tls".into()],
                payload: vec!["tls_client_hello".into()],
                out_range: Some("-d10".into()),
                in_range: None,
            }],
            probes: vec!["discord.com".into()],
            fallback: Some("discord_tls_split".into()),
        };
        (manifest, files)
    }

    /// Резолвер из списка (path, bytes).
    fn resolver(files: &[(String, Vec<u8>)]) -> impl Fn(&str) -> Option<Vec<u8>> + '_ {
        move |p: &str| {
            files
                .iter()
                .find(|(path, _)| path == p)
                .map(|(_, b)| b.clone())
        }
    }

    #[test]
    fn accepts_valid_builtin_pack() {
        let (m, files) = valid_manifest();
        let r = validate_pack(&m, caps(), resolver(&files));
        assert!(r.is_valid(), "errors: {:?}", r.errors);
    }

    #[test]
    fn rejects_missing_file() {
        let (m, _) = valid_manifest();
        // Резолвер ничего не находит.
        let r = validate_pack(&m, caps(), |_| None);
        assert!(!r.is_valid());
        assert!(r.errors.iter().any(|e| e.contains("отсутствует")));
    }

    #[test]
    fn rejects_hash_mismatch() {
        let (m, _) = valid_manifest();
        // Резолвер отдаёт ДРУГИЕ байты, чем в манифесте.
        let r = validate_pack(&m, caps(), |_| Some(b"tampered".to_vec()));
        assert!(!r.is_valid());
        assert!(r.errors.iter().any(|e| e.contains("хеш не совпал")));
    }

    #[test]
    fn rejects_incompatible_engine_version() {
        let (mut m, files) = valid_manifest();
        m.engine.winws2_min = ">=2.0.0".into(); // движок 1.0.2 < 2.0.0
        let r = validate_pack(&m, caps(), resolver(&files));
        assert!(!r.is_valid());
        assert!(r
            .errors
            .iter()
            .any(|e| e.contains("не удовлетворяет требованию")));
    }

    #[test]
    fn rejects_incompatible_lua_api() {
        let (mut m, files) = valid_manifest();
        m.engine.lua_api = 5; // движок поддерживает только 2
        let r = validate_pack(&m, caps(), resolver(&files));
        assert!(!r.is_valid());
        assert!(r.errors.iter().any(|e| e.contains("Lua API")));
    }

    #[test]
    fn rejects_duplicate_strategy_id() {
        let (mut m, files) = valid_manifest();
        let dup = m.strategies[0].clone();
        m.strategies.push(dup);
        let r = validate_pack(&m, caps(), resolver(&files));
        assert!(!r.is_valid());
        assert!(r.errors.iter().any(|e| e.contains("дублирующийся id")));
    }

    #[test]
    fn rejects_path_traversal_in_manifest() {
        for bad in ["../evil.lua", "/etc/passwd", "lua/../../x", "C:\\x", "a\\b"] {
            let (mut m, mut files) = valid_manifest();
            m.files[0].path = bad.to_string();
            m.strategies[0].lua = bad.to_string();
            files[0].0 = bad.to_string();
            let r = validate_pack(&m, caps(), resolver(&files));
            assert!(!r.is_valid(), "путь {bad:?} должен быть отклонён");
            assert!(r.errors.iter().any(|e| e.contains("небезопасный путь")));
        }
    }

    #[test]
    fn rejects_unknown_category_and_bad_fallback() {
        let (mut m, files) = valid_manifest();
        m.strategies[0].category = "nonexistent".into();
        m.fallback = Some("ghost".into());
        let r = validate_pack(&m, caps(), resolver(&files));
        assert!(!r.is_valid());
        assert!(r.errors.iter().any(|e| e.contains("неизвестную категорию")));
        assert!(r.errors.iter().any(|e| e.contains("fallback")));
    }

    #[test]
    fn accepts_declared_blob_and_rejects_missing_or_unsafe_blob() {
        let (mut m, mut files) = valid_manifest();
        let blob_bytes = b"fake".to_vec();
        files.push(("blobs/fake.bin".into(), blob_bytes.clone()));
        m.files.push(file_with("blobs/fake.bin", &blob_bytes));
        m.blobs.push(BlobDef {
            name: "tls_google".into(),
            path: "blobs/fake.bin".into(),
        });
        m.strategies[0].desync = vec!["fake:blob=tls_google:repeats=4".into()];
        assert!(validate_pack(&m, caps(), resolver(&files)).is_valid());

        m.blobs[0].path = "../fake.bin".into();
        let r = validate_pack(&m, caps(), resolver(&files));
        assert!(r
            .errors
            .iter()
            .any(|e| e.contains("необъявленный или небезопасный")));
    }

    #[test]
    fn rejects_unknown_l7_payload_empty_desync_and_blob_reference() {
        let (mut m, files) = valid_manifest();
        m.strategies[0].filter_l7 = vec!["not-a-protocol".into()];
        m.strategies[0].payload = vec!["not-a-payload".into()];
        m.strategies[0].desync.clear();
        let r = validate_pack(&m, caps(), resolver(&files));
        assert!(r.errors.iter().any(|e| e.contains("filter_l7")));
        assert!(r.errors.iter().any(|e| e.contains("payload")));
        assert!(r
            .errors
            .iter()
            .any(|e| e.contains("не содержит lua-desync")));

        m.strategies[0].desync = vec!["fake:blob=missing".into()];
        let r = validate_pack(&m, caps(), resolver(&files));
        assert!(r.errors.iter().any(|e| e.contains("необъявленный blob")));
    }

    #[test]
    fn accepts_safe_list_bindings_and_explicit_filters() {
        let (mut m, files) = valid_manifest();
        let strategy = &mut m.strategies[0];
        strategy.hostlist = Some("gaming-github.txt".into());
        strategy.ipset = Some("ipset-gaming.txt".into());
        strategy.filter_tcp = Some("80,443".into());
        strategy.filter_udp = Some("443,1024-65535".into());
        let report = validate_pack(&m, caps(), resolver(&files));
        assert!(report.is_valid(), "errors: {:?}", report.errors);
    }

    #[test]
    fn rejects_unsafe_lists_invalid_ports_and_unscoped_high_ports() {
        let (mut m, files) = valid_manifest();
        let strategy = &mut m.strategies[0];
        strategy.hostlist = Some("../gaming.txt".into());
        strategy.ipset = Some("C:\\ipset.txt".into());
        strategy.filter_tcp = Some("0,443".into());
        strategy.filter_udp = Some("443-80".into());
        let report = validate_pack(&m, caps(), resolver(&files));
        assert!(report.errors.iter().any(|error| error.contains("hostlist")));
        assert!(report.errors.iter().any(|error| error.contains("ipset")));
        assert!(report
            .errors
            .iter()
            .any(|error| error.contains("filter_tcp")));
        assert!(report
            .errors
            .iter()
            .any(|error| error.contains("filter_udp")));

        let (mut m, files) = valid_manifest();
        m.strategies[0].filter_udp = Some("1024-65535".into());
        let report = validate_pack(&m, caps(), resolver(&files));
        assert!(report
            .errors
            .iter()
            .any(|error| error.contains("high-port filter без ipset")));
    }

    #[test]
    fn parse_manifest_roundtrip() {
        let (m, _) = valid_manifest();
        let json = serde_json::to_vec(&m).unwrap();
        let parsed = parse_manifest(&json).unwrap();
        assert_eq!(parsed, m);
    }

    #[test]
    fn parse_manifest_rejects_garbage() {
        assert!(parse_manifest(b"{ not json ]").is_err());
    }
}
