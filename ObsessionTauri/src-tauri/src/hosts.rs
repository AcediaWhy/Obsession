//! Управление системным hosts-файлом (ИИ-обход) — ТРАНЗАКЦИОННО (WS1).
//!
//! Усилено против исходного `hosts_local_datasource.dart`: сырые байты (F.1),
//! байт-точные снапшоты и явный `last_known_good` вместо эвристики «предпоследний
//! файл», детект внешнего изменения, hash-verify после записи, точный откат.
//! Механику снапшотов/состояния держит [`crate::hosts_snapshot`], проверку
//! payload — [`crate::hosts_validate`].

use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::hosts_snapshot as snap;
use crate::hosts_validate::{validate_hosts_payload, ValidationLimits};
use crate::paths::HOSTS_PATH;
use crate::state::AppState;
use crate::util;

const MAX_HOSTS_BYTES: usize = 10 * 1024 * 1024;
const MARKER_PREFIX: &str = "# obsession:ai-provider=";
const ADDITIONAL_HOSTS_URL: &str =
    "https://raw.githubusercontent.com/AvenCores/Goida-AI-Unlocker/refs/heads/main/additional_hosts.json";

#[derive(Clone, Copy, PartialEq)]
pub enum Provider {
    Malw,
    Geohide,
}

impl Provider {
    pub fn parse(s: &str) -> Provider {
        match s {
            "geohide" => Provider::Geohide,
            _ => Provider::Malw,
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            Provider::Malw => "malw",
            Provider::Geohide => "geohide",
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            Provider::Malw => "dns.malw.link",
            Provider::Geohide => "GeoHide",
        }
    }
    pub fn hosts_url(&self) -> &'static str {
        match self {
            Provider::Malw => {
                "https://raw.githubusercontent.com/ImMALWARE/dns.malw.link/refs/heads/master/hosts"
            }
            Provider::Geohide => {
                "https://github.com/Internet-Helper/GeoHideDNS/raw/refs/heads/main/hosts/hosts"
            }
        }
    }
}

/// Статус ИИ-обхода для UI.
#[derive(Serialize)]
pub struct HostsStatus {
    pub provider: String,
    /// "installed" | "outdated" | "not_installed" | "offline"
    pub status: String,
    pub local_version: String,
    pub remote_version: String,
    /// Есть ли сохранённый last-known-good для «Вернуть рабочую версию» (WS1.7).
    pub rollback_available: bool,
}

fn hosts_path() -> PathBuf {
    PathBuf::from(HOSTS_PATH)
}

fn marker(p: Provider) -> String {
    format!("{MARKER_PREFIX}{}", p.name())
}

fn read_hosts() -> String {
    std::fs::read_to_string(hosts_path()).unwrap_or_default()
}

/// Сырые байты системного hosts (F.1 — без lossy-конверсии на пути записи).
fn read_hosts_bytes() -> Vec<u8> {
    std::fs::read(hosts_path()).unwrap_or_default()
}

/// True, если текущий файл уже помечен Obsession (значит это НЕ исходный hosts).
fn current_is_managed(current: &[u8]) -> bool {
    std::str::from_utf8(current)
        .map(|s| s.contains(MARKER_PREFIX))
        .unwrap_or(false)
}

fn is_installed(p: Provider) -> bool {
    let content = read_hosts();
    if content.contains(&marker(p)) {
        return true;
    }
    // Обратная совместимость по сигнатурным записям.
    match p {
        Provider::Geohide => content.contains("dns.geohide.ru"),
        Provider::Malw => content.contains("dns.malw.link") && !content.contains("dns.geohide.ru"),
    }
}

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("Obsession/1.0.0")
        .build()
        .unwrap_or_default()
}

/// Скачивает hosts провайдера как СЫРЫЕ байты (валидация/нормализация — позже).
async fn download_hosts(app: &AppHandle, p: Provider) -> Result<Vec<u8>, String> {
    let resp = http_client()
        .get(p.hosts_url())
        .send()
        .await
        .map_err(|e| format!("Не удалось скачать hosts ({}): {e}", p.label()))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("Ошибка чтения ответа: {e}"))?;
    if bytes.len() > MAX_HOSTS_BYTES {
        util::emit_log(
            app,
            "error",
            "hosts",
            &format!(
                "hosts-файл слишком большой ({} байт) — отклонён.",
                bytes.len()
            ),
        );
        return Err("hosts-файл слишком большой".to_string());
    }
    Ok(bytes.to_vec())
}

/// Доп. блок Goida AI Unlocker как СЫРЫЕ байты; пустой вектор при любой ошибке.
async fn download_additional_hosts() -> Vec<u8> {
    let resp = match http_client().get(ADDITIONAL_HOSTS_URL).send().await {
        Ok(r) if r.status().is_success() => r,
        _ => return Vec::new(),
    };
    let body = match resp.text().await {
        Ok(b) => b,
        Err(_) => return Vec::new(),
    };
    let data: serde_json::Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let version = data.get("version").and_then(|v| v.as_str()).unwrap_or("");
    let hosts_block = data
        .get("hosts")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if hosts_block.is_empty() {
        Vec::new()
    } else {
        format!("# additional_hosts_version {version}\n{hosts_block}").into_bytes()
    }
}

/// Пути состояния/бэкапов из AppState (инъектируемы для чистых функций snapshot).
fn state_paths(app: &AppHandle) -> (PathBuf, PathBuf) {
    let paths = app.state::<AppState>().paths.clone();
    (paths.hosts_state_path(), paths.backups_dir())
}

fn op_id() -> String {
    chrono::Local::now().format("%Y%m%dT%H%M%S%3f").to_string()
}

fn now_iso() -> String {
    chrono::Local::now().to_rfc3339()
}

pub async fn install(app: &AppHandle, p: Provider) -> Result<(), String> {
    util::emit_log(
        app,
        "info",
        "hosts",
        &format!("Установка ИИ-обхода ({})...", p.label()),
    );
    let (state_path, backups_dir) = state_paths(app);
    let mut state = snap::load_state(&state_path);
    let current = read_hosts_bytes();
    let name = p.name();
    let oid = op_id();
    let ts = now_iso();

    // 1. Захват исходного (до-Obsession) hosts — один раз, если ещё не помечен нами.
    if state.original.is_none() && !current_is_managed(&current) {
        if let Ok(orig) = snap::write_snapshot(
            &backups_dir,
            &format!("{oid}_original"),
            None,
            &ts,
            &current,
        ) {
            snap::set_original_if_absent(&mut state, orig);
        }
    }

    // 2. Детект внешнего изменения: не перезаписываем молча (WS1.4).
    let applied = state
        .providers
        .get(name)
        .and_then(|ps| ps.applied_sha256.clone());
    if snap::is_externally_modified(&current, applied.as_deref()) {
        let _ = snap::write_snapshot(
            &backups_dir,
            &format!("{oid}_external"),
            Some(name),
            &ts,
            &current,
        );
        let _ = snap::save_state(&state_path, &state);
        util::emit_log(
            app,
            "warn",
            "hosts",
            "hosts изменён вне Obsession — автообновление отменено, внешняя версия сохранена в снапшот.",
        );
        return Err(
            "hosts был изменён вне Obsession. Обновление отменено, внешняя версия сохранена."
                .to_string(),
        );
    }

    // 3. Скачивание провайдера + доп. блока (сеть НЕ трогает системный hosts).
    let hosts_bytes = download_hosts(app, p).await?;
    let additional = download_additional_hosts().await;
    let marker_line = marker(p);
    let mut prepared: Vec<u8> =
        Vec::with_capacity(marker_line.len() + 2 + hosts_bytes.len() + additional.len());
    prepared.extend_from_slice(marker_line.as_bytes());
    prepared.push(b'\n');
    prepared.extend_from_slice(&hosts_bytes);
    if !additional.is_empty() {
        prepared.push(b'\n');
        prepared.extend_from_slice(&additional);
    }

    // 4. Валидация подготовленного файла. Сбой = hosts НЕ меняется (аборт ДО записи).
    let limits = ValidationLimits {
        max_bytes: MAX_HOSTS_BYTES,
        required_domains: Vec::new(),
    };
    let report = validate_hosts_payload(&prepared, &limits);
    if !report.is_valid() {
        util::emit_log(
            app,
            "error",
            "hosts",
            &format!("payload не прошёл валидацию: {}", report.errors.join("; ")),
        );
        return Err(format!(
            "Загруженный hosts не прошёл проверку: {}",
            report.errors.first().cloned().unwrap_or_default()
        ));
    }
    let to_write = report.normalized; // LF, без BOM (F.1)

    // 5. Транзакция: pre-op snapshot → атомарная запись → re-read+hash verify.
    let pa = snap::snapshot_and_apply(&hosts_path(), &backups_dir, name, &to_write, &oid, &ts)
        .map_err(|e| format!("Ошибка применения hosts: {e}"))?;

    crate::dpi::flush_dns();

    // 6. Read-only probes AI-сервисов (WS1.6). Без токенов/cookies; откат ТОЛЬКО
    //    если провалены ВСЕ core-сервисы (защита от ложного отката на флапе одного).
    let probes = crate::ai_probe::probe_provider(name, 6).await;
    for pr in &probes {
        util::emit_log(
            app,
            if pr.ok { "info" } else { "warn" },
            "hosts",
            &format!(
                "Проба {} ({}): {}",
                pr.host,
                if pr.core { "core" } else { "опц" },
                if pr.ok {
                    "OK".to_string()
                } else {
                    pr.detail.clone()
                }
            ),
        );
    }
    if crate::ai_probe::all_core_failed(&probes) {
        // Обход не подтвердился ни на одном core-сервисе → точный откат к pre-op.
        let _ = snap::restore_snapshot(&hosts_path(), &backups_dir, &pa.pre_op);
        snap::remove_snapshot(&backups_dir, &pa.applied);
        snap::remove_snapshot(&backups_dir, &pa.pre_op);
        let _ = snap::save_state(&state_path, &state);
        crate::dpi::flush_dns();
        util::emit_log(
            app,
            "error",
            "hosts",
            "Ни один AI-сервис не ответил после обновления — выполнен откат к предыдущей версии.",
        );
        return Err("проверки AI-сервисов не прошли — обновление откачено".to_string());
    }

    // Успех: фиксируем new last-known-good.
    snap::commit_last_known_good(&mut state, name, pa.applied, &backups_dir);
    snap::remove_snapshot(&backups_dir, &pa.pre_op); // транзиентный pre-op больше не нужен
    snap::save_state(&state_path, &state)
        .map_err(|e| format!("не удалось сохранить состояние hosts: {e}"))?;

    util::emit_log(
        app,
        "success",
        "hosts",
        "hosts-файл обновлён (транзакционно), DNS кэш очищен.",
    );
    Ok(())
}

pub async fn uninstall(app: &AppHandle) -> Result<(), String> {
    util::emit_log(app, "info", "hosts", "Удаление ИИ-обхода...");
    let (state_path, backups_dir) = state_paths(app);
    let mut state = snap::load_state(&state_path);

    // Приоритет — точный откат к ИСХОДНОМУ (до-Obsession) снапшоту.
    if let Some(orig) = state.original.clone() {
        snap::restore_snapshot(&hosts_path(), &backups_dir, &orig)
            .map_err(|e| format!("не удалось восстановить исходный hosts: {e}"))?;
        for ps in state.providers.values_mut() {
            ps.applied_sha256 = None; // hosts теперь исходный — обнуляем «применённое»
        }
        let _ = snap::save_state(&state_path, &state);
        crate::dpi::flush_dns();
        util::emit_log(
            app,
            "success",
            "hosts",
            "hosts восстановлен к исходной версии.",
        );
        return Ok(());
    }

    // Нет сохранённого original (например Malw ставился старой версией) → стандартный hosts.
    util::emit_log(
        app,
        "warn",
        "hosts",
        "Исходный снапшот не найден — записываю стандартный hosts.",
    );
    let default_hosts = "# Copyright (c) 1993-2009 Microsoft Corp.\n\
        #\n\
        # This is a sample HOSTS file used by Microsoft TCP/IP for Windows.\n\
        #\n\
        # localhost name resolution is handled within DNS itself.\n\
        #\t127.0.0.1       localhost\n\
        #\t::1             localhost";
    snap::write_atomic(&hosts_path(), default_hosts.as_bytes())
        .map_err(|e| format!("не удалось записать hosts: {e}"))?;
    for ps in state.providers.values_mut() {
        ps.applied_sha256 = None;
    }
    let _ = snap::save_state(&state_path, &state);
    crate::dpi::flush_dns();
    Ok(())
}

/// «Вернуть рабочую версию»: точный откат к last-known-good провайдера
/// (команда `hosts_restore`).
pub async fn restore_last_known_good(app: &AppHandle, p: Provider) -> Result<(), String> {
    let (state_path, backups_dir) = state_paths(app);
    let mut state = snap::load_state(&state_path);
    let name = p.name();
    let lkg = state
        .providers
        .get(name)
        .and_then(|ps| ps.last_known_good.clone())
        .ok_or_else(|| "нет сохранённой рабочей версии".to_string())?;
    snap::restore_snapshot(&hosts_path(), &backups_dir, &lkg)
        .map_err(|e| format!("не удалось восстановить рабочую версию: {e}"))?;
    if let Some(ps) = state.providers.get_mut(name) {
        ps.applied_sha256 = Some(lkg.sha256.clone());
    }
    let _ = snap::save_state(&state_path, &state);
    crate::dpi::flush_dns();
    util::emit_log(
        app,
        "success",
        "hosts",
        "Восстановлена последняя рабочая версия hosts.",
    );
    Ok(())
}

pub async fn check_status(app: &AppHandle, p: Provider) -> HostsStatus {
    // Доступность отката = есть сохранённый last-known-good для провайдера.
    let rollback_available = {
        let (state_path, _) = state_paths(app);
        snap::load_state(&state_path)
            .providers
            .get(p.name())
            .map(|ps| ps.last_known_good.is_some())
            .unwrap_or(false)
    };

    if !is_installed(p) {
        return HostsStatus {
            provider: p.name().to_string(),
            status: "not_installed".to_string(),
            local_version: String::new(),
            remote_version: String::new(),
            rollback_available,
        };
    }

    let content = read_hosts();
    // Провайдеры пишут дату строкой `# Последнее обновление: <дата>`.
    // `# update:` оставлен для обратной совместимости со старым форматом.
    static VERSION_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"#\s*(?:Последнее обновление|update):\s*(.+)").unwrap()
    });
    let local_version = VERSION_RE
        .captures(&content)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
        .unwrap_or_default();

    let mut remote_version = String::new();
    let mut network_error = false;
    let ts = chrono::Local::now().timestamp_millis();
    let url = format!("{}?t={ts}", p.hosts_url());
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("Obsession/1.0.0")
        .build()
        .unwrap_or_default();
    match client
        .get(&url)
        .header("Range", "bytes=0-1024")
        .send()
        .await
    {
        Ok(resp) => match resp.text().await {
            Ok(body) => {
                remote_version = VERSION_RE
                    .captures(&body)
                    .and_then(|c| c.get(1))
                    .map(|m| m.as_str().trim().to_string())
                    .unwrap_or_default();
            }
            Err(_) => network_error = true,
        },
        Err(_) => network_error = true,
    }

    let status = if network_error {
        "offline"
    } else if !local_version.is_empty() && local_version == remote_version {
        "installed"
    } else {
        "outdated"
    };

    HostsStatus {
        provider: p.name().to_string(),
        status: status.to_string(),
        local_version,
        remote_version,
        rollback_available,
    }
}
