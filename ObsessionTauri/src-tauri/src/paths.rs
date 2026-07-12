//! Пути приложения и распаковка bundled-ресурсов в appdata.
//! Порт из `paths_local_datasource.dart`.

use std::fs;
use std::path::{Path, PathBuf};

pub const APP_VERSION: &str = "1.0.4";
pub const APP_DATA_FOLDER: &str = "Obsession";
pub const WINWS_EXE: &str = "winws.exe";
pub const TGPROXY_EXE: &str = "tg_ws_proxy.exe";

/// Системный hosts-файл Windows.
pub const HOSTS_PATH: &str = r"C:\Windows\System32\drivers\etc\hosts";

#[derive(Clone)]
pub struct Paths {
    pub base_dir: PathBuf,
}

impl Paths {
    /// Инициализирует папки в `%APPDATA%\Obsession` и распаковывает ресурсы.
    pub fn init(resource_dir: &Path) -> std::io::Result<Self> {
        let appdata = std::env::var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir());
        let base_dir = appdata.join(APP_DATA_FOLDER);

        let paths = Paths { base_dir };
        paths.ensure_dirs()?;
        paths.extract_assets(resource_dir);
        paths.sanitize_configs();
        Ok(paths)
    }

    pub fn bin_dir(&self) -> PathBuf {
        self.base_dir.join("bin")
    }
    pub fn configs_dir(&self) -> PathBuf {
        self.base_dir.join("configs")
    }
    pub fn lists_dir(&self) -> PathBuf {
        self.base_dir.join("lists")
    }
    pub fn autohosts_dir(&self) -> PathBuf {
        self.base_dir.join("autohosts")
    }
    pub fn backups_dir(&self) -> PathBuf {
        self.base_dir.join("hosts-backups")
    }
    pub fn logs_dir(&self) -> PathBuf {
        self.base_dir.join("logs")
    }
    pub fn profiles_dir(&self) -> PathBuf {
        self.base_dir.join("profiles")
    }
    pub fn icons_dir(&self) -> PathBuf {
        self.base_dir.join("icons")
    }

    /// Bundled-рейтинг стратегий под ASN_region (копируется из ресурсов в appdata).
    pub fn ranking_path(&self) -> PathBuf {
        self.base_dir.join("ranking.json")
    }
    /// L1-кэш «что работало в этой сети» (пишется рантаймом Мозга).
    pub fn netcache_path(&self) -> PathBuf {
        self.base_dir.join("netcache.json")
    }
    /// Кэш сетевой идентичности: MAC шлюза → ASN_region (мемоизация ipinfo).
    pub fn netid_cache_path(&self) -> PathBuf {
        self.base_dir.join("netid_cache.json")
    }
    /// Last-good список CF-фронтинг доменов TgWsProxy (передаётся ему через
    /// `--cfproxy-cache`, чтобы пережить недоступность GitHub при рестарте).
    pub fn cfproxy_cache_path(&self) -> PathBuf {
        self.base_dir.join("cfproxy_cache.json")
    }

    pub fn winws_path(&self) -> PathBuf {
        self.bin_dir().join(WINWS_EXE)
    }

    /// Ищет TgWsProxy по нескольким возможным именам.
    pub fn tgproxy_path(&self) -> Option<PathBuf> {
        let names = [
            TGPROXY_EXE,
            "tg_ws_proxy.exe",
            "TgWsProxy.exe",
            "tg-ws-proxy.exe",
        ];
        for n in names {
            let p = self.bin_dir().join(n);
            if p.exists() {
                return Some(p);
            }
        }
        None
    }

    pub fn tray_icon_path(&self) -> PathBuf {
        self.icons_dir().join("tray.ico")
    }

    /// Иконка трея для активного состояния (обход/прокси включены).
    pub fn tray_active_icon_path(&self) -> PathBuf {
        self.icons_dir().join("tray-active.png")
    }

    pub fn config_path(&self, category: &str, conf_file: &str) -> PathBuf {
        self.configs_dir().join(category).join(conf_file)
    }

    /// Категории DPI (папки внутри configs, кроме служебных).
    pub fn get_categories(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Ok(rd) = fs::read_dir(self.configs_dir()) {
            for e in rd.flatten() {
                if e.path().is_dir() {
                    if let Some(name) = e.file_name().to_str() {
                        if name != "lists" && name != "bin" {
                            out.push(name.to_string());
                        }
                    }
                }
            }
        }
        out.sort();
        out
    }

    /// `.conf`-файлы категории, отсортированные по имени.
    pub fn get_configs_for_category(&self, category: &str) -> Vec<String> {
        let mut out = Vec::new();
        if let Ok(rd) = fs::read_dir(self.configs_dir().join(category)) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().and_then(|s| s.to_str()) == Some("conf") {
                    if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
                        out.push(name.to_string());
                    }
                }
            }
        }
        out.sort();
        out
    }

    pub fn get_list_names(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Ok(rd) = fs::read_dir(self.lists_dir()) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().and_then(|s| s.to_str()) == Some("txt") {
                    if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                        out.push(stem.to_string());
                    }
                }
            }
        }
        out.sort();
        out
    }

    fn ensure_dirs(&self) -> std::io::Result<()> {
        for d in [
            self.base_dir.clone(),
            self.bin_dir(),
            self.configs_dir(),
            self.lists_dir(),
            self.autohosts_dir(),
            self.backups_dir(),
            self.logs_dir(),
            self.profiles_dir(),
            self.icons_dir(),
        ] {
            fs::create_dir_all(d)?;
        }
        Ok(())
    }

    fn version_marker(&self) -> PathBuf {
        self.base_dir.join(".assets_version")
    }

    /// True, если версия ассетов не совпадает с текущей версией приложения.
    fn should_overwrite(&self) -> bool {
        match fs::read_to_string(self.version_marker()) {
            Ok(s) => s.trim() != APP_VERSION,
            Err(_) => true,
        }
    }

    /// Копирует bundled-ресурсы (bin/configs/lists/icons) в appdata.
    /// При смене версии перезаписывает; иначе — только недостающие файлы.
    ///
    /// В dev-режиме ресурсы могут отсутствовать в `resource_dir`, поэтому есть
    /// fallback на исходную папку `src-tauri/resources` (через CARGO_MANIFEST_DIR).
    fn extract_assets(&self, resource_dir: &Path) {
        let force = self.should_overwrite();
        let dev_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources");
        for sub in ["bin", "configs", "lists", "icons"] {
            let src = {
                let primary = resource_dir.join(sub);
                if primary.exists() {
                    primary
                } else {
                    dev_dir.join(sub)
                }
            };
            let dst = self.base_dir.join(sub);
            if src.exists() {
                let _ = copy_dir(&src, &dst, force);
            }
        }
        // Одиночный bundled-файл рейтинга (цикл выше ходит только по директориям).
        {
            let primary = resource_dir.join("ranking.json");
            let src = if primary.exists() {
                primary
            } else {
                dev_dir.join("ranking.json")
            };
            let dst = self.ranking_path();
            if src.exists() && (force || !dst.exists()) {
                let _ = fs::copy(&src, &dst);
            }
        }
        if force {
            let _ = fs::write(self.version_marker(), APP_VERSION);
        }
    }

    /// Снимает UTF-8 BOM (EF BB BF) со всех `.conf` в configs. КРИТИЧНО: winws
    /// читает конфиг как `@file` и трактует BOM как часть ПЕРВОГО аргумента —
    /// `﻿--wf-tcp=...` не распознаётся, фильтр окна WinDivert не ставится, десинк
    /// не применяется НИ к чему (хендл открыт, но пакеты не захватываются).
    /// Идемпотентно и дёшево — гоняем на каждом старте, чинит и старые установки.
    fn sanitize_configs(&self) {
        let mut stack = vec![self.configs_dir()];
        while let Some(dir) = stack.pop() {
            let Ok(rd) = fs::read_dir(&dir) else { continue };
            for entry in rd.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().and_then(|s| s.to_str()) == Some("conf") {
                    if let Ok(bytes) = fs::read(&p) {
                        if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
                            let _ = fs::write(&p, &bytes[3..]);
                        }
                    }
                }
            }
        }
    }
}

/// Рекурсивно копирует директорию. `overwrite=false` пропускает существующие.
fn copy_dir(src: &Path, dst: &Path, overwrite: bool) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to, overwrite)?;
        } else {
            if to.exists() && !overwrite {
                continue;
            }
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}
