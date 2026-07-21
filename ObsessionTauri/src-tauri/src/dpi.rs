//! Управление DPI-процессами winws.
//! Порт из `process_local_datasource.dart` + `dpi_provider.dart` + `dpi_usecases.dart`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as TokioCommand;

use crate::adaptive_strategy::dsl::{override_key, StrategyCandidate, StrategyTransport};
use crate::state::{
    AppState, DpiLaunchSpec, DpiProc, DpiRuntimeSnapshot, LegacyCategoryRuntimeSnapshot,
    LegacyProcessOwner,
};
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

pub(crate) const LEGACY_PROCESS_EXIT_EVENT: &str = "legacy-reliability-process-exit";

/// Exact process-supervisor event. The complete owner token makes it safe to
/// correlate a delayed exit even after Windows has reused the numeric PID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LegacyProcessExit {
    pub owner: LegacyProcessOwner,
    pub selection_index: usize,
    pub intentional: bool,
    pub exit_code: Option<i32>,
    pub lived_ms: u64,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct LegacyProcessExitPayload<'a> {
    category: &'a str,
    config_file: &'a str,
    config_fingerprint: &'a str,
    pid: u32,
    process_start_identity: u64,
    runtime_generation: u64,
    lane_generation: u64,
    intentional: bool,
    exit_code: Option<i32>,
    lived_ms: u64,
}

pub(crate) fn recovery_process_owner(
    owner: &LegacyProcessOwner,
) -> crate::legacy_reliability::contracts::ProcessOwner {
    use crate::legacy_reliability::contracts::{ConfigFingerprint, ProcessStartIdentity};

    crate::legacy_reliability::contracts::ProcessOwner {
        pid: owner.pid,
        process_start_identity: ProcessStartIdentity::new(owner.process_identity.get()),
        config_fingerprint: ConfigFingerprint::new(owner.config_fingerprint.clone()),
        lane_generation: owner.lane_generation,
    }
}

fn emit_legacy_process_exit(app: &AppHandle, event: &LegacyProcessExit) {
    let owner = &event.owner;
    let _ = app.emit(
        LEGACY_PROCESS_EXIT_EVENT,
        LegacyProcessExitPayload {
            category: &owner.category,
            config_file: &owner.config_file,
            config_fingerprint: &owner.config_fingerprint,
            pid: owner.pid,
            process_start_identity: owner.process_identity.get(),
            runtime_generation: owner.runtime_generation,
            lane_generation: owner.lane_generation.get(),
            intentional: event.intentional,
            exit_code: event.exit_code,
            lived_ms: event.lived_ms,
        },
    );

    if event.intentional {
        util::emit_log(
            app,
            "debug",
            "dpi",
            &format!(
                "[{}] exact Legacy process stopped: pid={}, lane_generation={}, config={}",
                owner.category,
                owner.pid,
                owner.lane_generation.get(),
                owner.config_file
            ),
        );
        return;
    }

    let message = if event.lived_ms < 2000 {
        format!(
            "[{}] winws умер через {}мс (код {:?}); pid={}, lane_generation={}, config={}",
            owner.category,
            event.lived_ms,
            event.exit_code,
            owner.pid,
            owner.lane_generation.get(),
            owner.config_file
        )
    } else {
        format!(
            "[{}] winws неожиданно завершился (код {:?}); pid={}, lane_generation={}, config={}",
            owner.category,
            event.exit_code,
            owner.pid,
            owner.lane_generation.get(),
            owner.config_file
        )
    };
    util::emit_log(app, "error", "dpi", &message);
    util::notify_throttled(
        app,
        "down",
        "Obsession — обход прерван",
        &format!(
            "Процесс обхода «{}» неожиданно завершился. Остальные категории продолжают работу.",
            owner.category
        ),
    );
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CrashRetryDecision {
    IgnoreIntentional,
    Exhausted,
    Schedule,
}

fn crash_retry_decision(
    intentional: bool,
    was_retry: bool,
    retry_rearmed_by_health: bool,
) -> CrashRetryDecision {
    if intentional {
        CrashRetryDecision::IgnoreIntentional
    } else if was_retry && !retry_rearmed_by_health {
        CrashRetryDecision::Exhausted
    } else {
        CrashRetryDecision::Schedule
    }
}

fn schedule_legacy_crash_retry(app: &AppHandle, event: &LegacyProcessExit) {
    let was_retry = app
        .state::<AppState>()
        .legacy_crash_retry_owners
        .lock_recover()
        .remove(&event.owner);
    let retry_rearmed_by_health = was_retry
        && app
            .state::<AppState>()
            .legacy_manager
            .lock_recover()
            .as_ref()
            .is_some_and(|manager| {
                manager.snapshot().lanes.iter().any(|lane| {
                    lane.category == event.owner.category
                        && lane.phase == crate::legacy_reliability::assessment::LanePhase::Healthy
                })
            });
    match crash_retry_decision(event.intentional, was_retry, retry_rearmed_by_health) {
        CrashRetryDecision::IgnoreIntentional => return,
        CrashRetryDecision::Exhausted => {}
        CrashRetryDecision::Schedule => {
            let app_retry = app.clone();
            let event = event.clone();
            tauri::async_runtime::spawn(async move {
                let mut deferrals = 0u8;
                let outcome = loop {
                    match crate::legacy_reliability::executor::retry_crashed_legacy_lane(
                        &app_retry, &event,
                    )
                    .await
                    {
                        Ok(crate::legacy_reliability::executor::CrashRetryRunOutcome::Deferred)
                            if deferrals < 3 =>
                        {
                            deferrals += 1;
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }
                        Ok(crate::legacy_reliability::executor::CrashRetryRunOutcome::Deferred) => {
                            break Err(
                                "same-config crash retry remained blocked by recovery".to_owned()
                            );
                        }
                        outcome => break outcome,
                    }
                };
                match outcome {
                    Ok(crate::legacy_reliability::executor::CrashRetryRunOutcome::Restarted(
                        owner,
                    )) => {
                        util::emit_log(
                            &app_retry,
                            "success",
                            "legacy-reliability",
                            &format!(
                                "[{}] same-config crash retry started: pid={}, lane_generation={}",
                                event.owner.category,
                                owner.pid,
                                owner.lane_generation.get()
                            ),
                        );
                        util::notify_throttled(
                            &app_retry,
                            "up",
                            "Obsession — обход восстановлен",
                            &format!(
                                "Категория «{}» перезапущена с тем же конфигом.",
                                event.owner.category
                            ),
                        );
                    }
                    Ok(
                        crate::legacy_reliability::executor::CrashRetryRunOutcome::AlreadyHandled,
                    ) => {
                        return;
                    }
                    Ok(crate::legacy_reliability::executor::CrashRetryRunOutcome::Deferred) => {
                        unreachable!("deferred crash retry is consumed by the bounded loop")
                    }
                    Err(error) => {
                        util::emit_log(
                            &app_retry,
                            "error",
                            "legacy-reliability",
                            &format!(
                                "[{}] same-config crash retry failed: {error}",
                                event.owner.category
                            ),
                        );
                        crate::legacy_reliability::status::publish_current_blind(&app_retry);
                    }
                }
                emit_status(&app_retry);
            });
            return;
        }
    }

    util::emit_log(
        app,
        "error",
        "legacy-reliability",
        &format!(
            "[{}] same-config crash retry exhausted for {}",
            event.owner.category, event.owner.config_file
        ),
    );
    crate::legacy_reliability::status::publish_current_blind(app);
}

#[derive(Clone, Debug)]
struct LegacyCompatibilityFence {
    pid: u32,
    process_identity: Option<crate::dpi_supervisor::ProcessIdentity>,
    runtime_generation: u64,
    category: String,
    config_file: String,
}

impl LegacyCompatibilityFence {
    fn from_process(process: &DpiProc) -> Self {
        Self {
            pid: process.pid,
            process_identity: process.process_identity,
            runtime_generation: process.generation,
            category: process.category.clone(),
            config_file: process.config_file.clone(),
        }
    }

    fn matches(&self, process: &DpiProc) -> bool {
        self.process_identity.is_some()
            && process.pid == self.pid
            && process.process_identity == self.process_identity
            && process.generation == self.runtime_generation
            && process.engine == "legacy"
            && process.category == self.category
            && process.config_file == self.config_file
    }
}

#[derive(Debug, PartialEq, Eq)]
enum LegacyExitFinalization {
    Ignored,
    Exact(LegacyProcessExit),
    Compatibility { intentional: bool },
}

fn finalize_exact_legacy_exit(
    dpi: &mut crate::state::DpiState,
    snapshot: &LegacyCategoryRuntimeSnapshot,
    exit_code: Option<i32>,
    lived_ms: u64,
) -> Option<LegacyProcessExit> {
    let removal = dpi.remove_exact_legacy_category(snapshot).ok()?;
    let removed = removal.process?;
    let owner = removed.exact_legacy_owner()?;
    if owner != snapshot.owner {
        return None;
    }
    let compatibility_intentional = dpi.stopping.remove(&owner.pid);
    Some(LegacyProcessExit {
        owner,
        selection_index: snapshot.selection_index,
        intentional: removal.intentional || compatibility_intentional,
        exit_code,
        lived_ms,
    })
}

/// Finalizes the process represented by this monitor and nothing else. Exact
/// Phase 3 owners go through the scoped DpiState API. The compatibility arm is
/// retained only for the short startup window before the initial registry has
/// attached lane/fingerprint metadata; it still fences PID start identity and
/// removes at most the matching category selection.
fn finalize_legacy_exit(
    dpi: &mut crate::state::DpiState,
    fence: &LegacyCompatibilityFence,
    exit_code: Option<i32>,
    lived_ms: u64,
) -> LegacyExitFinalization {
    if !dpi.is_current_generation(fence.runtime_generation) {
        return LegacyExitFinalization::Ignored;
    }
    let Some(process) = dpi
        .procs
        .get(&fence.pid)
        .filter(|process| fence.matches(process))
    else {
        return LegacyExitFinalization::Ignored;
    };

    if let Some(owner) = process.exact_legacy_owner() {
        let Ok(snapshot) = dpi.snapshot_legacy_category(&fence.category) else {
            return LegacyExitFinalization::Ignored;
        };
        if snapshot.owner != owner {
            return LegacyExitFinalization::Ignored;
        }
        return finalize_exact_legacy_exit(dpi, &snapshot, exit_code, lived_ms)
            .map(LegacyExitFinalization::Exact)
            .unwrap_or(LegacyExitFinalization::Ignored);
    }

    let selection_index = match dpi.active_launch.as_ref() {
        Some(DpiLaunchSpec::Legacy { selections }) => {
            let matches = selections
                .iter()
                .enumerate()
                .filter(|(_, (category, config_file))| {
                    category == &fence.category && config_file == &fence.config_file
                })
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            let [index] = matches.as_slice() else {
                return LegacyExitFinalization::Ignored;
            };
            Some(*index)
        }
        None => None,
        Some(DpiLaunchSpec::Zapret2 { .. }) => return LegacyExitFinalization::Ignored,
    };

    dpi.procs.remove(&fence.pid);
    if let Some(index) = selection_index {
        let Some(DpiLaunchSpec::Legacy { selections }) = dpi.active_launch.as_mut() else {
            return LegacyExitFinalization::Ignored;
        };
        selections.remove(index);
        if selections.is_empty() {
            dpi.active_launch = None;
        }
    }
    LegacyExitFinalization::Compatibility {
        intentional: dpi.stopping.remove(&fence.pid),
    }
}

fn prepare_legacy_launch_paths(
    app: &AppHandle,
    selections: &[(String, String)],
    migrate_existing: bool,
) -> Result<crate::legacy_reliability::autohost_isolation::IsolationPrepareResult, String> {
    let base = app.state::<AppState>().paths.base_dir.clone();
    let prepared = crate::legacy_reliability::autohost_isolation::prepare_launches_from_disk(
        &base,
        selections,
        migrate_existing,
    )
    .map_err(|error| format!("Legacy isolation preparation failed: {error}"))?;
    if migrate_existing && prepared.migration != Default::default() {
        util::emit_log(
            app,
            "info",
            "legacy-reliability",
            &format!("Legacy auto-hostlist isolation: {}", prepared.migration),
        );
    }
    Ok(prepared)
}

fn preflight_legacy_launch_paths(
    app: &AppHandle,
    selections: &[(String, String)],
) -> Result<(), String> {
    let base = app.state::<AppState>().paths.base_dir.clone();
    crate::legacy_reliability::autohost_isolation::preflight_launches_from_disk(&base, selections)
        .map_err(|error| format!("Legacy isolation preflight failed: {error}"))
}

fn prepared_legacy_config_path<'a>(
    prepared: &'a crate::legacy_reliability::autohost_isolation::IsolationPrepareResult,
    category: &str,
    config_file: &str,
) -> Result<&'a Path, String> {
    prepared
        .effective_paths
        .get(&(category.to_ascii_lowercase(), config_file.to_string()))
        .map(std::path::PathBuf::as_path)
        .ok_or_else(|| {
            format!("Legacy isolation did not produce a launch path for {category}/{config_file}")
        })
}

/// Запускает winws с уже подготовленным immutable/effective конфигом категории.
async fn start_prepared(
    app: &AppHandle,
    category: &str,
    config_file: &str,
    conf: &Path,
) -> Result<u32, String> {
    let (winws, base) = {
        let state = app.state::<AppState>();
        (state.paths.winws_path(), state.paths.base_dir.clone())
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
    let monitor_fence = {
        let state = app.state::<AppState>();
        let mut d = state.dpi.lock_recover();
        let generation = d.generation;
        let process = DpiProc {
            pid,
            category: category.to_string(),
            config_file: config_file.to_string(),
            generation,
            engine: "legacy".to_string(),
            process_identity,
            lane_generation: None,
            config_fingerprint: None,
        };
        let monitor_fence = LegacyCompatibilityFence::from_process(&process);
        d.procs.insert(pid, process);
        monitor_fence
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
        let code = status.ok().and_then(|s| s.code());
        let lived = started.elapsed().as_millis().min(u64::MAX as u128) as u64;

        let finalization = {
            let state = app_mon.state::<AppState>();
            let mut d = state.dpi.lock_recover();
            finalize_legacy_exit(&mut d, &monitor_fence, code, lived)
        };

        match finalization {
            LegacyExitFinalization::Ignored => return,
            LegacyExitFinalization::Exact(event) => {
                emit_legacy_process_exit(&app_mon, &event);
                schedule_legacy_crash_retry(&app_mon, &event);
            }
            LegacyExitFinalization::Compatibility { intentional: false } => {
                util::emit_log(
                    &app_mon,
                    "error",
                    "dpi",
                    &format!(
                        "[{cat_mon}] unfenced startup process завершился через {lived}мс (код {code:?})"
                    ),
                );
                util::notify_throttled(
                    &app_mon,
                    "down",
                    "Obsession — обход прерван",
                    &format!(
                        "Процесс обхода «{cat_mon}» неожиданно завершился. Остальные категории продолжают работу."
                    ),
                );
            }
            LegacyExitFinalization::Compatibility { intentional: true } => {}
        }
        emit_status(&app_mon);
    });

    Ok(pid)
}

/// Запускает одиночный winws. Основной multi-category путь заранее готовит
/// полный набор через [`start_many`]; этот wrapper нужен ручному config-test.
pub async fn start(app: &AppHandle, category: &str, config_file: &str) -> Result<u32, String> {
    let selections = vec![(category.to_string(), config_file.to_string())];
    let prepared = prepare_legacy_launch_paths(app, &selections, false)?;
    let conf = prepared_legacy_config_path(&prepared, category, config_file)?;
    start_prepared(app, category, config_file, conf).await
}

/// A scoped process that passed readiness but is not yet visible in DpiState.
/// The caller must either install it through one of the methods below or abort
/// it. Dropping the guard is fail-safe and requests termination.
pub(crate) struct PendingLegacyLane {
    app: AppHandle,
    process: DpiProc,
    child: Option<tokio::process::Child>,
    started: Instant,
}

pub(crate) struct InstalledLegacyLane {
    pub owner: LegacyProcessOwner,
    pub exit: tokio::sync::oneshot::Receiver<LegacyProcessExit>,
}

pub(crate) struct PendingLegacyInstallFailure {
    pending: Box<PendingLegacyLane>,
    message: String,
}

impl PendingLegacyInstallFailure {
    pub(crate) fn into_parts(self) -> (PendingLegacyLane, String) {
        (*self.pending, self.message)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PendingLegacyStopError {
    MissingExactOwner,
    MissingChild,
    MissingOutcome,
    Process(crate::dpi_supervisor::ProcessStopState),
}

impl std::fmt::Display for PendingLegacyStopError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingExactOwner => {
                formatter.write_str("pending Legacy lane has no exact owner token")
            }
            Self::MissingChild => formatter.write_str("pending Legacy lane lost its child handle"),
            Self::MissingOutcome => {
                formatter.write_str("pending Legacy stop returned no process outcome")
            }
            Self::Process(state) => {
                write!(
                    formatter,
                    "pending Legacy process stop is unverified: {state:?}"
                )
            }
        }
    }
}

impl std::error::Error for PendingLegacyStopError {}

impl PendingLegacyLane {
    pub(crate) fn process(&self) -> &DpiProc {
        &self.process
    }

    pub(crate) fn install_candidate(
        mut self,
        previous: &LegacyCategoryRuntimeSnapshot,
    ) -> Result<InstalledLegacyLane, PendingLegacyInstallFailure> {
        let install = {
            let state = self.app.state::<AppState>();
            let mut dpi = state.dpi.lock_recover();
            dpi.commit_legacy_category_replacement(previous, self.process.clone())
        };
        match install {
            Ok(owner) => Ok(self.finish_install(owner, previous.selection_index, false)),
            Err(error) => Err(PendingLegacyInstallFailure {
                pending: Box::new(self),
                message: format!("candidate lane install rejected: {error}"),
            }),
        }
    }

    pub(crate) fn install_crash_retry(
        mut self,
        previous: &LegacyCategoryRuntimeSnapshot,
    ) -> Result<InstalledLegacyLane, PendingLegacyInstallFailure> {
        let install = {
            let state = self.app.state::<AppState>();
            let mut dpi = state.dpi.lock_recover();
            dpi.commit_legacy_category_replacement(previous, self.process.clone())
        };
        match install {
            Ok(owner) => Ok(self.finish_install(owner, previous.selection_index, true)),
            Err(error) => Err(PendingLegacyInstallFailure {
                pending: Box::new(self),
                message: format!("crash-retry lane install rejected: {error}"),
            }),
        }
    }

    pub(crate) fn install_rollback(
        mut self,
        previous: &LegacyCategoryRuntimeSnapshot,
        failed_candidate: &LegacyCategoryRuntimeSnapshot,
    ) -> Result<InstalledLegacyLane, PendingLegacyInstallFailure> {
        let install = {
            let state = self.app.state::<AppState>();
            let mut dpi = state.dpi.lock_recover();
            dpi.rollback_legacy_category_replacement(
                previous,
                failed_candidate,
                self.process.clone(),
            )
        };
        match install {
            Ok(owner) => Ok(self.finish_install(owner, previous.selection_index, false)),
            Err(error) => Err(PendingLegacyInstallFailure {
                pending: Box::new(self),
                message: format!("rollback lane install rejected: {error}"),
            }),
        }
    }

    fn finish_install(
        &mut self,
        owner: LegacyProcessOwner,
        selection_index: usize,
        crash_retry: bool,
    ) -> InstalledLegacyLane {
        let child = self
            .child
            .take()
            .expect("pending Legacy lane must own its child until install");
        if crash_retry {
            let state = self.app.state::<AppState>();
            let mut retry_owners = state.legacy_crash_retry_owners.lock_recover();
            retry_owners.retain(|existing| {
                existing.runtime_generation == owner.runtime_generation
                    && existing.category != owner.category
            });
            retry_owners.insert(owner.clone());
        }
        let exit = arm_exact_legacy_monitor(
            self.app.clone(),
            child,
            LegacyCategoryRuntimeSnapshot {
                owner: owner.clone(),
                selection_index,
            },
            self.started,
        );
        emit_status(&self.app);
        InstalledLegacyLane { owner, exit }
    }

    pub(crate) async fn abort(mut self) -> Result<(), PendingLegacyStopError> {
        let owner = self
            .process
            .exact_legacy_owner()
            .ok_or(PendingLegacyStopError::MissingExactOwner)?;
        let mut child = self
            .child
            .take()
            .ok_or(PendingLegacyStopError::MissingChild)?;
        let stopped = stop_uninstalled_legacy_owner_bounded(&owner).await;
        if stopped.is_ok() {
            // The PID + creation identity check above is authoritative. This
            // short wait only reaps Tokio's child handle after OS-confirmed exit.
            let _ = tokio::time::timeout(Duration::from_millis(500), child.wait()).await;
        }
        stopped
    }
}

impl Drop for PendingLegacyLane {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        let owner = self.process.exact_legacy_owner();
        let app = self.app.clone();
        tauri::async_runtime::spawn(async move {
            let result = match owner {
                Some(owner) => stop_uninstalled_legacy_owner_bounded(&owner).await,
                None => {
                    let _ = child.start_kill();
                    Err(PendingLegacyStopError::MissingExactOwner)
                }
            };
            let _ = tokio::time::timeout(Duration::from_millis(500), child.wait()).await;
            if let Err(error) = result {
                util::emit_log(
                    &app,
                    "error",
                    "dpi",
                    &format!("Pending Legacy lane cleanup failed: {error}"),
                );
            }
        });
    }
}

fn arm_exact_legacy_monitor(
    app: AppHandle,
    mut child: tokio::process::Child,
    snapshot: LegacyCategoryRuntimeSnapshot,
    started: Instant,
) -> tokio::sync::oneshot::Receiver<LegacyProcessExit> {
    let (exit_tx, exit_rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let exit_code = match child.wait().await {
            Ok(status) => status.code(),
            Err(error) => {
                util::emit_log(
                    &app,
                    "error",
                    "dpi",
                    &format!(
                        "[{}] exact Legacy wait failed for pid={}: {error}",
                        snapshot.owner.category, snapshot.owner.pid
                    ),
                );
                None
            }
        };
        let lived_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
        let event = {
            let state = app.state::<AppState>();
            let mut dpi = state.dpi.lock_recover();
            finalize_exact_legacy_exit(&mut dpi, &snapshot, exit_code, lived_ms)
        };
        let Some(event) = event else {
            // Another exact owner, a later runtime generation, or an executor
            // that already finalized this stop makes the monitor stale.
            return;
        };
        let _ = exit_tx.send(event.clone());
        emit_legacy_process_exit(&app, &event);
        schedule_legacy_crash_retry(&app, &event);
        emit_status(&app);
    });
    exit_rx
}

/// Spawns and checks one Legacy lane without mutating the aggregate launch or
/// any neighboring process. The caller holds `dpi_gate` across the assisted
/// transaction and installs the returned guard only after rechecking fences.
pub(crate) async fn spawn_scoped_legacy_lane_locked(
    app: &AppHandle,
    category: &str,
    config_file: &str,
    runtime_generation: u64,
    lane_generation: crate::legacy_reliability::contracts::LaneGeneration,
    config_fingerprint: &crate::legacy_reliability::contracts::ConfigFingerprint,
) -> Result<PendingLegacyLane, String> {
    if runtime_shutting_down(app) {
        return Err("Приложение завершает работу".into());
    }
    if runtime_generation == 0
        || lane_generation.get() == 0
        || config_fingerprint.as_str().trim().is_empty()
    {
        return Err("scoped Legacy start requires complete generation/fingerprint fences".into());
    }

    let (winws, base, isolation_selections) = {
        let state = app.state::<AppState>();
        let dpi = state.dpi.lock_recover();
        if !dpi.is_current_generation(runtime_generation) {
            return Err("Legacy runtime generation changed before scoped start".into());
        }
        if dpi
            .procs
            .values()
            .any(|process| process.engine == "legacy" && process.category == category)
            || matches!(
                dpi.active_launch.as_ref(),
                Some(DpiLaunchSpec::Legacy { selections })
                    if selections.iter().any(|(selected, _)| selected == category)
            )
        {
            return Err(format!("Legacy lane {category} is still occupied"));
        }
        if matches!(
            dpi.active_launch.as_ref(),
            Some(DpiLaunchSpec::Zapret2 { .. })
        ) {
            return Err("DPI runtime switched to Zapret2".into());
        }
        let mut isolation_selections = match dpi.active_launch.as_ref() {
            Some(DpiLaunchSpec::Legacy { selections }) => selections.clone(),
            None => Vec::new(),
            Some(DpiLaunchSpec::Zapret2 { .. }) => {
                return Err("DPI runtime switched to Zapret2".into())
            }
        };
        isolation_selections.push((category.to_owned(), config_file.to_owned()));
        (
            state.paths.winws_path(),
            state.paths.base_dir.clone(),
            isolation_selections,
        )
    };

    let conf = crate::legacy_reliability::autohost_isolation::prepare_scoped_launch_from_disk(
        &base,
        &isolation_selections,
        category,
        config_file,
    )
    .map_err(|error| format!("Legacy scoped isolation preparation failed: {error}"))?;

    crate::dpi_engine::resources::validate_engine_resources(&base, "zapret1")
        .map_err(|error| format!("Zapret Legacy resource validation failed: {error}"))?;
    if !winws.exists() {
        return Err(format!("Ядро winws.exe не найдено: {}", winws.display()));
    }
    if !conf.exists() {
        return Err(format!("Конфиг не найден: {}", conf.display()));
    }

    let mut std_cmd = util::std_command(&winws);
    std_cmd
        .arg(format!("@{}", conf.display()))
        .current_dir(&base)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut command = TokioCommand::from(std_cmd);
    command.kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|error| format!("Не удалось запустить scoped winws: {error}"))?;
    let pid = child
        .id()
        .ok_or_else(|| "Scoped winws не вернул PID".to_string())?;

    let identity_deadline = Instant::now() + Duration::from_millis(100);
    let process_identity = loop {
        if let Some(identity) = crate::dpi_supervisor::capture_process_identity(pid) {
            break identity;
        }
        if Instant::now() >= identity_deadline {
            let _ = child.start_kill();
            let _ = child.wait().await;
            return Err(format!(
                "Не удалось получить process start identity для scoped pid={pid}"
            ));
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    };

    let process = DpiProc {
        pid,
        category: category.to_owned(),
        config_file: config_file.to_owned(),
        generation: runtime_generation,
        lane_generation: Some(lane_generation),
        config_fingerprint: Some(config_fingerprint.as_str().to_owned()),
        engine: "legacy".into(),
        process_identity: Some(process_identity),
    };
    if process.exact_legacy_owner().is_none() {
        let _ = child.start_kill();
        let _ = child.wait().await;
        return Err("Scoped winws owner is incomplete".into());
    }

    let (readiness_tx, readiness_rx) = tokio::sync::mpsc::unbounded_channel();
    if let Some(stdout) = child.stdout.take() {
        spawn_reader(
            app.clone(),
            stdout,
            category.to_owned(),
            "info",
            Some(readiness_tx.clone()),
        );
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_reader(
            app.clone(),
            stderr,
            category.to_owned(),
            "error",
            Some(readiness_tx.clone()),
        );
    }
    drop(readiness_tx);

    let started = Instant::now();
    match crate::dpi_supervisor::wait_for_readiness(
        async { child.wait().await.map(|status| status.code()) },
        readiness_rx,
        Duration::from_millis(500),
    )
    .await
    {
        Ok(crate::dpi_supervisor::ReadinessState::Marker) => {}
        Ok(crate::dpi_supervisor::ReadinessState::BoundedFallback) => util::emit_log(
            app,
            "debug",
            "dpi",
            &format!("[{category}] scoped startup marker missing; bounded fallback accepted"),
        ),
        Err(crate::dpi_supervisor::ReadinessFailure::Exited(code)) => {
            return Err(format!(
                "Scoped winws ({category}) exited before readiness with code {code:?}"
            ));
        }
        Err(crate::dpi_supervisor::ReadinessFailure::WaitFailed(error)) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            return Err(format!("Scoped winws readiness wait failed: {error}"));
        }
    }

    let state_is_current = {
        let state = app.state::<AppState>();
        let dpi = state.dpi.lock_recover();
        dpi.is_current_generation(runtime_generation)
            && !dpi
                .procs
                .values()
                .any(|running| running.engine == "legacy" && running.category == category)
            && !matches!(
                dpi.active_launch.as_ref(),
                Some(DpiLaunchSpec::Legacy { selections })
                    if selections.iter().any(|(selected, _)| selected == category)
            )
            && !matches!(
                dpi.active_launch.as_ref(),
                Some(DpiLaunchSpec::Zapret2 { .. })
            )
    };
    if runtime_shutting_down(app) || !state_is_current {
        let _ = child.start_kill();
        let _ = child.wait().await;
        return Err("Legacy runtime changed during scoped readiness".into());
    }

    Ok(PendingLegacyLane {
        app: app.clone(),
        process,
        child: Some(child),
        started,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScopedLegacyStopError {
    State(crate::state::LegacyCategoryStateError),
    Process(crate::dpi_supervisor::ProcessStopState),
    MissingOutcome,
}

impl std::fmt::Display for ScopedLegacyStopError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::State(error) => write!(formatter, "scoped Legacy state rejected stop: {error}"),
            Self::Process(state) => write!(formatter, "scoped Legacy stop failed: {state:?}"),
            Self::MissingOutcome => formatter.write_str("scoped Legacy stop returned no outcome"),
        }
    }
}

impl std::error::Error for ScopedLegacyStopError {}

/// Stops a staged process that was never installed in `DpiState`. The exact
/// Windows creation identity makes PID reuse a successful retirement of the
/// original owner rather than a reason to kill an unrelated process.
pub(crate) async fn stop_uninstalled_legacy_owner_bounded(
    owner: &LegacyProcessOwner,
) -> Result<(), PendingLegacyStopError> {
    let mut outcomes = stop_owned_processes_async(vec![crate::dpi_supervisor::OwnedProcess {
        pid: owner.pid,
        identity: Some(owner.process_identity),
    }])
    .await;
    let outcome = outcomes
        .pop()
        .ok_or(PendingLegacyStopError::MissingOutcome)?;
    if outcome.original_exited() {
        Ok(())
    } else {
        Err(PendingLegacyStopError::Process(outcome.state))
    }
}

/// Stops and finalizes one exact owner. The monitor and this helper may race;
/// `remove_exact_legacy_category` makes the race idempotent, so exactly one side
/// consumes the intentional marker and neither can remove a reused PID.
pub(crate) async fn stop_scoped_legacy_lane_locked(
    app: &AppHandle,
    snapshot: &LegacyCategoryRuntimeSnapshot,
) -> Result<LegacyProcessOwner, ScopedLegacyStopError> {
    {
        let state = app.state::<AppState>();
        let mut dpi = state.dpi.lock_recover();
        dpi.mark_legacy_category_stopping(snapshot)
            .map_err(ScopedLegacyStopError::State)?;
    }

    let mut outcomes = stop_owned_processes_async(vec![crate::dpi_supervisor::OwnedProcess {
        pid: snapshot.owner.pid,
        identity: Some(snapshot.owner.process_identity),
    }])
    .await;
    let outcome = outcomes
        .pop()
        .ok_or(ScopedLegacyStopError::MissingOutcome)?;
    if !outcome.original_exited() {
        let state = app.state::<AppState>();
        let mut dpi = state.dpi.lock_recover();
        dpi.legacy_stopping.remove(&snapshot.owner);
        return Err(ScopedLegacyStopError::Process(outcome.state));
    }

    let event = {
        let state = app.state::<AppState>();
        let mut dpi = state.dpi.lock_recover();
        match dpi.remove_exact_legacy_category(snapshot) {
            Ok(removal) => removal.process.and_then(|removed| {
                let owner = removed.exact_legacy_owner()?;
                (owner == snapshot.owner).then_some(LegacyProcessExit {
                    owner,
                    selection_index: snapshot.selection_index,
                    intentional: removal.intentional,
                    exit_code: None,
                    lived_ms: 0,
                })
            }),
            Err(error) => return Err(ScopedLegacyStopError::State(error)),
        }
    };
    if let Some(event) = event {
        emit_legacy_process_exit(app, &event);
        emit_status(app);
    }
    Ok(snapshot.owner.clone())
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
    if !eyes_teardown_ready_for_start(app) {
        util::emit_log(
            app,
            "warn",
            "eyes",
            "Zapret2 Eyes не запущены: предыдущий teardown ещё не подтверждён",
        );
        return;
    }
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
            let mut slot = st.eyes.lock_recover();
            if slot.is_some() {
                drop(slot);
                let outcomes = retire_eyes_handle_bounded(app, handle, EYES_STOP_TIMEOUT);
                util::emit_log(
                    app,
                    "warn",
                    "eyes",
                    &format!("Zapret2 Eyes slot занят; staged cleanup: {outcomes:?}"),
                );
                return;
            }
            *slot = Some(handle);
            let msg = if n == 0 {
                "Наблюдатель запущен (sniff :443, хостлист пуст — все хосты)".to_string()
            } else {
                format!("Наблюдатель запущен (sniff :443, хостлист: {n} доменов)")
            };
            eprintln!("[eyes] {msg}");
            util::emit_log(app, "info", "eyes", &msg);
        }
        Err(error) => {
            let (message, _) = retain_eyes_start_failure(app, error);
            eprintln!("[eyes] Глаза не запустились: {message}");
            util::emit_log(
                app,
                "error",
                "eyes",
                &format!("Глаза не запустились: {message}"),
            );
        }
    }
}

#[cfg(windows)]
fn retain_eyes_start_failure_in(
    slot: &std::sync::Mutex<Vec<crate::dpi_supervisor::WorkerTeardown>>,
    error: crate::eyes::EyesStartError,
) -> (String, bool) {
    let (message, safe_to_retry, pending_teardown) = error.into_parts();
    if let Some(teardown) = pending_teardown {
        slot.lock_recover().push(teardown);
    }
    (message, safe_to_retry)
}

#[cfg(windows)]
fn retain_eyes_start_failure(
    app: &AppHandle,
    error: crate::eyes::EyesStartError,
) -> (String, bool) {
    let state = app.state::<AppState>();
    retain_eyes_start_failure_in(&state.eyes_teardowns, error)
}

#[cfg(windows)]
struct LegacyEyesStartFailure {
    message: String,
    safe_to_restore: bool,
}

#[cfg(windows)]
impl LegacyEyesStartFailure {
    fn clean(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            safe_to_restore: true,
        }
    }
}

#[cfg(windows)]
fn create_legacy_eyes(
    app: &AppHandle,
    registry: std::sync::Arc<crate::legacy_reliability::target_registry::TargetRegistry>,
    envelope: crate::legacy_reliability::contracts::EventEnvelope,
    lane_generations: std::sync::Arc<
        std::collections::BTreeMap<String, crate::legacy_reliability::contracts::LaneGeneration>,
    >,
    ingress: crate::legacy_reliability::ingress::LegacyIngress,
) -> Result<crate::eyes::EyesHandle, LegacyEyesStartFailure> {
    let dll = app
        .state::<AppState>()
        .paths
        .bin_dir()
        .join("WinDivert.dll");
    if !dll.exists() {
        return Err(LegacyEyesStartFailure::clean(
            "WinDivert.dll не найдена — Legacy Eyes не стартуют",
        ));
    }

    let hostlist = registry
        .active_target_suffixes()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let capture_plan = registry.active_capture_plan().map_err(|error| {
        LegacyEyesStartFailure::clean(format!("Legacy active capture plan недоступен: {error}"))
    })?;
    if hostlist.is_empty() || capture_plan.is_empty() {
        return Err(LegacyEyesStartFailure::clean(
            "Legacy TargetRegistry не содержит активных доменов или TCP capture plan",
        ));
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
        })
        .map_err(|error| {
            let (message, safe_to_restore) = retain_eyes_start_failure(app, error);
            LegacyEyesStartFailure {
                message,
                safe_to_restore,
            }
        })?;
    util::emit_log(
        app,
        "info",
        "legacy-reliability",
        &format!(
            "Наблюдатель Legacy запущен: целей={target_count}, диапазонов TCP={}",
            capture_plan.tcp_ranges().len()
        ),
    );
    Ok(handle)
}

#[cfg(windows)]
#[derive(Debug)]
struct EyesTeardownReport {
    outcomes: Vec<crate::dpi_supervisor::WorkerStopOutcome>,
    unresolved: bool,
}

#[cfg(windows)]
fn poll_pending_eyes_teardowns(app: &AppHandle, timeout: Duration) -> EyesTeardownReport {
    let state = app.state::<AppState>();
    poll_pending_eyes_teardowns_in(&state.eyes_teardowns, timeout)
}

#[cfg(windows)]
fn poll_pending_eyes_teardowns_in(
    slot: &std::sync::Mutex<Vec<crate::dpi_supervisor::WorkerTeardown>>,
    timeout: Duration,
) -> EyesTeardownReport {
    // Keep the same mutex held while tickets are polled. A concurrent start
    // must either see an unresolved ticket or wait until this function has
    // proved that every worker exited; an empty transient `mem::take` window
    // would permit a second WinDivert observer to open.
    let mut pending = slot.lock_recover();
    let deadline = Instant::now() + timeout;
    let mut outcomes = Vec::new();
    for teardown in pending.iter_mut() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        outcomes.extend(teardown.wait_bounded(remaining));
    }
    pending.retain(|teardown| !teardown.is_resolved());
    let unresolved = !pending.is_empty();
    EyesTeardownReport {
        outcomes,
        unresolved,
    }
}

#[cfg(windows)]
fn retire_eyes_handle_bounded(
    app: &AppHandle,
    handle: crate::eyes::EyesHandle,
    timeout: Duration,
) -> Vec<crate::dpi_supervisor::WorkerStopOutcome> {
    let mut teardown = handle.begin_stop();
    let outcomes = teardown.wait_bounded(timeout);
    if !teardown.is_resolved() {
        app.state::<AppState>()
            .eyes_teardowns
            .lock_recover()
            .push(teardown);
    }
    outcomes
}

#[cfg(windows)]
fn eyes_teardown_ready_for_start(app: &AppHandle) -> bool {
    let report = poll_pending_eyes_teardowns(app, Duration::ZERO);
    if report.unresolved {
        util::emit_log(
            app,
            "warn",
            "eyes",
            "Новый наблюдатель отложен: завершение предыдущего ещё не подтверждено",
        );
        false
    } else {
        true
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
    if !eyes_teardown_ready_for_start(app) {
        return Err("previous Eyes teardown is still pending".into());
    }
    if app.state::<AppState>().eyes.lock_recover().is_some() {
        return Err("Legacy Eyes slot is already occupied".into());
    }
    let handle = create_legacy_eyes(app, registry, envelope, lane_generations, ingress)
        .map_err(|error| error.message)?;
    let state = app.state::<AppState>();
    let mut slot = state.eyes.lock_recover();
    if slot.is_some() {
        drop(slot);
        let outcomes = retire_eyes_handle_bounded(app, handle, EYES_STOP_TIMEOUT);
        return Err(format!(
            "Legacy Eyes slot was claimed during start; staged cleanup: {outcomes:?}"
        ));
    }
    *slot = Some(handle);
    Ok(())
}

/// Останавливает наблюдателя, если запущен.
#[cfg(windows)]
fn stop_eyes(app: &AppHandle) -> EyesTeardownReport {
    let handle = app.state::<AppState>().eyes.lock_recover().take();
    if let Some(h) = handle {
        app.state::<AppState>()
            .eyes_teardowns
            .lock_recover()
            .push(h.begin_stop());
    }
    let report = poll_pending_eyes_teardowns(app, EYES_STOP_TIMEOUT);
    let clean = !report.unresolved
        && report
            .outcomes
            .iter()
            .all(|outcome| outcome.state == crate::dpi_supervisor::WorkerStopState::Joined);
    util::emit_log(
        app,
        if clean { "info" } else { "warn" },
        "eyes",
        if clean {
            "Наблюдатель остановлен"
        } else if report.unresolved {
            "Наблюдатель превысил bounded stop deadline; завершение отслеживается"
        } else {
            "Наблюдатель завершился с ошибкой worker-потока"
        },
    );
    if !clean {
        util::emit_log(
            app,
            "warn",
            "eyes",
            &format!("eyes_stop={:?}", report.outcomes),
        );
    }
    report
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ExactLegacyRuntimeSnapshot {
    runtime: DpiRuntimeSnapshot,
    owners: BTreeMap<String, LegacyProcessOwner>,
}

/// Exact target-lane shape expected by the observer transaction. Forward
/// preflight and restore-before-stop require the same live owner the executor
/// fenced; rollback-after-stop requires a complete target absence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LegacyObserverTargetExpectation {
    Present(crate::legacy_reliability::contracts::ProcessOwner),
    Absent,
}

/// Captures every live Legacy owner, not merely the global generation. This is
/// intentionally pure so observer restarts can re-check the same snapshot after
/// each await without holding `DpiState`'s std::Mutex across the await.
fn capture_exact_legacy_runtime(
    dpi: &crate::state::DpiState,
    expected_generation: u64,
) -> Result<ExactLegacyRuntimeSnapshot, String> {
    if expected_generation == 0 || dpi.generation != expected_generation {
        return Err(format!(
            "DPI runtime generation changed: expected {expected_generation}, current {}",
            dpi.generation
        ));
    }

    let mut owners = BTreeMap::new();
    for process in dpi.procs.values() {
        if process.engine != "legacy" {
            return Err("Legacy observer cannot share a runtime with a non-Legacy process".into());
        }
        let owner = process.exact_legacy_owner().ok_or_else(|| {
            format!(
                "Legacy process {} ({}) is missing an exact owner token",
                process.pid, process.category
            )
        })?;
        if owner.runtime_generation != expected_generation {
            return Err(format!(
                "Legacy process {} belongs to stale runtime generation {}",
                owner.pid, owner.runtime_generation
            ));
        }
        if dpi.stopping.contains(&owner.pid) || dpi.legacy_stopping.contains(&owner) {
            return Err(format!(
                "Legacy process {} ({}) is already stopping",
                owner.pid, owner.category
            ));
        }
        let category = owner.category.clone();
        if owners.insert(category.clone(), owner).is_some() {
            return Err(format!(
                "Legacy category {category} has more than one exact process owner"
            ));
        }
    }

    let mut selections = BTreeMap::new();
    match dpi.active_launch.as_ref() {
        Some(DpiLaunchSpec::Legacy { selections: active }) => {
            for (category, config_file) in active {
                if selections
                    .insert(category.clone(), config_file.clone())
                    .is_some()
                {
                    return Err(format!(
                        "Legacy category {category} appears more than once in active launch"
                    ));
                }
            }
        }
        Some(DpiLaunchSpec::Zapret2 { .. }) => {
            return Err("Legacy observer cannot replace a Zapret2 runtime".into())
        }
        None if owners.is_empty() => {}
        None => return Err("Legacy processes exist without an active Legacy launch".into()),
    }

    if selections.len() != owners.len()
        || selections.iter().any(|(category, config_file)| {
            owners
                .get(category)
                .is_none_or(|owner| owner.config_file != *config_file)
        })
    {
        return Err("Legacy launch selections do not match exact process owners".into());
    }

    Ok(ExactLegacyRuntimeSnapshot {
        runtime: dpi.runtime_snapshot(),
        owners,
    })
}

fn validate_captured_legacy_runtime(
    dpi: &crate::state::DpiState,
    expected: &ExactLegacyRuntimeSnapshot,
) -> Result<(), String> {
    let current = capture_exact_legacy_runtime(dpi, expected.runtime.generation)?;
    if current != *expected {
        return Err(format!(
            "exact Legacy runtime owners changed: expected {:?}, current {:?}",
            expected.owners.keys().collect::<Vec<_>>(),
            current.owners.keys().collect::<Vec<_>>()
        ));
    }
    Ok(())
}

fn validate_exact_legacy_runtime(
    app: &AppHandle,
    expected: &ExactLegacyRuntimeSnapshot,
) -> Result<(), String> {
    validate_captured_legacy_runtime(&app.state::<AppState>().dpi.lock_recover(), expected)
}

fn registry_owner_matches(
    registry: &crate::legacy_reliability::target_registry::TargetRegistry,
    owner: &LegacyProcessOwner,
    lane_generation: crate::legacy_reliability::contracts::LaneGeneration,
) -> bool {
    owner.lane_generation == lane_generation
        && registry.active_config(&owner.category) == Some(owner.config_file.as_str())
        && registry
            .config_fingerprint(&owner.category, &owner.config_file)
            .is_ok_and(|fingerprint| fingerprint.as_hex() == owner.config_fingerprint)
}

/// Validates an immutable candidate/rollback observer plan against the current
/// Manager session and all neighboring process-owner tokens. The one changed
/// lane may still own the old process (pre-stop) or be absent (post-stop), but
/// every neighboring winws must match both registry snapshots exactly.
#[allow(clippy::too_many_arguments)]
fn validate_observer_transition(
    previous: &crate::legacy_reliability::manager::ObserveOnlySnapshot,
    previous_registry: &crate::legacy_reliability::target_registry::TargetRegistry,
    session_id: crate::legacy_reliability::contracts::SessionId,
    network_fingerprint: &crate::legacy_reliability::contracts::NetworkFingerprint,
    registry: &crate::legacy_reliability::target_registry::TargetRegistry,
    lane_generations: &BTreeMap<String, crate::legacy_reliability::contracts::LaneGeneration>,
    runtime: &ExactLegacyRuntimeSnapshot,
    target_expectation: &LegacyObserverTargetExpectation,
) -> Result<String, String> {
    use crate::dpi_engine::EngineKind;

    let session = &previous.session;
    if session.closed || session.engine != EngineKind::Legacy {
        return Err("Legacy observer plan refers to a closed or non-Legacy session".into());
    }
    if session.session_id != session_id {
        return Err("Legacy observer plan changed the session id".into());
    }
    if &session.network_fingerprint_at_start != network_fingerprint {
        return Err("Legacy observer plan changed the network fingerprint".into());
    }
    if session.sensor_generation.get() == 0
        || session.target_registry_version.get() == 0
        || session.target_registry_version != previous_registry.version()
    {
        return Err("Current Legacy observer has an invalid sensor/registry fence".into());
    }
    if previous_registry.content_hash() != registry.content_hash() {
        return Err("Legacy observer plan changed immutable registry content".into());
    }

    let active_categories = session
        .active_categories
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    if active_categories.is_empty()
        || active_categories.len() != session.active_categories.len()
        || session
            .lane_generations
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>()
            != active_categories
        || lane_generations.keys().cloned().collect::<BTreeSet<_>>() != active_categories
    {
        return Err("Legacy observer plan changed the active category set".into());
    }
    if session
        .lane_generations
        .values()
        .chain(lane_generations.values())
        .any(|generation| generation.get() == 0)
    {
        return Err("Legacy observer plan contains a zero lane generation".into());
    }

    let previous_selections = previous_registry
        .active_selections()
        .map(|(category, config)| (category.to_owned(), config.to_owned()))
        .collect::<BTreeMap<_, _>>();
    let next_selections = registry
        .active_selections()
        .map(|(category, config)| (category.to_owned(), config.to_owned()))
        .collect::<BTreeMap<_, _>>();
    if previous_selections.keys().cloned().collect::<BTreeSet<_>>() != active_categories
        || next_selections.keys().cloned().collect::<BTreeSet<_>>() != active_categories
    {
        return Err("Legacy observer registry selections do not match active categories".into());
    }

    let changed_lanes = lane_generations
        .iter()
        .filter_map(|(category, next)| {
            (session.lane_generations.get(category) != Some(next)).then_some(category.clone())
        })
        .collect::<Vec<_>>();
    let [changed_category] = changed_lanes.as_slice() else {
        return Err("Legacy observer plan must change exactly one lane generation".into());
    };

    match target_expectation {
        LegacyObserverTargetExpectation::Present(expected) => {
            let current = runtime.owners.get(changed_category).ok_or_else(|| {
                format!("target Legacy category {changed_category} exited before observer teardown")
            })?;
            if recovery_process_owner(current) != *expected {
                return Err(format!(
                    "target Legacy category {changed_category} changed its exact process owner"
                ));
            }
        }
        LegacyObserverTargetExpectation::Absent => {
            if runtime.owners.contains_key(changed_category) {
                return Err(format!(
                    "target Legacy category {changed_category} is still present after exact stop"
                ));
            }
        }
    }

    if runtime
        .owners
        .keys()
        .any(|category| !active_categories.contains(category))
    {
        return Err("DPI runtime contains an owner outside the Manager session".into());
    }

    for category in active_categories
        .iter()
        .filter(|category| *category != changed_category)
    {
        let owner = runtime.owners.get(category).ok_or_else(|| {
            format!("neighboring Legacy category {category} lost its exact process owner")
        })?;
        let previous_lane = session.lane_generations[category];
        if lane_generations.get(category) != Some(&previous_lane)
            || !registry_owner_matches(previous_registry, owner, previous_lane)
            || !registry_owner_matches(registry, owner, previous_lane)
        {
            return Err(format!(
                "neighboring Legacy category {category} no longer matches its exact owner token"
            ));
        }
    }

    if let Some(owner) = runtime.owners.get(changed_category) {
        let matches_previous = registry_owner_matches(
            previous_registry,
            owner,
            session.lane_generations[changed_category],
        );
        let matches_next =
            registry_owner_matches(registry, owner, lane_generations[changed_category]);
        if !matches_previous && !matches_next {
            return Err(format!(
                "target Legacy category {changed_category} matches neither observer plan"
            ));
        }
    }

    Ok(changed_category.clone())
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ObserverRestorationState {
    Untouched,
    Restored(crate::legacy_reliability::contracts::SensorGeneration),
    Skipped(String),
    Failed(String),
}

impl std::fmt::Display for ObserverRestorationState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Untouched => formatter.write_str("старый наблюдатель не был остановлен"),
            Self::Restored(generation) => write!(
                formatter,
                "старый план восстановлен на свежем sensor generation {}",
                generation.get()
            ),
            Self::Skipped(reason) => write!(formatter, "восстановление пропущено: {reason}"),
            Self::Failed(reason) => write!(formatter, "восстановление не удалось: {reason}"),
        }
    }
}

fn observer_transaction_error(
    original_error: impl AsRef<str>,
    restoration: ObserverRestorationState,
) -> String {
    format!(
        "{}; состояние восстановления: {restoration}",
        original_error.as_ref()
    )
}

#[cfg(windows)]
struct CapturedLegacyObserver {
    snapshot: crate::legacy_reliability::manager::ObserveOnlySnapshot,
    registry: std::sync::Arc<crate::legacy_reliability::target_registry::TargetRegistry>,
    environment_gate: std::sync::Arc<
        tokio::sync::Mutex<crate::legacy_reliability::environment_gate::EnvironmentGate>,
    >,
}

#[cfg(windows)]
fn capture_legacy_observer(app: &AppHandle) -> Result<CapturedLegacyObserver, String> {
    let state = app.state::<AppState>();
    let manager_slot = state.legacy_manager.lock_recover();
    let manager = manager_slot
        .as_ref()
        .ok_or_else(|| "Legacy Manager is not running".to_owned())?;
    let snapshot = manager.snapshot();
    let registry = manager.registry();
    let environment_gate = manager
        .environment_gate()
        .ok_or_else(|| "Legacy Environment Gate baseline is unavailable".to_owned())?;
    if snapshot.session.closed
        || snapshot.session.target_registry_version != registry.version()
        || snapshot.session.sensor_generation.get() == 0
    {
        return Err("Legacy Manager snapshot is already closed or inconsistent".into());
    }
    Ok(CapturedLegacyObserver {
        snapshot,
        registry,
        environment_gate,
    })
}

#[cfg(windows)]
fn manager_matches_capture(
    manager: &crate::legacy_reliability::runtime::LegacyReliabilityHandle,
    captured: &CapturedLegacyObserver,
) -> bool {
    let current = manager.snapshot();
    let current_registry = manager.registry();
    let current_gate = manager.environment_gate();
    current.session == captured.snapshot.session
        && std::sync::Arc::ptr_eq(&current_registry, &captured.registry)
        && current_gate
            .is_some_and(|gate| std::sync::Arc::ptr_eq(&gate, &captured.environment_gate))
}

#[cfg(windows)]
fn take_captured_manager(
    app: &AppHandle,
    captured: &CapturedLegacyObserver,
) -> Result<crate::legacy_reliability::runtime::LegacyReliabilityHandle, String> {
    let state = app.state::<AppState>();
    let mut slot = state.legacy_manager.lock_recover();
    if slot
        .as_ref()
        .is_none_or(|manager| !manager_matches_capture(manager, captured))
    {
        return Err("Legacy Manager changed before observer teardown".into());
    }
    Ok(slot
        .take()
        .expect("validated Legacy Manager slot must remain populated"))
}

#[cfg(windows)]
fn allocate_fresh_sensor_generation(
    app: &AppHandle,
    closed: &[crate::legacy_reliability::contracts::SensorGeneration],
) -> Result<crate::legacy_reliability::contracts::SensorGeneration, String> {
    use crate::legacy_reliability::contracts::SensorGeneration;

    for _ in 0..=closed.len() {
        let generation =
            SensorGeneration::new(app.state::<AppState>().legacy_sensor_revision.bump());
        if generation.get() != 0 && !closed.contains(&generation) {
            return Ok(generation);
        }
    }
    Err("sensor generation allocator attempted to reuse a closed generation".into())
}

#[cfg(windows)]
struct ObserverStartFailure {
    message: String,
    sensor_generation: Option<crate::legacy_reliability::contracts::SensorGeneration>,
    safe_to_restore: bool,
}

#[cfg(windows)]
impl ObserverStartFailure {
    fn before_generation(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            sensor_generation: None,
            safe_to_restore: true,
        }
    }

    fn after_generation(
        message: impl Into<String>,
        sensor_generation: crate::legacy_reliability::contracts::SensorGeneration,
    ) -> Self {
        Self {
            message: message.into(),
            sensor_generation: Some(sensor_generation),
            safe_to_restore: true,
        }
    }

    fn append_cleanup(&mut self, cleanup: ObserverCleanupReport) {
        self.safe_to_restore &= cleanup.safe_to_restore;
        if !cleanup.failures.is_empty() {
            self.message.push_str("; observer cleanup: ");
            self.message.push_str(&cleanup.failures.join("; "));
        }
    }
}

#[cfg(windows)]
struct ObserverCleanupReport {
    failures: Vec<String>,
    safe_to_restore: bool,
}

#[cfg(windows)]
async fn cleanup_uninstalled_observer(
    app: &AppHandle,
    manager: crate::legacy_reliability::runtime::LegacyReliabilityHandle,
    eyes: Option<crate::eyes::EyesHandle>,
    expected_runtime: &ExactLegacyRuntimeSnapshot,
) -> ObserverCleanupReport {
    let mut report = ObserverCleanupReport {
        failures: Vec::new(),
        safe_to_restore: true,
    };

    // Retire the Manager fence before stopping capture so late callbacks from
    // the old Eyes generation can only hit a closed ingress.
    manager.shutdown().await;
    if let Err(error) = validate_exact_legacy_runtime(app, expected_runtime) {
        report
            .failures
            .push(format!("runtime changed after Manager cleanup: {error}"));
    }

    if let Some(eyes) = eyes {
        let app2 = app.clone();
        match tauri::async_runtime::spawn_blocking(move || {
            retire_eyes_handle_bounded(&app2, eyes, EYES_STOP_TIMEOUT)
        })
        .await
        {
            Ok(outcomes)
                if !outcomes.is_empty() && outcomes.iter().all(|outcome| outcome.is_clean()) => {}
            Ok(outcomes) => {
                report.safe_to_restore = false;
                report
                    .failures
                    .push(format!("Eyes bounded stop failed: {outcomes:?}"));
            }
            Err(error) => {
                report.safe_to_restore = false;
                report
                    .failures
                    .push(format!("Eyes stop worker failed: {error}"));
            }
        }
        if let Err(error) = validate_exact_legacy_runtime(app, expected_runtime) {
            report
                .failures
                .push(format!("runtime changed after Eyes cleanup: {error}"));
        }
    }
    report
}

#[cfg(windows)]
async fn cleanup_installed_observer_generation(
    app: &AppHandle,
    snapshot: &crate::legacy_reliability::manager::ObserveOnlySnapshot,
) -> ObserverCleanupReport {
    let (manager, eyes) = {
        let state = app.state::<AppState>();
        let mut manager_slot = state.legacy_manager.lock_recover();
        let mut eyes_slot = state.eyes.lock_recover();
        let owned = manager_slot.as_ref().is_some_and(|manager| {
            let current = manager.snapshot();
            current.session.session_id == snapshot.session.session_id
                && current.session.sensor_generation == snapshot.session.sensor_generation
        });
        if owned {
            (manager_slot.take(), eyes_slot.take())
        } else {
            (None, None)
        }
    };
    let Some(manager) = manager else {
        return ObserverCleanupReport {
            failures: vec!["installed observer generation no longer owns the Manager slot".into()],
            safe_to_restore: false,
        };
    };

    let mut report = ObserverCleanupReport {
        failures: Vec::new(),
        safe_to_restore: true,
    };
    manager.shutdown().await;
    match eyes {
        Some(eyes) => {
            let app2 = app.clone();
            match tauri::async_runtime::spawn_blocking(move || {
                retire_eyes_handle_bounded(&app2, eyes, EYES_STOP_TIMEOUT)
            })
            .await
            {
                Ok(outcomes)
                    if !outcomes.is_empty()
                        && outcomes.iter().all(|outcome| outcome.is_clean()) => {}
                Ok(outcomes) => {
                    report.safe_to_restore = false;
                    report
                        .failures
                        .push(format!("installed Eyes bounded stop failed: {outcomes:?}"));
                }
                Err(error) => {
                    report.safe_to_restore = false;
                    report
                        .failures
                        .push(format!("installed Eyes stop worker failed: {error}"));
                }
            }
        }
        None => {
            report.safe_to_restore = false;
            report
                .failures
                .push("installed observer generation lost its Eyes handle".into());
        }
    }
    report
}

#[cfg(windows)]
async fn record_observer_restart_gap(
    app: &AppHandle,
    manager: &crate::legacy_reliability::runtime::LegacyReliabilityHandle,
    envelope: crate::legacy_reliability::contracts::EventEnvelope,
    expected_runtime: &ExactLegacyRuntimeSnapshot,
) -> Result<(), String> {
    use crate::legacy_reliability::contracts::GapEvent;
    use crate::legacy_reliability::ingress::ControlIngressResult;

    let accepted = manager.ingress.try_gap(GapEvent {
        envelope,
        // Sensor clocks restart at zero. This is a generation-boundary Gap,
        // not producer queue loss, so it intentionally reports zero drops.
        from_ts: 0,
        to_ts: 0,
        dropped_events: 0,
    });
    if accepted != ControlIngressResult::Accepted {
        return Err(format!(
            "Legacy Manager rejected the observer restart Gap: {accepted:?}"
        ));
    }

    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        if manager.snapshot().last_gap_sequence.is_some() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("Legacy Manager did not acknowledge the observer restart Gap".into());
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
        validate_exact_legacy_runtime(app, expected_runtime)?;
        if runtime_shutting_down(app) {
            return Err("observer restart Gap wait cancelled during app shutdown".into());
        }
    }
}

#[cfg(windows)]
async fn wait_for_observer_clean_ready(
    app: &AppHandle,
    manager: &crate::legacy_reliability::runtime::LegacyReliabilityHandle,
    expected_runtime: &ExactLegacyRuntimeSnapshot,
    capture_started_at: Instant,
) -> Result<crate::legacy_reliability::manager::ObserveOnlySnapshot, String> {
    use crate::legacy_reliability::assessment::LanePhase;
    use crate::legacy_reliability::contracts::EyeHealthState;

    const CLEAN_READY_TIMEOUT: Duration = Duration::from_secs(13);
    const CLEAN_READY_WINDOW: Duration = Duration::from_secs(10);
    let deadline = capture_started_at + CLEAN_READY_TIMEOUT;
    loop {
        let snapshot = manager.snapshot();
        if snapshot.session.closed {
            return Err("Legacy Manager closed while waiting for restart clean window".into());
        }
        if snapshot.last_gap_sequence.is_some()
            && snapshot.health.state == EyeHealthState::Ready
            && capture_started_at.elapsed() >= CLEAN_READY_WINDOW
            && snapshot
                .lanes
                .iter()
                .all(|lane| lane.phase != LanePhase::SensorUnreliable)
        {
            return Ok(snapshot);
        }
        if matches!(
            snapshot.health.state,
            EyeHealthState::Blind | EyeHealthState::Stopped
        ) {
            return Err(format!(
                "Legacy Eyes became {:?} during restart clean window",
                snapshot.health.state
            ));
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "Legacy observer did not become clean Ready within {} seconds",
                CLEAN_READY_TIMEOUT.as_secs()
            ));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        validate_exact_legacy_runtime(app, expected_runtime)?;
        if runtime_shutting_down(app) {
            return Err("observer clean-window wait cancelled during app shutdown".into());
        }
    }
}

#[allow(clippy::too_many_arguments)]
#[cfg(windows)]
async fn start_legacy_observer_generation(
    app: &AppHandle,
    expected_runtime: &ExactLegacyRuntimeSnapshot,
    session_id: crate::legacy_reliability::contracts::SessionId,
    network_fingerprint: crate::legacy_reliability::contracts::NetworkFingerprint,
    active_categories: Vec<String>,
    registry: std::sync::Arc<crate::legacy_reliability::target_registry::TargetRegistry>,
    lane_generations: BTreeMap<String, crate::legacy_reliability::contracts::LaneGeneration>,
    environment_gate: std::sync::Arc<
        tokio::sync::Mutex<crate::legacy_reliability::environment_gate::EnvironmentGate>,
    >,
    closed_generations: &[crate::legacy_reliability::contracts::SensorGeneration],
) -> Result<crate::legacy_reliability::manager::ObserveOnlySnapshot, ObserverStartFailure> {
    use crate::legacy_reliability::contracts::{EventEnvelope, LegacySessionContext};
    use crate::legacy_reliability::status::{self, LegacyReliabilityStatus};

    if runtime_shutting_down(app) {
        return Err(ObserverStartFailure::before_generation(
            "Legacy observer start cancelled while the app is shutting down",
        ));
    }
    validate_exact_legacy_runtime(app, expected_runtime)
        .map_err(ObserverStartFailure::before_generation)?;
    if app
        .state::<AppState>()
        .legacy_manager
        .lock_recover()
        .is_some()
    {
        return Err(ObserverStartFailure::before_generation(
            "another Legacy Manager already owns the observer slot",
        ));
    }
    if !eyes_teardown_ready_for_start(app) {
        return Err(ObserverStartFailure::before_generation(
            "previous Eyes teardown is still pending",
        ));
    }
    if app.state::<AppState>().eyes.lock_recover().is_some() {
        return Err(ObserverStartFailure::before_generation(
            "another Eyes handle already owns the observer slot",
        ));
    }

    let sensor_generation = allocate_fresh_sensor_generation(app, closed_generations)
        .map_err(ObserverStartFailure::before_generation)?;
    status::publish(
        app,
        LegacyReliabilityStatus::starting(active_categories.clone(), session_id, sensor_generation),
    );

    let context =
        LegacySessionContext::new(session_id, active_categories.clone(), network_fingerprint);
    let (log_root, gate_clock_ms) = {
        let state = app.state::<AppState>();
        // This process clock is retained only for the shared Environment Gate
        // baseline. Manager health/evidence starts at zero for each sensor.
        (
            state.paths.legacy_reliability_logs_dir(),
            state.legacy_monotonic_ms(),
        )
    };
    let manager = crate::legacy_reliability::runtime::spawn_with_environment_gate(
        context,
        sensor_generation,
        std::sync::Arc::clone(&registry),
        lane_generations.clone(),
        log_root,
        Some(environment_gate),
        gate_clock_ms,
    )
    .map_err(|error| {
        ObserverStartFailure::after_generation(
            format!("Legacy Manager restart rejected: {error:?}"),
            sensor_generation,
        )
    })?;

    if let Err(error) = validate_exact_legacy_runtime(app, expected_runtime) {
        let mut failure = ObserverStartFailure::after_generation(error, sensor_generation);
        failure.append_cleanup(
            cleanup_uninstalled_observer(app, manager, None, expected_runtime).await,
        );
        return Err(failure);
    }

    let envelope = EventEnvelope::new(session_id, sensor_generation, registry.version());
    if let Err(error) = record_observer_restart_gap(app, &manager, envelope, expected_runtime).await
    {
        let mut failure = ObserverStartFailure::after_generation(error, sensor_generation);
        failure.append_cleanup(
            cleanup_uninstalled_observer(app, manager, None, expected_runtime).await,
        );
        return Err(failure);
    }
    if let Err(error) = validate_exact_legacy_runtime(app, expected_runtime) {
        let mut failure = ObserverStartFailure::after_generation(error, sensor_generation);
        failure.append_cleanup(
            cleanup_uninstalled_observer(app, manager, None, expected_runtime).await,
        );
        return Err(failure);
    }

    let eyes = match create_legacy_eyes(
        app,
        std::sync::Arc::clone(&registry),
        envelope,
        std::sync::Arc::new(lane_generations),
        manager.ingress.clone(),
    ) {
        Ok(eyes) => eyes,
        Err(error) => {
            let mut failure =
                ObserverStartFailure::after_generation(error.message, sensor_generation);
            failure.safe_to_restore &= error.safe_to_restore;
            failure.append_cleanup(
                cleanup_uninstalled_observer(app, manager, None, expected_runtime).await,
            );
            return Err(failure);
        }
    };
    let capture_started_at = Instant::now();

    let ready_snapshot =
        match wait_for_observer_clean_ready(app, &manager, expected_runtime, capture_started_at)
            .await
        {
            Ok(snapshot) => snapshot,
            Err(error) => {
                let mut failure = ObserverStartFailure::after_generation(error, sensor_generation);
                failure.append_cleanup(
                    cleanup_uninstalled_observer(app, manager, Some(eyes), expected_runtime).await,
                );
                return Err(failure);
            }
        };
    if let Err(error) = validate_exact_legacy_runtime(app, expected_runtime) {
        let mut failure = ObserverStartFailure::after_generation(error, sensor_generation);
        failure.append_cleanup(
            cleanup_uninstalled_observer(app, manager, Some(eyes), expected_runtime).await,
        );
        return Err(failure);
    }

    let final_validation = validate_exact_legacy_runtime(app, expected_runtime).and_then(|()| {
        if runtime_shutting_down(app) {
            Err("Legacy observer start cancelled while the app is shutting down".into())
        } else if app
            .state::<AppState>()
            .legacy_manager
            .lock_recover()
            .is_some()
        {
            Err("another Legacy Manager claimed the observer slot".into())
        } else if app.state::<AppState>().eyes.lock_recover().is_some() {
            Err("another Eyes handle claimed the observer slot".into())
        } else {
            Ok(())
        }
    });
    if let Err(error) = final_validation {
        let mut failure = ObserverStartFailure::after_generation(error, sensor_generation);
        failure.append_cleanup(
            cleanup_uninstalled_observer(app, manager, Some(eyes), expected_runtime).await,
        );
        return Err(failure);
    }

    let snapshot = ready_snapshot;
    let mut pending_manager = Some(manager);
    let mut pending_eyes = Some(eyes);
    let installed = {
        let state = app.state::<AppState>();
        let mut manager_slot = state.legacy_manager.lock_recover();
        let mut eyes_slot = state.eyes.lock_recover();
        if manager_slot.is_none() && eyes_slot.is_none() {
            let manager = pending_manager
                .take()
                .expect("pending observer Manager must be available once");
            manager.forward_public_status(app.clone());
            *eyes_slot = pending_eyes.take();
            *manager_slot = Some(manager);
            true
        } else {
            false
        }
    };
    if !installed {
        let mut failure = ObserverStartFailure::after_generation(
            "another Legacy Manager claimed the observer slot during install",
            sensor_generation,
        );
        failure.append_cleanup(
            cleanup_uninstalled_observer(
                app,
                pending_manager.expect("failed install retains pending Manager"),
                pending_eyes,
                expected_runtime,
            )
            .await,
        );
        return Err(failure);
    }

    status::publish_snapshot_if_owned(app, &snapshot);
    Ok(snapshot)
}

#[cfg(windows)]
struct CapturedObserverStopFailure {
    message: String,
    eyes_stopped_cleanly: bool,
}

#[cfg(windows)]
async fn stop_captured_observer(
    app: &AppHandle,
    manager: crate::legacy_reliability::runtime::LegacyReliabilityHandle,
    expected_runtime: &ExactLegacyRuntimeSnapshot,
) -> Result<(), CapturedObserverStopFailure> {
    let mut failures = Vec::new();

    // Close the old session fence first. Any callback racing with the bounded
    // Eyes stop can no longer mutate the retired Manager generation.
    manager.shutdown().await;
    if let Err(error) = validate_exact_legacy_runtime(app, expected_runtime) {
        failures.push(format!("runtime changed after old Manager stop: {error}"));
    }

    let app_for_eyes = app.clone();
    let eyes_stopped_cleanly =
        match tauri::async_runtime::spawn_blocking(move || stop_eyes(&app_for_eyes)).await {
            Ok(report)
                if !report.unresolved
                    && !report.outcomes.is_empty()
                    && report.outcomes.iter().all(|outcome| outcome.is_clean()) =>
            {
                true
            }
            Ok(report) => {
                failures.push(format!("Legacy Eyes bounded stop failed: {report:?}"));
                false
            }
            Err(error) => {
                failures.push(format!("Legacy Eyes stop worker failed: {error}"));
                false
            }
        };
    if let Err(error) = validate_exact_legacy_runtime(app, expected_runtime) {
        failures.push(format!("runtime changed after old Eyes stop: {error}"));
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(CapturedObserverStopFailure {
            message: failures.join("; "),
            eyes_stopped_cleanly,
        })
    }
}

#[cfg(windows)]
fn publish_failed_observer_blind(
    app: &AppHandle,
    captured: &CapturedLegacyObserver,
    sensor_generation: crate::legacy_reliability::contracts::SensorGeneration,
) {
    use crate::legacy_reliability::status::{self, LegacyReliabilityStatus};

    status::publish_if_owned(
        app,
        captured.snapshot.session.session_id,
        sensor_generation,
        LegacyReliabilityStatus::blind(
            captured.snapshot.session.active_categories.clone(),
            captured.snapshot.session.session_id,
            sensor_generation,
        ),
    );
}

#[cfg(windows)]
async fn restore_captured_observer(
    app: &AppHandle,
    expected_runtime: &ExactLegacyRuntimeSnapshot,
    captured: &CapturedLegacyObserver,
    closed_generations: &[crate::legacy_reliability::contracts::SensorGeneration],
) -> ObserverRestorationState {
    if runtime_shutting_down(app) {
        return ObserverRestorationState::Skipped("application is shutting down".into());
    }
    if let Err(error) = validate_exact_legacy_runtime(app, expected_runtime) {
        return ObserverRestorationState::Skipped(format!("DPI runtime changed: {error}"));
    }
    if app
        .state::<AppState>()
        .legacy_manager
        .lock_recover()
        .is_some()
        || app.state::<AppState>().eyes.lock_recover().is_some()
    {
        return ObserverRestorationState::Skipped(
            "another observer already owns Manager or Eyes".into(),
        );
    }

    let restored = start_legacy_observer_generation(
        app,
        expected_runtime,
        captured.snapshot.session.session_id,
        captured
            .snapshot
            .session
            .network_fingerprint_at_start
            .clone(),
        captured.snapshot.session.active_categories.clone(),
        std::sync::Arc::clone(&captured.registry),
        captured.snapshot.session.lane_generations.clone(),
        std::sync::Arc::clone(&captured.environment_gate),
        closed_generations,
    )
    .await;
    if let Err(error) = validate_exact_legacy_runtime(app, expected_runtime) {
        let mut cleanup_note = String::new();
        match &restored {
            Ok(snapshot) => {
                let cleanup = cleanup_installed_observer_generation(app, snapshot).await;
                if !cleanup.failures.is_empty() {
                    cleanup_note = format!(
                        "; installed observer cleanup: {}",
                        cleanup.failures.join("; ")
                    );
                }
                publish_failed_observer_blind(app, captured, snapshot.session.sensor_generation)
            }
            Err(failure) => {
                if let Some(generation) = failure.sensor_generation {
                    publish_failed_observer_blind(app, captured, generation);
                }
            }
        }
        return ObserverRestorationState::Failed(format!(
            "DPI runtime changed while restoring observer: {error}{cleanup_note}"
        ));
    }

    match restored {
        Ok(snapshot) => ObserverRestorationState::Restored(snapshot.session.sensor_generation),
        Err(failure) => {
            if let Some(generation) = failure.sensor_generation {
                publish_failed_observer_blind(app, captured, generation);
            } else {
                crate::legacy_reliability::status::publish_current_blind(app);
            }
            ObserverRestorationState::Failed(failure.message)
        }
    }
}

/// Transactionally installs a new immutable Legacy sensor/Manager generation
/// without touching a winws process. The caller must hold `dpi_gate` for the
/// entire assisted transaction. Any failure after teardown attempts to restore
/// the captured observer plan on another fresh sensor generation.
#[cfg(windows)]
pub(crate) async fn replace_legacy_observer_locked(
    app: &AppHandle,
    expected_dpi_generation: u64,
    session_id: crate::legacy_reliability::contracts::SessionId,
    network_fingerprint: crate::legacy_reliability::contracts::NetworkFingerprint,
    registry: std::sync::Arc<crate::legacy_reliability::target_registry::TargetRegistry>,
    lane_generations: std::collections::BTreeMap<
        String,
        crate::legacy_reliability::contracts::LaneGeneration,
    >,
    target_expectation: LegacyObserverTargetExpectation,
) -> Result<crate::legacy_reliability::manager::ObserveOnlySnapshot, String> {
    let untouched_error =
        |error: String| observer_transaction_error(error, ObserverRestorationState::Untouched);

    if runtime_shutting_down(app) {
        return Err(untouched_error(
            "Legacy observer restart cancelled while the app is shutting down".into(),
        ));
    }
    let captured = capture_legacy_observer(app).map_err(&untouched_error)?;
    if app.state::<AppState>().eyes.lock_recover().is_none() {
        return Err(untouched_error(
            "Legacy Eyes handle is absent before observer restart".into(),
        ));
    }
    let expected_runtime = capture_exact_legacy_runtime(
        &app.state::<AppState>().dpi.lock_recover(),
        expected_dpi_generation,
    )
    .map_err(&untouched_error)?;
    validate_observer_transition(
        &captured.snapshot,
        &captured.registry,
        session_id,
        &network_fingerprint,
        &registry,
        &lane_generations,
        &expected_runtime,
        &target_expectation,
    )
    .map_err(&untouched_error)?;

    let previous = take_captured_manager(app, &captured).map_err(&untouched_error)?;
    if let Err(error) = validate_exact_legacy_runtime(app, &expected_runtime) {
        let mut detached = Some(previous);
        let reinserted = {
            let state = app.state::<AppState>();
            let mut slot = state.legacy_manager.lock_recover();
            if slot.is_none() {
                *slot = detached.take();
                true
            } else {
                false
            }
        };
        if reinserted {
            return Err(untouched_error(error));
        }
        detached
            .expect("failed reinsert retains detached Legacy Manager")
            .shutdown()
            .await;
        let runtime_after_shutdown = validate_exact_legacy_runtime(app, &expected_runtime)
            .err()
            .map(|reason| format!("; runtime after detached Manager shutdown: {reason}"))
            .unwrap_or_default();
        publish_failed_observer_blind(app, &captured, captured.snapshot.session.sensor_generation);
        let restoration = ObserverRestorationState::Skipped(
            "another Manager claimed the slot while the captured Manager was detached".into(),
        );
        return Err(observer_transaction_error(
            format!("{error}{runtime_after_shutdown}"),
            restoration,
        ));
    }

    let old_sensor_generation = captured.snapshot.session.sensor_generation;
    if let Err(failure) = stop_captured_observer(app, previous, &expected_runtime).await {
        publish_failed_observer_blind(app, &captured, old_sensor_generation);
        let restoration = if failure.eyes_stopped_cleanly {
            restore_captured_observer(app, &expected_runtime, &captured, &[old_sensor_generation])
                .await
        } else {
            crate::legacy_reliability::status::publish_current_blind(app);
            ObserverRestorationState::Skipped(
                "the retired Eyes generation did not stop cleanly; opening a second WinDivert observer is unsafe"
                    .into(),
            )
        };
        return Err(observer_transaction_error(failure.message, restoration));
    }

    let candidate = start_legacy_observer_generation(
        app,
        &expected_runtime,
        session_id,
        network_fingerprint,
        captured.snapshot.session.active_categories.clone(),
        registry,
        lane_generations,
        std::sync::Arc::clone(&captured.environment_gate),
        &[old_sensor_generation],
    )
    .await;
    if let Err(error) = validate_exact_legacy_runtime(app, &expected_runtime) {
        let mut cleanup_note = String::new();
        match &candidate {
            Ok(snapshot) => {
                let cleanup = cleanup_installed_observer_generation(app, snapshot).await;
                if !cleanup.failures.is_empty() {
                    cleanup_note = format!(
                        "; installed observer cleanup: {}",
                        cleanup.failures.join("; ")
                    );
                }
                publish_failed_observer_blind(app, &captured, snapshot.session.sensor_generation)
            }
            Err(failure) => {
                if let Some(generation) = failure.sensor_generation {
                    publish_failed_observer_blind(app, &captured, generation);
                }
            }
        }
        return Err(observer_transaction_error(
            format!("DPI runtime changed while starting candidate observer: {error}{cleanup_note}"),
            ObserverRestorationState::Skipped("exact winws owners no longer match".into()),
        ));
    }
    match candidate {
        Ok(snapshot) => Ok(snapshot),
        Err(failure) => {
            if let Some(generation) = failure.sensor_generation {
                publish_failed_observer_blind(app, &captured, generation);
            } else {
                crate::legacy_reliability::status::publish_current_blind(app);
            }
            let mut closed_generations = vec![old_sensor_generation];
            if let Some(generation) = failure.sensor_generation {
                closed_generations.push(generation);
            }
            let restoration = if failure.safe_to_restore {
                restore_captured_observer(app, &expected_runtime, &captured, &closed_generations)
                    .await
            } else {
                crate::legacy_reliability::status::publish_current_blind(app);
                ObserverRestorationState::Skipped(
                    "the failed Eyes generation did not stop cleanly; opening a second WinDivert observer is unsafe"
                        .into(),
                )
            };
            Err(observer_transaction_error(failure.message, restoration))
        }
    }
}

#[cfg(not(windows))]
pub(crate) async fn replace_legacy_observer_locked(
    _app: &AppHandle,
    _expected_dpi_generation: u64,
    _session_id: crate::legacy_reliability::contracts::SessionId,
    _network_fingerprint: crate::legacy_reliability::contracts::NetworkFingerprint,
    _registry: std::sync::Arc<crate::legacy_reliability::target_registry::TargetRegistry>,
    _lane_generations: std::collections::BTreeMap<
        String,
        crate::legacy_reliability::contracts::LaneGeneration,
    >,
    _target_expectation: LegacyObserverTargetExpectation,
) -> Result<crate::legacy_reliability::manager::ObserveOnlySnapshot, String> {
    Err("Scoped Legacy observer restart поддерживается только на Windows".into())
}

#[derive(Debug)]
struct StopAllReport {
    eyes_clean: bool,
    unverified_pids: Vec<u32>,
}

impl StopAllReport {
    fn is_clean(&self) -> bool {
        self.eyes_clean && self.unverified_pids.is_empty()
    }

    fn start_blocker(&self, action: &str) -> String {
        let mut blockers = Vec::new();
        if !self.eyes_clean {
            blockers.push("остановка сетевого наблюдателя не подтверждена".to_string());
        }
        if !self.unverified_pids.is_empty() {
            blockers.push(format!(
                "остановка winws PID {} не подтверждена",
                self.unverified_pids
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        format!("{action} отменён: {}.", blockers.join("; "))
    }
}

fn unverified_tracked_processes(
    tracked: &[DpiProc],
    outcomes: &[crate::dpi_supervisor::ProcessStopOutcome],
) -> Vec<DpiProc> {
    tracked
        .iter()
        .filter(|process| {
            !outcomes
                .iter()
                .find(|outcome| outcome.pid == process.pid)
                .is_some_and(|outcome| outcome.original_exited())
        })
        .cloned()
        .collect()
}

/// Останавливает все свои DPI-процессы и ждёт подтверждения teardown. Владение
/// процессом сохраняется, пока bounded reaper не подтвердил завершение именно
/// исходного PID/identity. Это позволяет следующему stop повторить попытку и не
/// даёт start-path менять auto-hostlist рядом с ещё живым winws.
/// DNS не сбрасывается здесь: host-mapping paths вызывают `flush_dns` условно.
async fn stop_all_with_report(app: &AppHandle) -> StopAllReport {
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
            Ok(report) => !report.unresolved,
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

    let tracked_processes = {
        let state = app.state::<AppState>();
        let mut d = state.dpi.lock_recover();
        d.advance_generation();
        let processes = d.procs.values().cloned().collect::<Vec<_>>();
        for process in &processes {
            d.stopping.insert(process.pid);
        }
        d.procs.clear();
        d.active_launch = None;
        processes
    };

    let owned_processes = tracked_processes
        .iter()
        .map(|process| crate::dpi_supervisor::OwnedProcess {
            pid: process.pid,
            identity: process.process_identity,
        })
        .collect::<Vec<_>>();
    let process_outcomes = stop_owned_processes_async(owned_processes).await;
    let retained_processes = unverified_tracked_processes(&tracked_processes, &process_outcomes);
    let processes_clean = retained_processes.is_empty();
    for process in &retained_processes {
        let state = process_outcomes
            .iter()
            .find(|outcome| outcome.pid == process.pid)
            .map(|outcome| format!("{:?}", outcome.state))
            .unwrap_or_else(|| "MissingOutcome".to_string());
        util::emit_log(
            app,
            "warn",
            "dpi",
            &format!("process teardown pid={} state={state}", process.pid),
        );
    }
    {
        let state = app.state::<AppState>();
        let mut dpi = state.dpi.lock_recover();
        for process in &tracked_processes {
            dpi.stopping.remove(&process.pid);
        }
        for process in &retained_processes {
            dpi.procs
                .entry(process.pid)
                .or_insert_with(|| process.clone());
        }
    }
    emit_status(app);

    // Sleep остаётся только fallback, когда exit/handle evidence неполно.
    if !(eyes_clean && processes_clean) {
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    let mut unverified_pids = retained_processes
        .iter()
        .map(|process| process.pid)
        .collect::<Vec<_>>();
    unverified_pids.sort_unstable();
    StopAllReport {
        eyes_clean,
        unverified_pids,
    }
}

pub async fn stop_all(app: &AppHandle) {
    let report = stop_all_with_report(app).await;
    if !report.is_clean() {
        util::emit_log(
            app,
            "error",
            "dpi",
            &report.start_blocker("Полная остановка DPI"),
        );
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
    let stop = stop_all_with_report(app).await;
    if !stop.is_clean() {
        return Err(stop.start_blocker("Запуск Zapret2"));
    }
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
                lane_generation: None,
                config_fingerprint: None,
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
    // Read-only validation deliberately precedes teardown: malformed configs,
    // unsafe paths and an oversized migration plan must leave the currently
    // working bypass untouched.
    preflight_legacy_launch_paths(app, configs)?;
    let stop = stop_all_with_report(app).await;
    if !stop.is_clean() {
        return Err(stop.start_blocker("Запуск Legacy"));
    }
    if runtime_shutting_down(app) {
        return Err("Запуск отменён: приложение завершает работу.".to_string());
    }
    let prepared = prepare_legacy_launch_paths(app, configs, true)?;
    let mut started = Vec::new();
    let mut started_pairs = Vec::new();
    for (category, config_file) in configs {
        if runtime_shutting_down(app) {
            stop_all(app).await;
            return Err("Запуск отменён: приложение завершает работу.".to_string());
        }
        let conf = prepared_legacy_config_path(&prepared, category, config_file)?;
        match start_prepared(app, category, config_file, conf).await {
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
    let (start_generation, exact_runtime, initial_reliability_status) = {
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
        let initial_reliability_status = if exact_runtime {
            crate::legacy_reliability::status::LegacyReliabilityStatus::starting(
                active_categories.clone(),
                session_id,
                sensor_generation,
            )
        } else if dpi
            .procs
            .values()
            .any(|process| process.generation == generation && process.engine == "legacy")
        {
            crate::legacy_reliability::status::LegacyReliabilityStatus::blind(
                active_categories.clone(),
                session_id,
                sensor_generation,
            )
        } else {
            crate::legacy_reliability::status::LegacyReliabilityStatus::inactive()
        };
        (generation, exact_runtime, initial_reliability_status)
    };
    // The publisher reads the process-backed launch spec from `state.dpi`.
    // Publish only after dropping the commit guard; std::Mutex is not reentrant.
    #[cfg(windows)]
    crate::legacy_reliability::status::publish(app, initial_reliability_status);

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
                {
                    let state = app.state::<AppState>();
                    let mut dpi = state.dpi.lock_recover();
                    for process in dpi.procs.values_mut().filter(|process| {
                        process.generation == start_generation && process.engine == "legacy"
                    }) {
                        process.lane_generation = lane_generations.get(&process.category).copied();
                        process.config_fingerprint = registry
                            .config_fingerprint(&process.category, &process.config_file)
                            .ok()
                            .map(|fingerprint| fingerprint.as_hex().to_owned());
                    }
                }
                match crate::legacy_reliability::runtime::spawn(
                    context,
                    sensor_generation,
                    std::sync::Arc::clone(&registry),
                    lane_generations.clone(),
                    reliability_log_root,
                    app.state::<AppState>().legacy_monotonic_ms(),
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

    let stop = stop_all_with_report(app).await;
    if !stop.is_clean() {
        util::emit_log(
            app,
            "error",
            "dpi",
            &stop.start_blocker("Проверка конфигурации"),
        );
        return false;
    }
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

    #[cfg(windows)]
    #[test]
    fn partial_eyes_start_timeout_is_retained_until_the_worker_exits() {
        let (release, blocked) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _ = blocked.recv();
        });
        let mut teardown = crate::dpi_supervisor::WorkerTeardown::new(vec![(
            "eyes-capture-partial-start",
            worker,
        )]);
        assert!(!teardown.is_resolved());
        assert!(teardown
            .wait_bounded(Duration::ZERO)
            .iter()
            .any(|outcome| { outcome.state == crate::dpi_supervisor::WorkerStopState::TimedOut }));

        let error =
            crate::eyes::EyesStartError::partial("tracker spawn failed", false, Some(teardown));
        let pending = std::sync::Mutex::new(Vec::new());
        let (message, safe_to_retry) = retain_eyes_start_failure_in(&pending, error);

        assert_eq!(message, "tracker spawn failed");
        assert!(!safe_to_retry);
        let mut retained = pending.lock_recover().pop().unwrap();
        assert!(!retained.is_resolved());

        release.send(()).unwrap();
        retained.wait_bounded(Duration::from_secs(1));
        assert!(retained.is_resolved());
    }

    #[cfg(windows)]
    #[test]
    fn eyes_teardown_ticket_remains_atomically_visible_while_polled() {
        let (release, blocked) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _ = blocked.recv();
        });
        let pending = std::sync::Arc::new(std::sync::Mutex::new(vec![
            crate::dpi_supervisor::WorkerTeardown::new(vec![("eyes-capture", worker)]),
        ]));
        let polled = std::sync::Arc::clone(&pending);
        let polling = std::thread::spawn(move || {
            poll_pending_eyes_teardowns_in(&polled, Duration::from_secs(5))
        });

        let lock_deadline = Instant::now() + Duration::from_secs(1);
        loop {
            match pending.try_lock() {
                Err(std::sync::TryLockError::WouldBlock) => break,
                Err(std::sync::TryLockError::Poisoned(_)) => panic!("teardown slot was poisoned"),
                Ok(guard) => {
                    drop(guard);
                    assert!(
                        Instant::now() < lock_deadline,
                        "poller never made the pending ticket atomically visible"
                    );
                    std::thread::yield_now();
                }
            }
        }

        release.send(()).unwrap();
        let report = polling.join().unwrap();
        assert!(!report.unresolved);
        assert!(pending.lock_recover().is_empty());
    }

    #[test]
    fn crash_retry_is_one_shot_until_the_lane_is_healthy_again() {
        assert_eq!(
            crash_retry_decision(false, false, false),
            CrashRetryDecision::Schedule
        );
        assert_eq!(
            crash_retry_decision(false, true, false),
            CrashRetryDecision::Exhausted
        );
        assert_eq!(
            crash_retry_decision(true, false, false),
            CrashRetryDecision::IgnoreIntentional
        );
        assert_eq!(
            crash_retry_decision(false, true, true),
            CrashRetryDecision::Schedule
        );
    }

    fn exact_legacy_process(
        pid: u32,
        identity: u64,
        runtime_generation: u64,
        lane_generation: u64,
        category: &str,
        config_file: &str,
        fingerprint: &str,
    ) -> DpiProc {
        DpiProc {
            pid,
            category: category.into(),
            config_file: config_file.into(),
            generation: runtime_generation,
            lane_generation: Some(crate::legacy_reliability::contracts::LaneGeneration::new(
                lane_generation,
            )),
            config_fingerprint: Some(fingerprint.into()),
            engine: "legacy".into(),
            process_identity: Some(crate::dpi_supervisor::ProcessIdentity::from_raw(identity)),
        }
    }

    fn two_lane_runtime() -> crate::state::DpiState {
        let discord = exact_legacy_process(
            10,
            110,
            7,
            1,
            "discord",
            "discord_1.conf",
            "discord-fingerprint-1",
        );
        let youtube = exact_legacy_process(
            20,
            120,
            7,
            4,
            "youtube_twitch",
            "youtube_1.conf",
            "youtube-fingerprint-1",
        );
        crate::state::DpiState {
            generation: 7,
            procs: std::collections::HashMap::from([
                (discord.pid, discord),
                (youtube.pid, youtube),
            ]),
            active_launch: Some(DpiLaunchSpec::Legacy {
                selections: vec![
                    ("discord".into(), "discord_1.conf".into()),
                    ("youtube_twitch".into(), "youtube_1.conf".into()),
                ],
            }),
            ..crate::state::DpiState::default()
        }
    }

    #[test]
    fn global_stop_retains_only_processes_without_verified_teardown() {
        let runtime = two_lane_runtime();
        let mut tracked = runtime.procs.values().cloned().collect::<Vec<_>>();
        tracked.sort_by_key(|process| process.pid);
        let outcomes = vec![
            crate::dpi_supervisor::ProcessStopOutcome {
                pid: 10,
                state: crate::dpi_supervisor::ProcessStopState::Exited,
            },
            crate::dpi_supervisor::ProcessStopOutcome {
                pid: 20,
                state: crate::dpi_supervisor::ProcessStopState::TimedOut,
            },
        ];

        let retained = unverified_tracked_processes(&tracked, &outcomes);

        assert_eq!(retained.len(), 1);
        assert_eq!(retained[0].pid, 20);
        assert_eq!(retained[0].process_identity, tracked[1].process_identity);
        assert!(unverified_tracked_processes(
            &retained,
            &[crate::dpi_supervisor::ProcessStopOutcome {
                pid: 20,
                state: crate::dpi_supervisor::ProcessStopState::AlreadyExited,
            }]
        )
        .is_empty());
    }

    #[test]
    fn global_stop_treats_missing_reaper_outcome_as_unverified() {
        let runtime = two_lane_runtime();
        let tracked = runtime.procs.values().cloned().collect::<Vec<_>>();

        let retained = unverified_tracked_processes(&tracked, &[]);

        assert_eq!(retained.len(), tracked.len());
    }

    fn observer_registries() -> (
        std::sync::Arc<crate::legacy_reliability::target_registry::TargetRegistry>,
        std::sync::Arc<crate::legacy_reliability::target_registry::TargetRegistry>,
    ) {
        use crate::legacy_reliability::target_registry::{LegacyConfigRecord, TargetRegistry};

        let records = vec![
            LegacyConfigRecord::new(
                "discord",
                "discord_1.conf",
                "--wf-tcp=443 --hostlist=lists/discord-1.txt",
            )
            .with_hostlist("lists/discord-1.txt", "discord.example\n"),
            LegacyConfigRecord::new(
                "discord",
                "discord_2.conf",
                "--wf-tcp=443 --hostlist=lists/discord-2.txt",
            )
            .with_hostlist("lists/discord-2.txt", "candidate.discord.example\n"),
            LegacyConfigRecord::new(
                "youtube_twitch",
                "youtube_1.conf",
                "--wf-tcp=443 --hostlist=lists/youtube-1.txt",
            )
            .with_hostlist("lists/youtube-1.txt", "youtube.example\n"),
        ];
        let previous = TargetRegistry::from_records_with_active_selections(
            records.clone(),
            [
                ("discord", "discord_1.conf"),
                ("youtube_twitch", "youtube_1.conf"),
            ],
        )
        .unwrap();
        let candidate = TargetRegistry::from_records_with_active_selections(
            records,
            [
                ("discord", "discord_2.conf"),
                ("youtube_twitch", "youtube_1.conf"),
            ],
        )
        .unwrap();
        (
            std::sync::Arc::new(previous),
            std::sync::Arc::new(candidate),
        )
    }

    fn runtime_for_registry(
        registry: &crate::legacy_reliability::target_registry::TargetRegistry,
    ) -> crate::state::DpiState {
        let mut dpi = two_lane_runtime();
        for process in dpi.procs.values_mut() {
            process.config_fingerprint = Some(
                registry
                    .config_fingerprint(&process.category, &process.config_file)
                    .unwrap()
                    .as_hex()
                    .to_owned(),
            );
        }
        dpi
    }

    fn observer_snapshot(
        registry: std::sync::Arc<crate::legacy_reliability::target_registry::TargetRegistry>,
        lane_generations: BTreeMap<String, crate::legacy_reliability::contracts::LaneGeneration>,
    ) -> crate::legacy_reliability::manager::ObserveOnlySnapshot {
        use crate::legacy_reliability::contracts::{
            LegacySessionContext, NetworkFingerprint, SensorGeneration, SessionId,
        };

        let (ingress, receiver) = crate::legacy_reliability::ingress::channel();
        let manager = crate::legacy_reliability::manager::ObserveOnlyManager::new_with_registry(
            LegacySessionContext::new(
                SessionId::new(31),
                vec!["discord".into(), "youtube_twitch".into()],
                NetworkFingerprint::Stable {
                    key: "observer-transaction-test".into(),
                },
            ),
            SensorGeneration::new(9),
            registry,
            lane_generations,
            ingress.counters(),
            receiver,
        )
        .unwrap();
        manager.snapshot()
    }

    fn lanes(
        discord: u64,
        youtube: u64,
    ) -> BTreeMap<String, crate::legacy_reliability::contracts::LaneGeneration> {
        use crate::legacy_reliability::contracts::LaneGeneration;
        BTreeMap::from([
            ("discord".into(), LaneGeneration::new(discord)),
            ("youtube_twitch".into(), LaneGeneration::new(youtube)),
        ])
    }

    #[test]
    fn observer_transition_accepts_one_armed_target_and_exact_neighbors() {
        let (previous_registry, candidate_registry) = observer_registries();
        let dpi = runtime_for_registry(&previous_registry);
        let runtime = capture_exact_legacy_runtime(&dpi, 7).unwrap();
        let previous = observer_snapshot(std::sync::Arc::clone(&previous_registry), lanes(1, 4));
        let target = LegacyObserverTargetExpectation::Present(recovery_process_owner(
            &runtime.owners["discord"],
        ));

        let changed = validate_observer_transition(
            &previous,
            &previous_registry,
            previous.session.session_id,
            &previous.session.network_fingerprint_at_start,
            &candidate_registry,
            &lanes(2, 4),
            &runtime,
            &target,
        )
        .unwrap();

        assert_eq!(changed, "discord");
        assert_eq!(runtime.owners["youtube_twitch"].pid, 20);
    }

    #[test]
    fn observer_transition_accepts_rollback_after_target_lane_is_absent() {
        let (previous_registry, candidate_registry) = observer_registries();
        let mut dpi = runtime_for_registry(&previous_registry);
        dpi.procs.remove(&10);
        dpi.active_launch = Some(DpiLaunchSpec::Legacy {
            selections: vec![("youtube_twitch".into(), "youtube_1.conf".into())],
        });
        let runtime = capture_exact_legacy_runtime(&dpi, 7).unwrap();
        let previous = observer_snapshot(std::sync::Arc::clone(&candidate_registry), lanes(2, 4));

        let changed = validate_observer_transition(
            &previous,
            &candidate_registry,
            previous.session.session_id,
            &previous.session.network_fingerprint_at_start,
            &previous_registry,
            &lanes(3, 4),
            &runtime,
            &LegacyObserverTargetExpectation::Absent,
        )
        .unwrap();

        assert_eq!(changed, "discord");
        assert!(!runtime.owners.contains_key("discord"));
        assert_eq!(runtime.owners["youtube_twitch"].pid, 20);
    }

    #[test]
    fn observer_transition_rejects_forward_swap_after_target_exit() {
        let (previous_registry, candidate_registry) = observer_registries();
        let mut dpi = runtime_for_registry(&previous_registry);
        let expected_target = LegacyObserverTargetExpectation::Present(recovery_process_owner(
            &dpi.procs.get(&10).unwrap().exact_legacy_owner().unwrap(),
        ));
        dpi.procs.remove(&10);
        dpi.active_launch = Some(DpiLaunchSpec::Legacy {
            selections: vec![("youtube_twitch".into(), "youtube_1.conf".into())],
        });
        let runtime = capture_exact_legacy_runtime(&dpi, 7).unwrap();
        let previous = observer_snapshot(std::sync::Arc::clone(&previous_registry), lanes(1, 4));

        let error = validate_observer_transition(
            &previous,
            &previous_registry,
            previous.session.session_id,
            &previous.session.network_fingerprint_at_start,
            &candidate_registry,
            &lanes(2, 4),
            &runtime,
            &expected_target,
        )
        .unwrap_err();

        assert!(error.contains("exited before observer teardown"));
    }

    #[test]
    fn observer_transition_rejects_neighbor_fingerprint_drift() {
        let (previous_registry, candidate_registry) = observer_registries();
        let mut dpi = runtime_for_registry(&previous_registry);
        dpi.procs.get_mut(&20).unwrap().config_fingerprint = Some("drifted".into());
        let runtime = capture_exact_legacy_runtime(&dpi, 7).unwrap();
        let previous = observer_snapshot(std::sync::Arc::clone(&previous_registry), lanes(1, 4));
        let target = LegacyObserverTargetExpectation::Present(recovery_process_owner(
            &runtime.owners["discord"],
        ));

        let error = validate_observer_transition(
            &previous,
            &previous_registry,
            previous.session.session_id,
            &previous.session.network_fingerprint_at_start,
            &candidate_registry,
            &lanes(2, 4),
            &runtime,
            &target,
        )
        .unwrap_err();

        assert!(error.contains("neighboring Legacy category youtube_twitch"));
    }

    #[test]
    fn observer_transition_rejects_more_than_one_changed_lane() {
        let (previous_registry, candidate_registry) = observer_registries();
        let dpi = runtime_for_registry(&previous_registry);
        let runtime = capture_exact_legacy_runtime(&dpi, 7).unwrap();
        let previous = observer_snapshot(std::sync::Arc::clone(&previous_registry), lanes(1, 4));
        let target = LegacyObserverTargetExpectation::Present(recovery_process_owner(
            &runtime.owners["discord"],
        ));

        let error = validate_observer_transition(
            &previous,
            &previous_registry,
            previous.session.session_id,
            &previous.session.network_fingerprint_at_start,
            &candidate_registry,
            &lanes(2, 5),
            &runtime,
            &target,
        )
        .unwrap_err();

        assert!(error.contains("exactly one lane generation"));
    }

    #[test]
    fn captured_runtime_rejects_neighbor_pid_identity_reuse() {
        let dpi = two_lane_runtime();
        let captured = capture_exact_legacy_runtime(&dpi, 7).unwrap();
        let mut reused = dpi;
        reused.procs.get_mut(&20).unwrap().process_identity =
            Some(crate::dpi_supervisor::ProcessIdentity::from_raw(999));

        let error = validate_captured_legacy_runtime(&reused, &captured).unwrap_err();

        assert!(error.contains("exact Legacy runtime owners changed"));
    }

    #[test]
    fn observer_transaction_error_keeps_original_and_restoration_state() {
        use crate::legacy_reliability::contracts::SensorGeneration;

        let error = observer_transaction_error(
            "candidate Eyes failed",
            ObserverRestorationState::Restored(SensorGeneration::new(12)),
        );

        assert!(error.starts_with("candidate Eyes failed;"));
        assert!(error.contains("sensor generation 12"));
    }

    #[test]
    fn exact_legacy_exit_preserves_neighbor_and_aggregate_launch() {
        let mut dpi = two_lane_runtime();
        let neighbor = dpi.procs.get(&20).unwrap().clone();
        let fence = LegacyCompatibilityFence::from_process(dpi.procs.get(&10).unwrap());

        let result = finalize_legacy_exit(&mut dpi, &fence, Some(17), 2_500);

        let LegacyExitFinalization::Exact(event) = result else {
            panic!("expected exact process event, got {result:?}");
        };
        assert_eq!(event.owner.pid, 10);
        assert!(!event.intentional);
        assert_eq!(event.exit_code, Some(17));
        assert_eq!(dpi.procs.get(&20), Some(&neighbor));
        assert_eq!(
            dpi.active_launch,
            Some(DpiLaunchSpec::Legacy {
                selections: vec![("youtube_twitch".into(), "youtube_1.conf".into())],
            })
        );
    }

    #[test]
    fn intentional_exact_stop_consumes_only_exact_owner_marker() {
        let mut dpi = two_lane_runtime();
        let snapshot = dpi.snapshot_legacy_category("discord").unwrap();
        let fence = LegacyCompatibilityFence::from_process(dpi.procs.get(&10).unwrap());
        dpi.mark_legacy_category_stopping(&snapshot).unwrap();

        let result = finalize_legacy_exit(&mut dpi, &fence, Some(0), 100);

        let LegacyExitFinalization::Exact(event) = result else {
            panic!("expected exact process event, got {result:?}");
        };
        assert!(event.intentional);
        assert!(dpi.legacy_stopping.is_empty());
        assert!(dpi.procs.contains_key(&20));
    }

    #[test]
    fn stale_pid_reuse_exit_does_not_touch_new_owner_or_marker() {
        let mut dpi = two_lane_runtime();
        let stale_fence = LegacyCompatibilityFence::from_process(dpi.procs.get(&10).unwrap());
        let replacement = exact_legacy_process(
            10,
            999,
            7,
            2,
            "discord",
            "discord_1.conf",
            "discord-fingerprint-1",
        );
        let replacement_owner = replacement.exact_legacy_owner().unwrap();
        dpi.procs.insert(10, replacement.clone());
        dpi.legacy_stopping.insert(replacement_owner.clone());
        let launch_before = dpi.active_launch.clone();

        let result = finalize_legacy_exit(&mut dpi, &stale_fence, Some(0), 500);

        assert_eq!(result, LegacyExitFinalization::Ignored);
        assert_eq!(dpi.procs.get(&10), Some(&replacement));
        assert_eq!(dpi.active_launch, launch_before);
        assert!(dpi.legacy_stopping.contains(&replacement_owner));
    }

    #[test]
    fn unfenced_startup_exit_still_preserves_neighbor_selection() {
        let mut dpi = two_lane_runtime();
        let process = dpi.procs.get_mut(&10).unwrap();
        process.lane_generation = None;
        process.config_fingerprint = None;
        let fence = LegacyCompatibilityFence::from_process(process);
        let neighbor = dpi.procs.get(&20).unwrap().clone();

        let result = finalize_legacy_exit(&mut dpi, &fence, Some(1), 300);

        assert_eq!(
            result,
            LegacyExitFinalization::Compatibility { intentional: false }
        );
        assert_eq!(dpi.procs.get(&20), Some(&neighbor));
        assert_eq!(
            dpi.active_launch,
            Some(DpiLaunchSpec::Legacy {
                selections: vec![("youtube_twitch".into(), "youtube_1.conf".into())],
            })
        );
    }

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
