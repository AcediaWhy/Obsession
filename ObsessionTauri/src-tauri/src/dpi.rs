//! Управление DPI-процессами winws.
//! Порт из `process_local_datasource.dart` + `dpi_provider.dart` + `dpi_usecases.dart`.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as TokioCommand;

use crate::adaptive_strategy::dsl::{override_key, StrategyCandidate, StrategyTransport};
use crate::state::{AppState, DpiLaunchSpec, DpiProc, DpiRuntimeSnapshot};
use crate::util::{self, DpiProcPublic, DpiStatusPayload, LockExt, VersionedSection};

/// Регэкспы разбора legacy-моста/вывода команд — компилируются один раз на процесс.
#[allow(dead_code)]
static HOSTLIST_RE: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r#"--hostlist(?:-auto)?="([^"]+)""#).unwrap());
static ORPHAN_PID_RE: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r#""(\d+)""#).unwrap());

#[cfg(windows)]
const EYES_STOP_TIMEOUT: Duration = Duration::from_secs(2);

fn runtime_shutting_down(app: &AppHandle) -> bool {
    app.state::<AppState>().shutting_down.load(Ordering::SeqCst)
}

pub(crate) fn persist_engine_selection(app: &AppHandle, engine: &str) -> Result<(), String> {
    let state = app.state::<AppState>();
    let snapshot = {
        let mut settings = state.settings.lock_recover();
        settings.dpi_engine = engine.to_string();
        settings.clone()
    };
    snapshot
        .save(&state.paths.base_dir)
        .map_err(|e| format!("не удалось сохранить выбор DPI-движка: {e}"))?;
    state.settings_revision.bump();
    Ok(())
}

fn resolve_list_binding(
    lists_dir: &Path,
    file_name: &str,
    kind: crate::lists_validate::ListKind,
    required: bool,
) -> Result<Option<String>, String> {
    if file_name.is_empty()
        || file_name.len() > 128
        || !file_name.ends_with(".txt")
        || file_name.contains("..")
        || file_name.contains('/')
        || file_name.contains('\\')
        || file_name.contains(':')
        || !file_name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(format!("небезопасное имя списка: {file_name}"));
    }
    let path = lists_dir.join(file_name);
    if !path.exists() {
        if required {
            return Err(format!("список не найден: {}", path.display()));
        }
        return Ok(None);
    }
    let root = std::fs::canonicalize(lists_dir)
        .map_err(|error| format!("не удалось разрешить каталог списков: {error}"))?;
    let canonical = std::fs::canonicalize(&path)
        .map_err(|error| format!("не удалось разрешить список {}: {error}", path.display()))?;
    if !canonical.starts_with(&root) {
        return Err(format!(
            "список выходит за пределы lists_dir: {}",
            path.display()
        ));
    }
    let content = std::fs::read_to_string(&canonical).map_err(|error| {
        format!(
            "не удалось прочитать список {}: {error}",
            canonical.display()
        )
    })?;
    let report = crate::lists_validate::validate_list(&content, kind);
    if !report.is_valid() {
        return Err(format!(
            "список {} не прошёл проверку: {}",
            canonical.display(),
            report.errors.join("; ")
        ));
    }
    if report.entries == 0 {
        return Err(format!("список пуст: {}", canonical.display()));
    }
    Ok(Some(canonical.to_string_lossy().replace('\\', "/")))
}

fn resolve_strategy_hostlist(
    lists_dir: &Path,
    category: &str,
    strategy: &crate::dpi_engine::manifest::StrategyDef,
) -> Result<Option<String>, String> {
    let file_name = strategy
        .hostlist
        .clone()
        .or_else(|| strategy.ipset.is_none().then(|| format!("{category}.txt")));
    match file_name {
        Some(file_name) => resolve_list_binding(
            lists_dir,
            &file_name,
            crate::lists_validate::ListKind::Domains,
            strategy.hostlist.is_some(),
        ),
        None => Ok(None),
    }
}

fn resolve_strategy_ipset(
    lists_dir: &Path,
    strategy: &crate::dpi_engine::manifest::StrategyDef,
) -> Result<Option<String>, String> {
    strategy
        .ipset
        .as_deref()
        .map(|file_name| {
            resolve_list_binding(
                lists_dir,
                file_name,
                crate::lists_validate::ListKind::Ipset,
                true,
            )
        })
        .transpose()
        .map(|value| value.flatten())
}

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
    let mut d = state.dpi.lock_recover();
    let processes: Vec<DpiProcPublic> = d
        .procs
        .values()
        .map(|p| DpiProcPublic {
            pid: p.pid,
            category: p.category.clone(),
            config_file: p.config_file.clone(),
        })
        .collect();
    let started_at = d.sync_started_at(util::unix_secs());
    let revision = d.bump_revision();
    let _ = app.emit(
        "dpi-status",
        VersionedSection::new(
            revision,
            DpiStatusPayload {
                active: !processes.is_empty(),
                processes,
                started_at,
            },
        ),
    );
}

/// An unexpected Legacy process exit invalidates the exact Manager snapshot.
/// Process monitors remove the PID immediately, then serialize this teardown
/// with start/stop before touching the public owner. A later session has a new
/// DPI generation and is therefore left untouched.
async fn invalidate_legacy_reliability_after_unexpected_exit(app: &AppHandle, generation: u64) {
    let state = app.state::<AppState>();
    let _gate = state.dpi_gate.lock().await;
    let legacy_processes_remain = {
        let dpi = state.dpi.lock_recover();
        if !dpi.is_current_generation(generation) {
            return;
        }
        dpi.procs
            .values()
            .any(|process| process.generation == generation && process.engine == "legacy")
    };

    if legacy_processes_remain {
        crate::legacy_reliability::status::publish_current_blind(app);
    } else {
        crate::legacy_reliability::status::publish(
            app,
            crate::legacy_reliability::status::LegacyReliabilityStatus::inactive(),
        );
    }

    let manager = state.legacy_manager.lock_recover().take();
    if let Some(manager) = manager {
        manager.shutdown().await;
    }

    #[cfg(windows)]
    {
        let app = app.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || stop_eyes(&app)).await;
    }
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

    crate::dpi_engine::resources::validate_engine_resources(&base, "zapret1")
        .map_err(|e| format!("Zapret Legacy не прошёл проверку ресурсов: {e}"))?;

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

    let pid = match child.id() {
        Some(pid) => pid,
        None => {
            let _ = child.kill().await;
            return Err("Процесс не вернул PID".to_string());
        }
    };
    let process_identity = crate::dpi_supervisor::capture_process_identity(pid);

    // Регистрируем СРАЗУ после spawn. Раньше PID появлялся в AppState только
    // после 500мс ожидания, и shutdown в этом окне оставлял orphan winws.
    let generation = {
        let state = app.state::<AppState>();
        let mut d = state.dpi.lock_recover();
        let generation = d.generation;
        d.procs.insert(
            pid,
            DpiProc {
                pid,
                category: category.to_string(),
                config_file: config_file.to_string(),
                generation,
                engine: "legacy".to_string(),
                process_identity,
            },
        );
        generation
    };
    emit_status(app);

    // Стримим stdout/stderr в лог UI и одновременно ловим startup marker.
    let (readiness_tx, readiness_rx) = tokio::sync::mpsc::unbounded_channel();
    if let Some(out) = child.stdout.take() {
        spawn_reader(
            app.clone(),
            out,
            category.to_string(),
            "info",
            Some(readiness_tx.clone()),
        );
    }
    if let Some(err) = child.stderr.take() {
        spawn_reader(
            app.clone(),
            err,
            category.to_string(),
            "error",
            Some(readiness_tx.clone()),
        );
    }
    drop(readiness_tx);

    let started = Instant::now();
    let readiness = crate::dpi_supervisor::wait_for_readiness(
        async { child.wait().await.map(|status| status.code()) },
        readiness_rx,
        Duration::from_millis(500),
    )
    .await;
    match readiness {
        Ok(crate::dpi_supervisor::ReadinessState::Marker) => {}
        Ok(crate::dpi_supervisor::ReadinessState::BoundedFallback) => {
            util::emit_log(
                app,
                "debug",
                "dpi",
                &format!("[{category}] startup marker не получен; применён bounded fallback"),
            );
        }
        Err(crate::dpi_supervisor::ReadinessFailure::Exited(code)) => {
            {
                let state = app.state::<AppState>();
                let mut d = state.dpi.lock_recover();
                d.procs.remove(&pid);
                d.stopping.remove(&pid);
                d.active_launch = None;
            }
            emit_status(app);
            util::emit_log(
                app,
                "error",
                "dpi",
                &format!("[{category}] winws завершился сразу после запуска (код {code:?})"),
            );
            return Err(format!(
                "winws ({category}) завершился сразу. Причина: антивирус, конфликт или конфиг."
            ));
        }
        Err(crate::dpi_supervisor::ReadinessFailure::WaitFailed(error)) => {
            kill_pid_async(pid).await;
            {
                let state = app.state::<AppState>();
                let mut d = state.dpi.lock_recover();
                d.procs.remove(&pid);
                d.stopping.remove(&pid);
                d.active_launch = None;
            }
            emit_status(app);
            return Err(format!("Ошибка ожидания процесса: {error}"));
        }
    }

    // begin_exit выставляет флаг до ожидания gate. Не продолжаем startup, если
    // shutdown начался во время ранней проверки.
    if runtime_shutting_down(app) {
        kill_pid_async(pid).await;
        {
            let state = app.state::<AppState>();
            let mut d = state.dpi.lock_recover();
            d.procs.remove(&pid);
            d.stopping.remove(&pid);
            d.active_launch = None;
        }
        emit_status(app);
        return Err("Запуск winws отменён: приложение завершает работу.".to_string());
    }

    // Монитор завершения: обновляет UI и логирует крах.
    let app_mon = app.clone();
    let cat_mon = category.to_string();
    tokio::spawn(async move {
        let status = child.wait().await;
        let code = status.ok().and_then(|s| s.code()).unwrap_or(-1);
        let lived = started.elapsed().as_millis();

        let (intentional, unexpected_owned_exit, reliability_owner) = {
            let state = app_mon.state::<AppState>();
            let mut d = state.dpi.lock_recover();
            let owned = d
                .procs
                .get(&pid)
                .is_some_and(|proc| proc.generation == generation);
            if owned {
                d.procs.remove(&pid);
            }
            let intentional = d.stopping.remove(&pid);
            if owned && !intentional {
                // The exact launch snapshot is no longer true once any Legacy
                // process exits unexpectedly. Remaining processes stay owned,
                // but generation-aware operations must re-read their state.
                d.active_launch = None;
            }
            let unexpected_owned_exit = owned && !intentional;
            let reliability_owner = unexpected_owned_exit
                .then(|| crate::legacy_reliability::status::current_owner(&app_mon))
                .flatten();
            (intentional, unexpected_owned_exit, reliability_owner)
        };

        if let Some(owner) = reliability_owner {
            crate::legacy_reliability::status::publish_blind_if_owner(&app_mon, owner);
        }

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
            // Нативное уведомление: обход отвалился без нашего участия.
            util::notify_throttled(
                &app_mon,
                "down",
                "Obsession — обход прерван",
                &format!("Процесс обхода «{cat_mon}» неожиданно завершился. Возможно, защита не работает."),
            );
        }
        emit_status(&app_mon);
        if unexpected_owned_exit {
            invalidate_legacy_reliability_after_unexpected_exit(&app_mon, generation).await;
        }
    });

    Ok(pid)
}

/// Старый мост `домен → категория` для совместимости Legacy Brain. Новый
/// observe-only контур использует единый TargetRegistry с longest-suffix.
/// Если домен встречается в нескольких категориях — первая по порядку
/// `configs` (детерминизм). Нужен только для отложенной миграции Brain.
#[cfg(windows)]
#[allow(dead_code)]
pub fn collect_hostlist_by_category(
    app: &AppHandle,
    configs: &[(String, String)],
) -> std::collections::HashMap<String, String> {
    let st = app.state::<AppState>();
    let base = st.paths.base_dir.clone();
    let mut map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for (category, config_file) in configs {
        let conf = st.paths.config_path(category, config_file);
        let Ok(text) = std::fs::read_to_string(&conf) else {
            continue;
        };
        for cap in HOSTLIST_RE.captures_iter(&text) {
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

fn collect_category_hostlist(app: &AppHandle, selections: &[(String, String)]) -> Vec<String> {
    let lists_dir = app.state::<AppState>().paths.lists_dir();
    let mut domains = std::collections::BTreeSet::new();
    for (category, _) in selections {
        let path = lists_dir.join(format!("{category}.txt"));
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        for line in content.lines() {
            let domain = line.trim();
            if !domain.is_empty() && !domain.starts_with('#') {
                domains.insert(domain.to_ascii_lowercase());
            }
        }
    }
    domains.into_iter().collect()
}

/// Compatibility Eyes path for Zapret2 Adaptive. It intentionally keeps the
/// existing TCP/443 observation contract while remaining isolated from Legacy
/// Brain and Legacy Reliability Manager.
#[cfg(windows)]
fn start_zapret2_eyes(app: &AppHandle, hostlist: Vec<String>) {
    let runtime = {
        let state = app.state::<AppState>();
        let dpi = state.dpi.lock_recover();
        matches!(
            dpi.active_launch.as_ref(),
            Some(DpiLaunchSpec::Zapret2 { .. })
        )
        .then(|| {
            let dll = state
                .paths
                .winws2_path()
                .parent()
                .map(|dir| dir.join("WinDivert.dll"))
                .unwrap_or_else(|| state.paths.bin_dir().join("WinDivert.dll"));
            (dll, dpi.generation)
        })
    };
    let Some((dll, eyes_generation)) = runtime else {
        util::emit_log(
            app,
            "warn",
            "eyes",
            "Zapret2 Eyes не запущены: runtime fence уже закрыт",
        );
        return;
    };
    if !dll.exists() {
        util::emit_log(
            app,
            "warn",
            "eyes",
            "WinDivert.dll не найдена — Глаза не стартуют",
        );
        return;
    }
    let n = hostlist.len();
    let cfg = crate::eyes::Config {
        hostlist,
        ..Default::default()
    };
    let app_cb = app.clone();
    match crate::eyes::start(&dll, cfg, move |obs| {
        let current = {
            let state = app_cb.state::<AppState>();
            let dpi = state.dpi.lock_recover();
            dpi.is_current_generation(eyes_generation)
                && matches!(
                    dpi.active_launch.as_ref(),
                    Some(DpiLaunchSpec::Zapret2 { .. })
                )
        };
        if !current {
            return;
        }
        let line = format!(
            "{:?} {} :{} — {}",
            obs.verdict, obs.domain, obs.local_port, obs.evidence
        );
        // Дублируем в stderr процесса — видно в консоли dev-запуска для отладки.
        eprintln!("[eyes] {line}");
        util::emit_log(&app_cb, "info", "eyes", &line);
        // Сырое наблюдение во фронт (debug-читалка Глаз).
        let _ = app_cb.emit("eyes://observation", &obs);
        let adaptive_input = {
            let st = app_cb.state::<AppState>();
            let guard = st.adaptive.lock().ok();
            guard.and_then(|value| value.as_ref().map(|handle| handle.input.clone()))
        };
        if let Some(input) = adaptive_input {
            let _ = input.try_observation(obs.domain.clone(), obs.verdict, eyes_generation);
        }
    }) {
        Ok(handle) => {
            let st = app.state::<AppState>();
            *st.eyes.lock_recover() = Some(handle);
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

#[cfg(windows)]
fn start_legacy_eyes(
    app: &AppHandle,
    registry: std::sync::Arc<crate::legacy_reliability::target_registry::TargetRegistry>,
    envelope: crate::legacy_reliability::contracts::EventEnvelope,
    lane_generations: std::sync::Arc<
        std::collections::BTreeMap<String, crate::legacy_reliability::contracts::LaneGeneration>,
    >,
    ingress: crate::legacy_reliability::ingress::LegacyIngress,
) -> Result<(), String> {
    let dll = app
        .state::<AppState>()
        .paths
        .bin_dir()
        .join("WinDivert.dll");
    if !dll.exists() {
        return Err("WinDivert.dll не найдена — Legacy Eyes не стартуют".to_string());
    }

    let hostlist = registry
        .active_target_suffixes()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let capture_plan = registry
        .active_capture_plan()
        .map_err(|error| format!("Legacy active capture plan недоступен: {error}"))?;
    if hostlist.is_empty() || capture_plan.is_empty() {
        return Err(
            "Legacy TargetRegistry не содержит активных доменов или TCP capture plan".to_string(),
        );
    }
    let target_count = hostlist.len();
    let cfg = crate::eyes::Config {
        hostlist,
        working_signal_mode: crate::eyes::WorkingSignalMode::StrictTls,
        ..Default::default()
    };
    let counters = ingress.counters();
    let registry_cb = std::sync::Arc::clone(&registry);
    let lanes_cb = std::sync::Arc::clone(&lane_generations);
    let ingress_cb = ingress.clone();
    let handle =
        crate::eyes::start_legacy(&dll, cfg, &capture_plan, counters, move |observation| {
            if let Ok(adapted) = crate::legacy_reliability::adapter::adapt_observation(
                observation,
                envelope,
                &registry_cb,
                &lanes_cb,
            ) {
                let _ = ingress_cb.try_flow(adapted.event);
            }
        })?;
    *app.state::<AppState>().eyes.lock_recover() = Some(handle);
    util::emit_log(
        app,
        "info",
        "legacy-reliability",
        &format!(
            "Наблюдатель Legacy запущен: целей={target_count}, диапазонов TCP={}",
            capture_plan.tcp_ranges().len()
        ),
    );
    Ok(())
}

/// Останавливает наблюдателя, если запущен.
#[cfg(windows)]
fn stop_eyes(app: &AppHandle) -> Vec<crate::dpi_supervisor::WorkerStopOutcome> {
    let handle = app.state::<AppState>().eyes.lock_recover().take();
    if let Some(h) = handle {
        let outcomes = h.stop_bounded(EYES_STOP_TIMEOUT);
        let clean = outcomes
            .iter()
            .all(|outcome| outcome.state == crate::dpi_supervisor::WorkerStopState::Joined);
        util::emit_log(
            app,
            if clean { "info" } else { "warn" },
            "eyes",
            if clean {
                "Наблюдатель остановлен"
            } else {
                "Наблюдатель превысил bounded stop deadline; teardown продолжен"
            },
        );
        if !clean {
            util::emit_log(app, "warn", "eyes", &format!("eyes_stop={outcomes:?}"));
        }
        outcomes
    } else {
        Vec::new()
    }
}

/// Останавливает все свои DPI-процессы и ждёт подтверждения teardown.
/// DNS не сбрасывается здесь: host-mapping paths вызывают `flush_dns` условно.
pub async fn stop_all(app: &AppHandle) {
    // Invalidate the public owner before manager shutdown can emit Stopped.
    crate::legacy_reliability::status::publish(
        app,
        crate::legacy_reliability::status::LegacyReliabilityStatus::inactive(),
    );
    // Close the Legacy session fence before Eyes teardown. Late callbacks can
    // still race with WinDivert shutdown, but they no longer have a live sink.
    let legacy_manager = app.state::<AppState>().legacy_manager.lock_recover().take();
    if let Some(manager) = legacy_manager {
        manager.shutdown().await;
    }

    #[cfg(windows)]
    let eyes_clean = {
        let app2 = app.clone();
        match tauri::async_runtime::spawn_blocking(move || stop_eyes(&app2)).await {
            Ok(outcomes) => outcomes.iter().all(|outcome| outcome.is_clean()),
            Err(error) => {
                util::emit_log(
                    app,
                    "warn",
                    "eyes",
                    &format!("bounded eyes stop worker failed: {error}"),
                );
                false
            }
        }
    };
    #[cfg(not(windows))]
    let eyes_clean = true;

    let processes = {
        let state = app.state::<AppState>();
        let mut d = state.dpi.lock_recover();
        d.advance_generation();
        let processes = d
            .procs
            .values()
            .map(|process| crate::dpi_supervisor::OwnedProcess {
                pid: process.pid,
                identity: process.process_identity,
            })
            .collect::<Vec<_>>();
        for process in &processes {
            d.stopping.insert(process.pid);
        }
        d.procs.clear();
        d.active_launch = None;
        processes
    };

    let process_outcomes = stop_owned_processes_async(processes).await;
    let processes_clean = process_outcomes
        .iter()
        .all(|outcome| outcome.original_exited());
    for outcome in &process_outcomes {
        if !outcome.original_exited() {
            util::emit_log(
                app,
                "warn",
                "dpi",
                &format!(
                    "process teardown pid={} state={:?}",
                    outcome.pid, outcome.state
                ),
            );
        }
    }
    emit_status(app);

    // Sleep остаётся только fallback, когда exit/handle evidence неполно.
    if !(eyes_clean && processes_clean) {
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// Запускает Zapret2 (winws2) для набора категорий одним процессом с N профилями.
/// `selections` = (категория, strategy_id); пустой/неизвестный id → мягчайшая
/// стратегия категории из пака. Возвращает PID. Целостность пака проверяется
/// (`load_pack`) — на непроверенном Lua движок не стартует.
pub async fn start_zapret2(
    app: &AppHandle,
    selections: &[(String, String)],
) -> Result<u32, String> {
    let overrides =
        crate::adaptive_strategy::runtime::confirmed_overrides_for_current_network(app, selections)
            .await;
    start_zapret2_with_overrides(app, selections, &overrides).await
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Zapret2ProfileDescriptor {
    pub category: String,
    pub profile_id: String,
    pub source: String,
    pub transport: String,
    pub ports: String,
    pub hostlist: Option<String>,
    pub ipset: Option<String>,
    pub candidate_id: Option<String>,
    pub verification: String,
    pub trust: String,
    pub evidence_source: Option<String>,
    pub source_candidate_id: Option<String>,
    pub source_category: Option<String>,
    pub source_transport: Option<String>,
    pub recommendation_reason: Option<String>,
    pub data_plane: bool,
    pub broad_ipset: bool,
}

pub(crate) fn describe_zapret2_profiles(
    app: &AppHandle,
    selections: &[(String, String)],
    adaptive_overrides: &BTreeMap<String, StrategyCandidate>,
    adaptive_entries: &BTreeMap<String, crate::adaptive_strategy::cache::AdaptiveCacheEntry>,
) -> Result<Vec<Zapret2ProfileDescriptor>, String> {
    let state = app.state::<AppState>();
    let pack = crate::dpi_engine::load_pack(&state.paths.strategy_pack_dir("builtin"))?;
    let level = state.settings.lock_recover().zapret2_level;
    let mut descriptors = Vec::new();

    for (category, _) in selections {
        let Some(category_value) =
            crate::adaptive_strategy::dsl::AdaptiveCategory::from_key(category)
        else {
            continue;
        };
        let strategies = pack.profiles_for(category, level);
        let adaptive = [StrategyTransport::Tls, StrategyTransport::Quic]
            .into_iter()
            .filter_map(|transport| {
                adaptive_overrides.get(&override_key(category_value, transport))
            })
            .collect::<Vec<_>>();

        for candidate in &adaptive {
            let cache_key = override_key(category_value, candidate.transport);
            let metadata = adaptive_entries
                .get(&cache_key)
                .filter(|entry| entry.candidate_id == candidate.candidate_id());
            let control = strategies.iter().find(|strategy| {
                strategy.ipset.is_none()
                    && match candidate.transport {
                        StrategyTransport::Tls => {
                            strategy.transports.is_empty()
                                || strategy.transports.iter().any(|value| value == "tcp")
                        }
                        StrategyTransport::Quic => {
                            strategy.transports.iter().any(|value| value == "udp")
                        }
                    }
            });
            descriptors.push(Zapret2ProfileDescriptor {
                category: category.clone(),
                profile_id: candidate.candidate_id(),
                source: "adaptive".into(),
                transport: candidate.transport.as_key().into(),
                ports: "443".into(),
                hostlist: control
                    .and_then(|strategy| strategy.hostlist.clone())
                    .or_else(|| Some(format!("{category}.txt"))),
                ipset: None,
                candidate_id: Some(candidate.candidate_id()),
                verification: match metadata.map(|entry| entry.trust) {
                    Some(crate::adaptive_strategy::cache::StrategyTrust::Confirmed) => {
                        "verified".into()
                    }
                    Some(crate::adaptive_strategy::cache::StrategyTrust::Recommended) => {
                        "recommended".into()
                    }
                    _ => "not_actively_verified".into(),
                },
                trust: metadata
                    .map(|entry| entry.trust.as_key())
                    .unwrap_or("prepared")
                    .into(),
                evidence_source: metadata.and_then(|entry| entry.evidence_source.clone()),
                source_candidate_id: metadata.and_then(|entry| entry.source_candidate_id.clone()),
                source_category: metadata
                    .and_then(|entry| entry.source_category)
                    .map(|category| category.as_key().to_string()),
                source_transport: metadata
                    .and_then(|entry| entry.source_transport)
                    .map(|transport| transport.as_key().to_string()),
                recommendation_reason: metadata
                    .and_then(|entry| entry.recommendation_reason.clone()),
                data_plane: false,
                broad_ipset: false,
            });
        }

        for strategy in strategies {
            let replaced = strategy.ipset.is_none()
                && adaptive.iter().any(|candidate| match candidate.transport {
                    StrategyTransport::Tls => {
                        strategy.transports.is_empty()
                            || strategy.transports.iter().any(|value| value == "tcp")
                    }
                    StrategyTransport::Quic => {
                        strategy.transports.iter().any(|value| value == "udp")
                    }
                });
            if replaced {
                continue;
            }
            let profile =
                crate::dpi_engine::zapret2::profile_from_strategy_def(&strategy, None, None);
            let transport = if strategy.filter_l7.iter().any(|value| value == "quic") {
                "quic"
            } else if strategy.filter_l7.iter().any(|value| value == "tls") {
                "tls"
            } else if strategy.filter_l7.iter().any(|value| value == "http") {
                "http"
            } else if profile.filter_udp.is_some() {
                "udp"
            } else {
                "tcp"
            };
            let ports = profile
                .filter_tcp
                .or(profile.filter_udp)
                .unwrap_or_else(|| "443".into());
            let data_plane = strategy.ipset.is_some();
            let hostlist = strategy
                .hostlist
                .clone()
                .or_else(|| strategy.ipset.is_none().then(|| format!("{category}.txt")));
            descriptors.push(Zapret2ProfileDescriptor {
                category: category.clone(),
                profile_id: strategy.id,
                source: "builtin".into(),
                transport: transport.into(),
                ports,
                hostlist,
                broad_ipset: strategy.ipset.as_deref() == Some("ipset-gaming.txt"),
                ipset: strategy.ipset,
                candidate_id: None,
                verification: if data_plane {
                    "not_actively_verified".into()
                } else {
                    "baseline".into()
                },
                trust: "prepared".into(),
                evidence_source: None,
                source_candidate_id: None,
                source_category: None,
                source_transport: None,
                recommendation_reason: None,
                data_plane,
            });
        }
    }
    Ok(descriptors)
}
/// Вариант запуска Zapret2 с проверенными DSL-overrides. Override заменяет
/// только свой транспорт выбранной категории; противоположный транспорт и все
/// профили остальных категорий остаются из целостного bundled Strategy Pack.
pub(crate) async fn start_zapret2_with_overrides(
    app: &AppHandle,
    selections: &[(String, String)],
    adaptive_overrides: &BTreeMap<String, StrategyCandidate>,
) -> Result<u32, String> {
    use crate::dpi_engine::{load_pack, zapret2};

    // Backend-инвариант: даже прямой вызов вне UI не должен оставить Legacy и
    // Zapret2 одновременно на пересекающемся трафике.
    stop_all(app).await;
    if runtime_shutting_down(app) {
        return Err("Запуск Zapret2 отменён: приложение завершает работу.".to_string());
    }

    let (winws2, pack_dir, base, lists_dir, level) = {
        let state = app.state::<AppState>();
        let level = state.settings.lock_recover().zapret2_level;
        (
            state.paths.winws2_path(),
            state.paths.strategy_pack_dir("builtin"),
            state.paths.base_dir.clone(),
            state.paths.lists_dir(),
            level,
        )
    };
    crate::dpi_engine::resources::validate_engine_resources(&base, "zapret2")
        .map_err(|e| format!("Zapret2 не прошёл проверку ресурсов: {e}"))?;
    if !winws2.exists() {
        return Err(format!("winws2.exe не найден: {}", winws2.display()));
    }

    // Загрузка + проверка целостности встроенного пака (SHA-256 всех lua).
    let pack = load_pack(&pack_dir)?;

    // Абсолютизируем pack/hostlist-пути, чтобы argv не зависел от cwd.
    let abs = |rel: &str| pack_dir.join(rel).to_string_lossy().replace('\\', "/");

    let mut profiles: Vec<zapret2::Zapret2Profile> = Vec::new();
    let mut lua_init = vec![abs(zapret2::LUA_LIB)];
    for (category, _selected_config) in selections {
        let selected_profiles = pack.profiles_for(category, level);
        if selected_profiles.is_empty() {
            util::emit_log(
                app,
                "warn",
                "dpi",
                &format!("[zapret2] нет стратегий для категории {category} — пропуск"),
            );
            continue;
        }

        let adaptive = [StrategyTransport::Tls, StrategyTransport::Quic]
            .into_iter()
            .filter_map(|transport| {
                adaptive_overrides.get(&override_key(
                    match category.as_str() {
                        "discord" => crate::adaptive_strategy::dsl::AdaptiveCategory::Discord,
                        "youtube_twitch" => {
                            crate::adaptive_strategy::dsl::AdaptiveCategory::YoutubeTwitch
                        }
                        "gaming" => crate::adaptive_strategy::dsl::AdaptiveCategory::Gaming,
                        _ => return None,
                    },
                    transport,
                ))
            })
            .collect::<Vec<_>>();
        for candidate in &adaptive {
            if candidate.category.as_key() != category {
                return Err(format!(
                    "Adaptive override {} не соответствует категории {category}",
                    candidate.candidate_id()
                ));
            }
            let control = selected_profiles
                .iter()
                .find(|strategy| {
                    strategy.ipset.is_none()
                        && match candidate.transport {
                            StrategyTransport::Tls => {
                                strategy.transports.is_empty()
                                    || strategy.transports.iter().any(|value| value == "tcp")
                            }
                            StrategyTransport::Quic => {
                                strategy.transports.iter().any(|value| value == "udp")
                            }
                        }
                })
                .ok_or_else(|| {
                    format!(
                        "Adaptive override {} не нашёл control profile категории {category}",
                        candidate.candidate_id()
                    )
                })?;
            let hostlist = resolve_strategy_hostlist(&lists_dir, category, control)?
                .ok_or_else(|| format!("Adaptive override {category} требует hostlist"))?;
            let profile =
                crate::adaptive_strategy::compiler::compile(candidate, Path::new(&hostlist))
                    .map_err(|error| error.to_string())?;
            zapret2::validate_profile_scope(&profile)?;
            util::emit_log(
                app,
                "info",
                "dpi",
                &format!(
                    "[zapret2] {category} → adaptive {}",
                    candidate.candidate_id()
                ),
            );
            profiles.push(profile);
        }

        for strategy in selected_profiles {
            let replaced_transport = strategy.ipset.is_none()
                && adaptive.iter().any(|candidate| match candidate.transport {
                    StrategyTransport::Tls => {
                        strategy.transports.is_empty()
                            || strategy.transports.iter().any(|value| value == "tcp")
                    }
                    StrategyTransport::Quic => {
                        strategy.transports.iter().any(|value| value == "udp")
                    }
                });
            if replaced_transport {
                continue;
            }
            if !strategy.lua.is_empty() {
                let lua = abs(&strategy.lua);
                if !lua_init.contains(&lua) {
                    lua_init.push(lua);
                }
            }
            let hostlist = resolve_strategy_hostlist(&lists_dir, category, &strategy)?;
            let ipset = resolve_strategy_ipset(&lists_dir, &strategy)?;
            let profile = zapret2::profile_from_strategy_def(&strategy, hostlist, ipset);
            zapret2::validate_profile_scope(&profile)?;
            util::emit_log(
                app,
                "info",
                "dpi",
                &format!("[zapret2] {category} → профиль {}", strategy.id),
            );
            profiles.push(profile);
        }
    }

    if profiles.is_empty() {
        return Err("Zapret2: не собрано ни одного профиля".to_string());
    }

    let blobs = pack
        .manifest
        .blobs
        .iter()
        .map(|blob| zapret2::BlobArg {
            name: blob.name.clone(),
            path: abs(&blob.path),
        })
        .collect();
    let wf_tcp_out = zapret2::capture_ports(&profiles, true);
    let wf_udp_out = zapret2::capture_ports(&profiles, false);
    let invocation = zapret2::Zapret2Invocation {
        wf_tcp_out,
        wf_udp_out,
        lua_init,
        blobs,
        profiles,
    };
    let profile_names = invocation
        .profiles
        .iter()
        .map(|profile| profile.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let args = zapret2::build_winws2_args(&invocation);
    util::emit_log(
        app,
        "info",
        "dpi",
        &format!(
            "[zapret2] pack {} v{}; профили [{}]; capture tcp={} udp={}",
            pack.manifest.pack_id,
            pack.manifest.pack_version,
            profile_names,
            invocation.wf_tcp_out.as_deref().unwrap_or("off"),
            invocation.wf_udp_out.as_deref().unwrap_or("off")
        ),
    );
    util::emit_log(
        app,
        "info",
        "dpi",
        &format!("[zapret2] argv: {}", args.join(" ")),
    );

    let mut std_cmd = util::std_command(&winws2);
    std_cmd
        .args(&args)
        .current_dir(&base)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut cmd = TokioCommand::from(std_cmd);
    cmd.kill_on_drop(false);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Не удалось запустить winws2: {e}"))?;
    let pid = match child.id() {
        Some(pid) => pid,
        None => {
            let _ = child.kill().await;
            return Err("winws2 не вернул PID".to_string());
        }
    };
    let process_identity = crate::dpi_supervisor::capture_process_identity(pid);

    let generation = {
        let state = app.state::<AppState>();
        let mut d = state.dpi.lock_recover();
        let generation = d.generation;
        d.procs.insert(
            pid,
            DpiProc {
                pid,
                category: "zapret2".to_string(),
                config_file: "beta".to_string(),
                generation,
                engine: "zapret2".to_string(),
                process_identity,
            },
        );
        d.active_launch = Some(DpiLaunchSpec::Zapret2 {
            selections: selections.to_vec(),
            adaptive_overrides: adaptive_overrides.clone(),
        });
        generation
    };
    emit_status(app);

    let (readiness_tx, readiness_rx) = tokio::sync::mpsc::unbounded_channel();
    if let Some(out) = child.stdout.take() {
        spawn_reader(
            app.clone(),
            out,
            "zapret2".to_string(),
            "info",
            Some(readiness_tx.clone()),
        );
    }
    if let Some(err) = child.stderr.take() {
        spawn_reader(
            app.clone(),
            err,
            "zapret2".to_string(),
            "error",
            Some(readiness_tx.clone()),
        );
    }
    drop(readiness_tx);

    let started = Instant::now();
    let readiness = crate::dpi_supervisor::wait_for_readiness(
        async { child.wait().await.map(|status| status.code()) },
        readiness_rx,
        Duration::from_millis(500),
    )
    .await;
    match readiness {
        Ok(crate::dpi_supervisor::ReadinessState::Marker) => {}
        Ok(crate::dpi_supervisor::ReadinessState::BoundedFallback) => {
            util::emit_log(
                app,
                "debug",
                "dpi",
                "[zapret2] startup marker не получен; применён bounded fallback",
            );
        }
        Err(crate::dpi_supervisor::ReadinessFailure::Exited(code)) => {
            {
                let state = app.state::<AppState>();
                let mut d = state.dpi.lock_recover();
                d.procs.remove(&pid);
                d.stopping.remove(&pid);
                d.active_launch = None;
            }
            emit_status(app);
            let hint = match code.map(|value| value as u32) {
                Some(0xC000_0135) => {
                    "Не найдена обязательная DLL: проверьте cygwin1.dll и WinDivert.dll рядом с winws2.exe."
                }
                Some(0xC000_007B) => {
                    "DLL или исполняемый файл имеют несовместимую архитектуру."
                }
                _ => "Проверьте Lua/аргументы и совместимость WinDivert.",
            };
            return Err(format!(
                "winws2 завершился сразу после запуска (код {code:?}). {hint}"
            ));
        }
        Err(crate::dpi_supervisor::ReadinessFailure::WaitFailed(error)) => {
            kill_pid_async(pid).await;
            {
                let state = app.state::<AppState>();
                let mut d = state.dpi.lock_recover();
                d.procs.remove(&pid);
                d.stopping.remove(&pid);
                d.active_launch = None;
            }
            emit_status(app);
            return Err(format!("Ошибка ожидания winws2: {error}"));
        }
    }

    if runtime_shutting_down(app) {
        kill_pid_async(pid).await;
        {
            let state = app.state::<AppState>();
            let mut d = state.dpi.lock_recover();
            d.procs.remove(&pid);
            d.stopping.remove(&pid);
            d.active_launch = None;
        }
        emit_status(app);
        return Err("Запуск winws2 отменён: приложение завершает работу.".to_string());
    }

    #[cfg(windows)]
    {
        let hostlist = collect_category_hostlist(app, selections);
        start_zapret2_eyes(app, hostlist);
    }

    // Монитор краха: обновляет UI + сигналит (аналог winws-монитора).
    let app_mon = app.clone();
    tokio::spawn(async move {
        let status = child.wait().await;
        let code = status.ok().and_then(|s| s.code()).unwrap_or(-1);
        let lived = started.elapsed().as_millis();
        let fallback = {
            let state = app_mon.state::<AppState>();
            let mut d = state.dpi.lock_recover();
            d.detach_unexpected_zapret2(pid, generation)
        };
        let Some(fallback) = fallback else {
            emit_status(&app_mon);
            return;
        };

        // Временный adaptive candidate обязан откатиться к точному session
        // snapshot, а не к общему Legacy fallback. Generation в input не даёт
        // stale monitor перехватить уже подтверждённый/новый запуск.
        let adaptive_input = {
            let state = app_mon.state::<AppState>();
            let guard = state.adaptive.lock().ok();
            guard.and_then(|value| value.as_ref().map(|handle| handle.input.clone()))
        };
        if adaptive_input
            .as_ref()
            .is_some_and(|input| input.try_candidate_crashed(generation))
        {
            util::emit_log(
                &app_mon,
                "warn",
                "adaptive",
                "Временный candidate завершился — запрошен точный rollback.",
            );
            emit_status(&app_mon);
            return;
        }

        util::emit_log(
            &app_mon,
            "error",
            "dpi",
            &format!("[zapret2] winws2 неожиданно завершился через {lived}мс (код {code})."),
        );
        util::notify_throttled(
            &app_mon,
            "down",
            "Obsession — Zapret2 прерван",
            "Движок Zapret2 (Beta) неожиданно завершился.",
        );
        emit_status(&app_mon);

        let state = app_mon.state::<AppState>();
        let _gate = state.dpi_gate.lock().await;
        if runtime_shutting_down(&app_mon) {
            return;
        }
        let still_current = {
            let d = state.dpi.lock_recover();
            d.is_current_generation(generation) && d.procs.is_empty()
        };
        if !still_current {
            return;
        }
        if fallback.is_empty() {
            util::emit_log(
                &app_mon,
                "error",
                "dpi",
                "[zapret2] fallback невозможен: подтверждённый Legacy-набор отсутствует.",
            );
            util::notify_throttled(
                &app_mon,
                "error",
                "Obsession — обход остановлен",
                "Zapret2 завершился, а сохранённого Legacy-набора нет.",
            );
            return;
        }

        if let Err(error) = persist_engine_selection(&app_mon, "legacy") {
            util::emit_log(&app_mon, "error", "dpi", &error);
            return;
        }
        match start_many(&app_mon, &fallback).await {
            Ok(pids) if !pids.is_empty() => {
                util::emit_log(
                    &app_mon,
                    "warn",
                    "dpi",
                    "[zapret2] выполнен автоматический возврат к Zapret Legacy.",
                );
                util::notify_throttled(
                    &app_mon,
                    "warn",
                    "Obsession — возврат к Legacy",
                    "После сбоя Zapret2 восстановлен последний рабочий Legacy-набор.",
                );
            }
            Ok(_) => util::emit_log(
                &app_mon,
                "error",
                "dpi",
                "[zapret2] Legacy fallback не запустил ни одного процесса.",
            ),
            Err(error) => util::emit_log(
                &app_mon,
                "error",
                "dpi",
                &format!("[zapret2] ошибка Legacy fallback: {error}"),
            ),
        }
    });

    Ok(pid)
}

/// Запускает набор конфигов: сначала гасит предыдущие, затем стартует по очереди.
/// Порт из `StartDpiUseCase`. Возвращает список PID.
pub async fn start_many(app: &AppHandle, configs: &[(String, String)]) -> Result<Vec<u32>, String> {
    if runtime_shutting_down(app) {
        return Err("Приложение завершает работу.".to_string());
    }
    stop_all(app).await;
    if runtime_shutting_down(app) {
        return Err("Запуск отменён: приложение завершает работу.".to_string());
    }
    let mut started = Vec::new();
    let mut started_pairs = Vec::new();
    for (category, config_file) in configs {
        if runtime_shutting_down(app) {
            stop_all(app).await;
            return Err("Запуск отменён: приложение завершает работу.".to_string());
        }
        match start(app, category, config_file).await {
            Ok(pid) => {
                started.push(pid);
                started_pairs.push((category.clone(), config_file.clone()));
            }
            Err(e) => util::emit_log(app, "error", "dpi", &format!("{category}: {e}")),
        }
    }
    if started.is_empty() {
        return Err("Ни один DPI-процесс не был запущен".to_string());
    }

    #[cfg(windows)]
    let (session_id, sensor_generation, active_categories) = {
        use crate::legacy_reliability::contracts::{SensorGeneration, SessionId};
        let state = app.state::<AppState>();
        let active_categories = started_pairs
            .iter()
            .map(|(category, _)| category.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        (
            SessionId::new(state.legacy_session_revision.bump()),
            SensorGeneration::new(state.legacy_sensor_revision.bump()),
            active_categories,
        )
    };

    #[cfg(windows)]
    let (start_generation, exact_runtime) = {
        let state = app.state::<AppState>();
        let mut dpi = state.dpi.lock_recover();
        dpi.last_legacy_selection = started_pairs.clone();
        dpi.active_launch = Some(DpiLaunchSpec::Legacy {
            selections: started_pairs.clone(),
        });
        let generation = dpi.generation;
        let exact_runtime = dpi.owns_exact_legacy_runtime(generation, &started, &started_pairs);
        if !exact_runtime {
            dpi.active_launch = None;
        }
        if exact_runtime {
            crate::legacy_reliability::status::publish(
                app,
                crate::legacy_reliability::status::LegacyReliabilityStatus::starting(
                    active_categories.clone(),
                    session_id,
                    sensor_generation,
                ),
            );
        } else if dpi
            .procs
            .values()
            .any(|process| process.generation == generation && process.engine == "legacy")
        {
            crate::legacy_reliability::status::publish(
                app,
                crate::legacy_reliability::status::LegacyReliabilityStatus::blind(
                    active_categories.clone(),
                    session_id,
                    sensor_generation,
                ),
            );
        } else {
            crate::legacy_reliability::status::publish(
                app,
                crate::legacy_reliability::status::LegacyReliabilityStatus::inactive(),
            );
        }
        (generation, exact_runtime)
    };

    #[cfg(not(windows))]
    let start_generation = {
        let state = app.state::<AppState>();
        let mut dpi = state.dpi.lock_recover();
        dpi.last_legacy_selection = started_pairs.clone();
        dpi.active_launch = Some(DpiLaunchSpec::Legacy {
            selections: started_pairs.clone(),
        });
        dpi.generation
    };
    #[cfg(not(windows))]
    let _ = start_generation;

    #[cfg(windows)]
    if !exact_runtime {
        util::emit_log(
            app,
            "warn",
            "legacy-reliability",
            "Мониторинг Legacy не запущен: runtime изменился до фиксации сессии",
        );
        return Ok(started);
    }

    // Поднимаем Legacy Eyes после winws — драйвер WinDivert уже установлен,
    // NO_INSTALL-хендл откроется. Registry snapshots all eligible candidates,
    // while the running capture plan is limited to the selected configs so a
    // future broad candidate cannot turn Legacy Eyes into a watch-all sensor.
    #[cfg(windows)]
    {
        use crate::legacy_reliability::contracts::{
            EventEnvelope, LaneGeneration, LegacySessionContext,
        };
        use crate::legacy_reliability::status::{self, LegacyReliabilityStatus};

        let lane_generations = started_pairs
            .iter()
            .map(|(category, _)| (category.clone(), LaneGeneration::new(start_generation)))
            .collect::<std::collections::BTreeMap<_, _>>();

        tokio::time::sleep(Duration::from_millis(800)).await;
        if runtime_shutting_down(app) {
            stop_all(app).await;
            return Err("Запуск отменён: приложение завершает работу.".to_string());
        }
        if !app
            .state::<AppState>()
            .dpi
            .lock_recover()
            .owns_exact_legacy_runtime(start_generation, &started, &started_pairs)
        {
            util::emit_log(
                app,
                "warn",
                "legacy-reliability",
                "Observe-only Manager не запущен: Legacy runtime изменился до sensor startup",
            );
            status::publish_if_owned(
                app,
                session_id,
                sensor_generation,
                LegacyReliabilityStatus::blind(
                    active_categories.clone(),
                    session_id,
                    sensor_generation,
                ),
            );
            return Ok(started);
        }

        let paths = app.state::<AppState>().paths.clone();
        let reliability_log_root = paths.legacy_reliability_logs_dir();
        let selections = started_pairs.clone();
        let registry = tauri::async_runtime::spawn_blocking(move || {
            crate::legacy_reliability::registry_loader::load_target_registry(&paths, &selections)
        })
        .await;
        if !app
            .state::<AppState>()
            .dpi
            .lock_recover()
            .owns_exact_legacy_runtime(start_generation, &started, &started_pairs)
        {
            util::emit_log(
                app,
                "warn",
                "legacy-reliability",
                "Observe-only Manager не запущен: Legacy runtime изменился во время registry snapshot",
            );
            status::publish_if_owned(
                app,
                session_id,
                sensor_generation,
                LegacyReliabilityStatus::blind(
                    active_categories.clone(),
                    session_id,
                    sensor_generation,
                ),
            );
            return Ok(started);
        }
        match registry {
            Ok(Ok(registry)) => {
                let local_network = tokio::time::timeout(
                    Duration::from_secs(2),
                    crate::netid::resolve_local_read_only(),
                )
                .await
                .unwrap_or_default();
                let context = LegacySessionContext::new(
                    session_id,
                    active_categories.clone(),
                    local_network.fingerprint,
                );
                let registry = std::sync::Arc::new(registry);
                match crate::legacy_reliability::runtime::spawn(
                    context,
                    sensor_generation,
                    std::sync::Arc::clone(&registry),
                    lane_generations.clone(),
                    reliability_log_root,
                ) {
                    Ok(manager) => {
                        if !app
                            .state::<AppState>()
                            .dpi
                            .lock_recover()
                            .owns_exact_legacy_runtime(start_generation, &started, &started_pairs)
                        {
                            status::publish_if_owned(
                                app,
                                session_id,
                                sensor_generation,
                                LegacyReliabilityStatus::blind(
                                    active_categories.clone(),
                                    session_id,
                                    sensor_generation,
                                ),
                            );
                            manager.shutdown().await;
                            util::emit_log(
                                app,
                                "warn",
                                "legacy-reliability",
                                "Мониторинг Legacy не запущен: runtime изменился до запуска наблюдателя",
                            );
                            return Ok(started);
                        }
                        let envelope =
                            EventEnvelope::new(session_id, sensor_generation, registry.version());
                        let ingress = manager.ingress.clone();
                        let start_result = start_legacy_eyes(
                            app,
                            registry,
                            envelope,
                            std::sync::Arc::new(lane_generations),
                            ingress,
                        );
                        if !app
                            .state::<AppState>()
                            .dpi
                            .lock_recover()
                            .owns_exact_legacy_runtime(start_generation, &started, &started_pairs)
                        {
                            let eyes_started = start_result.is_ok();
                            if let Err(error) = &start_result {
                                util::emit_log(app, "error", "legacy-reliability", error);
                            }
                            status::publish_if_owned(
                                app,
                                session_id,
                                sensor_generation,
                                LegacyReliabilityStatus::blind(
                                    active_categories.clone(),
                                    session_id,
                                    sensor_generation,
                                ),
                            );
                            manager.shutdown().await;
                            if eyes_started {
                                let app_for_eyes = app.clone();
                                let _ = tauri::async_runtime::spawn_blocking(move || {
                                    stop_eyes(&app_for_eyes)
                                })
                                .await;
                            }
                            util::emit_log(
                                app,
                                "warn",
                                "legacy-reliability",
                                "Мониторинг Legacy остановлен: runtime изменился во время запуска наблюдателя",
                            );
                            return Ok(started);
                        }
                        if let Err(error) = &start_result {
                            use crate::legacy_reliability::contracts::{
                                EyeHealthCounters, EyeHealthState, HealthEvent,
                            };
                            let _ = manager.ingress.try_health(HealthEvent {
                                envelope,
                                state: EyeHealthState::Blind,
                                counters: EyeHealthCounters::default(),
                            });
                            status::publish_if_owned(
                                app,
                                session_id,
                                sensor_generation,
                                LegacyReliabilityStatus::blind(
                                    active_categories.clone(),
                                    session_id,
                                    sensor_generation,
                                ),
                            );
                            util::emit_log(app, "error", "legacy-reliability", error);
                        } else {
                            status::publish_snapshot_if_owned(app, &manager.snapshot());
                            manager.forward_public_status(app.clone());
                            util::emit_log(
                                app,
                                "success",
                                "legacy-reliability",
                                "Мониторинг Legacy запущен в режиме наблюдения; конфигурации не изменяются автоматически.",
                            );
                        }
                        *app.state::<AppState>().legacy_manager.lock_recover() = Some(manager);
                    }
                    Err(error) => {
                        status::publish_if_owned(
                            app,
                            session_id,
                            sensor_generation,
                            LegacyReliabilityStatus::blind(
                                active_categories.clone(),
                                session_id,
                                sensor_generation,
                            ),
                        );
                        util::emit_log(
                            app,
                            "error",
                            "legacy-reliability",
                            &format!("Не удалось создать observe-only Manager: {error:?}"),
                        );
                    }
                }
            }
            Ok(Err(error)) => {
                status::publish_if_owned(
                    app,
                    session_id,
                    sensor_generation,
                    LegacyReliabilityStatus::blind(
                        active_categories.clone(),
                        session_id,
                        sensor_generation,
                    ),
                );
                util::emit_log(
                    app,
                    "error",
                    "legacy-reliability",
                    &format!("TargetRegistry недоступен; Eyes отключены: {error}"),
                );
            }
            Err(error) => {
                status::publish_if_owned(
                    app,
                    session_id,
                    sensor_generation,
                    LegacyReliabilityStatus::blind(
                        active_categories,
                        session_id,
                        sensor_generation,
                    ),
                );
                util::emit_log(
                    app,
                    "error",
                    "legacy-reliability",
                    &format!("Сборка TargetRegistry прервана: {error}"),
                );
            }
        }
    }

    Ok(started)
}

/// Точный in-memory снимок активного DPI runtime. Вызывающий не должен держать
/// `dpi` std::Mutex через `.await`.
pub(crate) fn runtime_snapshot(app: &AppHandle) -> DpiRuntimeSnapshot {
    app.state::<AppState>()
        .dpi
        .lock_recover()
        .runtime_snapshot()
}

fn ensure_generation(app: &AppHandle, expected_generation: u64) -> Result<(), String> {
    let current = app.state::<AppState>().dpi.lock_recover().generation;
    if current != expected_generation {
        return Err(format!(
            "DPI runtime изменён другой операцией: expected generation {expected_generation}, current {current}"
        ));
    }
    Ok(())
}

/// Запускает candidate поверх исходного Zapret2 snapshot. Функция намеренно не
/// берёт `dpi_gate`: coordinator должен держать общие ворота на всём respawn.
pub(crate) async fn start_adaptive_candidate_locked(
    app: &AppHandle,
    original: &DpiRuntimeSnapshot,
    expected_generation: u64,
    category: &str,
    candidate: StrategyCandidate,
) -> Result<u64, String> {
    ensure_generation(app, expected_generation)?;
    let DpiLaunchSpec::Zapret2 {
        selections,
        adaptive_overrides,
    } = original
        .launch
        .as_ref()
        .ok_or_else(|| "DPI runtime остановлен — adaptive search недоступен".to_string())?
    else {
        return Err("Adaptive search работает только поверх активного Zapret2".to_string());
    };
    if candidate.category.as_key() != category {
        return Err("Категория adaptive candidate не совпадает с запросом".to_string());
    }
    if !selections.iter().any(|(selected, _)| selected == category) {
        return Err(format!("Категория {category} не активна в DPI runtime"));
    }

    let mut overrides = adaptive_overrides.clone();
    overrides.insert(
        override_key(candidate.category, candidate.transport),
        candidate,
    );
    start_zapret2_with_overrides(app, selections, &overrides).await?;
    Ok(runtime_snapshot(app).generation)
}

/// Восстанавливает точный исходный launch spec только если никто не сменил DPI
/// generation после candidate-start. Возвращает generation восстановленного
/// runtime, чтобы следующая попытка продолжала generation-safe цепочку.
pub(crate) async fn restore_runtime_snapshot_locked(
    app: &AppHandle,
    original: &DpiRuntimeSnapshot,
    expected_generation: u64,
) -> Result<u64, String> {
    ensure_generation(app, expected_generation)?;
    match original.launch.as_ref() {
        Some(DpiLaunchSpec::Legacy { selections }) => {
            start_many(app, selections).await?;
        }
        Some(DpiLaunchSpec::Zapret2 {
            selections,
            adaptive_overrides,
        }) => {
            start_zapret2_with_overrides(app, selections, adaptive_overrides).await?;
        }
        None => stop_all(app).await,
    }
    Ok(runtime_snapshot(app).generation)
}

/// True, если пользователь запросил отмену теста (флаг в состоянии).
#[inline]
fn test_cancelled(app: &AppHandle) -> bool {
    app.state::<AppState>().test_cancel.load(Ordering::SeqCst)
}

/// Отменяет текущий тест: ставит флаг и убивает тестовый winws, чтобы проба
/// оборвалась немедленно. PID помечаем как `stopping` — монитор не сочтёт это
/// крахом. Вызывается БЕЗ ворот (тест их держит), поэтому трогает только атомик
/// и список процессов.
pub fn cancel_test(app: &AppHandle) {
    app.state::<AppState>()
        .test_cancel
        .store(true, Ordering::SeqCst);
    let pids: Vec<u32> = {
        let state = app.state::<AppState>();
        let mut d = state.dpi.lock_recover();
        let pids: Vec<u32> = d.procs.keys().copied().collect();
        for p in &pids {
            d.stopping.insert(*p);
        }
        pids
    };
    for pid in pids {
        kill_pid(pid);
    }
    util::emit_log(app, "info", "dpi", "Тест отменён пользователем");
}

/// Тестирует один конфиг: старт → проверка URL → стоп. Порт из `TestDpiConfigUseCase`.
/// Проверяет флаг отмены между этапами — по нему обрывается досрочно.
pub async fn test(app: &AppHandle, category: &str, config_file: &str) -> bool {
    // Сбрасываем флаг на входе: фронт не вызывает следующий тест после отмены,
    // поэтому сброс на старте каждого теста безопасен.
    app.state::<AppState>()
        .test_cancel
        .store(false, Ordering::SeqCst);

    stop_all(app).await;
    if test_cancelled(app) {
        return false;
    }
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
    if !test_cancelled(app) {
        for url in test_urls(category) {
            if test_cancelled(app) {
                break;
            }
            if crate::net::test_url(url, 3).await {
                connected = true;
                break;
            }
        }
    }

    {
        let state = app.state::<AppState>();
        state.dpi.lock_recover().stopping.insert(pid);
    }
    kill_pid_async(pid).await;
    stop_all(app).await;

    connected
}

/// Обнаруживает чужие/orphan-процессы winws в системе (не свои).
pub fn detect_orphaned(app: &AppHandle) -> Vec<u32> {
    let tracked: std::collections::HashSet<u32> = {
        let state = app.state::<AppState>();
        let set = state.dpi.lock_recover().procs.keys().copied().collect();
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
            for line in text.lines() {
                if let Some(cap) = ORPHAN_PID_RE.captures(line.trim()) {
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

/// Точечно убивает список orphan-PID winws (только указанные PID, не по имени
/// образа). В отличие от [`emergency_kill_all`], не трогает ЧУЖИЕ winws.exe —
/// например, параллельно запущенный другой инструмент обхода (Zapret/GoodbyeDPI).
/// Используется на старте для зачистки собственных зависших процессов.
pub fn kill_orphans(app: &AppHandle, pids: &[u32]) {
    if pids.is_empty() {
        return;
    }
    util::emit_log(
        app,
        "warn",
        "dpi",
        &format!(
            "Зачистка зависших winws.exe от прошлого запуска: PID [{}].",
            pids.iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    );
    for pid in pids {
        // /T добивает дерево на случай, если winws успел породить детей.
        let _ = util::std_command("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .output();
    }
    emit_status(app);
}

/// Крайняя мера: убивает ВСЕ winws.exe/winws2.exe в системе (включая чужие).
pub fn emergency_kill_all(app: &AppHandle) {
    crate::legacy_reliability::status::publish(
        app,
        crate::legacy_reliability::status::LegacyReliabilityStatus::inactive(),
    );
    util::emit_log(
        app,
        "warn",
        "dpi",
        "EMERGENCY: завершение ВСЕХ процессов winws.exe/winws2.exe в системе.",
    );
    #[cfg(windows)]
    stop_eyes(app);
    {
        let state = app.state::<AppState>();
        let mut d = state.dpi.lock_recover();
        d.advance_generation();
        let tracked: Vec<u32> = d.procs.keys().copied().collect();
        d.stopping.extend(tracked);
        d.procs.clear();
        d.active_launch = None;
        d.started_at_unix = None;
    }
    let _ = util::std_command("taskkill")
        .args(["/F", "/T", "/IM", crate::paths::WINWS_EXE])
        .output();
    let _ = util::std_command("taskkill")
        .args(["/F", "/T", "/IM", "winws2.exe"])
        .output();
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

/// Убивает PID вне tokio-воркера: блокирующий taskkill уходит в blocking-пул.
/// На async-путях под dpi_gate / в teardown нельзя блокировать воркер синхронным
/// внешним процессом (сериализует рантайм, тормозит выход).
async fn kill_pid_async(pid: u32) {
    let _ = tauri::async_runtime::spawn_blocking(move || kill_pid(pid)).await;
}

#[cfg(windows)]
async fn stop_owned_processes_async(
    processes: Vec<crate::dpi_supervisor::OwnedProcess>,
) -> Vec<crate::dpi_supervisor::ProcessStopOutcome> {
    if processes.is_empty() {
        return Vec::new();
    }
    let pids = processes
        .iter()
        .map(|process| process.pid)
        .collect::<Vec<_>>();
    match tauri::async_runtime::spawn_blocking(move || {
        crate::dpi_supervisor::stop_processes_bounded(
            processes,
            Duration::from_secs(5),
            std::sync::Arc::new(crate::dpi_supervisor::WindowsProcessControl),
        )
    })
    .await
    {
        Ok(outcomes) => outcomes,
        Err(_) => pids
            .into_iter()
            .map(|pid| crate::dpi_supervisor::ProcessStopOutcome {
                pid,
                state: crate::dpi_supervisor::ProcessStopState::ReaperFailed,
            })
            .collect(),
    }
}

#[cfg(not(windows))]
async fn stop_owned_processes_async(
    processes: Vec<crate::dpi_supervisor::OwnedProcess>,
) -> Vec<crate::dpi_supervisor::ProcessStopOutcome> {
    let mut outcomes = Vec::with_capacity(processes.len());
    for process in processes {
        kill_pid_async(process.pid).await;
        outcomes.push(crate::dpi_supervisor::ProcessStopOutcome {
            pid: process.pid,
            state: crate::dpi_supervisor::ProcessStopState::Exited,
        });
    }
    outcomes
}

/// Читает поток построчно и эмитит строки в лог UI.
fn spawn_reader<R>(
    app: AppHandle,
    reader: R,
    category: String,
    level: &'static str,
    readiness: Option<tokio::sync::mpsc::UnboundedSender<()>>,
) where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    let msg = line.trim();
                    if !msg.is_empty() {
                        if crate::dpi_supervisor::is_startup_marker(msg) {
                            if let Some(readiness) = readiness.as_ref() {
                                let _ = readiness.send(());
                            }
                        }
                        util::emit_log(&app, level, "dpi", &format!("[{category}] {msg}"));
                    }
                }
                Ok(None) => break, // EOF — процесс закрыл поток
                Err(e) => {
                    // Напр. невалидный UTF-8 в выводе winws: раньше остаток stdout
                    // молча проглатывался. Логируем и прекращаем чтение этого потока.
                    util::emit_log(
                        &app,
                        "warn",
                        "dpi",
                        &format!("[{category}] чтение вывода прервано: {e}"),
                    );
                    break;
                }
            }
        }
    });
}
#[cfg(test)]
mod tests {
    use super::*;

    fn temp_lists_dir() -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("obsession-zapret2-lists-{nonce}"));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn resolves_and_validates_runtime_lists() {
        let dir = temp_lists_dir();
        std::fs::write(dir.join("gaming-github.txt"), "github.com\nroblox.com\n").unwrap();
        std::fs::write(dir.join("ipset-gaming.txt"), "1.1.1.0/24\n2606:4700::/32\n").unwrap();

        let hostlist = resolve_list_binding(
            &dir,
            "gaming-github.txt",
            crate::lists_validate::ListKind::Domains,
            true,
        )
        .unwrap();
        let ipset = resolve_list_binding(
            &dir,
            "ipset-gaming.txt",
            crate::lists_validate::ListKind::Ipset,
            true,
        )
        .unwrap();
        assert!(hostlist.unwrap().ends_with("gaming-github.txt"));
        assert!(ipset.unwrap().ends_with("ipset-gaming.txt"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rejects_missing_empty_and_unsafe_runtime_lists() {
        let dir = temp_lists_dir();
        std::fs::write(dir.join("empty.txt"), "# comments only\n").unwrap();
        assert!(resolve_list_binding(
            &dir,
            "missing.txt",
            crate::lists_validate::ListKind::Domains,
            false,
        )
        .unwrap()
        .is_none());
        assert!(resolve_list_binding(
            &dir,
            "missing.txt",
            crate::lists_validate::ListKind::Domains,
            true,
        )
        .is_err());
        assert!(resolve_list_binding(
            &dir,
            "empty.txt",
            crate::lists_validate::ListKind::Domains,
            true,
        )
        .is_err());
        assert!(resolve_list_binding(
            &dir,
            "../outside.txt",
            crate::lists_validate::ListKind::Domains,
            true,
        )
        .is_err());

        std::fs::remove_dir_all(dir).unwrap();
    }
}
