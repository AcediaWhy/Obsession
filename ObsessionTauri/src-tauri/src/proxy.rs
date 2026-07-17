//! Управление Telegram-прокси (TgWsProxy).
//!
//! TgWsProxy — это локальный MTProto-прокси (headless CLI-сборка из
//! `proxy/tg_ws_proxy.py`). Он слушает MTProto ровно на `127.0.0.1:<--port>`
//! и туннелирует соединения через WebSocket к серверам Telegram. Секрет
//! передаётся ему через `--secret`, а готовую ссылку `tg://proxy?...` он сам
//! печатает в лог (stderr) — мы её оттуда и вычитываем.
//!
//! Приложение:
//!   1. Запускает CLI с `--port/--secret[/--fake-tls-domain]` и ловит
//!      напечатанную им ссылку `tg://proxy?server=127.0.0.1&port=<port>&secret=dd...`.
//!   2. Для доступа с телефона поднимает TCP-форвардер `0.0.0.0:<port>` →
//!      `127.0.0.1:<port>` (слушает ВСЕ интерфейсы — телефон может прийти на
//!      любой адрес ПК) и отдаёт ту же ссылку с LAN IP вместо 127.0.0.1.
//!      LAN IP выбирается через `util::lan_ip_for_phone` (реальная физ. карта,
//!      не VPN/виртуальный адаптер).

use std::process::Stdio;
use std::time::Duration;

use rand::RngCore;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command as TokioCommand;
use tokio::sync::oneshot;
use tokio::task::JoinSet;

use crate::state::{AppState, ProxyFirewall, ProxyForwarder, ProxyState};
use crate::util::{self, LockExt, ProxyStatusPayload, VersionedSection};

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

/// True, если бинарник TgWsProxy найден.
pub fn available(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    state.paths.tgproxy_path().is_some()
}

fn runtime_shutting_down(app: &AppHandle) -> bool {
    app.state::<AppState>()
        .shutting_down
        .load(std::sync::atomic::Ordering::SeqCst)
}

const FORWARD_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const FORWARD_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_FORWARD_CONNECTIONS: usize = 512;

/// Имя firewall-правила — generation-aware: каждое поколение публикации имеет
/// уникальное имя, поэтому старое поколение не может удалить правило нового
/// (см. инвариант «generation-safe cleanup»).
fn firewall_rule_name(port: u16, generation: u64) -> String {
    format!("Obsession TgWsProxy {port} gen{generation}")
}

/// Строит argv для `netsh advfirewall firewall add rule` из ВАЛИДИРОВАННЫХ
/// числовых значений (порт u16, generation u64) и подсети. Чистая функция —
/// тестируется без запуска netsh (F.3). Правило строго ограничено:
/// - `profile=private` — только доверенные сети (не Public/Domain);
/// - `remoteip=<subnet>` — только локальная подсеть, а не весь интернет.
/// `subnet` = CIDR выбранного интерфейса; при `None` — безопасный keyword
/// `LocalSubnet` (Windows сам ограничивает текущей локальной подсетью).
fn build_add_rule_args(port: u16, name: &str, subnet: Option<&str>) -> Vec<String> {
    let remoteip = subnet.unwrap_or("LocalSubnet");
    vec![
        "advfirewall".into(),
        "firewall".into(),
        "add".into(),
        "rule".into(),
        format!("name={name}"),
        "group=Obsession".into(),
        "dir=in".into(),
        "action=allow".into(),
        "protocol=tcp".into(),
        format!("localport={port}"),
        "profile=private".into(),
        format!("remoteip={remoteip}"),
    ]
}

#[cfg(windows)]
fn add_firewall_rule(port: u16, name: &str, subnet: Option<&str>) -> Result<(), String> {
    let mut cmd = util::std_command("netsh");
    let out = cmd
        .args(build_add_rule_args(port, name, subnet))
        .output()
        .map_err(|e| format!("netsh add rule failed: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).to_string());
    }
    Ok(())
}

#[cfg(windows)]
fn remove_firewall_rule(name: &str) {
    let name_arg = format!("name={name}");
    let mut cmd = util::std_command("netsh");
    let _ = cmd
        .args(["advfirewall", "firewall", "delete", "rule", &name_arg])
        .output();
}

#[cfg(not(windows))]
fn add_firewall_rule(_port: u16, _name: &str, _subnet: Option<&str>) -> Result<(), String> {
    Ok(())
}

#[cfg(not(windows))]
fn remove_firewall_rule(_name: &str) {}

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

fn cleanup_nonprocess_runtime(detached: DetachedProxy) {
    if let Some(forwarder) = detached.forwarder {
        forwarder.handle.abort();
    }
    if let Some(firewall) = detached.firewall {
        remove_firewall_rule(&firewall.name);
    }
}

/// Текущее Unix-время в секундах.
fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Снимает ТОЛЬКО LAN-публикацию (0.0.0.0 forwarder + firewall) текущего
/// поколения, СОХРАНЯЯ процесс прокси и локальный `127.0.0.1` listener — чтобы
/// Telegram Desktop продолжал работать после закрытия доступа с телефона.
/// Забирает ресурсы под локом (быстро), гасит их вне лока (netsh/abort блокирующие).
pub async fn close_lan_publication(app: &AppHandle, reason: &str) {
    let taken = {
        let state = app.state::<AppState>();
        let mut p = state.proxy.lock_recover();
        if !p.lan_published {
            return; // публикации нет — нечего снимать
        }
        p.lan_published = false;
        p.lan_expiry_unix = None;
        p.lan_link = None;
        (p.forwarder.take(), p.firewall.take())
    };
    let (forwarder, firewall) = taken;
    if let Some(f) = forwarder {
        f.handle.abort();
    }
    if let Some(fw) = firewall {
        let name = fw.name.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || remove_firewall_rule(&name)).await;
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
}

/// Спавнит таймер авто-закрытия LAN-публикации. Проверяет generation+pid перед
/// закрытием, поэтому истёкший таймер старой сессии не тронет новую публикацию.
fn spawn_lan_expiry_timer(app: &AppHandle, generation: u64, pid: u32, secs: u16) {
    if secs == 0 {
        return; // 0 = без авто-закрытия
    }
    let app = app.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(secs as u64)).await;
        if !is_current(&app, generation, pid) {
            return; // сессия сменилась — таймер неактуален
        }
        close_lan_publication(&app, "истёк таймаут").await;
    });
}

/// Async-обёртка [`cleanup_previous_runtime`]: блокирующие taskkill/netsh уходят
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
        remove_firewall_rule(&firewall.name);
    }
    if let Some(pid) = detached.pid {
        let _ = util::std_command("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .output();
    }
    // PyInstaller-onefile создаёт дочерний процесс с тем же именем. Sweep
    // выполняется до нового spawn и под gate, поэтому не может задеть новый PID.
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
async fn start_locked(app: &AppHandle, port: u16, fake_tls_domain: &str) -> Result<String, String> {
    let (exe, bin_dir, cache_path, image) = {
        let state = app.state::<AppState>();
        match state.paths.tgproxy_path() {
            Some(e) => {
                let image = e.file_name().map(|n| n.to_string_lossy().to_string());
                (
                    e,
                    state.paths.bin_dir(),
                    state.paths.cfproxy_cache_path(),
                    image,
                )
            }
            None => return Err("TgWsProxy.exe не найден в bin/".to_string()),
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
    // Диск-кэш CF-доменов под %APPDATA%\Obsession — переживает недоступность GitHub.
    args.push("--cfproxy-cache".into());
    args.push(cache_path.to_string_lossy().to_string());

    util::emit_log(
        app,
        "info",
        "proxy",
        &format!("Запуск Telegram-прокси (MTProto на 127.0.0.1:{port})..."),
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
    tokio::spawn(async move {
        let status = child.wait().await;
        let code = status.ok().and_then(|s| s.code()).unwrap_or(-1);
        let lived = started.elapsed().as_millis();
        let Some(detached) = detach_if_current(&app_mon, generation, pid) else {
            return; // штатный stop или уже начался новый generation
        };
        cleanup_nonprocess_runtime(detached);
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
    let link = received_link
        .unwrap_or_else(|| generate_manual("127.0.0.1", port, &secret, fake_tls_domain));

    if !is_current(app, generation, pid) {
        return Err("Прокси завершился сразу после запуска.".to_string());
    }

    // LAN-ссылка для телефона: тот же proxy через локальный TCP-forwarder.
    let lan_link = if lan.ip != "127.0.0.1" {
        let bind = format!("0.0.0.0:{port}");
        let target = format!("127.0.0.1:{port}");
        let route_note = match &lan.route_ip {
            Some(r) if *r != lan.ip => format!(
                " Выход в интернет через {r} — это VPN/виртуальный адаптер, телефону он недоступен, поэтому взят LAN-адрес."
            ),
            _ => String::new(),
        };
        util::emit_log(
            app,
            "info",
            "proxy",
            &format!(
                "Телефон: адрес для QR {}:{port} (форвардер слушает 0.0.0.0:{port}, брандмауэр ограничен profile=private + {}). Найденные LAN-адреса: [{}].{route_note}",
                lan.ip,
                lan.subnet.as_deref().unwrap_or("LocalSubnet"),
                lan.candidates.join(", ")
            ),
        );

        match tokio::net::TcpListener::bind(&bind).await {
            Ok(listener) => {
                let rule_name = firewall_rule_name(port, generation);
                // netsh — блокирующий; уводим с воркера. Сначала снимаем возможно
                // оставшийся после crash exact-rule этого порта, затем добавляем.
                // Правило строго ограничено: profile=private + remoteip=<подсеть>.
                let rn = rule_name.clone();
                let subnet = lan.subnet.clone();
                let fw_res = tauri::async_runtime::spawn_blocking(move || {
                    remove_firewall_rule(&rn);
                    add_firewall_rule(port, &rn, subnet.as_deref())
                })
                .await
                .unwrap_or_else(|_| Err("firewall task panicked".to_string()));
                let firewall = match fw_res {
                    Ok(()) => Some(ProxyFirewall {
                        generation,
                        name: rule_name,
                    }),
                    Err(e) => {
                        util::emit_log(
                            app,
                            "warn",
                            "proxy",
                            &format!("Не удалось добавить правило брандмауэра: {e}"),
                        );
                        None
                    }
                };
                let handle = tokio::spawn(run_forwarder(listener, target));
                let mut forwarder = Some(ProxyForwarder { generation, handle });
                let mut firewall = firewall;
                // Таймаут LAN-публикации из настроек (0 = без авто-закрытия).
                let lan_secs = {
                    let state = app.state::<AppState>();
                    let s = state.settings.lock_recover();
                    s.lan_publish_secs
                };
                let expiry = if lan_secs == 0 {
                    None
                } else {
                    Some(now_unix() + lan_secs as u64)
                };
                let installed = {
                    let state = app.state::<AppState>();
                    let mut p = state.proxy.lock_recover();
                    if p.generation == generation && p.pid == Some(pid) {
                        p.forwarder = forwarder.take();
                        p.firewall = firewall.take();
                        p.lan_published = true;
                        p.lan_expiry_unix = expiry;
                        true
                    } else {
                        false
                    }
                };
                if installed {
                    spawn_lan_expiry_timer(app, generation, pid, lan_secs);
                }
                if !installed {
                    cleanup_nonprocess_runtime(DetachedProxy {
                        pid: None,
                        forwarder,
                        firewall,
                    });
                    return Err("Прокси завершился во время запуска LAN-форвардера.".to_string());
                }
                Some(link.replace("127.0.0.1", &lan.ip))
            }
            Err(e) => {
                util::emit_log(
                    app,
                    "warn",
                    "proxy",
                    &format!("LAN-форвардер не запущен ({bind}): {e}"),
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

    util::emit_log(app, "success", "proxy", &format!("Telegram-прокси: {link}"));
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
    let image = {
        let state = app.state::<AppState>();
        state
            .paths
            .tgproxy_path()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
    };
    let (_, detached) = begin_generation(app);
    let killed = cleanup_previous_runtime(app, detached, image.as_deref());
    emit_status(app);
    killed
}

/// Async-обёртка [`stop_locked`]: блокирующие taskkill/netsh уходят в blocking-
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

/// Генерирует tg://proxy ссылку вручную (fallback, если прокси не выдал её сам).
/// Секрет всегда с префиксом: `dd` для secure-режима (как печатает сам прокси),
/// либо `ee` + secret + домен в hex для fake-TLS. Порт — реальный `--port` прокси.
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
        static LINK_RE: std::sync::LazyLock<regex::Regex> =
            std::sync::LazyLock::new(|| regex::Regex::new(r"tg://proxy\?[^\s]+").unwrap());
        let mut lines = BufReader::new(stream).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if !is_current(&app, generation, pid) {
                break;
            }
            let msg = line.trim();
            if msg.is_empty() {
                continue;
            }
            util::emit_log(&app, "info", "proxy", &format!("[tg] {msg}"));
            if let Some(m) = LINK_RE.find(msg) {
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
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

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

    #[test]
    fn firewall_rule_name_is_generation_scoped() {
        // Разные поколения → разные имена: старый generation не удалит правило нового.
        let g1 = firewall_rule_name(1080, 7);
        let g2 = firewall_rule_name(1080, 8);
        assert_ne!(g1, g2);
        assert!(g1.contains("1080") && g1.contains("gen7"));
    }

    #[test]
    fn add_rule_args_are_scoped_to_private_and_subnet() {
        let args = build_add_rule_args(
            1080,
            "Obsession TgWsProxy 1080 gen3",
            Some("192.168.1.0/24"),
        );
        assert!(
            args.contains(&"profile=private".to_string()),
            "должен быть profile=private"
        );
        assert!(
            args.contains(&"remoteip=192.168.1.0/24".to_string()),
            "remoteip ограничен подсетью, не весь интернет"
        );
        assert!(args.contains(&"localport=1080".to_string()));
        assert!(args.contains(&"dir=in".to_string()));
        assert!(args.contains(&"action=allow".to_string()));
        // Никогда не публикуем на все профили / весь интернет.
        assert!(!args
            .iter()
            .any(|a| a == "profile=any" || a == "remoteip=any"));
    }

    #[test]
    fn add_rule_args_fallback_to_localsubnet_without_cidr() {
        let args = build_add_rule_args(1080, "n", None);
        assert!(
            args.contains(&"remoteip=LocalSubnet".to_string()),
            "без CIDR — безопасный keyword LocalSubnet, не any"
        );
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
