//! Поверхность Tauri-команд, вызываемых из фронтенда через `invoke`.

use std::collections::HashMap;

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::hosts::Provider;
use crate::profiles::Profile;
use crate::settings::{Settings, SettingsPatch};
use crate::state::AppState;
use crate::util::{LockExt, VersionedSection};

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
pub fn set_autostart(app: AppHandle, enable: bool) -> Result<(), String> {
    crate::autostart::set(enable)?;
    app.state::<AppState>().settings_revision.bump();
    Ok(())
}

// ─── DPI ────────────────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
pub struct DpiConfigArg {
    pub category: String,
    pub config_file: String,
}

fn runtime_is_shutting_down(app: &AppHandle) -> bool {
    app.state::<AppState>()
        .shutting_down
        .load(std::sync::atomic::Ordering::SeqCst)
}

pub(crate) async fn dpi_start_locked(
    app: &AppHandle,
    pairs: Vec<(String, String)>,
) -> Result<Vec<u32>, String> {
    if runtime_is_shutting_down(app) {
        return Err("Приложение завершает работу.".to_string());
    }
    if app.state::<AppState>().settings.lock_recover().dpi_engine == "zapret2" {
        let migration_app = app.clone();
        tauri::async_runtime::spawn_blocking(move || crate::protected_runtime::zapret2_picker::migrate_saved_profiles(&migration_app))
            .await.map_err(|error| error.to_string())??;
    }
    let (engine, zapret2_level, zapret2_clienthello, zapret2_profiles) = {
        let state = app.state::<AppState>();
        let settings = state.settings.lock_recover();
        (settings.dpi_engine.clone(), settings.zapret2_level, false, settings.zapret2_profiles.clone())
    };
    let protected_engine = if engine == "zapret2" {
        if !crate::protected_runtime::zapret2_available() {
            return Err("Zapret2 недоступен в защищённой службе.".to_string());
        }
        obsession_runtime_protocol::DpiEngine::Zapret2
    } else {
        obsession_runtime_protocol::DpiEngine::Legacy
    };

    crate::protected_runtime::dpi_start(app, protected_engine, pairs, zapret2_level, zapret2_clienthello, zapret2_profiles).await?;
    if runtime_is_shutting_down(app) {
        let _ = crate::protected_runtime::dpi_stop(app).await;
        return Err("Запуск отменён: приложение завершает работу.".to_string());
    }
    // PID остаются внутри LocalSystem-службы и намеренно не раскрываются UI.
    Ok(Vec::new())
}

pub(crate) async fn dpi_stop_locked(app: &AppHandle) -> Result<(), String> {
    // Сначала сообщаем Мозгу — штатный stop не должен выглядеть как сбой.
    send_brain_event(app, crate::brain::BrainEvent::SessionStop).await;
    crate::protected_runtime::dpi_stop(app).await
}

pub(crate) async fn dpi_start_session(
    app: &AppHandle,
    pairs: Vec<(String, String)>,
) -> Result<Vec<u32>, String> {
    crate::util::emit_log(
        app,
        "info",
        "dpi",
        "Запрошен запуск DPI: источник=команда интерфейса",
    );
    if runtime_is_shutting_down(app) {
        return Err("Приложение завершает работу.".to_string());
    }
    let state = app.state::<AppState>();
    let _gate = state.dpi_gate.lock().await;
    dpi_start_locked(app, pairs).await
}

pub(crate) async fn dpi_stop_session(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let _gate = state.dpi_gate.lock().await;
    dpi_stop_locked(app).await
}

/// Общий toggle для UI-independent входов (tray/hotkey). Возвращает итоговый
/// active state. Использует тот же Brain SessionStart/Stop, что и UI-команды.
pub(crate) async fn dpi_toggle_session(
    app: &AppHandle,
    pairs: Vec<(String, String)>,
) -> Result<bool, String> {
    if runtime_is_shutting_down(app) {
        return Err("Приложение завершает работу.".to_string());
    }
    let state = app.state::<AppState>();
    let _gate = state.dpi_gate.lock().await;
    if runtime_is_shutting_down(app) {
        return Err("Приложение завершает работу.".to_string());
    }
    let active = crate::protected_runtime::dpi_active().await?;
    crate::util::emit_log(
        app,
        "info",
        "dpi",
        &format!("Переключение DPI: служба до команды active={active}"),
    );
    if active {
        dpi_stop_locked(app).await?;
        Ok(false)
    } else {
        dpi_start_locked(app, pairs).await?;
        Ok(true)
    }
}

#[tauri::command]
pub async fn dpi_start(app: AppHandle, configs: Vec<DpiConfigArg>) -> Result<Vec<u32>, String> {
    let pairs = configs
        .into_iter()
        .map(|c| (c.category, c.config_file))
        .collect();
    dpi_start_session(&app, pairs).await
}

#[tauri::command]
pub async fn dpi_stop(app: AppHandle) -> Result<(), String> {
    dpi_stop_session(&app).await
}

/// Один движок для списка в UI: описание + доступность бинарника.
#[derive(serde::Serialize)]
pub struct EngineOption {
    pub kind: String,
    pub version: String,
    pub beta: bool,
    /// Установлен ли бинарник движка (winws/winws2 присутствует в bin/).
    pub available: bool,
    /// Выбран ли этот движок в настройках.
    pub selected: bool,
}

/// Список DPI-движков для UI. Доступность определяется возможностями
/// аутентифицированной системной службы, а не файлами в пользовательском профиле.
#[tauri::command]
pub fn dpi_engine_list(app: AppHandle) -> Vec<EngineOption> {
    use crate::dpi_engine::EngineKind;
    let state = app.state::<AppState>();
    let selected = state.settings.lock_recover().dpi_engine.clone();
    let protected_legacy_available = crate::protected_runtime::dpi_available();
    let protected_zapret2_available = crate::protected_runtime::zapret2_available();
    [EngineKind::Legacy, EngineKind::Zapret2]
        .into_iter()
        .map(|k| {
            let d = k.describe();
            let available = match k {
                EngineKind::Legacy => protected_legacy_available,
                EngineKind::Zapret2 => protected_zapret2_available,
            };
            EngineOption {
                kind: d.kind.to_string(),
                version: d.version,
                beta: d.beta,
                available,
                selected: selected == d.kind,
            }
        })
        .collect()
}

#[tauri::command]
pub fn dpi_zapret2_profiles(app: AppHandle, categories: Vec<String>) -> Result<Vec<crate::dpi::Zapret2ProfileDescriptor>, String> {
    crate::protected_runtime::zapret2_picker::migrate_saved_profiles(&app)?;
    let selections = crate::protected_runtime::zapret2_selections().unwrap_or_else(|| categories.into_iter().map(|category| (category, String::new())).collect());
    crate::dpi::describe_zapret2_profiles(&app, &selections)
}

#[tauri::command]
pub fn dpi_zapret2_catalog(app: AppHandle, categories: Vec<String>) -> Result<Vec<crate::protected_runtime::zapret2_picker::CatalogProfile>, String> {
    crate::protected_runtime::zapret2_picker::catalog(&app, &categories)
}

#[tauri::command]
pub async fn dpi_zapret2_select(app: AppHandle, category: String, transport: String, profile_id: String) -> Result<(), String> {
    // Closing the invoking view must not drop a launched runtime's cleanup.
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let _gate = state.dpi_gate.lock().await;
        crate::protected_runtime::zapret2_picker::select(&app, category, transport, profile_id).await
    }).await.map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn dpi_zapret2_pick(app: AppHandle, category: String, transport: String, mode: String,
    composition: Option<obsession_runtime_protocol::zapret2_composition::DiscordComposition>) -> Result<crate::protected_runtime::zapret2_picker::PickerResult, String> {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let _gate = state.dpi_gate.lock().await;
        crate::protected_runtime::zapret2_picker::pick(&app, category, transport, mode, composition).await
    }).await.map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn dpi_zapret2_apply_result(app: AppHandle, run_id: String, category: String, transport: String, profile_id: String) -> Result<(), String> {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let _gate = state.dpi_gate.lock().await;
        crate::protected_runtime::zapret2_picker::apply_result(&app, run_id, category, transport, profile_id).await
    }).await.map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn dpi_zapret2_test_scenario(app: AppHandle, request_id: String, run_id: String, category: String, transport: String,
    profile_id: String, scenario: crate::protected_runtime::zapret2_scenario::Scenario) -> Result<crate::protected_runtime::zapret2_scenario::ResultRecord, String> {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let _gate = state.dpi_gate.lock().await;
        crate::protected_runtime::zapret2_picker::test_scenario(&app, request_id, run_id, category, transport, profile_id, scenario).await
    }).await.map_err(|error| error.to_string())?
}

#[tauri::command]
pub fn dpi_zapret2_scenario_observe(trial_id: String, step_id: String,
    observation: crate::protected_runtime::zapret2_scenario::Observation) -> Result<(), String> {
    // Deliberately independent of dpi_gate: the trial owns it while waiting.
    crate::protected_runtime::zapret2_scenario::observe(&trial_id, &step_id, observation)
}

#[tauri::command]
pub async fn dpi_zapret2_history(app: AppHandle) -> Result<Vec<crate::protected_runtime::zapret2_history::Record>, String> {
    let base = app.state::<AppState>().paths.base_dir.clone();
    tauri::async_runtime::spawn_blocking(move || crate::protected_runtime::zapret2_history::load(&base))
        .await.map_err(|error| error.to_string())?
}

/// Меняет выбранный DPI-движок. Отклоняет всё, что ещё не предоставлено
/// защищённой службой.
/// Смена сериализуется dpi_gate — не пересекается с активным start/stop.
#[tauri::command]
pub async fn dpi_engine_set(app: AppHandle, engine: String) -> Result<(), String> {
    use crate::dpi_engine::EngineKind;
    let kind = EngineKind::parse(&engine);
    let state = app.state::<AppState>();
    if kind == EngineKind::Zapret2 && !crate::protected_runtime::zapret2_available() {
        return Err("Zapret2 ещё не перенесён в защищённую службу.".to_string());
    }
    // Сериализуем с активным DPI-циклом, чтобы не переключить движок посреди start.
    let _gate = state.dpi_gate.lock().await;
    let current = EngineKind::parse(&state.settings.lock_recover().dpi_engine);
    if current == kind {
        return Ok(());
    }
    let runtime_active = crate::protected_runtime::dpi_active().await?;
    if runtime_active {
        return Err("Сначала остановите активный обход, затем переключите DPI-движок.".to_string());
    }
    // mutate_settings использует blocking_lock и делает fsync/rename. Async-команда
    // не должна вызывать его прямо на Tokio worker: blocking_lock там паникует,
    // из-за чего переключатель в UI визуально не реагировал.
    let save_app = app.clone();
    let engine_name = kind.name().to_string();
    tauri::async_runtime::spawn_blocking(move || {
        mutate_settings(&save_app, |settings| settings.dpi_engine = engine_name)
    })
    .await
    .map_err(|error| format!("Не удалось завершить переключение DPI-движка: {error}"))??;
    crate::util::emit_log(
        &app,
        "info",
        "dpi",
        &format!("DPI-движок переключён на {}", kind.name()),
    );
    Ok(())
}

#[tauri::command]
pub async fn dpi_test(
    app: AppHandle,
    category: String,
    config_file: String,
    fail_fast: Option<bool>,
) -> Result<crate::protected_runtime::legacy_test::TestReport, String> {
    if runtime_is_shutting_down(&app) {
        return Err("Приложение завершает работу.".into());
    }
    let state = app.state::<AppState>();
    let _gate = state.dpi_gate.lock().await;
    if runtime_is_shutting_down(&app) {
        return Err("Приложение завершает работу.".into());
    }
    crate::protected_runtime::legacy_test::run(
        &app,
        &category,
        &config_file,
        fail_fast.unwrap_or(false),
    )
    .await
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
pub async fn dpi_emergency_kill(app: AppHandle) {
    let state = app.state::<AppState>();
    let _gate = state.dpi_gate.lock().await;
    send_brain_event(&app, crate::brain::BrainEvent::SessionStop).await;
    crate::legacy_reliability::status::publish(
        &app,
        crate::legacy_reliability::status::LegacyReliabilityStatus::inactive(),
    );
    // Close the Legacy session before the blocking emergency teardown. The
    // manager task owns its ingress sender, so merely stopping Eyes would
    // otherwise leave the observe-only runtime alive until the next start.
    let legacy_manager = state.legacy_manager.lock_recover().take();
    if let Some(manager) = legacy_manager {
        manager.shutdown().await;
    }
    // emergency_kill_all делает блокирующий stop_eyes(join) + taskkill /IM —
    // уводим с tokio-воркера, чтобы не занимать его под dpi_gate.
    let app2 = app.clone();
    let _ =
        tauri::async_runtime::spawn_blocking(move || crate::dpi::emergency_kill_all(&app2)).await;
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
    // Single-flight: под gate повторно проверяем кэш (его мог заполнить
    // конкурентный резолв, ждавший на этом же gate), иначе резолвим ipinfo раз.
    let state = app.state::<AppState>();
    let _gate = state.netid_gate.lock().await;
    if let Some(id) = app
        .state::<AppState>()
        .netid
        .lock()
        .ok()
        .and_then(|g| g.clone())
    {
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

/// Закрывает LAN-публикацию (доступ с телефона), не останавливая локальный прокси.
#[tauri::command]
pub async fn proxy_close_lan(app: AppHandle) {
    crate::proxy::close_lan_publication(&app, "закрыто вручную").await;
}

#[tauri::command]
pub fn proxy_link(app: AppHandle) -> String {
    crate::proxy::current_link(&app)
}

/// Схемы, которые приложению реально нужно открывать: `tg://proxy?...` для
/// Telegram-прокси и http(s) для внешних ссылок UI.
const EXTERNAL_URL_SCHEMES: [&str; 3] = ["tg://", "https://", "http://"];

/// Максимум для `tg://proxy?...`: server+port+secret с запасом. Всё длиннее —
/// не наша ссылка.
const MAX_EXTERNAL_URL_LEN: usize = 2048;

/// Проверяет URL перед передачей в `Start-Process -FilePath`.
/// Процесс работает с правами администратора, поэтому пути к файлам и
/// управляющие символы запрещены. Допустимые схемы перечислены в
/// [`EXTERNAL_URL_SCHEMES`].
fn validate_external_url(url: &str) -> Result<(), String> {
    if url.len() > MAX_EXTERNAL_URL_LEN {
        return Err("Ссылка слишком длинная.".to_string());
    }
    if url.chars().any(|c| c.is_control()) {
        return Err("Ссылка содержит управляющие символы.".to_string());
    }
    // Backslash в URL легитимным не бывает, зато превращает строку в путь
    // (в т.ч. UNC `\\host\share`), который Start-Process с радостью запустит.
    if url.contains('\\') {
        return Err("Ссылка содержит обратный слэш.".to_string());
    }

    let lowered = url.to_ascii_lowercase();
    if !EXTERNAL_URL_SCHEMES
        .iter()
        .any(|scheme| lowered.starts_with(scheme))
    {
        return Err("Разрешены только ссылки tg://, https:// и http://.".to_string());
    }
    Ok(())
}

/// Открывает произвольный URL через системную оболочку. Используется для
/// `tg://proxy?...` с параметрами (`&`), которые ломают `cmd /c start`.
/// PowerShell `Start-Process` корректно передаёт URL целиком в ShellExecute.
/// Строка проходит [`validate_external_url`] — `Start-Process -FilePath`
/// принимает не только URL, а процесс элевирован.
#[tauri::command]
pub fn open_external_url(url: String) -> Result<(), String> {
    validate_external_url(&url)?;

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
pub async fn hosts_status(app: AppHandle, provider: String) -> crate::hosts::HostsStatus {
    crate::hosts::check_status(&app, Provider::parse(&provider)).await
}

#[tauri::command]
pub async fn system_dns_status() -> Result<obsession_runtime_protocol::SystemDnsStatus, String> {
    crate::protected_runtime::system_dns_status().await
}

#[tauri::command]
pub async fn system_dns_restore(app: AppHandle) -> Result<obsession_runtime_protocol::SystemDnsStatus, String> {
    let state = app.state::<AppState>();
    let _gate = state.dpi_gate.lock().await;
    let status = crate::protected_runtime::system_dns_restore().await?;
    crate::util::emit_log(&app, "info", "dns", "dns_recovery_restored: исходные настройки системного DNS восстановлены");
    Ok(status)
}

#[tauri::command]
pub async fn hosts_install(
    app: AppHandle,
    provider: String,
) -> Result<obsession_runtime_protocol::HostsHealthSnapshot, String> {
    let state = app.state::<AppState>();
    let _gate = state.hosts_gate.lock().await;
    crate::hosts::install(&app, Provider::parse(&provider)).await
}

#[tauri::command]
pub async fn hosts_check(
    app: AppHandle,
    max_age_seconds: u32,
) -> Result<obsession_runtime_protocol::HostsHealthSnapshot, String> {
    let state = app.state::<AppState>();
    let _gate = state.hosts_gate.lock().await;
    crate::hosts::check_health(max_age_seconds).await
}

#[tauri::command]
pub async fn hosts_uninstall(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let _gate = state.hosts_gate.lock().await;
    crate::hosts::uninstall(&app).await
}

#[tauri::command]
pub async fn hosts_refresh_gemini(
    app: AppHandle,
    preference: obsession_runtime_protocol::GeminiRoutePreference,
) -> Result<obsession_runtime_protocol::HostsHealthSnapshot, String> {
    let state = app.state::<AppState>();
    let _gate = state.hosts_gate.lock().await;
    crate::protected_runtime::hosts_refresh_gemini(preference).await?;
    state.hosts_revision.bump();
    crate::protected_runtime::hosts_check(900).await
}

#[tauri::command]
pub async fn hosts_restore(
    app: AppHandle,
    provider: String,
) -> Result<obsession_runtime_protocol::HostsHealthSnapshot, String> {
    let state = app.state::<AppState>();
    let _gate = state.hosts_gate.lock().await;
    crate::hosts::restore_last_known_good(&app, Provider::parse(&provider)).await
}

// ─── Runtime snapshot (resume UI) ─────────────────────────────────────────

#[derive(Serialize)]
pub struct RuntimeSnapshot {
    pub dpi: crate::util::DpiStatusPayload,
    pub proxy: crate::util::ProxyStatusPayload,
    pub brain: Option<crate::brain::BrainStatus>,
}

#[tauri::command]
pub fn runtime_get_snapshot(app: AppHandle) -> RuntimeSnapshot {
    let dpi = crate::protected_runtime::dpi_status();
    let proxy = {
        let state = app.state::<AppState>();
        let p = state.proxy.lock_recover();
        crate::util::ProxyStatusPayload {
            running: p.pid.is_some(),
            link: p.link.clone(),
            lan_link: p.lan_link.clone(),
            lan_published: p.lan_published,
            lan_expiry_unix: p.lan_expiry_unix,
        }
    };
    let brain = {
        let state = app.state::<AppState>();
        state
            .brain
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(|bh| bh.status.borrow().clone()))
    };
    RuntimeSnapshot {
        dpi,
        proxy,
        brain,
    }
}

/// Версия wire-контракта единого startup/resume snapshot.
pub const BOOTSTRAP_SCHEMA_VERSION: u32 = 9;

#[derive(Serialize)]
pub struct BootstrapSettings {
    pub settings: Settings,
    pub elevated: bool,
    pub autostart: bool,
    #[serde(rename = "protectedRuntime")]
    pub protected_runtime: crate::protected_runtime::ProtectedRuntimeCapabilities,
    #[serde(rename = "protectedRuntimeAvailable")]
    pub protected_runtime_available: bool,
    #[serde(rename = "protectedDpiAvailable")]
    pub protected_dpi_available: bool,
    #[serde(rename = "protectedLegacyReliabilityAvailable")]
    pub protected_legacy_reliability_available: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapSnapshot {
    pub schema_version: u32,
    pub settings: VersionedSection<BootstrapSettings>,
    pub dpi: VersionedSection<crate::util::DpiStatusPayload>,
    pub proxy: VersionedSection<crate::util::ProxyStatusPayload>,
    pub brain: VersionedSection<Option<crate::brain::BrainStatus>>,
    pub legacy_reliability:
        VersionedSection<crate::legacy_reliability::status::LegacyReliabilityStatus>,
    pub hosts: VersionedSection<crate::hosts::HostsStatus>,
}

/// Единый быстрый snapshot для listener-first hydration.
///
/// Revision читается до значения каждой секции. Если параллельное событие
/// обновит значение между этими чтениями, оно получит большую revision и
/// frontend не позволит более старому snapshot перетереть событие.
#[tauri::command]
pub fn bootstrap_get_snapshot(app: AppHandle) -> BootstrapSnapshot {
    if let Err(error) = crate::protected_runtime::reconcile_legacy_recovery_selection(&app) {
        crate::util::emit_log(
            &app,
            "warn",
            "dpi",
            &format!("Не удалось сохранить подтверждённую Legacy-конфигурацию: {error}"),
        );
    }
    let state = app.state::<AppState>();

    let (settings_revision, settings_value) = {
        let guard = state.settings.lock_recover();
        (state.settings_revision.current(), guard.clone())
    };
    let _ = crate::protected_runtime::sync_legacy_recovery_controls(&settings_value);
    let protected_runtime = crate::protected_runtime::capability_snapshot();
    let provider = Provider::parse(&settings_value.ai_provider);
    let settings = VersionedSection::new(
        settings_revision,
        BootstrapSettings {
            settings: settings_value,
            elevated: crate::admin::is_elevated(),
            autostart: get_autostart(),
            protected_dpi_available: protected_runtime.dpi,
            protected_legacy_reliability_available: protected_runtime.legacy_reliability,
            protected_runtime,
            protected_runtime_available: crate::security::protected_runtime_available(),
        },
    );

    let dpi = {
        let guard = state.dpi.lock_recover();
        let revision = guard.revision;
        drop(guard);
        VersionedSection::new(revision, crate::protected_runtime::dpi_status())
    };

    let proxy = {
        let guard = state.proxy.lock_recover();
        let revision = guard.revision;
        VersionedSection::new(
            revision,
            crate::util::ProxyStatusPayload {
                running: guard.pid.is_some(),
                link: guard.link.clone(),
                lan_link: guard.lan_link.clone(),
                lan_published: guard.lan_published,
                lan_expiry_unix: guard.lan_expiry_unix,
            },
        )
    };

    let brain = {
        let revision = state.brain_revision.current();
        let value = state
            .brain
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(|handle| handle.status.borrow().clone()));
        VersionedSection::new(revision, value)
    };

    let legacy_reliability = {
        // The protected service owns its own observer revision. Bootstrap
        // projection advances the app-local section clock on every fresh IPC
        // read so a service restart cannot make a smaller remote revision look
        // stale to the already-running frontend.
        let revision = state.legacy_reliability_revision.bump();
        VersionedSection::new(
            revision,
            crate::protected_runtime::legacy_reliability_status(&app),
        )
    };

    let hosts_revision = state.hosts_revision.current();
    let hosts = VersionedSection::new(
        hosts_revision,
        crate::hosts::snapshot_status(&app, provider),
    );

    BootstrapSnapshot {
        schema_version: BOOTSTRAP_SCHEMA_VERSION,
        settings,
        dpi,
        proxy,
        brain,
        legacy_reliability,
        hosts,
    }
}

// ─── Settings ───────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Settings {
    let state = app.state::<AppState>();
    let s = state.settings.lock_recover().clone();
    s
}

fn patch_requires_legacy_reliability_reconcile(patch: &SettingsPatch) -> bool {
    // Mode/pause/freeze changes are consumed by the recovery overlay. Only the
    // master capability switch owns the Eyes/Manager lifecycle. In particular,
    // DPI category persistence before Zapret2 start must not enqueue a Legacy
    // observer teardown.
    patch.legacy_reliability_enabled.is_some()
}

fn patch_requires_legacy_controls_sync(patch: &SettingsPatch) -> bool {
    patch.legacy_reliability_enabled.is_some()
        || patch.legacy_reliability_mode.is_some()
        || patch.legacy_automatic_paused.is_some()
        || patch.legacy_reliability_frozen_categories.is_some()
}

pub(crate) fn mutate_settings<F>(app: &AppHandle, mutate: F) -> Result<Settings, String>
where
    F: FnOnce(&mut Settings),
{
    mutate_settings_checked(app, |_| Ok(()), mutate)
}

pub(crate) fn mutate_settings_checked<C, F>(app: &AppHandle, check: C, mutate: F) -> Result<Settings, String>
where
    C: FnOnce(&Settings) -> Result<(), String>,
    F: FnOnce(&mut Settings),
{
    let state = app.state::<AppState>();
    let base = state.paths.base_dir.clone();
    // All writers read their base snapshot AFTER taking the serialization gate.
    // A picker compares and updates its choices in this same transaction.
    let _save = state.settings_save_gate.blocking_lock();
    let (next, legacy_automation_changed) = {
        let guard = state
            .settings
            .lock()
            .map_err(|_| "settings lock poisoned".to_string())?;
        check(&guard)?;
        let mut next = guard.clone();
        mutate(&mut next);
        let changed = guard.legacy_reliability_enabled != next.legacy_reliability_enabled
            || guard.legacy_reliability_mode != next.legacy_reliability_mode
            || guard.legacy_automatic_paused != next.legacy_automatic_paused
            || guard.legacy_reliability_frozen_categories
                != next.legacy_reliability_frozen_categories;
        (next, changed)
    };
    // Запись на диск — под отдельным save-gate и ВНЕ std::Mutex settings,
    // чтобы fsync/rename не блокировали читателей (dpi_start, хоткей, close).
    next.save(&base).map_err(|e| e.to_string())?;
    {
        let mut guard = state
            .settings
            .lock()
            .map_err(|_| "settings lock poisoned".to_string())?;
        *guard = next.clone();
    }
    state.settings_revision.bump();
    if legacy_automation_changed {
        state.legacy_automation_revision.bump();
    }
    Ok(next)
}

pub(crate) fn persist_recovered_legacy_config(
    app: &AppHandle,
    category: &str,
    candidate_config: &str,
) -> Result<bool, String> {
    let state = app.state::<AppState>();
    let available = state.paths.get_configs_for_category(category);
    if !available.iter().any(|config| config == candidate_config) {
        return Err(format!(
            "подтверждённая Legacy-конфигурация {candidate_config} недоступна для {category}"
        ));
    }
    if state
        .settings
        .lock_recover()
        .selected_configs
        .get(category)
        .is_some_and(|selected| selected == candidate_config)
    {
        return Ok(false);
    }

    let category = category.to_owned();
    let candidate_config = candidate_config.to_owned();
    mutate_settings(app, move |settings| {
        settings.selected_configs.insert(category, candidate_config);
    })?;
    Ok(true)
}

#[tauri::command]
pub fn update_settings(app: AppHandle, patch: SettingsPatch) -> Result<Settings, String> {
    let reconcile_legacy_reliability = patch_requires_legacy_reliability_reconcile(&patch);
    let sync_legacy_controls = patch_requires_legacy_controls_sync(&patch);
    let previous =
        sync_legacy_controls.then(|| app.state::<AppState>().settings.lock_recover().clone());
    let settings = mutate_settings(&app, |settings| settings.apply_patch(patch))?;
    if sync_legacy_controls {
        if let Err(error) = crate::protected_runtime::sync_legacy_recovery_controls(&settings) {
            if let Some(previous) = previous {
                let _ = crate::protected_runtime::sync_legacy_recovery_controls(&previous);
                mutate_settings(&app, |settings| *settings = previous)?;
            }
            crate::legacy_reliability::status::refresh_recovery_overlay(&app);
            return Err(error);
        }
    }
    crate::legacy_reliability::status::refresh_recovery_overlay(&app);
    #[cfg(windows)]
    if reconcile_legacy_reliability {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            crate::dpi::reconcile_legacy_reliability(&app).await;
        });
    }
    Ok(settings)
}

/// Меняет глобальный хоткей вкл/выкл защиты: снимает прежнюю комбинацию, ставит
/// новую (пустая строка = выключить) и персистит в настройки. Формат — Tauri-
/// акселератор с Code-именем клавиши (`Ctrl+Shift+KeyO`). Возвращает ошибку, если
/// сочетание невалидно или занято другим приложением (прежнее при этом возвращаем).
#[tauri::command]
pub fn set_hotkey(app: AppHandle, hotkey: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let prev = state.settings.lock_recover().hotkey_toggle.clone();

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

    if let Err(e) = mutate_settings(&app, |settings| settings.hotkey_toggle = hotkey.clone()) {
        #[cfg(desktop)]
        {
            use tauri_plugin_global_shortcut::GlobalShortcutExt;
            let gs = app.global_shortcut();
            if !hotkey.trim().is_empty() {
                let _ = gs.unregister(hotkey.trim());
            }
            if !prev.trim().is_empty() {
                let _ = gs.register(prev.trim());
            }
        }
        return Err(e);
    }
    Ok(())
}

// ─── Мозг (авто-восстановление, L3) ───────────────────────────────────────

/// Шлёт событие Мозгу, если он запущен (иначе тихо игнорирует).
async fn send_brain_event(app: &AppHandle, ev: crate::brain::BrainEvent) {
    let input = {
        let st = app.state::<AppState>();
        let guard = st.brain.lock().ok();
        guard.and_then(|g| g.as_ref().map(|bh| bh.input.clone()))
    };
    if let Some(input) = input {
        let _ = input.send_control(ev).await;
    }
}

/// Включает/выключает авто-восстановление: спавнит или гасит задачу Мозга и
/// персистит флаг в настройках. Идемпотентно.
#[tauri::command]
pub fn brain_set_enabled(app: AppHandle, enabled: bool) -> Result<(), String> {
    let previous = app
        .state::<AppState>()
        .settings
        .lock()
        .map_err(|_| "settings lock poisoned".to_string())?
        .auto_recovery;
    mutate_settings(&app, |settings| settings.auto_recovery = enabled)?;

    let runtime_result = (|| {
        let st = app.state::<AppState>();
        let mut guard = st.brain.lock().map_err(|_| "brain lock".to_string())?;
        let running = guard.is_some();
        if enabled && !running {
            *guard = Some(crate::brain::runtime::start(app.clone()));
            st.brain_revision.bump();
        } else if !enabled && running {
            if let Some(bh) = guard.take() {
                bh.shutdown();
                st.brain_revision.bump();
            }
        }
        Ok::<(), String>(())
    })();
    if let Err(e) = runtime_result {
        let _ = mutate_settings(&app, |settings| settings.auto_recovery = previous);
        return Err(e);
    }
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

/// Accepts only the opaque proposal/attempt token. All category, config,
/// process, registry and network fences are reconstructed by the backend.
#[tauri::command]
pub async fn legacy_reliability_approve(
    app: AppHandle,
    approval: crate::legacy_reliability::recovery::AssistedApproval,
) -> Result<(), String> {
    if crate::protected_runtime::legacy_reliability_available() {
        return crate::protected_runtime::approve_legacy_recovery(&app, approval).await;
    }
    let initial = crate::legacy_reliability::status::approve_pending(&app, approval)?;
    let terminal =
        match crate::legacy_reliability::executor::run_scoped_recovery(&app, initial).await {
            Ok(terminal) => terminal,
            Err(error) => {
                crate::util::emit_log(
                    &app,
                    "error",
                    "legacy-reliability",
                    &format!("Assisted recovery failed: {error}"),
                );
                crate::legacy_reliability::status::refresh_recovery_overlay(&app);
                return Err(error.to_string());
            }
        };
    crate::legacy_reliability::status::refresh_recovery_overlay(&app);
    match terminal {
        crate::legacy_reliability::recovery::RecoveryAction::Complete { .. } => Ok(()),
        crate::legacy_reliability::recovery::RecoveryAction::ManualIntervention { .. } => {
            // The command itself completed deterministically; the typed public
            // status tells the UI that a manual restart is required.
            Ok(())
        }
        _ => Err("Legacy recovery завершился без terminal результата".into()),
    }
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

#[cfg(test)]
mod external_url_tests {
    use super::validate_external_url;

    #[test]
    fn accepts_the_links_the_app_actually_produces() {
        // Единственный реальный вызывающий — proxyStore.open() со ссылкой,
        // которую печатает Obsession Telegram Proxy (см. proxy.rs).
        validate_external_url("tg://proxy?server=192.168.1.10&port=1443&secret=dd00ff").unwrap();
        validate_external_url("https://t.me/proxy?server=1.2.3.4&port=443").unwrap();
        validate_external_url("http://localhost:1420").unwrap();
    }

    #[test]
    fn rejects_local_executables_and_unc_paths() {
        // Ровно то, что делало команду примитивом эскалации: Start-Process
        // -FilePath принимает не только URL, а процесс элевирован.
        for bad in [
            r"\\attacker\share\payload.exe",
            r"C:\Users\Public\dropped.exe",
            r"C:/Users/Public/dropped.exe",
            "payload.exe",
            r"..\..\Windows\System32\cmd.exe",
        ] {
            assert!(
                validate_external_url(bad).is_err(),
                "должно быть отклонено: {bad}"
            );
        }
    }

    #[test]
    fn rejects_schemes_outside_the_allowlist() {
        for bad in [
            "file:///C:/Windows/System32/cmd.exe",
            "javascript:alert(1)",
            "data:text/html,<script>1</script>",
            "ms-settings:",
            "search-ms:query=x",
            "vbscript:msgbox",
            "TG:\\\\evil", // схема есть, но разделитель не "://"
            "",
            "   ",
        ] {
            assert!(
                validate_external_url(bad).is_err(),
                "должно быть отклонено: {bad}"
            );
        }
    }

    #[test]
    fn scheme_check_is_case_insensitive_but_still_scheme_only() {
        validate_external_url("TG://proxy?server=1.2.3.4&port=443&secret=dd").unwrap();
        validate_external_url("HTTPS://t.me/proxy").unwrap();
        // Схема из allowlist не должна легализовать backslash в остатке.
        assert!(validate_external_url(r"https://t.me\..\payload.exe").is_err());
    }

    #[test]
    fn rejects_control_characters_and_overlong_input() {
        assert!(validate_external_url("tg://proxy?x=\n1").is_err());
        assert!(validate_external_url("tg://proxy?x=\r\n1").is_err());
        assert!(validate_external_url("tg://proxy?x=a\0b").is_err());
        let long = format!("tg://proxy?secret={}", "d".repeat(4096));
        assert!(validate_external_url(&long).is_err());
    }

    /// Доказывает, что валидатор ДЕЙСТВИТЕЛЬНО стоит на пути команды, а не
    /// просто существует рядом: без этого корректный валидатор мог бы остаться
    /// неподключённым и тесты выше всё равно были бы зелёными.
    /// Проверяем только ветку отказа — на валидной ссылке команда реально
    /// запустила бы Start-Process.
    #[test]
    fn command_itself_refuses_a_local_executable_before_spawning() {
        let error = super::open_external_url(r"C:\Users\Public\dropped.exe".to_string())
            .expect_err("команда обязана отклонить путь к исполняемому файлу");
        assert!(
            error.contains("обратный слэш") || error.contains("Разрешены только"),
            "ожидали отказ валидации, получили: {error}"
        );

        super::open_external_url("file:///C:/Windows/System32/cmd.exe".to_string())
            .expect_err("команда обязана отклонить схему вне allowlist");
    }
}

#[cfg(test)]
mod bootstrap_tests {
    use super::*;

    #[test]
    fn bootstrap_snapshot_serializes_schema_and_independent_revisions() {
        let snapshot = BootstrapSnapshot {
            schema_version: BOOTSTRAP_SCHEMA_VERSION,
            settings: VersionedSection::new(
                2,
                BootstrapSettings {
                    settings: Settings::default(),
                    elevated: true,
                    autostart: false,
                    protected_runtime: crate::protected_runtime::ProtectedRuntimeCapabilities {
                        service_available: true,
                        service_version: Some("1.2.3".into()),
                        dpi: true,
                        ..Default::default()
                    },
                    protected_runtime_available: false,
                    protected_dpi_available: true,
                    protected_legacy_reliability_available: false,
                },
            ),
            dpi: VersionedSection::new(
                3,
                crate::util::DpiStatusPayload {
                    active: false,
                    processes: Vec::new(),
                    started_at: None,
                },
            ),
            proxy: VersionedSection::new(
                4,
                crate::util::ProxyStatusPayload {
                    running: false,
                    link: String::new(),
                    lan_link: None,
                    lan_published: false,
                    lan_expiry_unix: None,
                },
            ),
            brain: VersionedSection::new(5, None),
            legacy_reliability: VersionedSection::new(
                7,
                crate::legacy_reliability::status::LegacyReliabilityStatus::inactive(),
            ),
            hosts: VersionedSection::new(
                8,
                crate::hosts::HostsStatus {
                    provider: "malw".to_string(),
                    status: "not_installed".to_string(),
                    local_version: String::new(),
                    remote_version: String::new(),
                    rollback_available: false,
                },
            ),
        };

        let value = serde_json::to_value(snapshot).expect("bootstrap snapshot serializes");
        assert_eq!(value["schemaVersion"], BOOTSTRAP_SCHEMA_VERSION);
        assert_eq!(value["settings"]["revision"], 2);
        assert_eq!(value["settings"]["value"]["elevated"], true);
        assert_eq!(
            value["settings"]["value"]["protectedRuntime"]["serviceAvailable"],
            true
        );
        assert_eq!(
            value["settings"]["value"]["protectedRuntime"]["serviceVersion"],
            "1.2.3"
        );
        assert_eq!(
            value["settings"]["value"]["protectedLegacyReliabilityAvailable"],
            false
        );
        assert_eq!(value["dpi"]["revision"], 3);
        assert_eq!(value["proxy"]["revision"], 4);
        assert_eq!(value["brain"]["revision"], 5);
        assert_eq!(value["legacyReliability"]["revision"], 7);
        assert_eq!(value["legacyReliability"]["value"]["phase"], "inactive");
        assert_eq!(value["hosts"]["revision"], 8);
        assert_eq!(value["hosts"]["value"]["provider"], "malw");
    }

    #[test]
    fn only_the_legacy_master_switch_reconciles_observer_lifecycle() {
        let mut patch = SettingsPatch {
            selected_categories: Some(vec!["discord".into()]),
            zapret2_selected_categories: Some(vec!["youtube_twitch".into()]),
            legacy_reliability_mode: Some("automatic".into()),
            ..SettingsPatch::default()
        };
        assert!(!patch_requires_legacy_reliability_reconcile(&patch));
        assert!(patch_requires_legacy_controls_sync(&patch));

        patch.legacy_reliability_enabled = Some(false);
        assert!(patch_requires_legacy_reliability_reconcile(&patch));
        assert!(patch_requires_legacy_controls_sync(&patch));

        let unrelated = SettingsPatch {
            selected_categories: Some(vec!["gaming".into()]),
            ..SettingsPatch::default()
        };
        assert!(!patch_requires_legacy_controls_sync(&unrelated));
    }
}
