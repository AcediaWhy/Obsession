//! Управление DPI-процессами winws.
//! Порт из `process_local_datasource.dart` + `dpi_provider.dart` + `dpi_usecases.dart`.

use std::process::Stdio;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as TokioCommand;

use crate::state::{AppState, DpiProc};
use crate::util::{self, DpiProcPublic, DpiStatusPayload};

/// URL для проверки обхода по категориям (порт из `TestDpiConfigUseCase`).
fn test_urls(category: &str) -> Vec<&'static str> {
    match category {
        "discord" => vec!["https://discord.com/"],
        "youtube_twitch" => vec!["https://www.youtube.com/", "https://www.twitch.tv/"],
        // Steam в white-list (исключён из обхода) — используем Epic/EA.
        "gaming" => vec!["https://www.epicgames.com/", "https://signin.ea.com/"],
        "universal" => vec!["https://www.google.com/"],
        _ => vec!["https://www.google.com/"],
    }
}

/// Эмитит актуальный статус DPI (активность + список процессов) в UI.
pub fn emit_status(app: &AppHandle) {
    let state = app.state::<AppState>();
    let d = state.dpi.lock().unwrap();
    let processes: Vec<DpiProcPublic> = d
        .procs
        .values()
        .map(|p| DpiProcPublic {
            pid: p.pid,
            category: p.category.clone(),
            config_file: p.config_file.clone(),
        })
        .collect();
    let _ = app.emit(
        "dpi-status",
        DpiStatusPayload {
            active: !processes.is_empty(),
            processes,
        },
    );
}

/// Запускает winws с конфигом категории. Возвращает PID запущенного процесса.
pub async fn start(app: &AppHandle, category: &str, config_file: &str) -> Result<u32, String> {
    let (winws, conf, base) = {
        let state = app.state::<AppState>();
        (
            state.paths.winws_path(),
            state.paths.config_path(category, config_file),
            state.paths.base_dir.clone(),
        )
    };

    if !winws.exists() {
        return Err(format!("Ядро winws.exe не найдено: {}", winws.display()));
    }
    if !conf.exists() {
        return Err(format!("Конфиг не найден: {}", conf.display()));
    }

    util::emit_log(
        app,
        "info",
        "dpi",
        &format!("Запускаю: {category} -> {config_file}"),
    );

    // winws принимает конфиг как `@<путь>`, пути внутри конфига относительные —
    // поэтому рабочая директория обязательно base_dir.
    let mut std_cmd = util::std_command(&winws);
    std_cmd
        .arg(format!("@{}", conf.display()))
        .current_dir(&base)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut cmd = TokioCommand::from(std_cmd);
    cmd.kill_on_drop(false);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Не удалось запустить winws: {e}"))?;

    let pid = child.id().ok_or("Процесс не вернул PID")?;

    // Стримим stdout/stderr в лог UI.
    if let Some(out) = child.stdout.take() {
        spawn_reader(app.clone(), out, category.to_string(), "info");
    }
    if let Some(err) = child.stderr.take() {
        spawn_reader(app.clone(), err, category.to_string(), "error");
    }

    // Ранний выход (<500мс) — процесс упал сразу, запуск неуспешен.
    match tokio::time::timeout(Duration::from_millis(500), child.wait()).await {
        Ok(Ok(status)) => {
            util::emit_log(
                app,
                "error",
                "dpi",
                &format!(
                    "[{category}] winws завершился сразу после запуска (код {:?})",
                    status.code()
                ),
            );
            return Err(format!(
                "winws ({category}) завершился сразу. Причина: антивирус, конфликт или конфиг."
            ));
        }
        Ok(Err(e)) => return Err(format!("Ошибка ожидания процесса: {e}")),
        Err(_) => { /* всё ещё работает — ок */ }
    }

    // Регистрируем в состоянии.
    {
        let state = app.state::<AppState>();
        let mut d = state.dpi.lock().unwrap();
        d.procs.insert(
            pid,
            DpiProc {
                pid,
                category: category.to_string(),
                config_file: config_file.to_string(),
            },
        );
    }
    emit_status(app);

    // Монитор завершения: обновляет UI и логирует крах.
    let app_mon = app.clone();
    let cat_mon = category.to_string();
    let started = Instant::now();
    tokio::spawn(async move {
        let status = child.wait().await;
        let code = status.ok().and_then(|s| s.code()).unwrap_or(-1);
        let lived = started.elapsed().as_millis();

        let intentional = {
            let state = app_mon.state::<AppState>();
            let mut d = state.dpi.lock().unwrap();
            d.procs.remove(&pid);
            d.stopping.remove(&pid)
        };

        if !intentional {
            if lived < 2000 {
                util::emit_log(
                    &app_mon,
                    "error",
                    "dpi",
                    &format!(
                        "[{cat_mon}] winws умер через {lived}мс (код {code}). Обход НЕ работает."
                    ),
                );
            } else {
                util::emit_log(
                    &app_mon,
                    "info",
                    "dpi",
                    &format!("Процесс {cat_mon} завершился (код {code})"),
                );
            }
        }
        emit_status(&app_mon);
    });

    Ok(pid)
}

/// Собирает хостлист для Глаз из активных конфигов winws: вытаскивает пути
/// `--hostlist="..."`/`--hostlist-auto="..."`, читает эти файлы и собирает домены.
/// Так Глаза следят ровно за теми хостами, что обходит текущая стратегия.
#[cfg(windows)]
fn collect_hostlist(app: &AppHandle, configs: &[(String, String)]) -> Vec<String> {
    use std::collections::BTreeSet;
    let st = app.state::<AppState>();
    let base = st.paths.base_dir.clone();
    let re = regex::Regex::new(r#"--hostlist(?:-auto)?="([^"]+)""#).unwrap();
    let mut domains: BTreeSet<String> = BTreeSet::new();
    for (category, config_file) in configs {
        let conf = st.paths.config_path(category, config_file);
        let Ok(text) = std::fs::read_to_string(&conf) else {
            continue;
        };
        for cap in re.captures_iter(&text) {
            // Пути в конфигах с бэкслешами (lists\discord.txt) — нормализуем.
            let rel = cap[1].replace('\\', "/");
            let path = base.join(&rel);
            let Ok(list) = std::fs::read_to_string(&path) else {
                continue; // autohosts может ещё не существовать — это норма
            };
            for line in list.lines() {
                let d = line.trim();
                if d.is_empty() || d.starts_with('#') {
                    continue;
                }
                domains.insert(d.to_ascii_lowercase());
            }
        }
    }
    domains.into_iter().collect()
}

/// Как [`collect_hostlist`], но строит мост `домен → категория` для Мозга: по
/// какой категории обхода наблюдается данный SNI. Если домен встречается в
/// нескольких категориях — первая по порядку `configs` (детерминизм). Нужен для
/// `BrainEvent::SessionStart`, чтобы Reset/Working скоупились по категории.
#[cfg(windows)]
pub fn collect_hostlist_by_category(
    app: &AppHandle,
    configs: &[(String, String)],
) -> std::collections::HashMap<String, String> {
    let st = app.state::<AppState>();
    let base = st.paths.base_dir.clone();
    let re = regex::Regex::new(r#"--hostlist(?:-auto)?="([^"]+)""#).unwrap();
    let mut map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for (category, config_file) in configs {
        let conf = st.paths.config_path(category, config_file);
        let Ok(text) = std::fs::read_to_string(&conf) else {
            continue;
        };
        for cap in re.captures_iter(&text) {
            let rel = cap[1].replace('\\', "/");
            let path = base.join(&rel);
            let Ok(list) = std::fs::read_to_string(&path) else {
                continue;
            };
            for line in list.lines() {
                let d = line.trim();
                if d.is_empty() || d.starts_with('#') {
                    continue;
                }
                map.entry(d.to_ascii_lowercase())
                    .or_insert_with(|| category.clone());
            }
        }
    }
    map
}

/// Запускает «Глаза» после старта winws. Льёт per-flow вердикты в общий лог
/// (source `eyes`) + stderr. Пустой хостлист = следим за всеми :443 (фолбэк).
#[cfg(windows)]
fn start_eyes(app: &AppHandle, hostlist: Vec<String>) {
    let dll = app.state::<AppState>().paths.bin_dir().join("WinDivert.dll");
    if !dll.exists() {
        util::emit_log(app, "warn", "eyes", "WinDivert.dll не найдена — Глаза не стартуют");
        return;
    }
    let n = hostlist.len();
    let cfg = crate::eyes::Config {
        hostlist,
        ..Default::default()
    };
    let app_cb = app.clone();
    match crate::eyes::start(&dll, cfg, move |obs| {
        let line = format!(
            "{:?} {} :{} — {}",
            obs.verdict, obs.domain, obs.local_port, obs.evidence
        );
        // Дублируем в stderr процесса — видно в консоли dev-запуска для отладки.
        eprintln!("[eyes] {line}");
        util::emit_log(&app_cb, "info", "eyes", &line);
        // Сырое наблюдение во фронт (debug-читалка Глаз).
        let _ = app_cb.emit("eyes://observation", &obs);
        // Форвард в Мозг, если авто-восстановление активно. `ts` перештампует
        // рантайм своей монотоникой — часы Глаз сбрасываются на респавне.
        // Клонируем sender в узком блоке, чтобы State-гард освободился до send.
        let brain_tx = {
            let st = app_cb.state::<AppState>();
            let guard = st.brain.lock().ok();
            guard.and_then(|g| g.as_ref().map(|bh| bh.tx.clone()))
        };
        if let Some(tx) = brain_tx {
            let _ = tx.send(crate::brain::BrainEvent::Observation {
                domain: obs.domain.clone(),
                verdict: obs.verdict,
                ts: obs.ts_ms,
            });
        }
    }) {
        Ok(handle) => {
            let st = app.state::<AppState>();
            *st.eyes.lock().unwrap() = Some(handle);
            let msg = if n == 0 {
                "Наблюдатель запущен (sniff :443, хостлист пуст — все хосты)".to_string()
            } else {
                format!("Наблюдатель запущен (sniff :443, хостлист: {n} доменов)")
            };
            eprintln!("[eyes] {msg}");
            util::emit_log(app, "info", "eyes", &msg);
        }
        Err(e) => {
            eprintln!("[eyes] Глаза не запустились: {e}");
            util::emit_log(app, "error", "eyes", &format!("Глаза не запустились: {e}"));
        }
    }
}

/// Останавливает наблюдателя, если запущен.
#[cfg(windows)]
fn stop_eyes(app: &AppHandle) {
    let handle = app.state::<AppState>().eyes.lock().unwrap().take();
    if let Some(h) = handle {
        h.stop();
        util::emit_log(app, "info", "eyes", "Наблюдатель остановлен");
    }
}

/// Останавливает все свои DPI-процессы, ждёт выгрузку WinDivert, чистит DNS.
pub async fn stop_all(app: &AppHandle) {
    #[cfg(windows)]
    stop_eyes(app);

    let pids: Vec<u32> = {
        let state = app.state::<AppState>();
        let mut d = state.dpi.lock().unwrap();
        let pids: Vec<u32> = d.procs.keys().copied().collect();
        for p in &pids {
            d.stopping.insert(*p);
        }
        d.procs.clear();
        pids
    };

    for pid in pids {
        kill_pid(pid);
    }
    emit_status(app);

    // Даём драйверу WinDivert время выгрузиться (иначе конфликт фильтров).
    tokio::time::sleep(Duration::from_millis(500)).await;
    flush_dns();
}

/// Запускает набор конфигов: сначала гасит предыдущие, затем стартует по очереди.
/// Порт из `StartDpiUseCase`. Возвращает список PID.
pub async fn start_many(app: &AppHandle, configs: &[(String, String)]) -> Result<Vec<u32>, String> {
    stop_all(app).await;
    let mut started = Vec::new();
    for (category, config_file) in configs {
        match start(app, category, config_file).await {
            Ok(pid) => started.push(pid),
            Err(e) => util::emit_log(app, "error", "dpi", &format!("{category}: {e}")),
        }
    }
    if started.is_empty() {
        return Err("Ни один DPI-процесс не был запущен".to_string());
    }

    // Поднимаем Глаза после winws — драйвер WinDivert уже установлен,
    // NO_INSTALL-хендл откроется. Даём winws мгновение на init. Хостлист —
    // домены из активных конфигов, чтобы следить только за обходимыми хостами.
    #[cfg(windows)]
    {
        tokio::time::sleep(Duration::from_millis(800)).await;
        let hostlist = collect_hostlist(app, configs);
        start_eyes(app, hostlist);
    }

    Ok(started)
}

/// Тестирует один конфиг: старт → проверка URL → стоп. Порт из `TestDpiConfigUseCase`.
pub async fn test(app: &AppHandle, category: &str, config_file: &str) -> bool {
    stop_all(app).await;
    tokio::time::sleep(Duration::from_millis(500)).await;

    let pid = match start(app, category, config_file).await {
        Ok(p) => p,
        Err(_) => {
            stop_all(app).await;
            return false;
        }
    };

    // Ждём инициализацию WinDivert.
    tokio::time::sleep(Duration::from_millis(1000)).await;

    let mut connected = false;
    for url in test_urls(category) {
        if crate::net::test_url(url, 4).await {
            connected = true;
            break;
        }
    }

    {
        let state = app.state::<AppState>();
        state.dpi.lock().unwrap().stopping.insert(pid);
    }
    kill_pid(pid);
    stop_all(app).await;

    connected
}

/// Обнаруживает чужие/orphan-процессы winws в системе (не свои).
pub fn detect_orphaned(app: &AppHandle) -> Vec<u32> {
    let tracked: std::collections::HashSet<u32> = {
        let state = app.state::<AppState>();
        let set = state.dpi.lock().unwrap().procs.keys().copied().collect();
        set
    };

    let output = util::std_command("tasklist")
        .args([
            "/FI",
            &format!("IMAGENAME eq {}", crate::paths::WINWS_EXE),
            "/NH",
            "/FO",
            "CSV",
        ])
        .output();

    let mut orphans = Vec::new();
    if let Ok(out) = output {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            // CSV: "winws.exe","1234","Console","1","12 345 K"
            let re = regex::Regex::new(r#""(\d+)""#).unwrap();
            for line in text.lines() {
                if let Some(cap) = re.captures(line.trim()) {
                    if let Ok(pid) = cap[1].parse::<u32>() {
                        if !tracked.contains(&pid) {
                            orphans.push(pid);
                        }
                    }
                }
            }
        }
    }
    orphans
}

/// Крайняя мера: убивает ВСЕ winws.exe в системе (включая чужие).
pub fn emergency_kill_all(app: &AppHandle) {
    util::emit_log(
        app,
        "warn",
        "dpi",
        "EMERGENCY: завершение ВСЕХ процессов winws.exe в системе.",
    );
    let _ = util::std_command("taskkill")
        .args(["/F", "/IM", crate::paths::WINWS_EXE])
        .output();
    {
        let state = app.state::<AppState>();
        let mut d = state.dpi.lock().unwrap();
        d.procs.clear();
    }
    emit_status(app);
}

/// Убивает процесс по PID через taskkill /F /PID.
fn kill_pid(pid: u32) {
    let _ = util::std_command("taskkill")
        .args(["/F", "/PID", &pid.to_string()])
        .output();
}

/// Сбрасывает DNS-кэш (ipconfig /flushdns).
pub fn flush_dns() {
    let _ = util::std_command("ipconfig").arg("/flushdns").output();
}

/// Читает поток построчно и эмитит строки в лог UI.
fn spawn_reader<R>(app: AppHandle, reader: R, category: String, level: &'static str)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let msg = line.trim();
            if !msg.is_empty() {
                util::emit_log(&app, level, "dpi", &format!("[{category}] {msg}"));
            }
        }
    });
}
