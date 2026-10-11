//! Explicit per-service selection of service-owned named presets.
//! Each measured runtime restores the exact launch request. Search is finite
//! and user initiated; sources never become service launch arguments.
use super::*;
use super::{zapret2_measure as measurement, zapret2_tournament as tournament};
use crate::dpi_engine::{LoadedPack, Zapret2ProfileSelection};
use tauri::Emitter;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogProfile {
    pub composition_eligible: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub composition: Option<obsession_runtime_protocol::zapret2_composition::DiscordComposition>,
    pub category: String,
    pub profile_id: String,
    pub name: String,
    pub transport: String,
    pub experimental: bool,
    pub data_plane: bool,
    pub level: u8,
    pub selected: bool,
    pub candidate: Option<obsession_runtime_protocol::zapret2_recipe::CandidateSource>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PickerResult {
    category: String,
    transport: String,
    profile_id: Option<String>,
    status: &'static str,
    checked: usize,
    total: usize,
    report: Option<legacy_test::TestReport>,
    tournament: TournamentResult,
    failure: Option<String>,
    history_warning: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TournamentResult {
    run_id: String,
    required_confirmations: usize,
    max_attempts: usize,
    budget_ms: u64,
    max_body_bytes: usize,
    elapsed_ms: u64,
    bytes_read: usize,
    stop_reason: String,
    candidates: Vec<tournament::CandidateSummary>,
    rounds: Vec<measurement::RoundMeasurement>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Progress<'a> {
    category: &'a str,
    transport: &'a str,
    current: usize,
    total: usize,
    profile_id: &'a str,
    label: &'a str,
    phase: &'a str,
}

pub(crate) fn migrate_saved_profiles(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    if state
        .settings
        .lock_recover()
        .zapret2_profile_migration_version
        >= 1
    {
        return Ok(());
    }
    let pack = crate::dpi_engine::load_pack(&state.paths.strategy_pack_dir("builtin"))?;
    let imported = crate::zapret2_saved_profiles::import_saved(&state.paths.base_dir, &pack);
    crate::commands::mutate_settings(app, |settings| {
        if settings.zapret2_profile_migration_version >= 1 {
            return;
        }
        if settings.zapret2_profiles.is_empty() {
            settings.zapret2_profiles = imported;
        }
        // The retired global override must not silently defeat a named choice.
        // ClientHello remains available as one explicitly selected TLS profile.
        settings.zapret2_clienthello = false;
        settings.zapret2_profile_migration_version = 1;
    })?;
    Ok(())
}

fn slot(strategy: &crate::dpi_engine::manifest::StrategyDef) -> &'static str {
    if crate::dpi_engine::zapret2::is_voice_profile(strategy) {
        "voice"
    } else if strategy.filter_l7.iter().any(|item| item == "tls") {
        "tls"
    } else if strategy.filter_l7.iter().any(|item| item == "quic") {
        "quic"
    } else if strategy.filter_l7.iter().any(|item| item == "http") {
        "http"
    } else if strategy.filter_udp.is_some() {
        "udp"
    } else {
        "tcp"
    }
}

fn profile_name(strategy: &crate::dpi_engine::manifest::StrategyDef) -> String {
    if let Some(source) = &strategy.candidate {
        return source.label.clone();
    }
    let suffix = strategy
        .id
        .split_once("_tls_")
        .or_else(|| strategy.id.split_once("_quic_"))
        .map(|(_, suffix)| suffix);
    let name = match suffix {
        Some("split_sni") => "Разбиение SNI",
        Some("split_extension") => "Разбиение расширения SNI",
        Some("disorder_sni") => "Перестановка SNI",
        Some("saved_disorder") => "Сохранённый вариант · 1 + SNI",
        Some("disorder_extension") => "Перестановка расширения SNI",
        Some("legacy_sni_extension") => "Перестановка SNI · legacy",
        Some("fake_md5_disorder") => "Fake + MD5 + перестановка",
        Some("fake_seq_split") => "Fake + разбиение SNI",
        Some("fake_default_once") => "Fake · 1 повтор",
        Some("fake_default_six") => "Fake · 6 повторов",
        Some("fake_google_twice") => "Fake Google · 2 повтора",
        _ => {
            return format!(
                "Исходный профиль {} · {}",
                strategy.aggressiveness,
                strategy
                    .desync
                    .iter()
                    .map(|step| step.split(':').next().unwrap_or(step))
                    .collect::<Vec<_>>()
                    .join(" + ")
            )
        }
    };
    name.into()
}

fn named_catalog(
    pack: &LoadedPack,
    categories: &[String],
    level: u8,
    choices: &[Zapret2ProfileSelection],
) -> Result<Vec<CatalogProfile>, String> {
    let mut result = Vec::new();
    for category in categories {
        if !matches!(category.as_str(), "discord" | "youtube_twitch" | "gaming") {
            return Err("Неизвестный сервис Zapret2.".into());
        }
        let selected = pack.profiles_for_selections(category, level, false, choices)?;
        for strategy in pack
            .manifest
            .strategies
            .iter()
            .chain(&pack.manifest.presets)
            .chain(&pack.manifest.experiments)
        {
            if strategy.category != *category {
                continue;
            }
            let experimental = strategy.candidate.is_some()
                || pack
                    .manifest
                    .experiments
                    .iter()
                    .any(|item| item.id == strategy.id);
            result.push(CatalogProfile {
                composition_eligible: !pack
                    .manifest
                    .experiments
                    .iter()
                    .any(|entry| entry.id == strategy.id)
                    && obsession_runtime_protocol::zapret2_composition::compatible(
                        &strategy.category,
                        strategy.filter_tcp.as_deref(),
                        strategy.filter_udp.as_deref(),
                        &strategy.filter_l7,
                        strategy.hostlist.as_deref(),
                        strategy.ipset.as_deref(),
                        &strategy.transports,
                    ),
                composition: None,
                category: category.clone(),
                profile_id: strategy.id.clone(),
                name: if strategy.id == crate::dpi_engine::CLIENTHELLO_EXPERIMENT_ID {
                    "ClientHello · эксперимент".into()
                } else {
                    profile_name(strategy)
                },
                transport: slot(strategy).into(),
                experimental,
                data_plane: strategy.ipset.is_some() || slot(strategy) == "voice",
                level: strategy.aggressiveness,
                selected: choices
                    .iter()
                    .find(|choice| {
                        choice.category == *category && choice.transport == slot(strategy)
                    })
                    .map_or_else(
                        || selected.iter().any(|item| item.id == strategy.id),
                        |choice| choice.profile_id == strategy.id,
                    ),
                candidate: strategy.candidate.clone(),
            });
        }
        for choice in choices.iter().filter(|choice| choice.category == *category) {
            append_composition_catalog(&mut result, pack, &choice.profile_id, true)?;
        }
    }
    Ok(result)
}

fn append_composition_catalog(
    result: &mut Vec<CatalogProfile>,
    pack: &LoadedPack,
    id: &str,
    selected: bool,
) -> Result<(), String> {
    use obsession_runtime_protocol::zapret2_composition::DiscordComposition;
    let Some(composition) = DiscordComposition::parse(id).map_err(str::to_owned)? else {
        return Ok(());
    };
    pack.discord_composition(id)?;
    if !result.iter().any(|entry| entry.profile_id == id) {
        result.push(CatalogProfile {
            category: "discord".into(),
            profile_id: id.into(),
            composition_eligible: false,
            name: "Сборка Discord · API/Gateway + CDN".into(),
            transport: "tls".into(),
            experimental: false,
            data_plane: false,
            level: 0,
            selected,
            candidate: None,
            composition: Some(composition),
        });
    }
    Ok(())
}

pub(crate) fn catalog(
    app: &AppHandle,
    categories: &[String],
) -> Result<Vec<CatalogProfile>, String> {
    migrate_saved_profiles(app)?;
    let state = app.state::<AppState>();
    let pack = crate::dpi_engine::load_pack(&state.paths.strategy_pack_dir("builtin"))?;
    let settings = state.settings.lock_recover().clone();
    let mut catalog = named_catalog(
        &pack,
        categories,
        settings.zapret2_level,
        &settings.zapret2_profiles,
    )?;
    #[cfg(windows)]
    if categories.iter().any(|category| category == "discord") {
        for receipt in RESULTS
            .lock_recover()
            .values()
            .filter(|receipt| receipt.expires > std::time::Instant::now())
        {
            for id in receipt.confirmed.keys() {
                append_composition_catalog(&mut catalog, &pack, id, false)?;
            }
        }
    }
    Ok(catalog)
}

fn tournament_candidates(
    pack: &LoadedPack,
    pack_root: &std::path::Path,
    category: &str,
    transport: &str,
    selected: Option<&str>,
    limit: usize,
) -> Result<Vec<tournament::Candidate>, String> {
    let resources = pack_root
        .parent()
        .and_then(|path| path.parent())
        .ok_or("Каталог пакета недоступен.")?;
    let mut entries: Vec<_> = pack
        .manifest
        .strategies
        .iter()
        .chain(&pack.manifest.presets)
        .chain(&pack.manifest.experiments)
        .filter(|item| {
            item.category == category
                && slot(item) == transport
                && item.ipset.is_none()
                && (!pack.manifest.experiments.iter().any(|e| e.id == item.id)
                    || selected == Some(item.id.as_str()))
                && (transport == "tls"
                    || item.candidate.is_none()
                    || selected == Some(item.id.as_str()))
        })
        .collect();
    entries.sort_by_key(|item| {
        (
            Some(item.id.as_str()) != selected,
            item.candidate.is_none(),
            item.aggressiveness,
        )
    });
    let engine_hash = crate::dpi_engine::manifest::sha256_hex(
        &std::fs::read(resources.join("bin/zapret2/winws2.exe"))
            .map_err(|_| "Не удалось прочитать закреплённый движок для идентичности рецепта.")?,
    );
    let mut result = Vec::new();
    for strategy in entries {
        let list = strategy
            .hostlist
            .clone()
            .unwrap_or_else(|| format!("{category}.txt"));
        let list_hash = crate::dpi_engine::manifest::sha256_hex(
            &std::fs::read(resources.join("lists").join(&list))
                .map_err(|_| "Не удалось прочитать список сервиса для идентичности рецепта.")?,
        );
        let profile = crate::dpi_engine::zapret2::profile_from_strategy_def(
            strategy,
            Some(list.clone()),
            None,
        );
        let invocation = crate::dpi_engine::zapret2::Zapret2Invocation {
            profiles: vec![profile],
            ..Default::default()
        };
        let arguments: Vec<_> = crate::dpi_engine::zapret2::build_winws2_args(&invocation)
            .into_iter()
            .filter(|arg| !arg.starts_with("--name="))
            .collect();
        let identity = serde_json::to_vec(&serde_json::json!({ "args": arguments, "list": list_hash,
            "engine": engine_hash, "packVersion": pack.manifest.pack_version, "schema": pack.manifest.schema_version,
            "files": pack.manifest.files, "blobs": pack.manifest.blobs })).map_err(|e| e.to_string())?;
        let mut family = strategy
            .candidate
            .as_ref()
            .map(|source| source.family.clone())
            .unwrap_or_else(|| {
                let actions = strategy.desync.join(":");
                if actions.contains("multidisorder") {
                    if actions.starts_with("fake:") {
                        "fake_reorder"
                    } else {
                        "reorder"
                    }
                } else if actions.contains("multisplit") {
                    if actions.starts_with("fake:") {
                        "fake_split"
                    } else {
                        "split"
                    }
                } else {
                    "fake"
                }
                .into()
            });
        // ACK and MD5 fooling are materially different mechanisms. Keeping
        // both under "hostfake" discarded the working MD5 candidate before
        // screening when the ACK sibling failed on large early-SNI requests.
        let actions: Vec<_> = strategy
            .desync
            .iter()
            .map(String::as_str)
            .chain(strategy.steps.iter().map(|step| step.action.as_str()))
            .collect();
        if actions
            .iter()
            .any(|action| action.split(':').any(|part| part == "tcp_md5"))
        {
            family.push_str("/md5");
        } else if actions
            .iter()
            .any(|action| action.split(':').any(|part| part.starts_with("tcp_ack=")))
        {
            family.push_str("/ack");
        }
        result.push(tournament::Candidate {
            profile_id: strategy.id.clone(),
            label: profile_name(strategy),
            family,
            fingerprint: crate::dpi_engine::manifest::sha256_hex(&identity),
        });
    }
    if let Some(id) = selected {
        if let Some(candidate) = composition_candidate(pack, pack_root, id)? {
            result.insert(0, candidate);
        }
    }
    Ok(tournament::diverse(result, selected, limit))
}

fn composition_candidate(
    pack: &LoadedPack,
    pack_root: &std::path::Path,
    id: &str,
) -> Result<Option<tournament::Candidate>, String> {
    let Some(parts) = pack.discord_composition(id)? else {
        return Ok(None);
    };
    let resources = pack_root
        .parent()
        .and_then(|path| path.parent())
        .ok_or("Каталог пакета недоступен.")?;
    let invocation = crate::dpi_engine::zapret2::Zapret2Invocation {
        profiles: parts
            .iter()
            .map(|part| {
                crate::dpi_engine::zapret2::profile_from_strategy_def(
                    part,
                    Some("discord.txt".into()),
                    None,
                )
            })
            .collect(),
        ..Default::default()
    };
    let identity = serde_json::to_vec(&serde_json::json!({
        "policy": "discord_hostname_partition_v1", "id": id,
        "args": crate::dpi_engine::zapret2::build_winws2_args(&invocation),
        "list": crate::dpi_engine::manifest::sha256_hex(&std::fs::read(resources.join("lists/discord.txt")).map_err(|e| e.to_string())?),
        "engine": crate::dpi_engine::manifest::sha256_hex(&std::fs::read(resources.join("bin/zapret2/winws2.exe")).map_err(|e| e.to_string())?),
        "packVersion": pack.manifest.pack_version, "schema": pack.manifest.schema_version,
        "files": pack.manifest.files, "blobs": pack.manifest.blobs
    })).map_err(|e| e.to_string())?;
    Ok(Some(tournament::Candidate {
        profile_id: id.into(),
        label: "Сборка Discord · API/Gateway + CDN".into(),
        family: "discord_composition".into(),
        fingerprint: crate::dpi_engine::manifest::sha256_hex(&identity),
    }))
}

#[derive(Clone, Copy)]
struct ModePlan {
    limit: usize,
    timeout: Duration,
    budget: Duration,
    attempt_reserve: Duration,
    screening: usize,
    finalists: usize,
    confirmations: usize,
}

fn mode_plan(mode: &str) -> Result<ModePlan, String> {
    let (maximum, timeout, budget) = match mode {
        "fast" => (3, Duration::from_secs(4), Duration::from_secs(90)),
        "balanced" => (5, Duration::from_secs(6), Duration::from_secs(180)),
        "deep" => (7, Duration::from_secs(8), Duration::from_secs(300)),
        _ => return Err("Неизвестный режим подбора.".into()),
    };
    // Fund one start, stabilization, all probes and worst-case cleanup:
    // original restore, one receipt-backed retry, exact candidate stop.
    let attempt_reserve = timeout + OPERATION_TIMEOUT * 4 + Duration::from_millis(4875);
    let limit = maximum.min((budget.as_millis() / attempt_reserve.as_millis()) as usize);
    let (screening, finalists, confirmations) = if mode == "fast" {
        (2, 1, 2)
    } else if mode == "balanced" {
        (3, 2, 2)
    } else {
        (3, 2, 3)
    };
    Ok(ModePlan {
        limit,
        timeout,
        budget,
        attempt_reserve,
        screening,
        finalists,
        confirmations,
    })
}

fn initial_queue(
    candidates: &[tournament::Candidate],
    plan: &ModePlan,
    explicit: bool,
) -> std::collections::VecDeque<(String, &'static str)> {
    if explicit {
        // An inconclusive first trial still owes the requested repetitions.
        // Keep every observation; later successes never erase an earlier gap.
        let id = candidates[0].profile_id.clone();
        (0..plan.confirmations)
            .map(|ordinal| {
                (
                    id.clone(),
                    if ordinal == 0 {
                        "composition"
                    } else {
                        "confirmation"
                    },
                )
            })
            .collect()
    } else {
        candidates
            .iter()
            .take(plan.screening)
            .map(|c| (c.profile_id.clone(), "screening"))
            .collect()
    }
}

#[cfg(windows)]
#[derive(Clone)]
struct ResultReceipt {
    category: String,
    transport: String,
    original: DpiStartRequest,
    generation: Option<u64>,
    settings: String,
    network: String,
    expires: std::time::Instant,
    confirmed: std::collections::HashMap<String, String>,
    blocked: std::collections::HashSet<String>,
}

#[cfg(windows)]
static RESULTS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, ResultReceipt>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

#[cfg(windows)]
fn settings_signature(settings: &crate::settings::Settings) -> String {
    serde_json::json!([
        settings.dpi_engine,
        settings.zapret2_level,
        settings.zapret2_profiles,
        settings.zapret2_selected_categories,
        settings.zapret2_clienthello
    ])
    .to_string()
}

#[cfg(windows)]
fn receipt_matches(
    receipt: &ResultReceipt,
    original: &DpiStartRequest,
    generation: Option<u64>,
    settings: &str,
    network: Option<&str>,
    now: std::time::Instant,
) -> bool {
    now < receipt.expires
        && receipt.original == *original
        && receipt.generation == generation
        && receipt.settings == settings
        && network == Some(receipt.network.as_str())
}

#[cfg(windows)]
fn set_slot(request: &mut DpiStartRequest, category: &str, transport: &str, id: &str) {
    request.options.zapret2_clienthello = false;
    request
        .options
        .zapret2_profiles
        .retain(|item| item.category != category || item.transport != transport);
    request
        .options
        .zapret2_profiles
        .push(obsession_runtime_protocol::Zapret2ProfileSelection {
            category: category.into(),
            transport: transport.into(),
            profile_id: id.into(),
        });
}

#[cfg(windows)]
async fn service_call(
    request: Request,
    owner: std::sync::Arc<ProjectionMutation>,
) -> Result<Response, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _owner = owner;
        call_service_raw("zapret2-profile", request, OPERATION_TIMEOUT)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(windows)]
async fn authoritative_snapshot() -> Result<RuntimeSnapshot, String> {
    tauri::async_runtime::spawn_blocking(|| runtime_snapshot_blocking(OPERATION_TIMEOUT))
        .await
        .map_err(|error| error.to_string())?
}

#[cfg(windows)]
fn basis(app: &AppHandle, snapshot: &RuntimeSnapshot) -> Result<DpiStartRequest, String> {
    if let Some(runtime) = &snapshot.dpi {
        let projection = lock_projection();
        if runtime.engine != DpiEngine::Zapret2 || projection.generation != Some(runtime.generation)
        {
            return Err("Для подбора запустите Zapret2 из текущего окна.".into());
        }
        return projection.launch_request.clone().ok_or_else(|| "Параметры этой сессии неизвестны. Выключите и снова включите Zapret2 перед подбором.".into());
    }
    let settings = app.state::<AppState>().settings.lock_recover().clone();
    if settings.dpi_engine != "zapret2" {
        return Err("Сначала выберите Zapret2.".into());
    }
    let selections = settings
        .zapret2_selected_categories
        .iter()
        .map(|category| (category.clone(), String::new()))
        .collect::<Vec<_>>();
    let Request::DpiStart(request) = start_request(
        DpiEngine::Zapret2,
        &selections,
        settings.zapret2_level,
        false,
        false,
        &settings.zapret2_profiles,
    )?
    else {
        unreachable!()
    };
    Ok(request)
}

#[cfg(windows)]
trait TransactionPort {
    fn call(
        &mut self,
        request: Request,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Response, String>> + Send + '_>>;
}

#[cfg(windows)]
struct RuntimePort {
    owner: std::sync::Arc<ProjectionMutation>,
}

#[cfg(windows)]
impl TransactionPort for RuntimePort {
    fn call(
        &mut self,
        request: Request,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Response, String>> + Send + '_>>
    {
        Box::pin(service_call(request, self.owner.clone()))
    }
}

#[cfg(windows)]
struct TransitionFailure {
    detail: String,
    restored_generation: Option<u64>,
}

#[cfg(windows)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Disposition {
    Original(u64),
    Stopped,
    Unknown,
}

#[cfg(windows)]
struct AttemptReceipt {
    disposition: Disposition,
    report: Option<legacy_test::TestReport>,
    error: Option<String>,
}

#[cfg(windows)]
impl AttemptReceipt {
    fn failed(disposition: Disposition, error: String) -> Self {
        Self {
            disposition,
            report: None,
            error: Some(error),
        }
    }
}

#[cfg(windows)]
async fn transition<P: TransactionPort>(
    port: &mut P,
    request: DpiStartRequest,
    expected: Option<u64>,
) -> Result<u64, TransitionFailure> {
    let request = match expected {
        Some(expected_generation) => {
            Request::DpiReplace(obsession_runtime_protocol::DpiReplaceRequest {
                expected_generation,
                runtime: request,
            })
        }
        None => Request::DpiStart(request),
    };
    match port.call(request).await {
        Ok(Response::Started(started)) => Ok(started.generation),
        Ok(Response::Error(error)) => Err(TransitionFailure {
            detail: format!(
                "защищённая служба отклонила операцию: {}",
                format_service_error("zapret2-profile", &error)
            ),
            restored_generation: error.restored_generation,
        }),
        Ok(_) => Err(TransitionFailure {
            detail: "Служба не подтвердила запуск выбранного профиля.".into(),
            restored_generation: None,
        }),
        Err(detail) => Err(TransitionFailure {
            detail,
            restored_generation: None,
        }),
    }
}

#[cfg(windows)]
async fn stop_generation<P: TransactionPort>(port: &mut P, generation: u64) -> Result<(), String> {
    match port
        .call(Request::DpiStop(DpiStopRequest { generation }))
        .await?
    {
        Response::Stopped => Ok(()),
        Response::Error(error) => Err(format_service_error("zapret2-profile-stop", &error)),
        _ => Err("Служба не подтвердила завершение тестовой сессии.".into()),
    }
}

/// A failed restore may have restarted the candidate. Only the response's exact
/// receipt, never a category-shaped snapshot, permits one retry or exact stop.
#[cfg(windows)]
async fn restore_original<P: TransactionPort>(
    port: &mut P,
    original: &DpiStartRequest,
    candidate_generation: u64,
) -> AttemptReceipt {
    let failure = match transition(port, original.clone(), Some(candidate_generation)).await {
        Ok(generation) => {
            return AttemptReceipt {
                disposition: Disposition::Original(generation),
                report: None,
                error: None,
            }
        }
        Err(error) => error,
    };
    let mut last_owned = candidate_generation;
    if let Some(restored_candidate) = failure.restored_generation {
        last_owned = restored_candidate;
        match transition(port, original.clone(), Some(restored_candidate)).await {
            Ok(generation) => return AttemptReceipt::failed(Disposition::Original(generation),
                format!("Восстановление потребовало повторного запуска. Исходный профиль возвращён; подбор остановлен. {}", failure.detail)),
            Err(retry) => { if let Some(generation) = retry.restored_generation { last_owned = generation; } },
        }
    }
    match stop_generation(port, last_owned).await {
        Ok(()) => AttemptReceipt::failed(Disposition::Stopped,
            format!("Исходный профиль восстановить не удалось. Тестовая сессия остановлена; включите обход заново. {}", failure.detail)),
        Err(cleanup) => AttemptReceipt::failed(Disposition::Unknown,
            format!("Не удалось подтвердить восстановление и завершение теста. Подбор остановлен; состояние сверится со службой. {}; {}", failure.detail, cleanup)),
    }
}

/// The same transaction core is exercised by fake RPC and deferred-start tests.
#[cfg(windows)]
async fn run_attempt<P, M, F, R>(
    port: &mut P,
    original: &DpiStartRequest,
    candidate: DpiStartRequest,
    baseline: Option<u64>,
    measure: M,
    should_restore: R,
) -> AttemptReceipt
where
    P: TransactionPort,
    M: FnOnce(u64) -> F,
    F: std::future::Future<Output = Result<legacy_test::TestReport, String>>,
    R: FnOnce() -> bool,
{
    let generation = match transition(port, candidate, baseline).await {
        Ok(generation) => generation,
        Err(failure) => {
            let disposition = if baseline.is_some() {
                failure
                    .restored_generation
                    .map(Disposition::Original)
                    .unwrap_or(Disposition::Unknown)
            } else {
                Disposition::Unknown
            };
            return AttemptReceipt::failed(disposition, failure.detail);
        }
    };
    let measured = measure(generation).await;
    let mut cleanup = if baseline.is_some() && should_restore() {
        restore_original(port, original, generation).await
    } else {
        match stop_generation(port, generation).await {
            Ok(()) => AttemptReceipt {
                disposition: Disposition::Stopped,
                report: None,
                error: None,
            },
            Err(error) => AttemptReceipt::failed(
                Disposition::Unknown,
                format!("Завершение теста не подтверждено; дальнейший подбор остановлен. {error}"),
            ),
        }
    };
    if cleanup.error.is_none() {
        match measured {
            Ok(report) => cleanup.report = Some(report),
            Err(error) => cleanup.error = Some(error),
        }
    }
    cleanup
}

#[cfg(windows)]
fn publish_launch(app: &AppHandle, request: DpiStartRequest, generation: u64) {
    let status = {
        let mut projection = lock_projection();
        projection.generation = Some(generation);
        projection.engine = Some(DpiEngine::Zapret2);
        projection.selections = request
            .selections
            .iter()
            .map(|item| {
                (
                    category_name(item.category).into(),
                    item.strategy_id.clone(),
                )
            })
            .collect();
        projection.zapret2_level = Some(request.options.zapret2_level);
        projection
            .started_at_unix
            .get_or_insert_with(util::unix_secs);
        projection.launch_request = Some(request);
        projection_status(&projection)
    };
    emit_status(app, status);
}

#[cfg(windows)]
fn publish_disposition(app: &AppHandle, original: &DpiStartRequest, disposition: Disposition) {
    match disposition {
        Disposition::Original(generation) => publish_launch(app, original.clone(), generation),
        Disposition::Stopped => {
            *lock_projection() = DpiProjection::default();
            emit_status(app, inactive_status());
        }
        Disposition::Unknown => lock_projection().launch_request = None,
    }
}

#[cfg(windows)]
fn valid_choice(
    pack: &LoadedPack,
    request: &DpiStartRequest,
    category: &str,
    transport: &str,
    id: &str,
) -> Result<(), String> {
    if !request
        .selections
        .iter()
        .any(|item| category_name(item.category) == category)
    {
        return Err("Сервис не выбран для этой сессии.".into());
    }
    if pack.discord_composition(id)?.is_some() {
        return if category == "discord" && transport == "tls" {
            Ok(())
        } else {
            Err("Сборка доступна только для TLS Discord.".into())
        };
    }
    let option = pack
        .profile_catalog()
        .into_iter()
        .find(|item| {
            item.category == category && item.transport == transport && item.profile_id == id
        })
        .ok_or_else(|| "Профиль отсутствует в проверенном каталоге этого сервиса.".to_string())?;
    let _ = option;
    Ok(())
}

pub(crate) async fn select(
    app: &AppHandle,
    category: String,
    transport: String,
    profile_id: String,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        select_checked(app, category, transport, profile_id, None).await
    }
    #[cfg(not(windows))]
    {
        let _ = (app, category, transport, profile_id);
        Err(unavailable())
    }
}

#[cfg(windows)]
async fn validate_result_context(
    receipt: &ResultReceipt,
    pack: &LoadedPack,
    original: &DpiStartRequest,
    generation: Option<u64>,
    settings: &str,
    profile_id: &str,
) -> Result<(), String> {
    let network = measurement::network_key().await;
    if !receipt_matches(
        receipt,
        original,
        generation,
        settings,
        network.as_deref(),
        std::time::Instant::now(),
    ) {
        return Err(
            "Результат устарел: сеть, настройки или исходная сессия изменились. Повторите турнир."
                .into(),
        );
    }
    let candidate = tournament_candidates(
        pack,
        &pack.dir,
        &receipt.category,
        &receipt.transport,
        Some(profile_id),
        32,
    )?
    .into_iter()
    .find(|candidate| candidate.profile_id == profile_id)
    .ok_or("Рецепт больше недоступен.")?;
    if receipt.confirmed.get(profile_id) != Some(&candidate.fingerprint) {
        return Err("Рецепт или его ресурсы изменились после турнира. Повторите проверку.".into());
    }
    Ok(())
}

#[cfg(windows)]
async fn select_checked(
    app: &AppHandle,
    category: String,
    transport: String,
    profile_id: String,
    expected: Option<ResultReceipt>,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        let migration_app = app.clone();
        tauri::async_runtime::spawn_blocking(move || migrate_saved_profiles(&migration_app))
            .await
            .map_err(|error| error.to_string())??;
        let state = app.state::<AppState>();
        if state.shutting_down.load(Ordering::SeqCst) {
            return Err("Приложение завершает работу.".into());
        }
        let pack = crate::dpi_engine::load_pack(&state.paths.strategy_pack_dir("builtin"))?;
        let initial = state.settings.lock_recover().clone();
        let snapshot = authoritative_snapshot().await?;
        let original = basis(app, &snapshot)?;
        if let Some(receipt) = &expected {
            validate_result_context(
                receipt,
                &pack,
                &original,
                snapshot.dpi.as_ref().map(|runtime| runtime.generation),
                &settings_signature(&initial),
                &profile_id,
            )
            .await?;
        }
        valid_choice(&pack, &original, &category, &transport, &profile_id)?;
        let mut request = original.clone();
        set_slot(&mut request, &category, &transport, &profile_id);
        let mut port = RuntimePort {
            owner: begin_projection_mutation()?,
        };
        let active = snapshot.dpi.as_ref().map(|runtime| runtime.generation);
        let new_generation = if let Some(generation) = active {
            match transition(&mut port, request.clone(), Some(generation)).await {
                Ok(generation) => Some(generation),
                Err(error) => {
                    publish_disposition(
                        app,
                        &original,
                        error
                            .restored_generation
                            .map(Disposition::Original)
                            .unwrap_or(Disposition::Unknown),
                    );
                    return Err(error.detail);
                }
            }
        } else {
            None
        };
        let commit_app = app.clone();
        let saved = tauri::async_runtime::spawn_blocking(move || {
            crate::commands::mutate_settings_checked(
                &commit_app,
                |settings| {
                    if commit_app
                        .state::<AppState>()
                        .shutting_down
                        .load(Ordering::SeqCst)
                    {
                        return Err("Приложение завершает работу.".into());
                    }
                    if settings_signature(settings) != settings_signature(&initial) {
                        return Err("Выбор изменился во время применения профиля.".into());
                    }
                    Ok(())
                },
                |settings| {
                    settings
                        .zapret2_profiles
                        .retain(|item| item.category != category || item.transport != transport);
                    settings.zapret2_profiles.push(Zapret2ProfileSelection {
                        category,
                        transport,
                        profile_id,
                    });
                    settings.zapret2_clienthello = false;
                },
            )
            .map(|_| ())
        })
        .await
        .unwrap_or_else(|error| Err(error.to_string()));
        if let Err(error) = saved {
            if let Some(generation) = new_generation {
                let restored = if state.shutting_down.load(Ordering::SeqCst) {
                    match stop_generation(&mut port, generation).await {
                        Ok(()) => AttemptReceipt {
                            disposition: Disposition::Stopped,
                            report: None,
                            error: None,
                        },
                        Err(error) => AttemptReceipt::failed(Disposition::Unknown, error),
                    }
                } else {
                    restore_original(&mut port, &original, generation).await
                };
                publish_disposition(app, &original, restored.disposition);
                if let Some(cleanup_error) = restored.error {
                    return Err(format!("{error}; {cleanup_error}"));
                }
            }
            return Err(error);
        }
        if let Some(generation) = new_generation {
            publish_launch(app, request, generation);
        }
        Ok(())
    }
}

pub(crate) async fn apply_result(
    app: &AppHandle,
    run_id: String,
    category: String,
    transport: String,
    profile_id: String,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        let receipt = RESULTS
            .lock_recover()
            .get(&run_id)
            .cloned()
            .ok_or("Результат турнира больше недоступен. Повторите проверку.")?;
        if receipt.category != category
            || receipt.transport != transport
            || !receipt.confirmed.contains_key(&profile_id)
        {
            return Err("Профиль не подтверждён этим турниром для выбранного сервиса.".into());
        }
        if receipt.blocked.contains(&profile_id) {
            return Err(
                "Профиль не прошёл проверку в приложении. Повтори испытание или турнир.".into(),
            );
        }
        select_checked(app, category, transport, profile_id, Some(receipt)).await?;
        RESULTS.lock_recover().remove(&run_id);
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (app, run_id, category, transport, profile_id);
        Err(unavailable())
    }
}

pub(crate) async fn test_scenario(
    app: &AppHandle,
    request_id: String,
    run_id: String,
    category: String,
    transport: String,
    profile_id: String,
    scenario: super::zapret2_scenario::Scenario,
) -> Result<super::zapret2_scenario::ResultRecord, String> {
    #[cfg(windows)]
    {
        use super::zapret2_scenario as guided;
        if request_id.is_empty()
            || request_id.len() > 64
            || !request_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err("Неверный идентификатор запроса испытания.".into());
        }
        if !scenario.allowed(&category, &transport) {
            return Err("Этот сценарий не подходит выбранному сервису и транспорту.".into());
        }
        let state = app.state::<AppState>();
        if state.shutting_down.load(Ordering::SeqCst) {
            return Err("Приложение завершает работу.".into());
        }
        state.test_cancel.store(false, Ordering::SeqCst);
        let epoch = state.test_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let ticket = RESULTS
            .lock_recover()
            .get(&run_id)
            .cloned()
            .ok_or("Результат турнира недоступен. Повтори подбор.")?;
        if ticket.category != category
            || ticket.transport != transport
            || !ticket.confirmed.contains_key(&profile_id)
        {
            return Err("Профиль не подтверждён этим турниром.".into());
        }
        let pack = crate::dpi_engine::load_pack(&state.paths.strategy_pack_dir("builtin"))?;
        let snapshot = authoritative_snapshot().await?;
        let original = basis(app, &snapshot)?;
        let scenario_settings = settings_signature(&state.settings.lock_recover());
        validate_result_context(
            &ticket,
            &pack,
            &original,
            snapshot.dpi.as_ref().map(|runtime| runtime.generation),
            &scenario_settings,
            &profile_id,
        )
        .await?;
        valid_choice(&pack, &original, &category, &transport, &profile_id)?;
        if legacy_test::cancelled(app, epoch) {
            return Err("Испытание отменено до запуска профиля.".into());
        }
        let trial_id = format!("{:032x}", rand::random::<u128>());
        let started = std::time::Instant::now();
        let mut request = original.clone();
        set_slot(&mut request, &category, &transport, &profile_id);
        let configuration_fingerprint = crate::dpi_engine::manifest::sha256_hex(
            &serde_json::to_vec(&request).map_err(|error| error.to_string())?,
        );
        let mut round = measurement::RoundMeasurement {
            run_id: trial_id.clone(),
            profile_id: profile_id.clone(),
            fingerprint: ticket.confirmed[&profile_id].clone(),
            runtime_generation: None,
            ordinal: 0,
            phase: if scenario.automatic() {
                "player_scenario"
            } else {
                "guided_scenario"
            }
            .into(),
            transport: transport.clone(),
            tester_version: if scenario.automatic() {
                super::youtube_playback::VERSION
            } else {
                guided::VERSION
            },
            execution: "unverified",
            network_before: None,
            network_after: None,
            comparable: false,
            context_reason: None,
            functions: Vec::new(),
            packet_capture: None,
        };
        guided::pending(&mut round, scenario);
        let mut progress = guided::Progress {
            request_id: request_id.clone(),
            trial_id: trial_id.clone(),
            run_id: run_id.clone(),
            category: category.clone(),
            transport: transport.clone(),
            profile_id: profile_id.clone(),
            scenario,
            phase: "starting",
            step_id: None,
            step_index: 0,
            total: scenario.steps().len(),
            label: "Запускаем профиль для испытания",
            instruction: "Дождись первого шага. По окончании исходный профиль будет возвращён.",
            confirm_after_unix_ms: 0,
            expires_unix_ms: 0,
        };
        let _ = app.emit("zapret2-scenario-progress", &progress);
        let raw = std::sync::Arc::new(std::sync::Mutex::new(None));
        let capture = raw.clone();
        let playback_metrics = std::sync::Arc::new(std::sync::Mutex::new(None));
        let capture_playback = playback_metrics.clone();
        let measured_round = round.clone();
        let measured_progress = progress.clone();
        let expected_network = ticket.network.clone();
        let mut port = RuntimePort {
            owner: begin_projection_mutation()?,
        };
        let baseline = snapshot.dpi.as_ref().map(|runtime| runtime.generation);
        let receipt = run_attempt(
            &mut port,
            &original,
            request,
            baseline,
            |generation| async move {
                tokio::time::sleep(Duration::from_millis(800)).await;
                let mut measured_round = measured_round;
                measured_round.runtime_generation = Some(generation);
                let measured = if scenario.automatic() {
                    super::youtube_playback::measure(
                        app,
                        epoch,
                        measured_round,
                        measured_progress.clone(),
                        &expected_network,
                    )
                    .await
                    .map(|(report, metrics)| {
                        *capture_playback.lock_recover() = Some(metrics);
                        report
                    })
                } else {
                    guided::measure(
                        app,
                        epoch,
                        scenario,
                        measured_round,
                        measured_progress.clone(),
                        &expected_network,
                    )
                    .await
                };
                if let Ok(report) = &measured {
                    *capture.lock_recover() = report.measurement.clone();
                }
                let mut restoring = measured_progress;
                restoring.phase = "restoring";
                restoring.step_id = None;
                restoring.label = "Возвращаем исходный профиль";
                restoring.instruction = "Дождись завершения возврата.";
                let _ = app.emit("zapret2-scenario-progress", &restoring);
                measured
            },
            || !state.shutting_down.load(Ordering::SeqCst),
        )
        .await;
        publish_disposition(app, &original, receipt.disposition);
        let measured = raw.lock_recover().take().unwrap_or(round);
        let restored = match (baseline, receipt.disposition) {
            (Some(_), Disposition::Original(_)) | (None, Disposition::Stopped) => true,
            _ => false,
        };
        let status = guided::status(
            &measured,
            legacy_test::cancelled(app, epoch),
            restored,
            receipt.error.is_some(),
        );
        let current_network = measurement::network_key().await;
        let context_valid = restored
            && receipt.error.is_none()
            && measured.comparable
            && !state.shutting_down.load(Ordering::SeqCst)
            && std::time::Instant::now() < ticket.expires
            && current_network.as_deref() == Some(ticket.network.as_str())
            && settings_signature(&state.settings.lock_recover()) == ticket.settings;
        let mut recommendation_available = false;
        {
            let mut receipts = RESULTS.lock_recover();
            if context_valid {
                if let Some(cached) = receipts.get_mut(&run_id) {
                    cached.generation = match receipt.disposition {
                        Disposition::Original(generation) => Some(generation),
                        _ => None,
                    };
                    if scenario.blocks_application_on_failure() {
                        if status == "failed" {
                            cached.blocked.insert(profile_id.clone());
                        } else if status == "passed" {
                            cached.blocked.remove(&profile_id);
                        }
                    }
                    recommendation_available = !cached.blocked.contains(&profile_id);
                }
            } else {
                receipts.remove(&run_id);
            }
        }
        progress.phase = "idle";
        progress.step_id = None;
        let _ = app.emit("zapret2-scenario-progress", &progress);
        let mut result = guided::ResultRecord {
            request_id,
            trial_id,
            run_id,
            category,
            transport,
            profile_id,
            scenario,
            source: scenario.source(),
            scope: "whole_configuration",
            actual_transport: "unverified",
            configuration_fingerprint,
            status,
            restored,
            context_valid,
            recommendation_available,
            failure: receipt.error,
            history_warning: None,
            elapsed_ms: started.elapsed().as_millis() as u64,
            measurement: measured,
            playback: playback_metrics.lock_recover().take(),
        };
        match serde_json::to_value(&result) {
            Ok(report) => {
                result.history_warning = super::zapret2_history::save(
                    app,
                    super::zapret2_history::Kind::Scenario,
                    scenario.label().into(),
                    status,
                    report,
                )
                .await
            }
            Err(error) => result.history_warning = Some(error.to_string()),
        }
        Ok(result)
    }
    #[cfg(not(windows))]
    {
        let _ = (
            app, request_id, run_id, category, transport, profile_id, scenario,
        );
        Err(unavailable())
    }
}

pub(crate) async fn pick(
    app: &AppHandle,
    category: String,
    transport: String,
    mode: String,
    composition: Option<obsession_runtime_protocol::zapret2_composition::DiscordComposition>,
) -> Result<PickerResult, String> {
    #[cfg(windows)]
    {
        if transport != "tls" && !(transport == "quic" && category == "youtube_twitch") {
            return Err("Автоподбор доступен для TLS и для QUIC YouTube. Остальные профили выбираются вручную.".into());
        }
        let plan = mode_plan(&mode)?;
        let composition = composition
            .map(|pair| {
                if category != "discord" || transport != "tls" {
                    return Err("Сборка доступна только для TLS Discord.".to_owned());
                }
                obsession_runtime_protocol::zapret2_composition::DiscordComposition::new(
                    &pair.api_profile_id,
                    &pair.cdn_profile_id,
                )
                .map_err(str::to_owned)
            })
            .transpose()?;
        RESULTS
            .lock_recover()
            .retain(|_, receipt| !(receipt.category == category && receipt.transport == transport));
        let state = app.state::<AppState>();
        if state.shutting_down.load(Ordering::SeqCst) {
            return Err("Приложение завершает работу.".into());
        }
        // Arm cancellation before the first await. A cancel received while
        // migration/snapshot is pending must never be cleared afterwards.
        state.test_cancel.store(false, Ordering::SeqCst);
        let epoch = state.test_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let deadline = tokio::time::Instant::now() + plan.budget;
        let migration_app = app.clone();
        tauri::async_runtime::spawn_blocking(move || migrate_saved_profiles(&migration_app))
            .await
            .map_err(|error| error.to_string())??;
        let pack = crate::dpi_engine::load_pack(&state.paths.strategy_pack_dir("builtin"))?;
        let snapshot = authoritative_snapshot().await?;
        let original = basis(app, &snapshot)?;
        let original_settings = settings_signature(&state.settings.lock_recover());
        let choices = original
            .options
            .zapret2_profiles
            .iter()
            .map(|item| Zapret2ProfileSelection {
                category: item.category.clone(),
                transport: item.transport.clone(),
                profile_id: item.profile_id.clone(),
            })
            .collect::<Vec<_>>();
        let selected = pack.profiles_for_selections(
            &category,
            original.options.zapret2_level,
            false,
            &choices,
        )?;
        let current = choices
            .iter()
            .find(|choice| choice.category == category && choice.transport == transport)
            .map(|choice| choice.profile_id.as_str())
            .or_else(|| {
                selected
                    .iter()
                    .find(|item| slot(item) == transport && item.ipset.is_none())
                    .map(|item| item.id.as_str())
            });
        let candidates = if let Some(pair) = &composition {
            vec![composition_candidate(&pack, &pack.dir, &pair.id())?.ok_or("Сборка недоступна.")?]
        } else {
            tournament_candidates(&pack, &pack.dir, &category, &transport, current, 16)?
        };
        if candidates.is_empty() {
            return Err("Для этого транспорта нет конфигураций в каталоге.".into());
        }
        valid_choice(
            &pack,
            &original,
            &category,
            &transport,
            &candidates[0].profile_id,
        )?;
        let run_id = format!("{:032x}", rand::random::<u128>());
        let started = std::time::Instant::now();
        let initial_network = measurement::network_key().await;
        let mut competition = tournament::Tournament::new(
            candidates,
            plan.confirmations,
            if composition.is_some() {
                plan.confirmations
            } else {
                plan.limit
            },
        );
        let eligible = if composition.is_none() && category == "discord" && transport == "tls" {
            use obsession_runtime_protocol::zapret2_composition::compatible;
            pack.manifest
                .strategies
                .iter()
                .chain(&pack.manifest.presets)
                .filter(|parent| {
                    compatible(
                        &parent.category,
                        parent.filter_tcp.as_deref(),
                        parent.filter_udp.as_deref(),
                        &parent.filter_l7,
                        parent.hostlist.as_deref(),
                        parent.ipset.as_deref(),
                        &parent.transports,
                    )
                })
                .map(|parent| parent.id.clone())
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let mut composer = super::zapret2_composition::Composer::default();
        let mut queue = initial_queue(&competition.candidates, &plan, composition.is_some());
        let mut confirming = composition.is_some();
        let mut port = RuntimePort {
            owner: begin_projection_mutation()?,
        };
        let mut baseline_generation = snapshot.dpi.as_ref().map(|runtime| runtime.generation);
        let mut reports = Vec::new();
        let mut stop_reason = "completed";
        let mut failure = None;
        loop {
            if legacy_test::cancelled(app, epoch) {
                stop_reason = "cancelled";
                break;
            }
            if deadline.saturating_duration_since(tokio::time::Instant::now())
                < plan.attempt_reserve
            {
                stop_reason = "budget";
                break;
            }
            if queue.is_empty() {
                if confirming {
                    break;
                }
                use super::zapret2_composition::Action;
                match composer.next(&mut competition, &eligible) {
                    Action::Recheck(ids) => {
                        queue.extend(ids.into_iter().map(|id| (id, "confirmation")))
                    }
                    Action::Trial(pair) => {
                        let id = pair.id();
                        let candidate = composition_candidate(&pack, &pack.dir, &id)?
                            .ok_or("Сборка недоступна.")?;
                        competition
                            .add_composition(candidate)
                            .map_err(str::to_owned)?;
                        confirming = true;
                        queue.push_back((id.clone(), "composition"));
                        queue.extend((1..plan.confirmations).map(|_| (id.clone(), "confirmation")));
                    }
                    Action::None => {}
                }
                if queue.is_empty() {
                    match competition.next_stage(plan.finalists) {
                        tournament::NextStage::Screening(id) => queue.push_back((id, "screening")),
                        tournament::NextStage::Confirmation(finalists) => {
                            confirming = true;
                            for round in 0..plan.confirmations - 1 {
                                queue.extend(
                                    tournament::confirmation_order(&finalists, round)
                                        .into_iter()
                                        .map(|id| (id, "confirmation")),
                                );
                            }
                        }
                        tournament::NextStage::Finished => break,
                    }
                }
            }
            if competition.rounds.len() >= competition.max_attempts() {
                break;
            }
            let total = competition.max_attempts();
            let (id, phase) = queue.pop_front().unwrap();
            let candidate = competition
                .candidates
                .iter()
                .find(|candidate| candidate.profile_id == id)
                .unwrap()
                .clone();
            let ordinal = competition.rounds.len() + 1;
            let round = measurement::RoundMeasurement {
                run_id: run_id.clone(),
                profile_id: id.clone(),
                fingerprint: candidate.fingerprint.clone(),
                runtime_generation: None,
                ordinal,
                phase: phase.into(),
                transport: transport.clone(),
                tester_version: measurement::TESTER_VERSION,
                execution: "unverified",
                network_before: None,
                network_after: None,
                comparable: false,
                context_reason: None,
                functions: Vec::new(),
                packet_capture: None,
            };
            let mut request = original.clone();
            set_slot(&mut request, &category, &transport, &id);
            request.options.zapret2_diagnostics = transport == "tls";
            let _ = app.emit(
                "zapret2-picker-progress",
                Progress {
                    category: &category,
                    transport: &transport,
                    current: ordinal,
                    total,
                    profile_id: &id,
                    label: &candidate.label,
                    phase,
                },
            );
            let (category_ref, transport_ref, id_ref) =
                (category.as_str(), transport.as_str(), id.as_str());
            let initial_network_ref = initial_network.as_deref();
            let round_for_measure = round.clone();
            let receipt = run_attempt(
                &mut port,
                &original,
                request,
                baseline_generation,
                |generation| async move {
                    tokio::time::sleep(Duration::from_millis(800)).await;
                    let mut measured_round = round_for_measure;
                    measured_round.runtime_generation = Some(generation);
                    let measured = measurement::measure(
                        app,
                        category_ref,
                        transport_ref,
                        epoch,
                        if phase == "screening" {
                            Duration::from_secs(3)
                        } else {
                            plan.timeout
                        },
                        measured_round,
                        initial_network_ref,
                    )
                    .await;
                    let _ = app.emit(
                        "zapret2-picker-progress",
                        Progress {
                            category: category_ref,
                            transport: transport_ref,
                            current: ordinal,
                            total,
                            profile_id: id_ref,
                            label: "Возвращаем исходный профиль",
                            phase: "restoring",
                        },
                    );
                    measured
                },
                || !state.shutting_down.load(Ordering::SeqCst),
            )
            .await;
            publish_disposition(app, &original, receipt.disposition);
            baseline_generation = match receipt.disposition {
                Disposition::Original(generation) => Some(generation),
                _ => None,
            };
            if let Some(error) = receipt.error {
                if matches!(receipt.disposition, Disposition::Unknown)
                    || (snapshot.dpi.is_some()
                        && !matches!(receipt.disposition, Disposition::Original(_)))
                {
                    stop_reason = "runtime_unknown";
                    failure = Some(error);
                    break;
                }
                // A launch/setup failure with confirmed cleanup is not a
                // negative network result. Keep it and try the next candidate.
                let report = measurement::unavailable(round, &category, &transport, &error);
                competition
                    .record(report.measurement.clone().unwrap())
                    .map_err(str::to_owned)?;
                reports.push(report);
                continue;
            }
            let report = receipt
                .report
                .ok_or_else(|| "Служба не вернула результат завершённой проверки.".to_owned())?;
            let round = report
                .measurement
                .clone()
                .ok_or("Нет результатов функций в измерении.")?;
            let network_changed = round.context_reason == Some("network_changed");
            competition.record(round).map_err(str::to_owned)?;
            reports.push(report);
            if network_changed {
                stop_reason = "network_changed";
                break;
            }
        }
        if legacy_test::cancelled(app, epoch) {
            stop_reason = "cancelled";
        }
        let winner = if matches!(stop_reason, "completed" | "budget") {
            competition.winner()
        } else {
            None
        };
        let summaries = competition.summary();
        let status = if stop_reason == "cancelled" {
            "cancelled"
        } else if stop_reason == "runtime_unknown" {
            "aborted"
        } else if stop_reason == "network_changed" || initial_network.is_none() {
            "inconclusive"
        } else if winner.is_some() {
            "passed"
        } else if competition.tied() {
            "tie"
        } else if summaries.iter().any(|s| s.complete_rounds > 0) {
            "insufficient"
        } else if summaries.iter().any(|s| s.status == "partial") {
            "partial"
        } else if summaries
            .iter()
            .any(|s| s.status == "inconclusive" || (s.rounds > 0 && s.status == "not_tested"))
        {
            "inconclusive"
        } else {
            "exhausted"
        };
        let report = winner
            .as_ref()
            .and_then(|id| {
                reports.iter().rev().find(|r| {
                    r.measurement
                        .as_ref()
                        .is_some_and(|round| &round.profile_id == id)
                })
            })
            .cloned()
            .or_else(|| reports.last().cloned());
        let bytes_read = competition.rounds.iter().map(|r| r.bytes()).sum();
        if matches!(status, "passed" | "tie") {
            if let Some(network) = initial_network {
                let confirmed = summaries
                    .iter()
                    .filter(|s| s.status == "confirmed")
                    .map(|s| (s.profile_id.clone(), s.fingerprint.clone()))
                    .collect();
                let mut receipts = RESULTS.lock_recover();
                receipts.retain(|_, receipt| {
                    receipt.expires > std::time::Instant::now()
                        && !(receipt.category == category && receipt.transport == transport)
                });
                if receipts.len() >= 8 {
                    receipts.clear();
                }
                receipts.insert(
                    run_id.clone(),
                    ResultReceipt {
                        category: category.clone(),
                        transport: transport.clone(),
                        original,
                        generation: baseline_generation,
                        settings: original_settings,
                        network,
                        expires: std::time::Instant::now() + Duration::from_secs(600),
                        confirmed,
                        blocked: Default::default(),
                    },
                );
            }
        }
        let mut result = PickerResult {
            category: category.clone(),
            transport: transport.clone(),
            profile_id: winner,
            status,
            checked: competition.rounds.len(),
            total: competition.max_attempts(),
            report,
            failure,
            history_warning: None,
            tournament: TournamentResult {
                run_id,
                required_confirmations: plan.confirmations,
                max_attempts: competition.max_attempts(),
                budget_ms: plan.budget.as_millis() as u64,
                max_body_bytes: competition.max_attempts() * measurement::MAX_ROUND_BYTES,
                elapsed_ms: started.elapsed().as_millis() as u64,
                bytes_read,
                stop_reason: stop_reason.into(),
                candidates: summaries,
                rounds: competition.rounds,
            },
        };
        match serde_json::to_value(&result) {
            Ok(report) => {
                result.history_warning = super::zapret2_history::save(
                    app,
                    super::zapret2_history::Kind::Tournament,
                    format!("Турнир сетевых проб · {category} / {transport}"),
                    status,
                    report,
                )
                .await
            }
            Err(error) => result.history_warning = Some(error.to_string()),
        }
        let _ = app.emit(
            "zapret2-picker-progress",
            Progress {
                category: &category,
                transport: &transport,
                current: result.checked,
                total: result.total,
                profile_id: "",
                label: "",
                phase: "idle",
            },
        );
        Ok(result)
    }
    #[cfg(not(windows))]
    {
        let _ = (app, category, transport, mode, composition);
        Err(unavailable())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_pair_runs_all_requested_trials_and_never_hides_first_gap() {
        use super::super::zapret2_measure::Outcome;
        for mode in ["fast", "balanced", "deep"] {
            let plan = mode_plan(mode).unwrap();
            let candidate = tournament::Candidate {
                profile_id: "dc1_a--b".into(),
                label: "pair".into(),
                family: "discord_composition".into(),
                fingerprint: "dc1_a--b".into(),
            };
            let mut t = tournament::Tournament::new(
                vec![candidate],
                plan.confirmations,
                plan.confirmations,
            );
            let mut queue = initial_queue(&t.candidates, &plan, true);
            while let Some((id, phase)) = queue.pop_front() {
                let mut round = tournament::tests_round(&id, true);
                round.ordinal = t.rounds.len() + 1;
                round.runtime_generation = Some(round.ordinal as u64);
                round.phase = phase.into();
                if round.ordinal == 1 {
                    round.functions[0].outcome = Outcome::Inconclusive;
                }
                t.record(round).unwrap();
            }
            assert_eq!(t.rounds.len(), plan.confirmations);
            assert_eq!(t.summary()[0].complete_rounds, plan.confirmations - 1);
            assert!(t.winner().is_none());
        }
    }

    #[test]
    fn composed_catalog_and_identity_survive_reload_without_marking_donors_selected() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources/strategy-packs/builtin");
        let mut pack = crate::dpi_engine::load_pack(&root).unwrap();
        let pair = obsession_runtime_protocol::zapret2_composition::DiscordComposition::new(
            "research_discord_tls_hostfake_md5",
            "classic_discord_tls_split_sni",
        )
        .unwrap();
        let choice = Zapret2ProfileSelection {
            category: "discord".into(),
            transport: "tls".into(),
            profile_id: pair.id(),
        };
        let rows = named_catalog(&pack, &["discord".into()], 4, &[choice.clone()]).unwrap();
        let selected: Vec<_> = rows
            .iter()
            .filter(|row| row.transport == "tls" && row.selected)
            .collect();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].composition, Some(pair.clone()));
        let first = composition_candidate(&pack, &root, &pair.id())
            .unwrap()
            .unwrap();
        assert_eq!(
            tournament_candidates(&pack, &root, "discord", "tls", Some(&pair.id()), 16).unwrap()[0]
                .fingerprint,
            first.fingerprint
        );
        let profiles = pack
            .profiles_for_selections("discord", 4, false, &[choice])
            .unwrap();
        let invocation = crate::dpi_engine::zapret2::Zapret2Invocation {
            profiles: profiles
                .iter()
                .map(|part| {
                    crate::dpi_engine::zapret2::profile_from_strategy_def(
                        part,
                        Some("discord.txt".into()),
                        None,
                    )
                })
                .collect(),
            ..Default::default()
        };
        let args = crate::dpi_engine::zapret2::build_winws2_args(&invocation);
        let blocks: Vec<_> = args.split(|arg| arg == "--new").collect();
        let api = blocks
            .iter()
            .find(|block| block.contains(&format!("--name={}", pair.api_profile_id)))
            .unwrap();
        let cdn = blocks
            .iter()
            .find(|block| block.contains(&format!("--name={}", pair.cdn_profile_id)))
            .unwrap();
        assert!(api.iter().any(|arg| arg == "--hostlist=discord.txt"));
        assert!(api
            .iter()
            .any(|arg| arg.starts_with("--hostlist-exclude-domains=")));
        assert!(!cdn.iter().any(|arg| arg.starts_with("--hostlist=")));
        assert!(cdn.iter().any(|arg| arg.starts_with("--hostlist-domains=")));
        let parent = pack
            .manifest
            .presets
            .iter_mut()
            .find(|parent| parent.id == pair.cdn_profile_id)
            .unwrap();
        parent.desync[0].push_str(":repeats=2");
        assert_ne!(
            composition_candidate(&pack, &root, &pair.id())
                .unwrap()
                .unwrap()
                .fingerprint,
            first.fingerprint
        );
        parent_test_incompatible(&mut pack, &pair);
    }

    fn parent_test_incompatible(
        pack: &mut LoadedPack,
        pair: &obsession_runtime_protocol::zapret2_composition::DiscordComposition,
    ) {
        pack.manifest
            .presets
            .iter_mut()
            .find(|parent| parent.id == pair.cdn_profile_id)
            .unwrap()
            .filter_tcp = Some("443,8443".into());
        assert!(pack.discord_composition(&pair.id()).is_err());
    }

    #[test]
    fn plans_fund_start_probe_and_worst_case_cleanup_without_growing() {
        for (mode, count, seconds) in [("fast", 3, 90), ("balanced", 5, 180), ("deep", 7, 300)] {
            let plan = mode_plan(mode).unwrap();
            assert_eq!(plan.limit, count);
            assert_eq!(plan.budget, Duration::from_secs(seconds));
            assert!(plan.attempt_reserve * count as u32 <= plan.budget);
            assert!(Duration::from_secs(3) <= plan.timeout);
        }
        assert!(mode_plan("unknown").is_err());
    }
    #[test]
    fn catalog_keeps_baseline_then_distinct_families_and_exact_recipe_identities() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources/strategy-packs/builtin");
        let mut pack = crate::dpi_engine::load_pack(&root).unwrap();
        let rows =
            named_catalog(&pack, &["discord".into(), "youtube_twitch".into()], 4, &[]).unwrap();
        assert_eq!(
            rows.iter().filter(|row| row.candidate.is_some()).count(),
            11
        );
        let entries = tournament_candidates(
            &pack,
            &root,
            "discord",
            "tls",
            Some("discord_tls_multidisorder_alt"),
            16,
        )
        .unwrap();
        assert_eq!(entries[0].profile_id, "discord_tls_multidisorder_alt");
        assert_ne!(entries[0].family, entries[1].family);
        assert_ne!(entries[1].family, entries[2].family);
        assert_eq!(
            entries.len(),
            entries
                .iter()
                .map(|entry| &entry.fingerprint)
                .collect::<std::collections::HashSet<_>>()
                .len()
        );
        assert!(entries
            .iter()
            .all(|entry| entry.profile_id != "discord_tls_clienthello_clone"));
        assert!(entries
            .iter()
            .any(|entry| entry.profile_id.starts_with("research_")));
        let chosen = "research_discord_tls_window_disorder";
        let before =
            tournament_candidates(&pack, &root, "discord", "tls", Some(chosen), 16).unwrap();
        assert_eq!(before[0].profile_id, chosen);
        let ack = before
            .iter()
            .find(|c| c.profile_id == "research_discord_tls_hostfake_ack")
            .unwrap();
        let md5 = before
            .iter()
            .find(|c| c.profile_id == "research_discord_tls_hostfake_md5")
            .unwrap();
        assert_ne!(ack.family, md5.family);
        assert!(
            before
                .iter()
                .take(5)
                .any(|c| c.profile_id == md5.profile_id),
            "MD5 must be screened independently of the failed ACK sibling"
        );
        pack.manifest
            .presets
            .iter_mut()
            .find(|p| p.id == chosen)
            .unwrap()
            .candidate
            .as_mut()
            .unwrap()
            .notes = "Other source wording".into();
        let after =
            tournament_candidates(&pack, &root, "discord", "tls", Some(chosen), 16).unwrap();
        assert_eq!(before[0].fingerprint, after[0].fingerprint);
        pack.manifest
            .presets
            .iter_mut()
            .find(|p| p.id == chosen)
            .unwrap()
            .steps[0]
            .action = "wssize:wsize=2:scale=6".into();
        assert_ne!(
            before[0].fingerprint,
            tournament_candidates(&pack, &root, "discord", "tls", Some(chosen), 16).unwrap()[0]
                .fingerprint
        );
        assert!(
            tournament_candidates(&pack, &root, "youtube_twitch", "quic", None, 16)
                .unwrap()
                .iter()
                .all(|c| !c.profile_id.starts_with("research_"))
        );
    }

    #[cfg(windows)]
    mod transactions {
        use super::*;
        use obsession_runtime_protocol::{RuntimeStarted, ServiceError};
        use std::collections::VecDeque;

        struct FakePort {
            replies: VecDeque<Result<Response, String>>,
            requests: Vec<Request>,
        }
        impl FakePort {
            fn new(replies: Vec<Response>) -> Self {
                Self {
                    replies: replies.into_iter().map(Ok).collect(),
                    requests: vec![],
                }
            }
        }
        impl TransactionPort for FakePort {
            fn call(
                &mut self,
                request: Request,
            ) -> std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<Response, String>> + Send + '_>,
            > {
                self.requests.push(request);
                Box::pin(async { self.replies.pop_front().expect("unexpected RPC") })
            }
        }
        fn started(generation: u64) -> Response {
            Response::Started(RuntimeStarted {
                generation,
                materialized_fingerprint: None,
                initialization_retries: 0,
            })
        }
        fn failed(restored: Option<u64>) -> Response {
            Response::Error(ServiceError {
                code: ServiceErrorCode::RuntimeFailed,
                runtime_detail: None,
                restored_generation: restored,
            })
        }
        fn original() -> DpiStartRequest {
            let Request::DpiStart(mut request) = start_request(
                DpiEngine::Zapret2,
                &[
                    ("discord".into(), String::new()),
                    ("youtube_twitch".into(), String::new()),
                ],
                0,
                false,
                false,
                &[],
            )
            .unwrap() else {
                unreachable!()
            };
            set_slot(
                &mut request,
                "youtube_twitch",
                "quic",
                "classic_youtube_twitch_quic_fake_default_once",
            );
            request
        }
        fn candidate(original: &DpiStartRequest) -> DpiStartRequest {
            let mut request = original.clone();
            set_slot(
                &mut request,
                "discord",
                "tls",
                "classic_discord_tls_split_sni",
            );
            request
        }

        #[test]
        fn applying_a_result_requires_the_original_session_network_settings_and_fresh_receipt() {
            let request = original();
            let settings = settings_signature(&crate::settings::Settings::default());
            let now = std::time::Instant::now();
            let receipt = ResultReceipt {
                category: "discord".into(),
                transport: "tls".into(),
                original: request.clone(),
                generation: Some(10),
                settings: settings.clone(),
                network: "one".into(),
                expires: now + Duration::from_secs(600),
                confirmed: std::collections::HashMap::new(),
                blocked: Default::default(),
            };
            assert!(receipt_matches(
                &receipt,
                &request,
                Some(10),
                &settings,
                Some("one"),
                now
            ));
            assert!(!receipt_matches(
                &receipt,
                &request,
                Some(11),
                &settings,
                Some("one"),
                now
            ));
            assert!(!receipt_matches(
                &receipt,
                &request,
                Some(10),
                "changed",
                Some("one"),
                now
            ));
            assert!(!receipt_matches(
                &receipt,
                &request,
                Some(10),
                &settings,
                Some("two"),
                now
            ));
            assert!(!receipt_matches(
                &receipt,
                &request,
                Some(10),
                &settings,
                None,
                now
            ));
            assert!(!receipt_matches(
                &receipt,
                &request,
                Some(10),
                &settings,
                Some("one"),
                receipt.expires
            ));
            assert!(!receipt_matches(
                &receipt,
                &candidate(&request),
                Some(10),
                &settings,
                Some("one"),
                now
            ));
        }
        fn passed() -> legacy_test::TestReport {
            legacy_test::report(
                vec![legacy_test::ProbeResult {
                    url: "https://fixture.invalid/".into(),
                    passed: true,
                    elapsed_ms: 1,
                    bytes: 10,
                    attempts: 1,
                    http_status: Some(200),
                    error: None,
                }],
                false,
            )
        }
        fn assert_restore(request: &Request, expected: u64, original: &DpiStartRequest) {
            let Request::DpiReplace(replace) = request else {
                panic!("not an exact replacement")
            };
            assert_eq!(replace.expected_generation, expected);
            assert_eq!(&replace.runtime, original);
        }
        fn assert_stop(request: &Request, expected: u64) {
            let Request::DpiStop(stop) = request else {
                panic!("not a generation-owned stop")
            };
            assert_eq!(stop.generation, expected);
        }

        #[tokio::test]
        async fn admits_pass_only_after_exact_original_is_restored() {
            let original = original();
            let tested = candidate(&original);
            assert_eq!(
                tested.options.zapret2_profiles[0],
                original.options.zapret2_profiles[0]
            );
            let mut port = FakePort::new(vec![started(20), started(21)]);
            let receipt = run_attempt(
                &mut port,
                &original,
                tested,
                Some(10),
                |_| async { Ok(passed()) },
                || true,
            )
            .await;
            assert_eq!(receipt.disposition, Disposition::Original(21));
            assert!(receipt.report.unwrap().passed);
            assert!(receipt.error.is_none());
            assert_restore(&port.requests[1], 20, &original);
        }
        #[tokio::test]
        async fn cancelled_inactive_test_stops_only_its_returned_generation() {
            let original = original();
            let mut port = FakePort::new(vec![started(20), Response::Stopped]);
            let receipt = run_attempt(
                &mut port,
                &original,
                candidate(&original),
                None,
                |_| async { Ok(legacy_test::report(vec![], true)) },
                || true,
            )
            .await;
            assert_eq!(receipt.disposition, Disposition::Stopped);
            assert_eq!(receipt.report.unwrap().status, "cancelled");
            assert_stop(&port.requests[1], 20);
        }
        #[tokio::test]
        async fn probe_error_and_shutdown_both_finish_owned_cleanup() {
            let original = original();
            let mut port = FakePort::new(vec![started(20), started(21)]);
            let receipt = run_attempt(
                &mut port,
                &original,
                candidate(&original),
                Some(10),
                |_| async { Err("probe failed".into()) },
                || true,
            )
            .await;
            assert_eq!(receipt.disposition, Disposition::Original(21));
            assert_eq!(receipt.error.as_deref(), Some("probe failed"));
            assert!(receipt.report.is_none());
            assert_restore(&port.requests[1], 20, &original);
            let mut port = FakePort::new(vec![started(30), Response::Stopped]);
            let receipt = run_attempt(
                &mut port,
                &original,
                candidate(&original),
                Some(21),
                |_| async { Ok(passed()) },
                || false,
            )
            .await;
            assert_eq!(receipt.disposition, Disposition::Stopped);
            assert_stop(&port.requests[1], 30);
        }
        #[tokio::test]
        async fn rejected_candidate_tracks_service_restored_generation_without_probing() {
            let original = original();
            let mut port = FakePort::new(vec![failed(Some(12))]);
            let receipt = run_attempt(
                &mut port,
                &original,
                candidate(&original),
                Some(10),
                |_| async {
                    panic!("must not probe a rejected launch");
                    #[allow(unreachable_code)]
                    Ok(passed())
                },
                || true,
            )
            .await;
            assert_eq!(receipt.disposition, Disposition::Original(12));
            assert!(receipt.error.is_some());
            assert!(receipt.report.is_none());
            assert_eq!(port.requests.len(), 1);
        }
        #[tokio::test]
        async fn failed_restore_retries_reported_candidate_and_never_admits_pass() {
            let original = original();
            let mut port = FakePort::new(vec![started(20), failed(Some(22)), started(23)]);
            let receipt = run_attempt(
                &mut port,
                &original,
                candidate(&original),
                Some(10),
                |_| async { Ok(passed()) },
                || true,
            )
            .await;
            assert_eq!(receipt.disposition, Disposition::Original(23));
            assert!(receipt.error.is_some());
            assert!(receipt.report.is_none());
            assert_restore(&port.requests[1], 20, &original);
            assert_restore(&port.requests[2], 22, &original);
        }
        #[tokio::test]
        async fn double_restore_failure_stops_last_receipted_candidate() {
            let original = original();
            let mut port = FakePort::new(vec![
                started(20),
                failed(Some(22)),
                failed(Some(24)),
                Response::Stopped,
            ]);
            let receipt = run_attempt(
                &mut port,
                &original,
                candidate(&original),
                Some(10),
                |_| async { Ok(passed()) },
                || true,
            )
            .await;
            assert_eq!(receipt.disposition, Disposition::Stopped);
            assert!(receipt.report.is_none());
            assert_stop(&port.requests[3], 24);
        }
        #[tokio::test]
        async fn missing_receipt_and_stop_conflict_cannot_claim_a_restore_or_stop_someone_else() {
            let original = original();
            let mut port = FakePort::new(vec![started(20)]);
            port.replies.push_back(Err("RPC timeout".into()));
            port.replies.push_back(Ok(Response::Error(ServiceError {
                code: ServiceErrorCode::Conflict,
                runtime_detail: None,
                restored_generation: None,
            })));
            let receipt = run_attempt(
                &mut port,
                &original,
                candidate(&original),
                Some(10),
                |_| async { Ok(passed()) },
                || true,
            )
            .await;
            assert_eq!(receipt.disposition, Disposition::Unknown);
            assert!(receipt.error.is_some());
            assert!(receipt.report.is_none());
            assert_stop(&port.requests[2], 20);
            assert_eq!(port.requests.len(), 3);
        }
        #[tokio::test]
        async fn cancellation_during_deferred_start_waits_for_receipt_and_restores() {
            struct DeferredPort {
                ready: Option<tokio::sync::oneshot::Sender<()>>,
                launch: Option<tokio::sync::oneshot::Receiver<Response>>,
                requests: Vec<Request>,
            }
            impl TransactionPort for DeferredPort {
                fn call(
                    &mut self,
                    request: Request,
                ) -> std::pin::Pin<
                    Box<dyn std::future::Future<Output = Result<Response, String>> + Send + '_>,
                > {
                    self.requests.push(request);
                    Box::pin(async {
                        if let Some(launch) = self.launch.take() {
                            self.ready.take().unwrap().send(()).unwrap();
                            launch.await.map_err(|error| error.to_string())
                        } else {
                            Ok(started(21))
                        }
                    })
                }
            }
            let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
            let (launch_tx, launch_rx) = tokio::sync::oneshot::channel();
            let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let signal = cancelled.clone();
            let task = tokio::spawn(async move {
                let original = original();
                let mut port = DeferredPort {
                    ready: Some(ready_tx),
                    launch: Some(launch_rx),
                    requests: vec![],
                };
                let receipt = run_attempt(
                    &mut port,
                    &original,
                    candidate(&original),
                    Some(10),
                    |_| async { Ok(legacy_test::report(vec![], signal.load(Ordering::SeqCst))) },
                    || true,
                )
                .await;
                (receipt, port.requests, original)
            });
            ready_rx.await.unwrap();
            cancelled.store(true, Ordering::SeqCst);
            launch_tx.send(started(20)).unwrap();
            let (receipt, requests, original) = task.await.unwrap();
            assert_eq!(receipt.disposition, Disposition::Original(21));
            assert_eq!(receipt.report.unwrap().status, "cancelled");
            assert_restore(&requests[1], 20, &original);
        }
    }
}
