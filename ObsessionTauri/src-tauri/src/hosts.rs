//! Управление системным hosts-файлом (ИИ-обход).
//! Порт из `hosts_local_datasource.dart`.

use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Manager};

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

async fn download_hosts(app: &AppHandle, p: Provider) -> Result<String, String> {
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
            &format!("hosts-файл слишком большой ({} байт) — отклонён.", bytes.len()),
        );
        return Err("hosts-файл слишком большой".to_string());
    }
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

async fn download_additional_hosts() -> String {
    let resp = match http_client().get(ADDITIONAL_HOSTS_URL).send().await {
        Ok(r) if r.status().is_success() => r,
        _ => return String::new(),
    };
    let body = match resp.text().await {
        Ok(b) => b,
        Err(_) => return String::new(),
    };
    let data: serde_json::Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => return String::new(),
    };
    let version = data.get("version").and_then(|v| v.as_str()).unwrap_or("");
    let hosts_block = data
        .get("hosts")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if hosts_block.is_empty() {
        String::new()
    } else {
        format!("# additional_hosts_version {version}\n{hosts_block}")
    }
}

fn backup_hosts(app: &AppHandle, action: &str) -> Option<PathBuf> {
    let src = read_hosts();
    if src.is_empty() && !hosts_path().exists() {
        return None;
    }
    let backups_dir = {
        let state = app.state::<AppState>();
        state.paths.backups_dir()
    };
    let _ = std::fs::create_dir_all(&backups_dir);
    let ts = chrono::Local::now().format("%Y-%m-%dT%H-%M-%S").to_string();
    let name = format!("hosts_backup_{action}_{ts}.txt");
    let backup = backups_dir.join(name);
    let header = format!(
        "# Obsession hosts backup\n# action: {action}\n# created_at: {}\n# source: {HOSTS_PATH}\n\n",
        chrono::Local::now()
    );
    match std::fs::write(&backup, format!("{header}{src}")) {
        Ok(_) => {
            util::emit_log(app, "info", "hosts", &format!("Бэкап hosts создан: {}", backup.display()));
            Some(backup)
        }
        Err(e) => {
            util::emit_log(app, "error", "hosts", &format!("Ошибка бэкапа hosts: {e}"));
            None
        }
    }
}

/// Атомарная запись hosts: temp рядом + rename, fallback на прямую запись.
fn apply_hosts(app: &AppHandle, content: &str) -> Result<(), String> {
    let path = hosts_path();
    let tmp = PathBuf::from(format!("{HOSTS_PATH}.obsession.tmp"));

    let write_result = std::fs::write(&tmp, content.as_bytes())
        .and_then(|_| std::fs::rename(&tmp, &path));

    if let Err(_e) = write_result {
        // Fallback: прямая перезапись.
        let _ = std::fs::remove_file(&tmp);
        std::fs::write(&path, content.as_bytes())
            .map_err(|e| format!("Не удалось записать hosts (нет прав?): {e}"))?;
    }

    crate::dpi::flush_dns();
    util::emit_log(app, "success", "hosts", "hosts-файл обновлён, DNS кэш очищен.");
    Ok(())
}

pub async fn install(app: &AppHandle, p: Provider) -> Result<(), String> {
    util::emit_log(app, "info", "hosts", &format!("Установка ИИ-обхода ({})...", p.label()));

    if backup_hosts(app, "install").is_none() {
        return Err("Не удалось создать бэкап, установка отменена.".to_string());
    }

    let hosts = download_hosts(app, p).await?;
    let additional = download_additional_hosts().await;
    let combined = if additional.is_empty() {
        hosts
    } else {
        format!("{hosts}\n{additional}")
    };
    let marked = format!("{}\n{combined}", marker(p));
    apply_hosts(app, &marked)
}

pub async fn uninstall(app: &AppHandle) -> Result<(), String> {
    util::emit_log(app, "info", "hosts", "Удаление ИИ-обхода...");

    if backup_hosts(app, "uninstall").is_none() {
        return Err("Не удалось создать бэкап, удаление отменено.".to_string());
    }

    if restore_latest_backup(app) {
        util::emit_log(app, "success", "hosts", "hosts-файл восстановлен из бэкапа.");
        return Ok(());
    }

    util::emit_log(
        app,
        "warn",
        "hosts",
        "Бэкапов не найдено, записываю стандартный hosts.",
    );
    let default_hosts = "# Copyright (c) 1993-2009 Microsoft Corp.\n\
        #\n\
        # This is a sample HOSTS file used by Microsoft TCP/IP for Windows.\n\
        #\n\
        # localhost name resolution is handled within DNS itself.\n\
        #\t127.0.0.1       localhost\n\
        #\t::1             localhost";
    apply_hosts(app, default_hosts)
}

/// Восстанавливает предпоследний бэкап (последний — только что созданный).
fn restore_latest_backup(app: &AppHandle) -> bool {
    let backups_dir = {
        let state = app.state::<AppState>();
        state.paths.backups_dir()
    };
    let mut files: Vec<PathBuf> = match std::fs::read_dir(&backups_dir) {
        Ok(rd) => rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.extension().and_then(|s| s.to_str()) == Some("txt")
                    && p.file_name()
                        .and_then(|s| s.to_str())
                        .map(|n| n.contains("hosts_backup_"))
                        .unwrap_or(false)
            })
            .collect(),
        Err(_) => return false,
    };
    if files.is_empty() {
        return false;
    }
    files.sort();
    files.reverse();
    // Предпоследний (индекс 1), либо единственный.
    let target = if files.len() > 1 { &files[1] } else { &files[0] };
    let content = match std::fs::read_to_string(target) {
        Ok(c) => c,
        Err(_) => return false,
    };

    // Убираем заголовок бэкапа.
    let mut host_lines = Vec::new();
    let mut skip_header = true;
    for line in content.lines() {
        if skip_header
            && (line.starts_with("# Obsession")
                || line.starts_with("# action")
                || line.starts_with("# created_at")
                || line.starts_with("# source")
                || line.trim().is_empty())
        {
            continue;
        }
        skip_header = false;
        host_lines.push(line);
    }
    apply_hosts(app, host_lines.join("\n").trim()).is_ok()
}

pub async fn check_status(p: Provider) -> HostsStatus {
    if !is_installed(p) {
        return HostsStatus {
            provider: p.name().to_string(),
            status: "not_installed".to_string(),
            local_version: String::new(),
            remote_version: String::new(),
        };
    }

    let content = read_hosts();
    let re = regex::Regex::new(r"# update:\s*(.+)").unwrap();
    let local_version = re
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
    match client.get(&url).header("Range", "bytes=0-1024").send().await {
        Ok(resp) => match resp.text().await {
            Ok(body) => {
                remote_version = re
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
    }
}
