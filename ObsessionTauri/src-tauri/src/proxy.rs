//! Управление Obsession Telegram Proxy.
//!
//! Это собственный локальный MTProto-прокси Obsession, написанный на Rust по
//! мотивам прежнего `proxy/tg_ws_proxy.py`. Он слушает MTProto ровно на `127.0.0.1:<--port>`
//! и туннелирует соединения через WebSocket к серверам Telegram. Секрет
//! передаётся ему через `--secret`, а готовую ссылку `tg://proxy?...` он сам
//! печатает в stderr — мы её оттуда вычитываем, не копируя секрет в app.log.
//!
//! Приложение:
//!   1. Запускает CLI с `--port/--secret[/--fake-tls-domain]` и ловит
//!      напечатанную им ссылку `tg://proxy?server=127.0.0.1&port=<port>&secret=dd...`.
//!   2. Для доступа с телефона поднимает TCP-форвардер `0.0.0.0:<port>` →
//!      `127.0.0.1:<port>` (слушает ВСЕ интерфейсы — телефон может прийти на
//!      любой адрес ПК) и отдаёт ту же ссылку с LAN IP вместо 127.0.0.1.
//!      LAN IP выбирается через `util::lan_ip_for_phone` (реальная физ. карта,
//!      не VPN/виртуальный адаптер).

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use rand::RngCore;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command as TokioCommand;
use tokio::sync::oneshot;
use tokio::task::JoinSet;

use crate::state::{AppState, ProxyFirewall, ProxyForwarder, ProxyState};
use crate::util::{self, LockExt, ProxyStatusPayload, VersionedSection};

const TGPROXY_RELATIVE: &str = "bin/obsession-tg-proxy.exe";
const TGPROXY_MANIFEST_RELATIVE: &str = "runtime/runtime-manifest.json";
const MAX_RUNTIME_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_TGPROXY_BYTES: u64 = 256 * 1024 * 1024;
const PROXY_LAN_FIREWALL_LEASE_SECONDS: u16 = 90;
const PROXY_LAN_FIREWALL_RENEW_INTERVAL: Duration = Duration::from_secs(30);
static TG_PROXY_LINK_RE: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r"tg://proxy\?[^\s]+").unwrap());

fn redact_proxy_log_line(message: &str) -> std::borrow::Cow<'_, str> {
    TG_PROXY_LINK_RE.replace_all(message, "tg://proxy?[redacted]")
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeResourceManifest {
    schema_version: u32,
    engines: Vec<RuntimeResourceEngine>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeResourceEngine {
    #[serde(rename = "engine")]
    _engine: String,
    #[serde(rename = "executable")]
    _executable: String,
    files: Vec<RuntimeResourceFile>,
    #[serde(rename = "strategies")]
    _strategies: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeResourceFile {
    path: String,
    size: u64,
    sha256: String,
}

fn emit_status(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut p = state.proxy.lock_recover();
    let revision = p.bump_revision();
    let _ = app.emit(
        "proxy-status",
        VersionedSection::new(
            revision,
            ProxyStatusPayload {
                running: p.pid.is_some(),
                link: p.link.clone(),
                lan_link: p.lan_link.clone(),
                lan_published: p.lan_published,
                lan_expiry_unix: p.lan_expiry_unix,
            },
        ),
    );
}

/// True, если защищённый бинарник Telegram-прокси найден.
pub fn available(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    verified_tgproxy_path(state.paths.resource_dir()).is_some()
}

fn verified_tgproxy_path(resource_root: &Path) -> Option<PathBuf> {
    let canonical_root = fs::canonicalize(resource_root).ok()?;
    let manifest_path = canonical_root.join(TGPROXY_MANIFEST_RELATIVE);
    let manifest_metadata = fs::symlink_metadata(&manifest_path).ok()?;
    if !manifest_metadata.is_file()
        || manifest_metadata.file_type().is_symlink()
        || manifest_metadata.len() == 0
        || manifest_metadata.len() > MAX_RUNTIME_MANIFEST_BYTES
    {
        return None;
    }
    let mut bytes = Vec::with_capacity(manifest_metadata.len() as usize);
    File::open(&manifest_path)
        .ok()?
        .take(MAX_RUNTIME_MANIFEST_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_RUNTIME_MANIFEST_BYTES {
        return None;
    }
    let manifest: RuntimeResourceManifest = serde_json::from_slice(&bytes).ok()?;
    if manifest.schema_version != 1 || manifest.engines.is_empty() || manifest.engines.len() > 2 {
        return None;
    }
    let mut records = manifest
        .engines
        .iter()
        .flat_map(|engine| engine.files.iter())
        .filter(|file| file.path.eq_ignore_ascii_case(TGPROXY_RELATIVE));
    let record = records.next()?;
    if records.next().is_some()
        || record.path != TGPROXY_RELATIVE
        || record.size == 0
        || record.size > MAX_TGPROXY_BYTES
        || record.sha256.len() != 64
        || !record.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }

    let candidate = canonical_root.join(TGPROXY_RELATIVE);
    let metadata = fs::symlink_metadata(&candidate).ok()?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() != record.size {
        return None;
    }
    let canonical_bin = fs::canonicalize(canonical_root.join("bin")).ok()?;
    let canonical_candidate = fs::canonicalize(&candidate).ok()?;
    if canonical_candidate.parent() != Some(canonical_bin.as_path())
        || canonical_candidate
            .file_name()
            .and_then(|name| name.to_str())
            != Some(crate::paths::TGPROXY_EXE)
        || !sha256_file(&canonical_candidate)
            .ok()?
            .eq_ignore_ascii_case(&record.sha256)
    {
        return None;
    }
    Some(canonical_candidate)
}

fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn runtime_shutting_down(app: &AppHandle) -> bool {
    app.state::<AppState>()
        .shutting_down
        .load(std::sync::atomic::Ordering::SeqCst)
}

const FORWARD_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const FORWARD_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_FORWARD_CONNECTIONS: usize = 512;

struct DetachedProxy {
    pid: Option<u32>,
    forwarder: Option<ProxyForwarder>,
    firewall: Option<ProxyFirewall>,
}

impl DetachedProxy {
    fn had_runtime(&self) -> bool {
        self.pid.is_some() || self.forwarder.is_some() || self.firewall.is_some()
    }
}

/// Инвалидирует callbacks предыдущей сессии и забирает все её ресурсы.
fn begin_generation_state(p: &mut ProxyState) -> (u64, DetachedProxy) {
    p.generation = p.generation.wrapping_add(1);
    if p.generation == 0 {
        p.generation = 1;
    }
    let generation = p.generation;
    // Отменяем спящий таймер предыдущей сессии — он неактуален.
    if let Some(abort) = p.lan_expiry_abort.take() {
        abort.abort();
    }
    let detached = DetachedProxy {
        pid: p.pid.take(),
        forwarder: p.forwarder.take(),
        firewall: p.firewall.take(),
    };
    p.link.clear();
    p.lan_link = None;
    p.lan_published = false;
    p.lan_expiry_unix = None;
    (generation, detached)
}

fn is_current_state(p: &ProxyState, generation: u64, pid: u32) -> bool {
    p.generation == generation && p.pid == Some(pid)
}

/// Забирает ресурсы только если завершившийся monitor всё ещё владеет сессией.
fn detach_if_current_state(p: &mut ProxyState, generation: u64, pid: u32) -> Option<DetachedProxy> {
    if p.generation != generation || p.pid != Some(pid) {
        return None;
    }
    p.pid = None;
    p.link.clear();
    p.lan_link = None;
    p.lan_published = false;
    p.lan_expiry_unix = None;
    if let Some(forwarder) = &p.forwarder {
        debug_assert_eq!(forwarder.generation, generation);
    }
    if let Some(firewall) = &p.firewall {
        debug_assert_eq!(firewall.generation, generation);
    }
    Some(DetachedProxy {
        pid: None,
        forwarder: p.forwarder.take(),
        firewall: p.firewall.take(),
    })
}

fn begin_generation(app: &AppHandle) -> (u64, DetachedProxy) {
    let state = app.state::<AppState>();
    let mut p = state.proxy.lock_recover();
    begin_generation_state(&mut p)
}

fn is_current(app: &AppHandle, generation: u64, pid: u32) -> bool {
    let state = app.state::<AppState>();
    let p = state.proxy.lock_recover();
    is_current_state(&p, generation, pid)
}

fn detach_if_current(app: &AppHandle, generation: u64, pid: u32) -> Option<DetachedProxy> {
    let state = app.state::<AppState>();
    let mut p = state.proxy.lock_recover();
    detach_if_current_state(&mut p, generation, pid)
}

async fn cleanup_nonprocess_runtime(detached: DetachedProxy) {
    if let Some(forwarder) = detached.forwarder {
        forwarder.handle.abort();
    }
    if let Some(firewall) = detached.firewall {
        firewall.renewal_abort.abort();
        let _ = tauri::async_runtime::spawn_blocking(
            crate::protected_runtime::proxy_lan_firewall_close_blocking,
        )
        .await;
    }
}

/// Текущее Unix-время в секундах.
fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LanCloseOrigin {
    External,
    ExpiryTask,
    RenewalTask,
}

/// Снимает ТОЛЬКО LAN-публикацию (0.0.0.0 forwarder + firewall) текущего
/// поколения, СОХРАНЯЯ процесс прокси и локальный `127.0.0.1` listener — чтобы
/// Telegram Desktop продолжал работать после закрытия доступа с телефона.
/// Вызывается под `proxy_gate`: fixed firewall rule нельзя закрывать параллельно
/// с публикацией следующего поколения.
async fn close_lan_publication_locked(
    app: &AppHandle,
    expected: Option<(u64, u32)>,
    reason: &str,
    origin: LanCloseOrigin,
) -> bool {
    let taken = {
        let state = app.state::<AppState>();
        let mut p = state.proxy.lock_recover();
        if !p.lan_published
            || expected.is_some_and(|(generation, pid)| !is_current_state(&p, generation, pid))
        {
            return false; // публикации нет или задача относится к старому поколению
        }
        p.lan_published = false;
        p.lan_expiry_unix = None;
        p.lan_link = None;
        let expiry_abort = p.lan_expiry_abort.take();
        (p.forwarder.take(), p.firewall.take(), expiry_abort)
    };
    let (forwarder, firewall, expiry_abort) = taken;
    if origin != LanCloseOrigin::ExpiryTask {
        if let Some(abort) = expiry_abort {
            abort.abort();
        }
    }
    if let Some(f) = forwarder {
        f.handle.abort();
    }
    if let Some(fw) = firewall {
        if origin != LanCloseOrigin::RenewalTask {
            fw.renewal_abort.abort();
        }
        if let Err(error) = crate::protected_runtime::proxy_lan_firewall_close().await {
            util::emit_log(
                app,
                "warn",
                "proxy",
                &format!("Защищённая служба не подтвердила закрытие firewall lease: {error}"),
            );
        }
    }
    util::emit_log(
        app,
        "info",
        "proxy",
        &format!(
            "LAN-публикация закрыта ({reason}). Локальный прокси для Telegram Desktop работает."
        ),
    );
    emit_status(app);
    true
}

pub async fn close_lan_publication(app: &AppHandle, reason: &str) {
    let state = app.state::<AppState>();
    let _gate = state.proxy_gate.lock().await;
    let _ = close_lan_publication_locked(app, None, reason, LanCloseOrigin::External).await;
}

/// Спавнит таймер авто-закрытия LAN-публикации. Проверяет generation+pid перед
/// закрытием, поэтому истёкший таймер старой сессии не тронет новую публикацию.
fn spawn_lan_expiry_timer(app: &AppHandle, generation: u64, pid: u32, secs: u16) {
    if secs == 0 {
        return; // 0 = без авто-закрытия
    }
    let app2 = app.clone();
    let handle = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(secs as u64)).await;
        let state = app2.state::<AppState>();
        let _gate = state.proxy_gate.lock().await;
        let _ = close_lan_publication_locked(
            &app2,
            Some((generation, pid)),
            "истёк таймаут",
            LanCloseOrigin::ExpiryTask,
        )
        .await;
    });
    // AbortHandle позволяет отменить таймер при stop/shutdown.
    app.state::<AppState>()
        .proxy
        .lock_recover()
        .lan_expiry_abort = Some(handle.abort_handle());
}

fn spawn_lan_lease_renewal(app: &AppHandle, generation: u64, pid: u32, port: u16) -> ProxyFirewall {
    let app2 = app.clone();
    let handle = tokio::spawn(async move {
        loop {
            tokio::time::sleep(PROXY_LAN_FIREWALL_RENEW_INTERVAL).await;
            let state = app2.state::<AppState>();
            let _gate = state.proxy_gate.lock().await;
            let still_published = {
                let proxy = state.proxy.lock_recover();
                is_current_state(&proxy, generation, pid) && proxy.lan_published
            };
            if !still_published {
                return;
            }
            if let Err(error) = crate::protected_runtime::proxy_lan_firewall_open(
                port,
                PROXY_LAN_FIREWALL_LEASE_SECONDS,
            )
            .await
            {
                util::emit_log(
                    &app2,
                    "warn",
                    "proxy",
                    &format!("Firewall lease не продлён; LAN-публикация будет закрыта: {error}"),
                );
                let _ = close_lan_publication_locked(
                    &app2,
                    Some((generation, pid)),
                    "не удалось продлить firewall lease",
                    LanCloseOrigin::RenewalTask,
                )
                .await;
                return;
            }
        }
    });
    ProxyFirewall {
        generation,
        renewal_abort: handle.abort_handle(),
    }
}

/// Async-обёртка [`cleanup_previous_runtime`]: блокирующие taskkill/IPC уходят
/// в blocking-пул, чтобы не занимать tokio-воркер под `proxy_gate` / в teardown.
async fn cleanup_previous_runtime_async(
    app: &AppHandle,
    detached: DetachedProxy,
    image: Option<String>,
) -> bool {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        cleanup_previous_runtime(&app2, detached, image.as_deref())
    })
    .await
    .unwrap_or(false)
}

/// Очищает предыдущую proxy-сессию. Вызывается только под `proxy_gate`.
fn cleanup_previous_runtime(app: &AppHandle, detached: DetachedProxy, image: Option<&str>) -> bool {
    let mut killed = detached.had_runtime();
    if let Some(forwarder) = detached.forwarder {
        forwarder.handle.abort();
    }
    if let Some(firewall) = detached.firewall {
        firewall.renewal_abort.abort();
        let _ = crate::protected_runtime::proxy_lan_firewall_close_blocking();
    }
    if let Some(pid) = detached.pid {
        let _ = util::std_command("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .output();
    }
    // Sweep выполняется до нового spawn и под gate, поэтому не может задеть
    // новый PID. Уникальное имя образа ограничивает sweep процессами Obsession,
    // не затрагивая апстримный tg-ws-proxy или другие Telegram-прокси.
    if let Some(image) = image {
        let swept = util::std_command("taskkill")
            .args(["/F", "/T", "/IM", image])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if swept {
            killed = true;
            util::emit_log(
                app,
                "warn",
                "proxy",
                "[tg] Добиты осиротевшие процессы прокси.",
            );
        }
    }
    killed
}

/// Запускает прокси. Возвращает `tg://proxy?...` ссылку.
pub async fn start(app: &AppHandle, port: u16, fake_tls_domain: &str) -> Result<String, String> {
    if runtime_shutting_down(app) {
        return Err("Приложение завершает работу.".to_string());
    }
    let state = app.state::<AppState>();
    let _gate = state.proxy_gate.lock().await;
    if runtime_shutting_down(app) {
        return Err("Приложение завершает работу.".to_string());
    }
    start_locked(app, port, fake_tls_domain).await
}

/// Атомарный toggle для трея: проверка состояния и операция выполняются под
/// одним gate, поэтому два быстрых клика не превращаются в двойной restart.
pub async fn toggle(app: &AppHandle, port: u16, fake_tls_domain: &str) -> Result<bool, String> {
    if runtime_shutting_down(app) {
        return Err("Приложение завершает работу.".to_string());
    }
    let state = app.state::<AppState>();
    let _gate = state.proxy_gate.lock().await;
    if runtime_shutting_down(app) {
        return Err("Приложение завершает работу.".to_string());
    }
    if state.proxy.lock_recover().pid.is_some() {
        stop_locked_async(app).await;
        Ok(false)
    } else {
        start_locked(app, port, fake_tls_domain).await.map(|_| true)
    }
}

/// Внутренняя реализация start; вызывается только под `proxy_gate`.
pub(crate) async fn start_locked(
    app: &AppHandle,
    port: u16,
    fake_tls_domain: &str,
) -> Result<String, String> {
    let (exe, bin_dir, cache_path, image) = {
        let state = app.state::<AppState>();
        match verified_tgproxy_path(state.paths.resource_dir()) {
            Some(e) => {
                let image = e.file_name().map(|n| n.to_string_lossy().to_string());
                (
                    e,
                    state.paths.bin_dir(),
                    state.paths.cfproxy_cache_path(),
                    image,
                )
            }
            None => {
                return Err(
                    "Telegram-прокси отсутствует или не прошёл проверку runtime-manifest"
                        .to_string(),
                )
            }
        }
    };

    // Новый generation инвалидирует monitor/link-reader старого запуска ДО
    // cleanup. Поэтому старый callback уже не может очистить новую сессию.
    let (generation, previous) = begin_generation(app);
    let had_previous = cleanup_previous_runtime_async(app, previous, image.clone()).await;
    emit_status(app);
    if had_previous {
        // Даём Windows освободить порт после taskkill дерева PyInstaller.
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    if runtime_shutting_down(app) {
        return Err("Запуск прокси отменён: приложение завершает работу.".to_string());
    }

    let lan = util::lan_ip_for_phone();
    let secret = gen_secret();
    let mut args: Vec<String> = vec![
        "--port".into(),
        port.to_string(),
        "--secret".into(),
        secret.clone(),
    ];
    if !fake_tls_domain.is_empty() {
        args.push("--fake-tls-domain".into());
        args.push(fake_tls_domain.to_string());
    }
    // Сначала прокси подключается к Telegram DC напрямую. Если провайдер режет
    // этот маршрут, разрешаем fallback через публичные CF relay; кэш под
    // %APPDATA%\Obsession хранит последние рабочие домены между запусками.
    args.push("--cfproxy".into());
    args.push("--cfproxy-cache".into());
    args.push(cache_path.to_string_lossy().to_string());

    util::emit_log(
        app,
        "info",
        "proxy",
        &format!(
            "Запуск Telegram-прокси (MTProto на 127.0.0.1:{port}, публичные relay — только резервный маршрут)..."
        ),
    );

    let mut std_cmd = util::std_command(&exe);
    std_cmd
        .args(&args)
        .current_dir(&bin_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut cmd = TokioCommand::from(std_cmd);
    cmd.kill_on_drop(false);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Не удалось запустить прокси: {e}"))?;
    let pid = child.id().ok_or("Прокси не вернул PID")?;

    // PID регистрируется сразу после spawn — shutdown всегда сможет увидеть его,
    // даже пока идёт ранняя проверка или ожидание ссылки.
    {
        // Проверяем generation и (если он ещё наш) регистрируем PID под локом в
        // отдельном скоупе — иначе guard/State жили бы через .await ниже, и future
        // команды перестаёт быть Send.
        let regen = {
            let state = app.state::<AppState>();
            let mut p = state.proxy.lock_recover();
            if p.generation != generation {
                true
            } else {
                p.pid = Some(pid);
                false
            }
        };
        if regen {
            // Более новая операция сменила generation — гасим свой PID вне лока.
            let _ = tauri::async_runtime::spawn_blocking(move || {
                let _ = util::std_command("taskkill")
                    .args(["/F", "/T", "/PID", &pid.to_string()])
                    .output();
            })
            .await;
            return Err("Запуск прокси отменён более новой операцией.".to_string());
        }
    }

    // Прокси печатает готовую ссылку `tg://proxy?...` в stderr, но некоторые
    // сборки используют stdout. Первое совпадение выигрывает.
    let (tx, rx) = oneshot::channel::<String>();
    let tx = std::sync::Arc::new(std::sync::Mutex::new(Some(tx)));
    if let Some(out) = child.stdout.take() {
        spawn_link_reader(app.clone(), out, tx.clone(), generation, pid);
    }
    if let Some(err) = child.stderr.take() {
        spawn_link_reader(app.clone(), err, tx.clone(), generation, pid);
    }

    // Ранний выход (<500мс) — запуск неуспешен. В отличие от прежней версии PID
    // уже tracked, поэтому параллельный shutdown не оставит orphan.
    let started = std::time::Instant::now();
    match tokio::time::timeout(Duration::from_millis(500), child.wait()).await {
        Ok(Ok(status)) => {
            let (_, detached) = begin_generation(app);
            cleanup_previous_runtime_async(app, detached, image.clone()).await;
            emit_status(app);
            return Err(format!(
                "Прокси завершился сразу после запуска (код {:?}).",
                status.code()
            ));
        }
        Ok(Err(e)) => {
            let (_, detached) = begin_generation(app);
            cleanup_previous_runtime_async(app, detached, image.clone()).await;
            emit_status(app);
            return Err(format!("Ошибка ожидания прокси: {e}"));
        }
        Err(_) => { /* процесс всё ещё работает */ }
    }
    if runtime_shutting_down(app) {
        let (_, detached) = begin_generation(app);
        cleanup_previous_runtime_async(app, detached, image.clone()).await;
        emit_status(app);
        return Err("Запуск прокси отменён: приложение завершает работу.".to_string());
    }

    // Monitor имеет право на cleanup только пока generation+PID актуальны.
    let app_mon = app.clone();
    let (dead_tx, dead_rx) = oneshot::channel::<()>();
    let mut dead_rx = Some(dead_rx);
    tokio::spawn(async move {
        let status = child.wait().await;
        let code = status.ok().and_then(|s| s.code()).unwrap_or(-1);
        let lived = started.elapsed().as_millis();
        let _ = dead_tx.send(());
        let state = app_mon.state::<AppState>();
        let _gate = state.proxy_gate.lock().await;
        let Some(detached) = detach_if_current(&app_mon, generation, pid) else {
            return; // штатный stop или уже начался новый generation
        };
        cleanup_nonprocess_runtime(detached).await;
        util::emit_log(
            &app_mon,
            "warn",
            "proxy",
            &format!("[tg] Прокси неожиданно остановлен через {lived}мс (код {code})."),
        );
        emit_status(&app_mon);
    });

    // Ждём ссылку до 5с, но проверяем shutdown каждые 50мс: begin_exit не
    // должен ждать весь timeout, удерживая proxy_gate.
    let mut rx = rx;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let received_link = loop {
        if runtime_shutting_down(app) {
            let (_, detached) = begin_generation(app);
            cleanup_previous_runtime_async(app, detached, image.clone()).await;
            emit_status(app);
            return Err("Запуск прокси отменён: приложение завершает работу.".to_string());
        }
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break None;
        }
        let slice = remaining.min(Duration::from_millis(50));
        match tokio::time::timeout(slice, &mut rx).await {
            Ok(Ok(link)) => break Some(link),
            Ok(Err(_)) => break None,
            Err(_) => {}
        }
    };
    let link = match received_link {
        Some(l) => l,
        None => {
            // Если ссылка не получена за 5 с, проверяем состояние процесса через
            // monitor-канал: child уже передан задаче мониторинга. Различаем
            // таймаут живого процесса и завершение при запуске.
            let alive = matches!(
                dead_rx.as_mut().unwrap().try_recv(),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty)
            );
            let (_, detached) = begin_generation(app);
            cleanup_previous_runtime_async(app, detached, image.clone()).await;
            emit_status(app);
            return Err(if alive {
                format!(
                    "Прокси запущен, но не выдал ссылку за 5с (порт {port}?). \
                     Попробуйте другой порт или повторите."
                )
            } else {
                "Прокси завершился, не выдав ссылку.".to_string()
            });
        }
    };

    if !is_current(app, generation, pid) {
        return Err("Прокси завершился сразу после запуска.".to_string());
    }

    // LAN-публикация fail-closed: сначала служба подтверждает bounded firewall
    // lease, и только затем приложение начинает слушать 0.0.0.0.
    let lan_link = if lan.ip != "127.0.0.1" {
        match crate::protected_runtime::proxy_lan_firewall_open(
            port,
            PROXY_LAN_FIREWALL_LEASE_SECONDS,
        )
        .await
        {
            Ok(()) => {
                let bind = format!("0.0.0.0:{port}");
                let target = format!("127.0.0.1:{port}");
                match tokio::net::TcpListener::bind(&bind).await {
                    Ok(listener) => {
                        let route_note = match &lan.route_ip {
                            Some(route) if *route != lan.ip => format!(
                                " Выход в интернет через {route} — это VPN/виртуальный адаптер; для QR выбран физический LAN-адрес."
                            ),
                            _ => String::new(),
                        };
                        util::emit_log(
                            app,
                            "info",
                            "proxy",
                            &format!(
                                "Телефон: адрес для QR {}:{port}; firewall lease ограничен Private + LocalSubnet (выбранная подсеть: {}). Найденные LAN-адреса: [{}].{route_note}",
                                lan.ip,
                                lan.subnet.as_deref().unwrap_or("не определена"),
                                lan.candidates.join(", ")
                            ),
                        );
                        let handle = tokio::spawn(run_forwarder(listener, target));
                        let mut forwarder = Some(ProxyForwarder { generation, handle });
                        let mut firewall =
                            Some(spawn_lan_lease_renewal(app, generation, pid, port));
                        let lan_secs = {
                            let state = app.state::<AppState>();
                            let seconds = state.settings.lock_recover().lan_publish_secs;
                            seconds
                        };
                        let expiry = (lan_secs != 0).then(|| now_unix() + u64::from(lan_secs));
                        let installed = {
                            let state = app.state::<AppState>();
                            let mut proxy = state.proxy.lock_recover();
                            if is_current_state(&proxy, generation, pid) {
                                proxy.forwarder = forwarder.take();
                                proxy.firewall = firewall.take();
                                proxy.lan_published = true;
                                proxy.lan_expiry_unix = expiry;
                                true
                            } else {
                                false
                            }
                        };
                        if !installed {
                            cleanup_nonprocess_runtime(DetachedProxy {
                                pid: None,
                                forwarder,
                                firewall,
                            })
                            .await;
                            return Err(
                                "Прокси завершился во время запуска LAN-форвардера.".to_string()
                            );
                        }
                        spawn_lan_expiry_timer(app, generation, pid, lan_secs);
                        Some(link.replace("127.0.0.1", &lan.ip))
                    }
                    Err(error) => {
                        let _ = crate::protected_runtime::proxy_lan_firewall_close().await;
                        util::emit_log(
                            app,
                            "warn",
                            "proxy",
                            &format!("LAN-форвардер не запущен ({bind}): {error}"),
                        );
                        None
                    }
                }
            }
            Err(error) => {
                util::emit_log(
                    app,
                    "warn",
                    "proxy",
                    &format!(
                        "Защищённая LAN-публикация недоступна; локальный прокси продолжает работать: {error}"
                    ),
                );
                None
            }
        }
    } else {
        util::emit_log(
            app,
            "warn",
            "proxy",
            "LAN-адрес для телефона не найден — QR будет работать только на этом ПК.",
        );
        None
    };

    let applied = {
        let state = app.state::<AppState>();
        let mut p = state.proxy.lock_recover();
        if p.generation == generation && p.pid == Some(pid) {
            p.link = link.clone();
            p.lan_link = lan_link.clone();
            true
        } else {
            false
        }
    };
    if !applied {
        return Err("Прокси завершился до окончания запуска.".to_string());
    }

    util::emit_log(
        app,
        "success",
        "proxy",
        &format!("Telegram-прокси запущен на 127.0.0.1:{port}; ссылка сохранена без записи секрета в лог."),
    );
    emit_status(app);
    Ok(link)
}

/// Останавливает proxy и TCP-forwarder. Все вызовы сериализуются gate-ом.
pub async fn stop(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    let _gate = state.proxy_gate.lock().await;
    stop_locked_async(app).await
}

pub(crate) fn stop_locked(app: &AppHandle) -> bool {
    let (_, detached) = begin_generation(app);
    let killed = cleanup_previous_runtime(app, detached, Some(crate::paths::TGPROXY_EXE));
    emit_status(app);
    killed
}

/// Async-обёртка [`stop_locked`]: блокирующие taskkill/IPC уходят в blocking-
/// пул, чтобы не занимать tokio-воркер под `proxy_gate` / в teardown.
pub(crate) async fn stop_locked_async(app: &AppHandle) -> bool {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || stop_locked(&app2))
        .await
        .unwrap_or(false)
}

pub fn current_link(app: &AppHandle) -> String {
    let state = app.state::<AppState>();
    let link = state.proxy.lock_recover().link.clone();
    link
}

/// Генерирует tg://proxy ссылку вручную. Использовалась как fallback, когда
/// прокси не выдавал ссылку сам — сейчас не вызывается (таймаут ссылки = ошибка
/// запуска, а не молчаливая подмена). Сохранена для возможного будущего
/// verified-fallback с явной пометкой «unverified».
#[allow(dead_code)]
fn generate_manual(host: &str, port: u16, secret: &str, fake_tls_domain: &str) -> String {
    if !fake_tls_domain.is_empty() {
        let domain_hex: String = fake_tls_domain
            .as_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        format!("tg://proxy?server={host}&port={port}&secret=ee{secret}{domain_hex}")
    } else {
        format!("tg://proxy?server={host}&port={port}&secret=dd{secret}")
    }
}

/// Читает поток построчно и принимает ссылку только от актуального generation.
fn spawn_link_reader<R>(
    app: AppHandle,
    stream: R,
    tx: std::sync::Arc<std::sync::Mutex<Option<oneshot::Sender<String>>>>,
    generation: u64,
    pid: u32,
) where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(stream).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if !is_current(&app, generation, pid) {
                break;
            }
            let msg = line.trim();
            if msg.is_empty() {
                continue;
            }
            let safe_msg = redact_proxy_log_line(msg);
            util::emit_log(&app, "info", "proxy", &format!("[tg] {safe_msg}"));
            if let Some(m) = TG_PROXY_LINK_RE.find(msg) {
                let link = m.as_str().to_string();
                let accepted = {
                    let state = app.state::<AppState>();
                    let mut p = state.proxy.lock_recover();
                    if p.generation == generation && p.pid == Some(pid) {
                        p.link = link.clone();
                        true
                    } else {
                        false
                    }
                };
                if accepted {
                    if let Some(tx) = tx.lock_recover().take() {
                        let _ = tx.send(link);
                    }
                }
            }
        }
    });
}

/// Listener владеет JoinSet всех соединений. Drop/abort listener-задачи
/// автоматически abort-ит children и закрывает их сокеты.
async fn run_forwarder(listener: tokio::net::TcpListener, target_addr: String) {
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                match accepted {
                    Ok((client, _)) => {
                        if connections.len() >= MAX_FORWARD_CONNECTIONS {
                            drop(client); // жёсткая граница памяти под flood
                            continue;
                        }
                        let _ = client.set_nodelay(true);
                        let target = target_addr.clone();
                        connections.spawn(async move {
                            let _ = forward_connection(client, target).await;
                        });
                    }
                    Err(_) => {
                        // Не допускаем busy loop на постоянной ошибке accept.
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                }
            }
            Some(_) = connections.join_next(), if !connections.is_empty() => {
                // Reap завершённой connection task; память JoinSet не растёт.
            }
        }
    }
}

async fn forward_connection(
    client: tokio::net::TcpStream,
    target_addr: String,
) -> std::io::Result<()> {
    let server = tokio::time::timeout(
        FORWARD_CONNECT_TIMEOUT,
        tokio::net::TcpStream::connect(&target_addr),
    )
    .await
    .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "proxy target timeout"))??;
    let _ = server.set_nodelay(true);

    let (mut cr, mut cw) = client.into_split();
    let (mut sr, mut sw) = server.into_split();
    // MTProto-сессия завершается целиком при EOF/error любого направления.
    // Второй copy future отменяется, затем обе write-half закрываются.
    let result = tokio::select! {
        result = tokio::io::copy(&mut cr, &mut sw) => result,
        result = tokio::io::copy(&mut sr, &mut cw) => result,
    };
    let _ = tokio::time::timeout(FORWARD_SHUTDOWN_TIMEOUT, async {
        let _ = cw.shutdown().await;
        let _ = sw.shutdown().await;
    })
    .await;
    result.map(|_| ())
}

fn gen_secret() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn proxy_links_are_redacted_without_losing_surrounding_diagnostics() {
        let secret = "dd00112233445566778899aabbccddeeff";
        let line = format!("ready tg://proxy?server=127.0.0.1&port=24445&secret={secret} ok");
        let safe = redact_proxy_log_line(&line);
        assert_eq!(safe, "ready tg://proxy?[redacted] ok");
        assert!(!safe.contains(secret));
        assert_eq!(
            TG_PROXY_LINK_RE.find(&line).unwrap().as_str(),
            &line[6..line.len() - 3]
        );
    }

    fn tgproxy_fixture(record_path: &str, duplicate: bool) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("obsession-tgproxy-test-{nonce}"));
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::create_dir_all(root.join("runtime")).unwrap();
        let proxy = root.join(TGPROXY_RELATIVE);
        fs::write(&proxy, b"verified telegram proxy").unwrap();
        let record = serde_json::json!({
            "path": record_path,
            "size": fs::metadata(&proxy).unwrap().len(),
            "sha256": sha256_file(&proxy).unwrap(),
        });
        let files = if duplicate {
            vec![record.clone(), record]
        } else {
            vec![record]
        };
        fs::write(
            root.join(TGPROXY_MANIFEST_RELATIVE),
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "engines": [{
                    "engine": "legacy",
                    "executable": "bin/winws.exe",
                    "files": files,
                    "strategies": []
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        root
    }

    #[test]
    fn tgproxy_requires_one_exact_hash_verified_manifest_record() {
        let root = tgproxy_fixture(TGPROXY_RELATIVE, false);
        assert_eq!(
            verified_tgproxy_path(&root),
            Some(fs::canonicalize(root.join(TGPROXY_RELATIVE)).unwrap())
        );

        fs::write(root.join(TGPROXY_RELATIVE), b"tampered telegram proxy").unwrap();
        assert!(verified_tgproxy_path(&root).is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn tgproxy_rejects_alias_and_duplicate_manifest_records() {
        let alias = tgproxy_fixture("bin/TgWsProxy.exe", false);
        assert!(verified_tgproxy_path(&alias).is_none());
        fs::remove_dir_all(alias).unwrap();

        let duplicate = tgproxy_fixture(TGPROXY_RELATIVE, true);
        assert!(verified_tgproxy_path(&duplicate).is_none());
        fs::remove_dir_all(duplicate).unwrap();
    }

    #[test]
    fn stale_generation_cannot_detach_new_runtime() {
        let mut state = ProxyState::default();

        let (old_generation, initial) = begin_generation_state(&mut state);
        assert!(!initial.had_runtime());
        state.pid = Some(111);
        state.link = "old".into();

        let (new_generation, old_runtime) = begin_generation_state(&mut state);
        assert_eq!(old_runtime.pid, Some(111));
        assert_ne!(new_generation, old_generation);

        state.pid = Some(222);
        state.link = "new".into();

        assert!(!is_current_state(&state, old_generation, 111));
        assert!(detach_if_current_state(&mut state, old_generation, 111).is_none());
        assert_eq!(state.generation, new_generation);
        assert_eq!(state.pid, Some(222));
        assert_eq!(state.link, "new");
    }

    #[test]
    fn proxy_generation_never_uses_zero_after_wrap() {
        let mut state = ProxyState {
            generation: u64::MAX,
            ..ProxyState::default()
        };

        let (generation, detached) = begin_generation_state(&mut state);

        assert_eq!(generation, 1);
        assert_eq!(state.generation, 1);
        assert!(!detached.had_runtime());
    }

    #[tokio::test]
    async fn half_close_finishes_both_forwarding_directions() {
        let ingress = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let ingress_addr = ingress.local_addr().unwrap();
        let target_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target_addr = target_listener.local_addr().unwrap().to_string();

        let forward = tokio::spawn(async move {
            let (accepted, _) = ingress.accept().await.unwrap();
            forward_connection(accepted, target_addr).await
        });
        let target = tokio::spawn(async move {
            let (mut peer, _) = target_listener.accept().await.unwrap();
            let mut byte = [0u8; 1];
            peer.read_exact(&mut byte).await.unwrap();
            assert_eq!(byte[0], 7);
            let n = tokio::time::timeout(Duration::from_secs(1), peer.read(&mut byte))
                .await
                .expect("target did not observe close")
                .unwrap();
            assert_eq!(n, 0);
        });

        let mut client = tokio::net::TcpStream::connect(ingress_addr).await.unwrap();
        client.write_all(&[7]).await.unwrap();
        client.shutdown().await.unwrap();
        let mut byte = [0u8; 1];
        let n = tokio::time::timeout(Duration::from_secs(1), client.read(&mut byte))
            .await
            .expect("client side stayed open")
            .unwrap();
        assert_eq!(n, 0);

        tokio::time::timeout(Duration::from_secs(1), forward)
            .await
            .expect("forward task leaked")
            .unwrap()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(1), target)
            .await
            .expect("target task leaked")
            .unwrap();
    }

    #[tokio::test]
    async fn aborting_listener_aborts_active_connections() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let listen_addr = listener.local_addr().unwrap();
        let target_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target_addr = target_listener.local_addr().unwrap().to_string();
        let (accepted_tx, accepted_rx) = oneshot::channel();

        let target = tokio::spawn(async move {
            let (mut peer, _) = target_listener.accept().await.unwrap();
            let _ = accepted_tx.send(());
            let mut byte = [0u8; 1];
            let n = tokio::time::timeout(Duration::from_secs(1), peer.read(&mut byte))
                .await
                .expect("target socket survived forwarder abort")
                .unwrap();
            assert_eq!(n, 0);
        });
        let forwarder = tokio::spawn(run_forwarder(listener, target_addr));
        let mut client = tokio::net::TcpStream::connect(listen_addr).await.unwrap();
        accepted_rx.await.unwrap();

        forwarder.abort();
        let _ = forwarder.await;
        let mut byte = [0u8; 1];
        let n = tokio::time::timeout(Duration::from_secs(1), client.read(&mut byte))
            .await
            .expect("client socket survived forwarder abort")
            .unwrap();
        assert_eq!(n, 0);
        tokio::time::timeout(Duration::from_secs(1), target)
            .await
            .expect("target task leaked")
            .unwrap();
    }
}
