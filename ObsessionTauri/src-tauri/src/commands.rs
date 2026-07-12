//! Поверхность Tauri-команд, вызываемых из фронтенда через `invoke`.

use std::collections::HashMap;

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::hosts::Provider;
use crate::profiles::Profile;
use crate::settings::Settings;
use crate::state::AppState;

#[derive(Serialize)]
pub struct AppConfig {
    pub categories: Vec<String>,
    pub configs: HashMap<String, Vec<String>>,
    pub lists: Vec<String>,
}

/// Категории/конфиги/списки для наполнения UI.
#[tauri::command]
pub fn get_config(app: AppHandle) -> AppConfig {
    let state = app.state::<AppState>();
    let categories = state.paths.get_categories();
    let mut configs = HashMap::new();
    for cat in &categories {
        configs.insert(cat.clone(), state.paths.get_configs_for_category(cat));
    }
    let lists = state.paths.get_list_names();
    AppConfig {
        categories,
        configs,
        lists,
    }
}

#[tauri::command]
pub fn is_elevated() -> bool {
    crate::admin::is_elevated()
}

/// Диагностика доступности заблокированных ресурсов (HTTPS-GET).
#[tauri::command]
pub async fn diagnose() -> Vec<crate::diag::DiagResult> {
    crate::diag::run().await
}

// ─── Автозапуск ───────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_autostart() -> bool {
    crate::autostart::is_enabled()
}

#[tauri::command]
pub fn set_autostart(enable: bool) -> Result<(), String> {
    crate::autostart::set(enable)
}

// ─── DPI ────────────────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
pub struct DpiConfigArg {
    pub category: String,
    pub config_file: String,
}

#[tauri::command]
pub async fn dpi_start(app: AppHandle, configs: Vec<DpiConfigArg>) -> Result<Vec<u32>, String> {
    // Ворота: сериализуем со stop/test, чтобы старт не прервался на середине
    // (иначе Глаза поднимутся на уже убитый winws — обход не детектится).
    let state = app.state::<AppState>();
    let _gate = state.dpi_gate.lock().await;
    let pairs: Vec<(String, String)> = configs
        .into_iter()
        .map(|c| (c.category, c.config_file))
        .collect();
    let pids = crate::dpi::start_many(&app, &pairs).await?;
    // Если Мозг включён — открываем сессию (сбор кандидатов + резолв сети).
    if brain_is_running(&app) {
        let ev = crate::brain::runtime::build_session_start(&app, pairs).await;
        send_brain_event(&app, ev);
    }
    Ok(pids)
}

#[tauri::command]
pub async fn dpi_stop(app: AppHandle) {
    let state = app.state::<AppState>();
    let _gate = state.dpi_gate.lock().await;
    // Сначала сообщаем Мозгу — чтобы он не воспринял штатный стоп как сбой.
    send_brain_event(&app, crate::brain::BrainEvent::SessionStop);
    crate::dpi::stop_all(&app).await;
}

#[tauri::command]
pub async fn dpi_test(app: AppHandle, category: String, config_file: String) -> bool {
    let state = app.state::<AppState>();
    let _gate = state.dpi_gate.lock().await;
    crate::dpi::test(&app, &category, &config_file).await
}

/// Отмена текущего теста. Намеренно НЕ берёт `dpi_gate` (его держит бегущий
/// `dpi_test`) — только ставит флаг и убивает тестовый winws.
#[tauri::command]
pub fn dpi_test_cancel(app: AppHandle) {
    crate::dpi::cancel_test(&app);
}

#[tauri::command]
pub fn dpi_detect_orphaned(app: AppHandle) -> Vec<u32> {
    crate::dpi::detect_orphaned(&app)
}

#[tauri::command]
pub fn dpi_emergency_kill(app: AppHandle) {
    crate::dpi::emergency_kill_all(&app);
}

// ─── Сеть (идентичность для дашборда) ─────────────────────────────────────

#[derive(Serialize)]
pub struct NetworkInfo {
    /// Удалось ли определить сеть через интернет (ipinfo).
    pub online: bool,
    /// Ключ рейтинга `AS<asn>_<COUNTRY>-<REGION>`.
    pub asn_region: Option<String>,
    /// Человекочитаемое имя оператора.
    pub org: Option<String>,
    /// MAC шлюза с маской (приватность): `aa:··:··:··:··:ff`.
    pub gateway_mac_masked: Option<String>,
}

fn mask_mac(mac: &str) -> String {
    let parts: Vec<&str> = mac.split(':').collect();
    if parts.len() == 6 {
        format!("{}:··:··:··:··:{}", parts[0], parts[5])
    } else {
        "··".to_string()
    }
}

/// Идентичность текущей сети: кэш (если Мозг уже резолвил) или резолв на месте
/// (memoization по MAC → ipinfo дёргается один раз на сеть).
async fn current_netid(app: &AppHandle) -> crate::netid::NetIdentity {
    let cached = {
        let st = app.state::<AppState>();
        let g = st.netid.lock().ok();
        g.and_then(|g| g.clone())
    };
    if let Some(id) = cached {
        return id;
    }
    let paths = app.state::<AppState>().paths.clone();
    let id = crate::netid::resolve(&paths).await;
    if let Ok(mut slot) = app.state::<AppState>().netid.lock() {
        *slot = Some(id.clone());
    }
    id
}

/// Идентичность текущей сети для дашборда.
#[tauri::command]
pub async fn get_network_identity(app: AppHandle) -> NetworkInfo {
    let id = current_netid(&app).await;
    NetworkInfo {
        online: id.asn_region.is_some() || id.org.is_some(),
        asn_region: id.asn_region,
        org: id.org,
        gateway_mac_masked: id.gateway_mac.as_deref().map(mask_mac),
    }
}

// ─── Статистика надёжности конфигов (netcache) ────────────────────────────

#[derive(Serialize)]
pub struct ConfStat {
    pub conf: String,
    pub success_count: u64,
    pub confirmed_at: u64,
}

/// Записи надёжности из L1-кэша для текущей сети (по категориям). Пусто, если
/// сеть не идентифицируется или в кэше ничего нет.
#[tauri::command]
pub async fn get_netcache_stats(app: AppHandle) -> HashMap<String, ConfStat> {
    let mac = match current_netid(&app).await.gateway_mac {
        Some(m) => m,
        None => return HashMap::new(),
    };
    let paths = app.state::<AppState>().paths.clone();
    let cache = crate::netcache::NetCache::load(&paths);
    cache
        .network_entries(&mac)
        .into_iter()
        .map(|(cat, e)| {
            (
                cat,
                ConfStat {
                    conf: e.conf,
                    success_count: e.success_count,
                    confirmed_at: e.confirmed_at,
                },
            )
        })
        .collect()
}

/// Отмечает конфиг как рабочий для текущей сети (L1-кэш). Вызывается после
/// успешного ручного теста/авто-подбора — наполняет статистику надёжности и
/// заодно засевает L1 Мозгу. Тихо ничего не делает, если сеть не определяется.
#[tauri::command]
pub async fn record_working_config(app: AppHandle, category: String, conf: String) {
    let id = current_netid(&app).await;
    let Some(mac) = id.gateway_mac else {
        return;
    };
    let paths = app.state::<AppState>().paths.clone();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut cache = crate::netcache::NetCache::load(&paths);
    cache.put(&mac, id.asn_region.as_deref(), &category, &conf, now);
    cache.save(&paths);
}

// ─── Proxy ──────────────────────────────────────────────────────────────

#[tauri::command]
pub fn proxy_available(app: AppHandle) -> bool {
    crate::proxy::available(&app)
}

#[tauri::command]
pub async fn proxy_start(
    app: AppHandle,
    port: u16,
    fake_tls_domain: String,
) -> Result<String, String> {
    crate::proxy::start(&app, port, &fake_tls_domain).await
}

#[tauri::command]
pub async fn proxy_stop(app: AppHandle) {
    crate::proxy::stop(&app).await;
}

#[tauri::command]
pub fn proxy_link(app: AppHandle) -> String {
    crate::proxy::current_link(&app)
}

/// Открывает произвольный URL через системную оболочку. Используется для
/// `tg://proxy?...` с параметрами (`&`), которые ломают `cmd /c start`.
/// PowerShell `Start-Process` корректно передаёт URL целиком в ShellExecute.
#[tauri::command]
pub fn open_external_url(url: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let mut cmd = crate::util::std_command("powershell");
        cmd.args(["-NoProfile", "-NonInteractive", "-Command"])
            .arg(format!(
                "Start-Process -FilePath '{}'",
                url.replace('\'', "''")
            ))
            .spawn()
            .map_err(|e| format!("Не удалось открыть ссылку: {e}"))?;
    }
    #[cfg(target_os = "macos")]
    {
        let mut cmd = std::process::Command::new("open");
        cmd.arg(&url)
            .spawn()
            .map_err(|e| format!("Не удалось открыть ссылку: {e}"))?;
    }
    #[cfg(target_os = "linux")]
    {
        let mut cmd = std::process::Command::new("xdg-open");
        cmd.arg(&url)
            .spawn()
            .map_err(|e| format!("Не удалось открыть ссылку: {e}"))?;
    }
    Ok(())
}

// ─── Hosts (AI) ─────────────────────────────────────────────────────────

#[tauri::command]
pub async fn hosts_status(provider: String) -> crate::hosts::HostsStatus {
    crate::hosts::check_status(Provider::parse(&provider)).await
}

#[tauri::command]
pub async fn hosts_install(app: AppHandle, provider: String) -> Result<(), String> {
    crate::hosts::install(&app, Provider::parse(&provider)).await
}

#[tauri::command]
pub async fn hosts_uninstall(app: AppHandle) -> Result<(), String> {
    crate::hosts::uninstall(&app).await
}

// ─── Settings ───────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Settings {
    let state = app.state::<AppState>();
    let s = state.settings.lock().unwrap().clone();
    s
}

#[tauri::command]
pub fn save_settings(app: AppHandle, settings: Settings) -> Result<(), String> {
    let state = app.state::<AppState>();
    let base = state.paths.base_dir.clone();
    settings.save(&base).map_err(|e| e.to_string())?;
    *state.settings.lock().unwrap() = settings;
    Ok(())
}

/// Меняет глобальный хоткей вкл/выкл защиты: снимает прежнюю комбинацию, ставит
/// новую (пустая строка = выключить) и персистит в настройки. Формат — Tauri-
/// акселератор с Code-именем клавиши (`Ctrl+Shift+KeyO`). Возвращает ошибку, если
/// сочетание невалидно или занято другим приложением (прежнее при этом возвращаем).
#[tauri::command]
pub fn set_hotkey(app: AppHandle, hotkey: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let prev = state.settings.lock().unwrap().hotkey_toggle.clone();

    #[cfg(desktop)]
    {
        use tauri_plugin_global_shortcut::GlobalShortcutExt;
        let gs = app.global_shortcut();
        if !prev.trim().is_empty() {
            let _ = gs.unregister(prev.trim());
        }
        let next = hotkey.trim();
        if !next.is_empty() {
            if let Err(e) = gs.register(next) {
                // Откат: возвращаем прежнее сочетание, чтобы не остаться без хоткея.
                if !prev.trim().is_empty() {
                    let _ = gs.register(prev.trim());
                }
                return Err(format!("Сочетание недоступно: {e}"));
            }
        }
    }

    let base = state.paths.base_dir.clone();
    let mut settings = state.settings.lock().unwrap();
    settings.hotkey_toggle = hotkey;
    settings.save(&base).map_err(|e| e.to_string())?;
    Ok(())
}

// ─── Мозг (авто-восстановление, L3) ───────────────────────────────────────

/// Есть ли живая задача Мозга в состоянии.
fn brain_is_running(app: &AppHandle) -> bool {
    app.state::<AppState>()
        .brain
        .lock()
        .map(|g| g.is_some())
        .unwrap_or(false)
}

/// Шлёт событие Мозгу, если он запущен (иначе тихо игнорирует).
fn send_brain_event(app: &AppHandle, ev: crate::brain::BrainEvent) {
    let tx = {
        let st = app.state::<AppState>();
        let guard = st.brain.lock().ok();
        guard.and_then(|g| g.as_ref().map(|bh| bh.tx.clone()))
    };
    if let Some(tx) = tx {
        let _ = tx.send(ev);
    }
}

/// Включает/выключает авто-восстановление: спавнит или гасит задачу Мозга и
/// персистит флаг в настройках. Идемпотентно.
#[tauri::command]
pub fn brain_set_enabled(app: AppHandle, enabled: bool) -> Result<(), String> {
    {
        let st = app.state::<AppState>();
        let mut guard = st.brain.lock().map_err(|_| "brain lock".to_string())?;
        let running = guard.is_some();
        if enabled && !running {
            *guard = Some(crate::brain::runtime::start(app.clone()));
        } else if !enabled && running {
            if let Some(bh) = guard.take() {
                let _ = bh.tx.send(crate::brain::BrainEvent::Shutdown);
                bh.shutdown();
            }
        }
    }
    // Персист флага.
    let st = app.state::<AppState>();
    let base = st.paths.base_dir.clone();
    let mut settings = st.settings.lock().unwrap();
    settings.auto_recovery = enabled;
    settings.save(&base).map_err(|e| e.to_string())?;
    Ok(())
}

/// Текущий агрегированный статус Мозга (для UI-читалки).
#[tauri::command]
pub fn brain_get_status(app: AppHandle) -> Option<crate::brain::BrainStatus> {
    let st = app.state::<AppState>();
    let guard = st.brain.lock().ok()?;
    let bh = guard.as_ref()?;
    let status = bh.status.borrow().clone();
    Some(status)
}

// ─── Списки (домены / IP) ─────────────────────────────────────────────────

#[tauri::command]
pub fn lists_all(app: AppHandle) -> Vec<crate::lists::ListInfo> {
    let state = app.state::<AppState>();
    crate::lists::list_all(&state.paths.lists_dir())
}

#[tauri::command]
pub fn read_list(app: AppHandle, name: String) -> Result<String, String> {
    let state = app.state::<AppState>();
    crate::lists::read_list(&state.paths.lists_dir(), &name)
}

#[tauri::command]
pub fn save_list(app: AppHandle, name: String, content: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    crate::lists::save_list(&state.paths.lists_dir(), &name, &content)
}

#[tauri::command]
pub fn create_list(app: AppHandle, name: String) -> Result<Vec<crate::lists::ListInfo>, String> {
    let state = app.state::<AppState>();
    crate::lists::create_list(&state.paths.lists_dir(), &name)
}

#[tauri::command]
pub fn delete_list(app: AppHandle, name: String) -> Result<Vec<crate::lists::ListInfo>, String> {
    let state = app.state::<AppState>();
    crate::lists::delete_list(&state.paths.lists_dir(), &name)
}

// ─── Профили (пресеты) ────────────────────────────────────────────────────

#[tauri::command]
pub fn get_profiles(app: AppHandle) -> Vec<Profile> {
    let state = app.state::<AppState>();
    crate::profiles::load(&state.paths.base_dir)
}

#[tauri::command]
pub fn save_profile(app: AppHandle, profile: Profile) -> Result<Vec<Profile>, String> {
    if profile.id.trim().is_empty() {
        return Err("Пустой id профиля".into());
    }
    let state = app.state::<AppState>();
    let base = state.paths.base_dir.clone();
    crate::profiles::upsert(&base, profile).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_profile(app: AppHandle, id: String) -> Result<Vec<Profile>, String> {
    let state = app.state::<AppState>();
    let base = state.paths.base_dir.clone();
    crate::profiles::delete(&base, &id).map_err(|e| e.to_string())
}
