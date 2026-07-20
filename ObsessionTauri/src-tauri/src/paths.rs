//! Пути приложения и распаковка bundled-ресурсов в appdata.
//! Порт из `paths_local_datasource.dart`.

use std::fs;
use std::path::{Path, PathBuf};

/// Версия набора bundled-ассетов = версия крейта. Когда она меняется, при
/// следующем старте `extract_assets` перезаписывает распакованные в appdata
/// файлы (иконки/конфиги/бинарники), иначе старые копии остаются навсегда.
/// Берём из Cargo, чтобы гейт не разъезжался с реальной версией приложения.
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
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
    /// Персистентное состояние управления hosts: снапшоты, last-known-good,
    /// applied_sha256 (WS1). Рядом с backups в base_dir.
    pub fn hosts_state_path(&self) -> PathBuf {
        self.base_dir.join("hosts-state.json")
    }
    pub fn logs_dir(&self) -> PathBuf {
        self.base_dir.join("logs")
    }
    pub fn legacy_reliability_logs_dir(&self) -> PathBuf {
        self.logs_dir().join("legacy-reliability")
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
    /// Подтверждённые Safe Strategy DSL-кандидаты Zapret2 по отпечатку сети.
    /// Хранится отдельно от Legacy `netcache.json`, где `conf` означает `.conf`.
    pub fn adaptive_strategy_cache_path(&self) -> PathBuf {
        self.base_dir.join("adaptive-strategies.json")
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

    /// Путь к winws2.exe (движок Zapret2 Beta). Наличие проверяется отдельно —
    /// бинарник поставляется в 3.1; до него движок помечается недоступным.
    pub fn winws2_path(&self) -> PathBuf {
        self.bin_dir().join("zapret2").join("winws2.exe")
    }

    /// Каталог встроенных Strategy Pack'ов Zapret2 (manifest.json + lua/).
    pub fn strategy_packs_dir(&self) -> PathBuf {
        self.base_dir.join("strategy-packs")
    }
    /// Каталог конкретного встроенного пака (`builtin` по умолчанию).
    #[allow(dead_code)] // потребляется runtime-spawn winws2 (WS3.4, живой тест)
    pub fn strategy_pack_dir(&self, pack: &str) -> PathBuf {
        self.strategy_packs_dir().join(pack)
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

    /// Иконка трея для активного состояния (обход/прокси включены). Раньше это
    /// была отдельная картинка `tray-active.png` — на релизе там осталась старая
    /// космо-аватарка («чёрная дыра»), и `refresh_tray` при включении обхода
    /// менял глаз на неё. Активное состояние теперь показывает ТОТ ЖЕ глаз, что и
    /// покой (различие несут тултип и галочки меню), поэтому возвращаем ту же
    /// `tray.ico`, а не отдельный файл.
    pub fn tray_active_icon_path(&self) -> PathBuf {
        self.icons_dir().join("tray.ico")
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
            self.legacy_reliability_logs_dir(),
            self.profiles_dir(),
            self.icons_dir(),
            self.strategy_packs_dir(),
        ] {
            fs::create_dir_all(d)?;
        }
        Ok(())
    }

    fn version_marker(&self) -> PathBuf {
        self.base_dir.join(".assets_version")
    }

    /// True, если версия ассетов не совпадает с текущей версией приложения,
    /// ЛИБО отсутствует ключевой новый ассет (страховка: новые файлы должны
    /// доезжать в appdata даже без bump версии — иначе dev/апгрейд без смены
    /// версии оставляет winws2/пак недокопированными).
    fn should_overwrite(&self) -> bool {
        let version_mismatch = match fs::read_to_string(self.version_marker()) {
            Ok(s) => s.trim() != APP_VERSION,
            Err(_) => true,
        };
        version_mismatch || self.missing_key_asset()
    }

    /// True, если хотя бы один ожидаемый ассет отсутствует в appdata. Держим
    /// список маленьким — только «якорные» файлы, появление которых означает
    /// новую поставку (winws2 + его runtime DLL + Lua Strategy Pack).
    fn missing_key_asset(&self) -> bool {
        let dev_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources");
        // Проверяем только те, что реально есть в исходнике (иначе на машине без
        // бинарника форсили бы копирование впустую).
        let checks: [(PathBuf, PathBuf); 4] = [
            (
                dev_dir.join("bin/zapret2/winws2.exe"),
                self.bin_dir().join("zapret2/winws2.exe"),
            ),
            (
                dev_dir.join("bin/zapret2/cygwin1.dll"),
                self.bin_dir().join("zapret2/cygwin1.dll"),
            ),
            (
                dev_dir.join("strategy-packs/builtin/lua/zapret-antidpi.lua"),
                self.strategy_packs_dir()
                    .join("builtin/lua/zapret-antidpi.lua"),
            ),
            (
                dev_dir.join("lists/gaming-github.txt"),
                self.lists_dir().join("gaming-github.txt"),
            ),
        ];
        checks
            .iter()
            .any(|(src, dst)| src.exists() && !dst.exists())
    }

    /// Копирует bundled-ресурсы (bin/configs/lists/icons) в appdata.
    /// При смене версии перезаписывает; иначе — только недостающие файлы.
    ///
    /// В dev-режиме ресурсы могут отсутствовать в `resource_dir`, поэтому есть
    /// fallback на исходную папку `src-tauri/resources` (через CARGO_MANIFEST_DIR).
    fn extract_assets(&self, resource_dir: &Path) {
        let force = self.should_overwrite();
        let dev_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources");
        for sub in ["bin", "configs", "lists", "icons", "strategy-packs"] {
            let dst = self.base_dir.join(sub);
            // Копируем из ОБОИХ источников: сначала bundled `resource_dir` (истина
            // в проде), затем `dev_dir` поверх недостающих файлов. В dev-режиме
            // Tauri staging (`target/debug/<sub>`) бывает СТАРЫМ/частичным (новые
            // файлы доезжают только на полный rebuild) — раньше он «перекрывал»
            // полный `resources/<sub>`, и winws2/lua не копировались. Двойной
            // проход заполняет недостающие файлы. Для Strategy Pack в debug он
            // перезаписывает существующие файлы: иначе изменение manifest.json
            // при той же версии приложения не доедет в AppData для live-теста.
            let primary = resource_dir.join(sub);
            if primary.exists() {
                let _ = copy_dir(&primary, &dst, force);
            }
            let fallback = dev_dir.join(sub);
            if fallback.exists() && fallback != primary {
                let overwrite_dev_pack = cfg!(debug_assertions) && sub == "strategy-packs";
                let _ = copy_dir(&fallback, &dst, overwrite_dev_pack);
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
        // Манифест целостности доверенных бинарников (F.2).
        {
            let primary = resource_dir.join("manifest.json");
            let dev_manifest = dev_dir.join("manifest.json");
            // В `tauri dev` staged resource_dir может отставать на один rebuild:
            // бинарник уже скопирован, а manifest.json ещё старый. Поэтому debug
            // всегда берёт и обновляет манифест прямо из исходных resources.
            // В release источником истины остаётся bundled resource_dir.
            let src = if cfg!(debug_assertions) && dev_manifest.exists() {
                dev_manifest
            } else if primary.exists() {
                primary
            } else {
                dev_manifest
            };
            let dst = self.base_dir.join("manifest.json");
            if src.exists() && (force || !dst.exists() || cfg!(debug_assertions)) {
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
