//! Управление Telegram-прокси (TgWsProxy).
//! Порт из `proxy_local_datasource.dart` + `proxy_provider.dart`.

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
        },
    );
}

/// True, если бинарник TgWsProxy найден.
pub fn available(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    state.paths.tgproxy_path().is_some()
}

/// Запускает прокси. Возвращает `tg://proxy?...` ссылку.
pub async fn start(app: &AppHandle, port: u16, fake_tls_domain: &str) -> Result<String, String> {
    stop(app).await;

    let (exe, bin_dir) = {
        let state = app.state::<AppState>();
        match state.paths.tgproxy_path() {
            Some(e) => (e, state.paths.bin_dir()),
            None => return Err("TgWsProxy.exe не найден в bin/".to_string()),
        }
    };

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

    util::emit_log(
        app,
        "info",
        "proxy",
        &format!("Запуск Telegram-прокси на порту {port}..."),
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
        p.stopping = false;
    }

    // Канал: reader сообщает, как только найдёт tg:// ссылку в stdout.
    let (tx, rx) = oneshot::channel::<String>();
    if let Some(out) = child.stdout.take() {
        let app_out = app.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(out).lines();
            let mut tx = Some(tx);
            let re = regex::Regex::new(r"tg://proxy\?[^\s]+").unwrap();
            while let Ok(Some(line)) = lines.next_line().await {
                let msg = line.trim();
                if msg.is_empty() {
                    continue;
                }
                util::emit_log(&app_out, "info", "proxy", &format!("[tg] {msg}"));
                if let Some(m) = re.find(msg) {
                    let link = m.as_str().to_string();
                    let state = app_out.state::<AppState>();
                    state.proxy.lock().unwrap().link = link.clone();
                    if let Some(tx) = tx.take() {
                        let _ = tx.send(link);
                    }
                }
            }
        });
    }
    if let Some(err) = child.stderr.take() {
        let app_err = app.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(err).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let msg = line.trim();
                if !msg.is_empty() {
                    util::emit_log(&app_err, "error", "proxy", &format!("[tg] {msg}"));
                }
            }
        });
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
            }
            std::mem::take(&mut p.stopping)
        };
        if !intentional {
            util::emit_log(&app_mon, "warn", "proxy", "[tg] Прокси остановлен.");
        }
        emit_status(&app_mon);
    });

    // Ждём ссылку до 5с, иначе генерируем вручную.
    let link = match tokio::time::timeout(Duration::from_secs(5), rx).await {
        Ok(Ok(l)) => l,
        _ => generate_manual(port, &secret, fake_tls_domain),
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

    {
        let state = app.state::<AppState>();
        state.proxy.lock().unwrap().link = link.clone();
    }
    util::emit_log(
        app,
        "success",
        "proxy",
        &format!("Telegram-прокси запущен на порту {port}"),
    );
    emit_status(app);
    Ok(link)
}

/// Останавливает прокси (по своему PID).
pub async fn stop(app: &AppHandle) {
    let pid = {
        let state = app.state::<AppState>();
        let mut p = state.proxy.lock().unwrap();
        if let Some(pid) = p.pid.take() {
            p.stopping = true;
            p.link = String::new();
            Some(pid)
        } else {
            None
        }
    };
    if let Some(pid) = pid {
        let _ = util::std_command("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .output();
    }
    emit_status(app);
}

pub fn current_link(app: &AppHandle) -> String {
    let state = app.state::<AppState>();
    let link = state.proxy.lock().unwrap().link.clone();
    link
}

/// Генерирует tg://proxy ссылку вручную (fallback, если прокси не выдал её сам).
fn generate_manual(port: u16, secret: &str, fake_tls_domain: &str) -> String {
    const HOST: &str = "127.0.0.1";
    if !fake_tls_domain.is_empty() {
        let domain_hex: String = fake_tls_domain
            .as_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        format!("tg://proxy?server={HOST}&port={port}&secret=ee{secret}{domain_hex}")
    } else {
        format!("tg://proxy?server={HOST}&port={port}&secret=dd{secret}")
    }
}

fn gen_secret() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
