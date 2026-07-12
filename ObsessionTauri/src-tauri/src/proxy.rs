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
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as TokioCommand;
use tokio::sync::oneshot;

use crate::state::AppState;
use crate::util::{self, ProxyStatusPayload};

fn emit_status(app: &AppHandle) {
    let state = app.state::<AppState>();
    let p = state.proxy.lock().unwrap();
    let _ = app.emit(
        "proxy-status",
        ProxyStatusPayload {
            running: p.pid.is_some(),
            link: p.link.clone(),
            lan_link: p.lan_link.clone(),
        },
    );
}

/// True, если бинарник TgWsProxy найден.
pub fn available(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    state.paths.tgproxy_path().is_some()
}

#[cfg(windows)]
fn add_firewall_rule(port: u16) -> Result<(), String> {
    let mut cmd = util::std_command("netsh");
    let out = cmd
        .args([
            "advfirewall",
            "firewall",
            "add",
            "rule",
            "name=Obsession TgWsProxy",
            "dir=in",
            "action=allow",
            "protocol=tcp",
            &format!("localport={port}"),
        ])
        .output()
        .map_err(|e| format!("netsh add rule failed: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).to_string());
    }
    Ok(())
}

#[cfg(windows)]
fn remove_firewall_rule() {
    let mut cmd = util::std_command("netsh");
    let _ = cmd
        .args([
            "advfirewall",
            "firewall",
            "delete",
            "rule",
            "name=Obsession TgWsProxy",
        ])
        .output();
}

#[cfg(not(windows))]
fn add_firewall_rule(_port: u16) -> Result<(), String> {
    Ok(())
}

#[cfg(not(windows))]
fn remove_firewall_rule() {}

/// Запускает прокси. Возвращает `tg://proxy?...` ссылку.
pub async fn start(app: &AppHandle, port: u16, fake_tls_domain: &str) -> Result<String, String> {
    if stop(app).await {
        // Кого-то убили — даём Windows момент освободить порт перед новым биндом.
        tokio::time::sleep(Duration::from_millis(300)).await;
    }

    let (exe, bin_dir, cache_path) = {
        let state = app.state::<AppState>();
        match state.paths.tgproxy_path() {
            Some(e) => (e, state.paths.bin_dir(), state.paths.cfproxy_cache_path()),
            None => return Err("TgWsProxy.exe не найден в bin/".to_string()),
        }
    };

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

    {
        let state = app.state::<AppState>();
        let mut p = state.proxy.lock().unwrap();
        p.pid = Some(pid);
        p.link = String::new();
        p.lan_link = None;
        p.stopping = false;
    }

    // Прокси печатает готовую ссылку `tg://proxy?...` в лог (у CLI это stderr).
    // Сканируем ОБА потока — первое совпадение выигрывает через общий sender.
    let (tx, rx) = oneshot::channel::<String>();
    let tx = std::sync::Arc::new(std::sync::Mutex::new(Some(tx)));
    if let Some(out) = child.stdout.take() {
        spawn_link_reader(app.clone(), out, tx.clone());
    }
    if let Some(err) = child.stderr.take() {
        spawn_link_reader(app.clone(), err, tx.clone());
    }

    // Монитор завершения.
    let app_mon = app.clone();
    tokio::spawn(async move {
        let _ = child.wait().await;
        let intentional = {
            let state = app_mon.state::<AppState>();
            let mut p = state.proxy.lock().unwrap();
            if p.pid == Some(pid) {
                p.pid = None;
                p.link = String::new();
                p.lan_link = None;
            }
            if let Some(h) = p.forwarder.take() {
                h.abort();
            }
            std::mem::take(&mut p.stopping)
        };
        remove_firewall_rule();
        if !intentional {
            util::emit_log(&app_mon, "warn", "proxy", "[tg] Прокси остановлен.");
        }
        emit_status(&app_mon);
    });

    // Ждём ссылку до 5с, иначе генерируем вручную (с 127.0.0.1 для ПК).
    let link = match tokio::time::timeout(Duration::from_secs(5), rx).await {
        Ok(Ok(l)) => l,
        _ => generate_manual("127.0.0.1", port, &secret, fake_tls_domain),
    };

    // Проверяем, что процесс всё ещё жив.
    let alive = {
        let state = app.state::<AppState>();
        let alive = state.proxy.lock().unwrap().pid == Some(pid);
        alive
    };
    if !alive {
        return Err("Прокси завершился сразу после запуска.".to_string());
    }

    // LAN-ссылка для телефона: тот же прокси, но с адресом, по которому до ПК
    // достучится телефон в общей Wi-Fi (см. util::lan_ip_for_phone).
    let lan_link = if lan.ip != "127.0.0.1" {
        // Форвардер слушает ВСЕ интерфейсы (0.0.0.0), а не выбранный IP: телефон
        // может прийти на любой адрес ПК. Прежний бинд на конкретный IP давал
        // «connection refused», если авто-выбор адреса промахивался.
        let bind = format!("0.0.0.0:{port}");
        let target = format!("127.0.0.1:{port}");
        // Диагностика в UI-лог: какой адрес уехал в QR, все кандидаты и — если
        // выход в интернет идёт мимо него — предупреждение про VPN/виртуалку.
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
                "Телефон: адрес для QR {}:{port} (форвардер слушает 0.0.0.0:{port}). Найденные LAN-адреса: [{}].{route_note}",
                lan.ip,
                lan.candidates.join(", ")
            ),
        );
        if let Err(e) = add_firewall_rule(port) {
            util::emit_log(
                app,
                "warn",
                "proxy",
                &format!("Не удалось добавить правило брандмауэра: {e}"),
            );
        }
        let forwarder = tokio::spawn(run_forwarder(bind, target));
        {
            let state = app.state::<AppState>();
            state.proxy.lock().unwrap().forwarder = Some(forwarder);
        }
        Some(link.replace("127.0.0.1", &lan.ip))
    } else {
        util::emit_log(
            app,
            "warn",
            "proxy",
            "LAN-адрес для телефона не найден — QR будет работать только на этом ПК.",
        );
        None
    };

    {
        let state = app.state::<AppState>();
        let mut p = state.proxy.lock().unwrap();
        p.link = link.clone();
        p.lan_link = lan_link.clone();
    }
    util::emit_log(
        app,
        "success",
        "proxy",
        &format!("Telegram-прокси: {link}"),
    );
    emit_status(app);
    Ok(link)
}

/// Останавливает прокси и TCP-форвардер. Возвращает true, если было что убивать.
///
/// ВАЖНО: TgWsProxy — PyInstaller-onefile, то есть ДВА процесса: наш PID — это
/// бутлоадер-родитель, а реальный Python-код (он и держит порт) — его ребёнок
/// с тем же именем образа. `taskkill /PID` без `/T` убивал только родителя:
/// прокси продолжал работать при «выключенном» UI, а рестарт падал с занятым
/// портом. Поэтому убиваем дерево (/T) и добиваем сирот по имени образа
/// (остатки прошлых сессий или падения лаунчера).
pub async fn stop(app: &AppHandle) -> bool {
    let (pid, image) = {
        let state = app.state::<AppState>();
        let image = state
            .paths
            .tgproxy_path()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()));
        let mut p = state.proxy.lock().unwrap();
        if let Some(h) = p.forwarder.take() {
            h.abort();
        }
        let pid = p.pid.take();
        if pid.is_some() {
            p.stopping = true;
            p.link = String::new();
            p.lan_link = None;
        }
        (pid, image)
    };
    remove_firewall_rule();
    let mut killed = false;
    if let Some(pid) = pid {
        killed = true;
        let _ = util::std_command("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .output();
    }
    // Добивание по имени образа: после /T обычно никого нет, успех = были сироты.
    if let Some(image) = image {
        let swept = util::std_command("taskkill")
            .args(["/F", "/T", "/IM", &image])
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
    emit_status(app);
    killed
}

pub fn current_link(app: &AppHandle) -> String {
    let state = app.state::<AppState>();
    let link = state.proxy.lock().unwrap().link.clone();
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

/// Читает поток построчно, логирует каждую строку с префиксом `[tg]` и на первом
/// совпадении `tg://proxy?...` шлёт ссылку через общий (stdout+stderr) sender.
fn spawn_link_reader<R>(
    app: AppHandle,
    stream: R,
    tx: std::sync::Arc<std::sync::Mutex<Option<oneshot::Sender<String>>>>,
) where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let re = regex::Regex::new(r"tg://proxy\?[^\s]+").unwrap();
        let mut lines = BufReader::new(stream).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let msg = line.trim();
            if msg.is_empty() {
                continue;
            }
            util::emit_log(&app, "info", "proxy", &format!("[tg] {msg}"));
            if let Some(m) = re.find(msg) {
                let link = m.as_str().to_string();
                app.state::<AppState>().proxy.lock().unwrap().link = link.clone();
                if let Some(tx) = tx.lock().unwrap().take() {
                    let _ = tx.send(link);
                }
            }
        }
    });
}

/// Простой TCP-форвардер: `bind_addr` → `target_addr`.
async fn run_forwarder(bind_addr: String, target_addr: String) {
    let listener = match tokio::net::TcpListener::bind(&bind_addr).await {
        Ok(l) => l,
        Err(_) => return,
    };

    loop {
        let (client, _) = match listener.accept().await {
            Ok(c) => c,
            Err(_) => continue,
        };
        // MTProto — поток мелких пакетов; без TCP_NODELAY алгоритм Нейгла
        // копит их и добавляет задержку (жалобы «работает медленно»).
        let _ = client.set_nodelay(true);
        let target = target_addr.clone();
        tokio::spawn(async move {
            let server = match tokio::net::TcpStream::connect(&target).await {
                Ok(s) => s,
                Err(_) => return,
            };
            let _ = server.set_nodelay(true);
            let (mut cr, mut cw) = client.into_split();
            let (mut sr, mut sw) = server.into_split();
            let c2s = tokio::io::copy(&mut cr, &mut sw);
            let s2c = tokio::io::copy(&mut sr, &mut cw);
            let _ = tokio::join!(c2s, s2c);
        });
    }
}

fn gen_secret() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
