//! Side-effect coordinator безопасного adaptive recovery для Zapret2.
//!
//! Чистые решения остаются в `model`; этот модуль только сериализует DPI
//! respawn через `dpi_gate`, запускает probes, пишет отдельный cache и эмитит UI.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::async_runtime::{self, JoinHandle};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{mpsc, watch, Notify};

use super::cache::{AdaptiveCacheEntry, AdaptiveStrategyCache, ProbeSummary, StrategyTrust};
use super::candidate_runtime;
use super::compiler;
use super::dsl::{override_key, AdaptiveCategory, StrategyCandidate, StrategyTransport};
use super::generator::{self, AdaptiveDiagnosis, GeneratorInput};
use super::model::{
    DiagnosisReason, PreparationFailure, RecoveryAction, RecoveryCfg, RecoveryEvent, RecoveryModel,
    RecoveryPhase, RecoveryStatus, RollbackReason, SearchSessionMode,
};
use super::probe::{self, ProbeSeries, ProbeTarget, SessionDnsCache};
use super::recommendation::{self, StrategyRecommendation};
use super::rollback;
use super::tasks::SessionTasks;
use crate::eyes::Verdict;
use crate::state::{AppState, DpiLaunchSpec, DpiRuntimeSnapshot};

const OBSERVATION_QUEUE_CAP: usize = 512;
const CONTROL_QUEUE_CAP: usize = 64;
const DETECTION_WINDOW_MS: u64 = 30_000;
const DETECTION_THRESHOLD: usize = 3;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SearchTuning {
    probe_timeout: Duration,
    probe_interval: Duration,
    stabilization_delay: Duration,
    candidate_rounds: u8,
    required_successes: u8,
    base_recheck_rounds: u8,
    candidate_budget: usize,
    eyes_quiet_window: Duration,
    eyes_quiet_deadline: Duration,
}

fn search_tuning_for(mode: &str) -> SearchTuning {
    match mode {
        "fast" => SearchTuning {
            probe_timeout: Duration::from_secs(3),
            probe_interval: Duration::from_millis(300),
            stabilization_delay: Duration::from_millis(500),
            candidate_rounds: 2,
            required_successes: 2,
            base_recheck_rounds: 1,
            candidate_budget: 4,
            eyes_quiet_window: Duration::from_millis(150),
            eyes_quiet_deadline: Duration::from_millis(300),
        },
        "deep" => SearchTuning {
            probe_timeout: Duration::from_secs(7),
            probe_interval: Duration::from_millis(850),
            stabilization_delay: Duration::from_millis(1_000),
            candidate_rounds: 4,
            required_successes: 3,
            base_recheck_rounds: 2,
            candidate_budget: 12,
            eyes_quiet_window: Duration::from_millis(350),
            eyes_quiet_deadline: Duration::from_millis(700),
        },
        _ => SearchTuning {
            probe_timeout: Duration::from_secs(5),
            probe_interval: Duration::from_millis(550),
            stabilization_delay: Duration::from_millis(700),
            candidate_rounds: 3,
            required_successes: 2,
            base_recheck_rounds: 1,
            candidate_budget: 8,
            eyes_quiet_window: Duration::from_millis(250),
            eyes_quiet_deadline: Duration::from_millis(500),
        },
    }
}

fn search_tuning(app: &AppHandle) -> SearchTuning {
    let mode = app
        .state::<AppState>()
        .settings
        .lock()
        .map(|settings| settings.adaptive_search_mode.clone())
        .unwrap_or_else(|error| error.into_inner().adaptive_search_mode.clone());
    search_tuning_for(&mode)
}

#[derive(Debug)]
enum RuntimeEvent {
    Observation {
        domain: String,
        verdict: Verdict,
        generation: u64,
    },
    StartSearch(AdaptiveCategory, Option<StrategyTransport>),
    PreparationDiscoveryFinished {
        session_id: u64,
    },
    PreparationProgress {
        session_id: u64,
        current_round: u8,
        total_rounds: u8,
    },
    PreparationFinished {
        session_id: u64,
        result: Result<PreparedSearch, PreparationFailure>,
    },
    CandidateStartFinished {
        session_id: u64,
        attempt_id: u64,
        candidate_id: String,
        outcome: candidate_runtime::CandidateStartOutcome,
    },
    Cancel,
    Confirm {
        session_id: u64,
        candidate_id: String,
    },
    Reject {
        session_id: u64,
        candidate_id: String,
    },
    ResetSaved(AdaptiveCategory),
    ProbeFinished {
        session_id: u64,
        attempt_id: u64,
        generation: u64,
        candidate_id: String,
        series: ProbeSeries,
    },
    RollbackFinished {
        session_id: u64,
        attempt_id: u64,
        outcome: rollback::RollbackOutcome,
    },
    CandidateCrashed {
        generation: u64,
    },
    Shutdown,
}

#[derive(Default)]
struct EvidenceState {
    generation: u64,
    revision: u64,
    reset_count: u32,
    blackhole_count: u32,
    working_by_host: BTreeMap<String, u32>,
}

#[derive(Default)]
pub(super) struct EvidenceWindow {
    state: Mutex<EvidenceState>,
    changed: Notify,
}

impl EvidenceWindow {
    pub(super) fn begin(&self, generation: u64) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        *state = EvidenceState {
            generation,
            ..Default::default()
        };
    }

    fn observe(&self, generation: u64, domain: &str, verdict: Verdict) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if generation == 0 || state.generation != generation {
            return;
        }
        match verdict {
            Verdict::Reset => state.reset_count = state.reset_count.saturating_add(1),
            Verdict::Blackhole => {
                state.blackhole_count = state.blackhole_count.saturating_add(1);
            }
            Verdict::Working => {
                let domain = domain.trim_end_matches('.').to_ascii_lowercase();
                let count = state.working_by_host.entry(domain).or_default();
                *count = count.saturating_add(1);
            }
        }
        state.revision = state.revision.saturating_add(1);
        drop(state);
        self.changed.notify_one();
    }

    pub(super) fn snapshot(&self) -> probe::EyesProbeEvidence {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        probe::EyesProbeEvidence {
            reset_count: state.reset_count,
            blackhole_count: state.blackhole_count,
            working_by_host: state.working_by_host.clone(),
        }
    }

    fn revision_for_generation(&self, generation: u64) -> Option<u64> {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        (generation != 0 && state.generation == generation).then_some(state.revision)
    }

    pub(super) async fn wait_for_quiet(
        &self,
        generation: u64,
        quiet_for: Duration,
        max_wait: Duration,
    ) {
        if quiet_for.is_zero() || max_wait.is_zero() {
            return;
        }
        let Some(mut revision) = self.revision_for_generation(generation) else {
            return;
        };
        let started = tokio::time::Instant::now();
        let hard_deadline = started + max_wait;
        let mut quiet_deadline = (started + quiet_for).min(hard_deadline);

        loop {
            let notified = self.changed.notified();
            tokio::select! {
                _ = tokio::time::sleep_until(hard_deadline) => return,
                _ = tokio::time::sleep_until(quiet_deadline) => {
                    let Some(current) = self.revision_for_generation(generation) else {
                        return;
                    };
                    if current == revision {
                        return;
                    }
                    revision = current;
                    quiet_deadline =
                        (tokio::time::Instant::now() + quiet_for).min(hard_deadline);
                }
                _ = notified => {
                    let Some(current) = self.revision_for_generation(generation) else {
                        return;
                    };
                    if current != revision {
                        revision = current;
                        quiet_deadline =
                            (tokio::time::Instant::now() + quiet_for).min(hard_deadline);
                    }
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct AdaptiveInput {
    observation_tx: mpsc::Sender<RuntimeEvent>,
    control_tx: mpsc::Sender<RuntimeEvent>,
    candidate_generation: Arc<AtomicU64>,
    evidence: Arc<EvidenceWindow>,
}

impl AdaptiveInput {
    pub fn try_observation(&self, domain: String, verdict: Verdict, generation: u64) -> bool {
        self.evidence.observe(generation, &domain, verdict);
        self.observation_tx
            .try_send(RuntimeEvent::Observation {
                domain,
                verdict,
                generation,
            })
            .is_ok()
    }

    pub fn try_candidate_crashed(&self, generation: u64) -> bool {
        if generation == 0 || self.candidate_generation.load(Ordering::SeqCst) != generation {
            return false;
        }
        self.control_tx
            .try_send(RuntimeEvent::CandidateCrashed { generation })
            .is_ok()
    }

    async fn send(&self, event: RuntimeEvent) -> Result<(), String> {
        self.control_tx
            .send(event)
            .await
            .map_err(|_| "Adaptive runtime остановлен".to_string())
    }

    pub async fn start_search(
        &self,
        category: AdaptiveCategory,
        transport: Option<StrategyTransport>,
    ) -> Result<(), String> {
        self.send(RuntimeEvent::StartSearch(category, transport))
            .await
    }

    pub async fn cancel(&self) -> Result<(), String> {
        self.send(RuntimeEvent::Cancel).await
    }

    pub async fn confirm(&self, session_id: u64, candidate_id: String) -> Result<(), String> {
        self.send(RuntimeEvent::Confirm {
            session_id,
            candidate_id,
        })
        .await
    }

    pub async fn reject(&self, session_id: u64, candidate_id: String) -> Result<(), String> {
        self.send(RuntimeEvent::Reject {
            session_id,
            candidate_id,
        })
        .await
    }

    pub async fn reset_saved(&self, category: AdaptiveCategory) -> Result<(), String> {
        self.send(RuntimeEvent::ResetSaved(category)).await
    }
}

pub struct AdaptiveHandle {
    pub input: AdaptiveInput,
    pub status: watch::Receiver<RecoveryStatus>,
    join: JoinHandle<()>,
}

impl AdaptiveHandle {
    pub async fn shutdown(self) {
        let _ = self.input.send(RuntimeEvent::Shutdown).await;
        let mut join = self.join;
        if tokio::time::timeout(Duration::from_secs(8), &mut join)
            .await
            .is_err()
        {
            join.abort();
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdaptiveRecommendationDescriptor {
    pub category: AdaptiveCategory,
    pub transport: StrategyTransport,
    pub candidate_id: String,
    pub trust: StrategyTrust,
    pub evidence_source: String,
    pub source_candidate_id: String,
    pub source_category: AdaptiveCategory,
    pub source_transport: StrategyTransport,
    pub recommendation_reason: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SuggestionPayload {
    category: AdaptiveCategory,
    reason: DiagnosisReason,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PendingRollback {
    session_id: u64,
    attempt_id: u64,
    reason: RollbackReason,
}
fn take_pending_rollback(
    pending: &mut Option<PendingRollback>,
    session_id: u64,
    attempt_id: u64,
) -> Option<RecoveryAction> {
    if !pending
        .is_some_and(|pending| pending.session_id == session_id && pending.attempt_id == attempt_id)
    {
        return None;
    }
    let pending = pending.take().unwrap();
    Some(RecoveryAction::Rollback {
        session_id: pending.session_id,
        attempt_id: pending.attempt_id,
        reason: pending.reason,
    })
}

#[derive(Default)]
struct SessionContext {
    original: Option<DpiRuntimeSnapshot>,
    expected_generation: u64,
    category: Option<AdaptiveCategory>,
    network_key: Option<String>,
    asn_region: Option<String>,
    tried: HashSet<String>,
    probe_transport: Option<StrategyTransport>,
    probe_targets: Vec<ProbeTarget>,
    dns_cache: SessionDnsCache,
    session_mode: Option<SearchSessionMode>,
    candidate_generation: Arc<AtomicU64>,
    tasks: SessionTasks,
    pending_rollback: Option<PendingRollback>,
    last_probe: Option<ProbeSeries>,
}

#[derive(Debug)]
struct PreparedSearch {
    snapshot: DpiRuntimeSnapshot,
    network_key: Option<String>,
    asn_region: Option<String>,
    category: AdaptiveCategory,
    transport: StrategyTransport,
    targets: Vec<ProbeTarget>,
    dns_cache: SessionDnsCache,
    candidates: Vec<StrategyCandidate>,
    mode: SearchSessionMode,
}

fn classify_session_mode(
    transport: StrategyTransport,
    result: &super::model::CandidateProbeResult,
) -> Result<SearchSessionMode, PreparationFailure> {
    if result.is_success() {
        return Ok(SearchSessionMode::Comparison);
    }
    if transport == StrategyTransport::Quic
        && result.dns_ok
        && matches!(
            result.failure_stage,
            super::evidence::FailureStage::Quic | super::evidence::FailureStage::Https
        )
    {
        return Ok(SearchSessionMode::Recovery);
    }
    Err(PreparationFailure::ProbeUnreliable)
}

#[derive(Default)]
struct PassiveDetector {
    failures: HashMap<AdaptiveCategory, VecDeque<u64>>,
}

impl PassiveDetector {
    fn observe(
        &mut self,
        category: AdaptiveCategory,
        verdict: Verdict,
        now: u64,
    ) -> Option<DiagnosisReason> {
        let queue = self.failures.entry(category).or_default();
        if verdict == Verdict::Working {
            queue.clear();
            return None;
        }
        queue.push_back(now);
        while queue
            .front()
            .is_some_and(|oldest| now.saturating_sub(*oldest) > DETECTION_WINDOW_MS)
        {
            queue.pop_front();
        }
        if queue.len() < DETECTION_THRESHOLD {
            return None;
        }
        queue.clear();
        Some(match verdict {
            Verdict::Reset => DiagnosisReason::RepeatedReset,
            Verdict::Blackhole => DiagnosisReason::TlsBlackhole,
            Verdict::Working => return None,
        })
    }
}

pub fn start(app: AppHandle) -> AdaptiveHandle {
    let (observation_tx, mut observation_rx) = mpsc::channel(OBSERVATION_QUEUE_CAP);
    let (control_tx, mut control_rx) = mpsc::channel(CONTROL_QUEUE_CAP);
    let (status_tx, status_rx) =
        watch::channel(RecoveryModel::new(RecoveryCfg::default()).status());
    let candidate_generation = Arc::new(AtomicU64::new(0));
    let evidence = Arc::new(EvidenceWindow::default());
    let input = AdaptiveInput {
        observation_tx,
        control_tx: control_tx.clone(),
        candidate_generation: candidate_generation.clone(),
        evidence: evidence.clone(),
    };

    let join = async_runtime::spawn(async move {
        let started = Instant::now();
        let mut model = RecoveryModel::new(RecoveryCfg::default());
        let mut context = SessionContext {
            candidate_generation: candidate_generation.clone(),
            ..Default::default()
        };
        let mut detector = PassiveDetector::default();
        let mut shutdown_requested = false;
        let mut tick = tokio::time::interval(Duration::from_millis(500));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            let event = tokio::select! {
                biased;
                control = control_rx.recv() => control.unwrap_or(RuntimeEvent::Shutdown),
                _ = tick.tick() => {
                    let actions = model.step(RecoveryEvent::Tick(elapsed_ms(started)));
                    execute_actions(
                        &app, &mut model, &mut context, actions, &status_tx,
                        &control_tx, &candidate_generation, &evidence, started,
                    ).await;
                    continue;
                }
                observation = observation_rx.recv() => {
                    observation.unwrap_or(RuntimeEvent::Shutdown)
                }
            };
            if matches!(event, RuntimeEvent::Shutdown) {
                shutdown_requested = true;
            }
            let actions = handle_event(
                &app,
                &mut model,
                &mut context,
                &mut detector,
                &evidence,
                &control_tx,
                event,
                elapsed_ms(started),
            )
            .await;
            execute_actions(
                &app,
                &mut model,
                &mut context,
                actions,
                &status_tx,
                &control_tx,
                &candidate_generation,
                &evidence,
                started,
            )
            .await;
            if runtime_shutdown_complete(
                shutdown_requested,
                model.status().phase,
                context.tasks.candidate_running(),
                context.tasks.rollback_running(),
            ) {
                candidate_generation.store(0, Ordering::SeqCst);
                evidence.begin(0);
                break;
            }
        }
    });

    AdaptiveHandle {
        input,
        status: status_rx,
        join,
    }
}

/// Подтверждённые стратегии автоматически возвращаются только на том же
/// network fingerprint и только при включённом feature flag. Невалидный cache
/// уже отфильтрован `confirmed_candidate`, поэтому normal DPI start безопасно
/// деградирует к bundled Strategy Pack.
pub(crate) async fn confirmed_overrides_for_current_network(
    app: &AppHandle,
    selections: &[(String, String)],
) -> std::collections::BTreeMap<String, StrategyCandidate> {
    let mut overrides = std::collections::BTreeMap::new();
    if !adaptive_enabled(app) {
        return overrides;
    }
    let paths = app.state::<AppState>().paths.clone();
    let net = cached_or_resolve_network_identity(app, &paths).await;
    let Some(network_key) = net.gateway_mac.filter(|value| value != "unknown") else {
        return overrides;
    };
    let cache = AdaptiveStrategyCache::load(&paths);
    let engine = engine_version();
    for (category, _) in selections {
        let category_value = match category.as_str() {
            "discord" => Some(AdaptiveCategory::Discord),
            "youtube_twitch" => Some(AdaptiveCategory::YoutubeTwitch),
            "gaming" => Some(AdaptiveCategory::Gaming),
            _ => None,
        };
        let Some(category_value) = category_value else {
            continue;
        };
        let Some(scope_fingerprint) = category_scope_fingerprint(&paths, category_value) else {
            continue;
        };
        for transport in [StrategyTransport::Tls, StrategyTransport::Quic] {
            if let Some(candidate) = cache.confirmed_candidate_for_transport_scoped(
                &network_key,
                category_value,
                transport,
                &engine,
                Some(&scope_fingerprint),
            ) {
                crate::util::emit_log(
                    app,
                    "info",
                    "adaptive",
                    &format!(
                        "Применяется подтверждённая стратегия {} для {category}/{}",
                        candidate.candidate_id(),
                        transport.as_key()
                    ),
                );
                overrides.insert(override_key(category_value, transport), candidate);
            }
        }
    }
    overrides
}

struct ResolvedRecommendation {
    recommendation: StrategyRecommendation,
    network_key: String,
    asn_region: Option<String>,
    target_scope_fingerprint: String,
}

impl From<&StrategyRecommendation> for AdaptiveRecommendationDescriptor {
    fn from(value: &StrategyRecommendation) -> Self {
        Self {
            category: value.candidate.category,
            transport: value.candidate.transport,
            candidate_id: value.candidate.candidate_id(),
            trust: StrategyTrust::Recommended,
            evidence_source: value.source_category.as_key().to_string(),
            source_candidate_id: value.source_candidate_id.clone(),
            source_category: value.source_category,
            source_transport: value.source_transport,
            recommendation_reason: value.reason.clone(),
        }
    }
}

async fn cached_or_resolve_network_identity(
    app: &AppHandle,
    paths: &crate::paths::Paths,
) -> crate::netid::NetIdentity {
    if let Some(identity) = app
        .state::<AppState>()
        .netid
        .lock()
        .ok()
        .and_then(|slot| slot.clone())
    {
        return identity;
    }
    // Single-flight (см. commands::current_netid): под gate повторно проверяем
    // кэш, чтобы конкурентные вызовы не резолвили ipinfo дважды.
    let state = app.state::<AppState>();
    let _gate = state.netid_gate.lock().await;
    if let Some(identity) = app
        .state::<AppState>()
        .netid
        .lock()
        .ok()
        .and_then(|slot| slot.clone())
    {
        return identity;
    }
    let identity = crate::netid::resolve(paths).await;
    if let Ok(mut slot) = app.state::<AppState>().netid.lock() {
        *slot = Some(identity.clone());
    }
    identity
}

async fn resolve_gaming_recommendation(
    app: &AppHandle,
    transport: StrategyTransport,
) -> Result<Option<ResolvedRecommendation>, String> {
    let paths = app.state::<AppState>().paths.clone();
    let net = cached_or_resolve_network_identity(app, &paths).await;
    let Some(network_key) = net.gateway_mac.filter(|value| value != "unknown") else {
        return Ok(None);
    };
    let Some(target_scope_fingerprint) =
        category_scope_fingerprint(&paths, AdaptiveCategory::Gaming)
    else {
        return Err("Не удалось проверить Gaming + GitHub lists".into());
    };
    let cache = AdaptiveStrategyCache::load(&paths);
    let recommendation = recommendation::for_gaming(
        &cache,
        &network_key,
        transport,
        &engine_version(),
        Some(&target_scope_fingerprint),
        |category| category_scope_fingerprint(&paths, category),
    );
    Ok(recommendation.map(|recommendation| ResolvedRecommendation {
        recommendation,
        network_key,
        asn_region: net.asn_region,
        target_scope_fingerprint,
    }))
}

pub async fn gaming_recommendation_for_current_network(
    app: &AppHandle,
    transport: StrategyTransport,
) -> Result<Option<AdaptiveRecommendationDescriptor>, String> {
    Ok(resolve_gaming_recommendation(app, transport)
        .await?
        .as_ref()
        .map(|resolved| AdaptiveRecommendationDescriptor::from(&resolved.recommendation)))
}

pub async fn apply_gaming_recommendation(
    app: &AppHandle,
    transport: StrategyTransport,
) -> Result<AdaptiveRecommendationDescriptor, String> {
    if !adaptive_enabled(app) {
        return Err("Adaptive Strategy Brain выключен в настройках".into());
    }
    let resolved = resolve_gaming_recommendation(app, transport)
        .await?
        .ok_or_else(|| "Недостаточно подтверждённых данных этой сети".to_string())?;
    let descriptor = AdaptiveRecommendationDescriptor::from(&resolved.recommendation);
    let paths = app.state::<AppState>().paths.clone();
    let mut cache = AdaptiveStrategyCache::load(&paths);
    cache
        .put_recommended_scoped(
            &resolved.network_key,
            resolved.asn_region.as_deref(),
            &engine_version(),
            resolved.recommendation.candidate.clone(),
            Some(&resolved.target_scope_fingerprint),
            resolved.recommendation.source_category,
            &resolved.recommendation.source_candidate_id,
            resolved.recommendation.source_scope_fingerprint.as_deref(),
            unix_secs(),
            &resolved.recommendation.reason,
        )
        .and_then(|_| cache.save(&paths))
        .map_err(|error| error.to_string())?;

    let snapshot = crate::dpi::runtime_snapshot(app);
    let Some(DpiLaunchSpec::Zapret2 {
        selections,
        mut adaptive_overrides,
    }) = snapshot.launch
    else {
        return Err("Рекомендацию можно применить только при активном Zapret2".into());
    };
    adaptive_overrides.insert(
        override_key(AdaptiveCategory::Gaming, transport),
        resolved.recommendation.candidate,
    );

    let state = app.state::<AppState>();
    let _gate = state.dpi_gate.lock().await;
    if crate::dpi::runtime_snapshot(app).generation != snapshot.generation {
        return Err("Конфигурация Zapret2 изменилась во время применения рекомендации".into());
    }
    crate::dpi::start_zapret2_with_overrides(app, &selections, &adaptive_overrides).await?;
    Ok(descriptor)
}

pub(crate) async fn cached_entries_for_overrides(
    app: &AppHandle,
    adaptive_overrides: &BTreeMap<String, StrategyCandidate>,
) -> BTreeMap<String, AdaptiveCacheEntry> {
    let mut entries = BTreeMap::new();
    let paths = app.state::<AppState>().paths.clone();
    let net = cached_or_resolve_network_identity(app, &paths).await;
    let Some(network_key) = net.gateway_mac.filter(|value| value != "unknown") else {
        return entries;
    };
    let cache = AdaptiveStrategyCache::load(&paths);
    let engine = engine_version();

    for (key, candidate) in adaptive_overrides {
        let Some(scope_fingerprint) = category_scope_fingerprint(&paths, candidate.category) else {
            continue;
        };
        let Some(entry) = cache.entry_for_transport_scoped(
            &network_key,
            candidate.category,
            candidate.transport,
            &engine,
            Some(&scope_fingerprint),
        ) else {
            continue;
        };
        if entry.candidate_id != candidate.candidate_id() {
            continue;
        }
        if entry.trust == StrategyTrust::Recommended {
            let (Some(source_category), Some(source_transport), Some(source_candidate_id)) = (
                entry.source_category,
                entry.source_transport,
                entry.source_candidate_id.as_deref(),
            ) else {
                continue;
            };
            let Some(source_scope) = category_scope_fingerprint(&paths, source_category) else {
                continue;
            };
            let source_valid = cache
                .entry_for_transport_scoped(
                    &network_key,
                    source_category,
                    source_transport,
                    &engine,
                    Some(&source_scope),
                )
                .is_some_and(|source| {
                    source.trust == StrategyTrust::Confirmed
                        && source.candidate_id == source_candidate_id
                });
            if !source_valid {
                continue;
            }
        }
        entries.insert(key.clone(), entry);
    }
    entries
}

fn probe_completion_matches_status(
    status: &RecoveryStatus,
    session_id: u64,
    attempt_id: u64,
    candidate_id: &str,
) -> bool {
    status.phase == RecoveryPhase::CandidateProbe
        && status.session_id == Some(session_id)
        && status.attempt_id == Some(attempt_id)
        && status.candidate_id.as_deref() == Some(candidate_id)
}

async fn handle_event(
    app: &AppHandle,
    model: &mut RecoveryModel,
    context: &mut SessionContext,
    detector: &mut PassiveDetector,
    evidence: &Arc<EvidenceWindow>,
    control_tx: &mpsc::Sender<RuntimeEvent>,
    event: RuntimeEvent,
    now: u64,
) -> Vec<RecoveryAction> {
    match event {
        RuntimeEvent::Observation {
            domain,
            verdict,
            generation,
        } => {
            if !adaptive_enabled(app) {
                return Vec::new();
            }
            let Some(category) = category_for_domain(&domain) else {
                return Vec::new();
            };
            if crate::dpi::runtime_snapshot(app).generation != generation {
                return Vec::new();
            }
            if !zapret2_is_active(app) {
                return Vec::new();
            }
            detector
                .observe(category, verdict, now)
                .map(|reason| model.step(RecoveryEvent::DiagnosisConfirmed { category, reason }))
                .unwrap_or_default()
        }
        RuntimeEvent::StartSearch(category, transport) => {
            if !adaptive_enabled(app) {
                crate::util::emit_log(
                    app,
                    "warn",
                    "adaptive",
                    "Adaptive Strategy Brain выключен в настройках",
                );
                return Vec::new();
            }
            if !zapret2_is_active(app) {
                crate::util::emit_log(
                    app,
                    "warn",
                    "adaptive",
                    "Поиск доступен только при активном Zapret2",
                );
                return Vec::new();
            }
            if matches!(
                model.status().phase,
                RecoveryPhase::DiscoveringQuic
                    | RecoveryPhase::Calibrating
                    | RecoveryPhase::Searching
                    | RecoveryPhase::CandidateProbe
                    | RecoveryPhase::TemporaryVerification
                    | RecoveryPhase::Applying
                    | RecoveryPhase::RollingBack
            ) {
                crate::util::emit_log(app, "warn", "adaptive", "search_already_running");
                return Vec::new();
            }

            let reason = model
                .status()
                .diagnosis
                .unwrap_or(DiagnosisReason::ProbeFailure);
            let mut actions = Vec::new();
            if model.status().phase != RecoveryPhase::Suggested
                || model.status().category != Some(category)
            {
                actions.extend(model.step(RecoveryEvent::DiagnosisConfirmed { category, reason }));
            }
            let transport = transport.unwrap_or(match reason {
                DiagnosisReason::QuicBlackhole => StrategyTransport::Quic,
                _ => StrategyTransport::Tls,
            });
            actions.extend(model.step(RecoveryEvent::PreparationStarted {
                category,
                transport,
            }));
            let Some(session_id) = model.status().session_id else {
                return actions;
            };

            context.tasks.abort_all();
            let app = app.clone();
            let tx = control_tx.clone();
            let evidence = evidence.clone();
            let task = async_runtime::spawn(async move {
                let result = prepare_search_data(
                    &app, &tx, &evidence, session_id, category, transport, reason,
                )
                .await;
                let _ = tx
                    .send(RuntimeEvent::PreparationFinished { session_id, result })
                    .await;
            });
            context.tasks.replace_preparation(task);
            actions
        }
        RuntimeEvent::PreparationDiscoveryFinished { session_id } => {
            model.step(RecoveryEvent::PreparationDiscoveryFinished { session_id })
        }
        RuntimeEvent::PreparationProgress {
            session_id,
            current_round,
            total_rounds,
        } => model.step(RecoveryEvent::PreparationProgress {
            session_id,
            current_round,
            total_rounds,
        }),
        RuntimeEvent::PreparationFinished { session_id, result } => {
            if model.status().session_id != Some(session_id) {
                return Vec::new();
            }
            context.tasks.finish_preparation();
            match result {
                Ok(prepared) => {
                    if crate::dpi::runtime_snapshot(app).generation != prepared.snapshot.generation
                    {
                        return model.step(RecoveryEvent::PreparationFailed {
                            session_id,
                            reason: PreparationFailure::ProbeUnreliable,
                        });
                    }
                    context.original = Some(prepared.snapshot.clone());
                    context.expected_generation = prepared.snapshot.generation;
                    context.category = Some(prepared.category);
                    context.network_key = prepared.network_key;
                    context.asn_region = prepared.asn_region;
                    context.tried.clear();
                    context.probe_transport = Some(prepared.transport);
                    context.probe_targets = prepared.targets;
                    context.dns_cache = prepared.dns_cache;
                    context.session_mode = Some(prepared.mode);
                    context.last_probe = None;
                    model.step(RecoveryEvent::PreparationReady {
                        session_id,
                        candidates: prepared.candidates,
                        mode: prepared.mode,
                    })
                }
                Err(reason) => model.step(RecoveryEvent::PreparationFailed { session_id, reason }),
            }
        }
        RuntimeEvent::CandidateStartFinished {
            session_id,
            attempt_id,
            candidate_id,
            outcome,
        } => {
            context.tasks.finish_candidate();
            let status = model.status();
            if status.session_id != Some(session_id)
                || status.attempt_id != Some(attempt_id)
                || status.candidate_id.as_deref() != Some(candidate_id.as_str())
                || !matches!(
                    status.phase,
                    RecoveryPhase::Searching | RecoveryPhase::RollingBack
                )
            {
                return Vec::new();
            }

            context.expected_generation = outcome.generation_after;
            if let Some(rollback) =
                take_pending_rollback(&mut context.pending_rollback, session_id, attempt_id)
            {
                context.candidate_generation.store(0, Ordering::SeqCst);
                evidence.begin(0);
                return vec![rollback];
            }

            let ok = outcome.result.is_ok();
            if let Err(error) = outcome.result {
                crate::util::emit_log(
                    app,
                    "error",
                    "adaptive",
                    &format!(
                        "candidate_start_failed session={session_id} attempt={attempt_id}: {error}"
                    ),
                );
            }
            if ok {
                context
                    .candidate_generation
                    .store(outcome.generation_after, Ordering::SeqCst);
                evidence.begin(outcome.generation_after);
            } else {
                context.candidate_generation.store(0, Ordering::SeqCst);
                evidence.begin(0);
            }
            model.step(RecoveryEvent::CandidateStarted {
                session_id,
                attempt_id,
                candidate_id,
                ok,
            })
        }
        RuntimeEvent::Cancel => {
            context.tasks.abort_interruptible();
            model.step(RecoveryEvent::UserCancel)
        }
        RuntimeEvent::Confirm {
            session_id,
            candidate_id,
        } => {
            let status = model.status();
            let Some(attempt_id) = status.attempt_id.filter(|_| {
                status.session_id == Some(session_id)
                    && status.candidate_id.as_deref() == Some(candidate_id.as_str())
            }) else {
                return Vec::new();
            };
            model.step(RecoveryEvent::UserConfirm {
                session_id,
                attempt_id,
                candidate_id,
            })
        }
        RuntimeEvent::Reject {
            session_id,
            candidate_id,
        } => {
            let status = model.status();
            let Some(attempt_id) = status.attempt_id.filter(|_| {
                status.session_id == Some(session_id)
                    && status.candidate_id.as_deref() == Some(candidate_id.as_str())
            }) else {
                return Vec::new();
            };
            model.step(RecoveryEvent::UserReject {
                session_id,
                attempt_id,
                candidate_id,
            })
        }
        RuntimeEvent::ResetSaved(category) => {
            reset_saved(app, category).await;
            Vec::new()
        }
        RuntimeEvent::ProbeFinished {
            session_id,
            attempt_id,
            generation,
            candidate_id,
            series,
        } => {
            let status = model.status();
            if context.expected_generation != generation
                || crate::dpi::runtime_snapshot(app).generation != generation
                || !probe_completion_matches_status(&status, session_id, attempt_id, &candidate_id)
            {
                return Vec::new();
            }
            context.tasks.finish_probe();
            for batch in &series.rounds {
                let _ = app.emit("adaptive://probe", batch);
            }
            let eyes = evidence.snapshot();
            let result = series.evaluate(&eyes);
            log_probe_series(app, "candidate", &series, &result);
            context.last_probe = Some(series);
            model.step(RecoveryEvent::ProbeFinished {
                session_id,
                attempt_id,
                candidate_id,
                result,
                now,
            })
        }
        RuntimeEvent::CandidateCrashed { generation } => {
            if context.expected_generation != generation {
                return Vec::new();
            }
            let status = model.status();
            match (status.session_id, status.attempt_id, status.candidate_id) {
                (Some(session_id), Some(attempt_id), Some(candidate_id)) => {
                    model.step(RecoveryEvent::CandidateCrashed {
                        session_id,
                        attempt_id,
                        candidate_id,
                    })
                }
                _ => Vec::new(),
            }
        }
        RuntimeEvent::RollbackFinished {
            session_id,
            attempt_id,
            outcome,
        } => {
            context.tasks.finish_rollback();
            let status = model.status();
            if status.phase != RecoveryPhase::RollingBack
                || status.session_id != Some(session_id)
                || status.attempt_id != Some(attempt_id)
            {
                return Vec::new();
            }
            context.expected_generation = outcome.generation_after;
            model.step(RecoveryEvent::RollbackFinished {
                session_id,
                attempt_id,
                restored: outcome.restored,
                base_healthy: outcome.base_healthy,
                base_probe_reliable: outcome.base_probe_reliable,
            })
        }
        RuntimeEvent::Shutdown => {
            context.tasks.abort_interruptible();
            model.step(RecoveryEvent::Shutdown)
        }
    }
}

async fn prepare_search_data(
    app: &AppHandle,
    control_tx: &mpsc::Sender<RuntimeEvent>,
    evidence: &Arc<EvidenceWindow>,
    session_id: u64,
    category: AdaptiveCategory,
    transport: StrategyTransport,
    reason: DiagnosisReason,
) -> Result<PreparedSearch, PreparationFailure> {
    let snapshot = crate::dpi::runtime_snapshot(app);
    let Some(DpiLaunchSpec::Zapret2 {
        adaptive_overrides, ..
    }) = snapshot.launch.as_ref()
    else {
        crate::util::emit_log(
            app,
            "warn",
            "adaptive",
            "Поиск доступен только при активном Zapret2",
        );
        return Err(PreparationFailure::ProbeUnreliable);
    };

    let paths = app.state::<AppState>().paths.clone();
    let net_future = async {
        Ok::<_, PreparationFailure>(cached_or_resolve_network_identity(app, &paths).await)
    };
    let probe_future = async {
        let tuning = search_tuning(app);
        let dns_cache = SessionDnsCache::new();
        let targets = if transport == StrategyTransport::Quic {
            let targets =
                probe::discover_quic_targets_with_cache(category, tuning.probe_timeout, &dns_cache)
                    .await;
            if targets.is_empty() {
                crate::util::emit_log(
                    app,
                    "warn",
                    "adaptive",
                    &format!(
                        "quic_targets_unavailable: category={} endpoints не объявили HTTP/3",
                        category.as_key()
                    ),
                );
                return Err(PreparationFailure::QuicTargetsUnavailable);
            }
            let _ = control_tx
                .send(RuntimeEvent::PreparationDiscoveryFinished { session_id })
                .await;
            targets
        } else {
            probe::targets_for(category, transport)
        };

        let baseline_ids = generator::builtin_baseline_candidates(category)
            .iter()
            .map(compiler::effective_fingerprint)
            .collect::<Vec<_>>()
            .join(",");
        crate::util::emit_log(
            app,
            "info",
            "adaptive",
            &format!(
                "baseline calibration started: category={} transport={:?} fingerprints=[{}] targets=[{}]",
                category.as_key(),
                transport,
                baseline_ids,
                targets
                    .iter()
                    .map(|target| target.host)
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        );

        let _ = control_tx.try_send(RuntimeEvent::PreparationProgress {
            session_id,
            current_round: 0,
            total_rounds: tuning.candidate_rounds,
        });
        evidence.begin(snapshot.generation);
        let calibration = probe::run_probe_series_for_targets_with_progress_and_cache(
            category,
            transport,
            &targets,
            tuning.probe_timeout,
            tuning.candidate_rounds,
            tuning.required_successes,
            tuning.probe_interval,
            &dns_cache,
            |batch| {
                let _ = control_tx.try_send(RuntimeEvent::PreparationProgress {
                    session_id,
                    current_round: batch.round,
                    total_rounds: tuning.candidate_rounds,
                });
                let _ = app.emit("adaptive://probe", batch);
            },
        )
        .await;
        let initial_eyes = evidence.snapshot();
        if calibration.evaluate(&initial_eyes).is_success() {
            evidence
                .wait_for_quiet(
                    snapshot.generation,
                    tuning.eyes_quiet_window,
                    tuning.eyes_quiet_deadline,
                )
                .await;
        }
        let eyes = evidence.snapshot();
        let mut probe_targets = targets;
        let mut calibration_result = calibration.evaluate(&eyes);
        log_probe_series(
            app,
            "baseline_calibration",
            &calibration,
            &calibration_result,
        );

        if category == AdaptiveCategory::Discord
            && transport == StrategyTransport::Tls
            && !calibration_result.is_success()
        {
            let stable_targets = calibration.stable_core_targets(&probe_targets, &eyes);
            if !stable_targets.is_empty() && stable_targets.len() < probe_targets.len() {
                let stable_hosts = stable_targets
                    .iter()
                    .map(|target| target.host)
                    .collect::<Vec<_>>();
                let excluded_hosts = probe_targets
                    .iter()
                    .filter(|target| !stable_hosts.contains(&target.host))
                    .map(|target| target.host)
                    .collect::<Vec<_>>();
                let restricted = calibration.restricted_to(&stable_targets);
                let restricted_result = restricted.evaluate(&eyes);
                if restricted_result.is_success() {
                    crate::util::emit_log(
                        app,
                        "warn",
                        "adaptive",
                        &format!(
                            "discord_probe_targets_quarantined: excluded=[{}] active=[{}]",
                            excluded_hosts.join(","),
                            stable_hosts.join(",")
                        ),
                    );
                    log_probe_series(
                        app,
                        "baseline_calibration_viable_targets",
                        &restricted,
                        &restricted_result,
                    );
                    probe_targets = stable_targets;
                    calibration_result = restricted_result;
                }
            }
        }

        let mode = match classify_session_mode(transport, &calibration_result) {
            Ok(SearchSessionMode::Recovery) => {
                crate::util::emit_log(
                    app,
                    "warn",
                    "adaptive",
                    &format!(
                        "baseline QUIC не работает ({}): recovery search разрешен",
                        calibration_result.failure_stage.as_str()
                    ),
                );
                SearchSessionMode::Recovery
            }
            Ok(SearchSessionMode::Comparison) => SearchSessionMode::Comparison,
            Err(error) => {
                crate::util::emit_log(
                    app,
                    "error",
                    "adaptive",
                    &format!(
                        "baseline calibration failed at {}: probe environment unreliable",
                        calibration_result.failure_stage.as_str()
                    ),
                );
                return Err(error);
            }
        };

        Ok::<_, PreparationFailure>((probe_targets, dns_cache, mode, tuning.candidate_budget))
    };
    let (net, (probe_targets, dns_cache, mode, candidate_budget)) =
        tokio::try_join!(net_future, probe_future)?;
    let network_key = net.gateway_mac.filter(|value| value != "unknown");
    let cache = AdaptiveStrategyCache::load(&paths);
    let engine = engine_version();
    let scope_fingerprint = category_scope_fingerprint(&paths, category);
    let saved = network_key.as_deref().and_then(|key| {
        cache.confirmed_candidate_for_transport_scoped(
            key,
            category,
            transport,
            &engine,
            scope_fingerprint.as_deref(),
        )
    });
    let current = adaptive_overrides.get(&override_key(category, transport));

    let diagnosis = if transport == StrategyTransport::Quic {
        AdaptiveDiagnosis::QuicHandshake
    } else {
        match reason {
            DiagnosisReason::RepeatedReset => AdaptiveDiagnosis::Reset,
            DiagnosisReason::QuicBlackhole => AdaptiveDiagnosis::QuicHandshake,
            DiagnosisReason::TlsBlackhole => AdaptiveDiagnosis::TlsHandshake,
            DiagnosisReason::ProbeFailure => AdaptiveDiagnosis::Blackhole,
        }
    };
    let empty_tried = HashSet::new();
    let mut candidates = generator::generate(GeneratorInput {
        category,
        diagnosis,
        network_confirmed: saved.as_ref(),
        current,
        tried: &empty_tried,
    });
    candidates.retain(|candidate| candidate.transport == transport);
    candidates.truncate(candidate_budget);

    Ok(PreparedSearch {
        snapshot,
        network_key,
        asn_region: net.asn_region,
        category,
        transport,
        targets: probe_targets,
        dns_cache,
        candidates,
        mode,
    })
}
#[allow(clippy::too_many_arguments)]
async fn execute_actions(
    app: &AppHandle,
    model: &mut RecoveryModel,
    context: &mut SessionContext,
    actions: Vec<RecoveryAction>,
    status_tx: &watch::Sender<RecoveryStatus>,
    control_tx: &mpsc::Sender<RuntimeEvent>,
    candidate_generation: &Arc<AtomicU64>,
    evidence: &Arc<EvidenceWindow>,
    started: Instant,
) {
    let mut pending = VecDeque::from(actions);
    while let Some(action) = pending.pop_front() {
        let follow_up = match action {
            RecoveryAction::NotifySuggestion { category, reason } => {
                let _ = app.emit(
                    "adaptive://suggestion",
                    SuggestionPayload { category, reason },
                );
                None
            }
            RecoveryAction::StartCandidate {
                session_id,
                attempt_id,
                candidate,
                ..
            } => {
                let candidate_id = candidate.candidate_id();
                let fingerprint = compiler::effective_fingerprint(&candidate);
                crate::util::emit_log(
                    app,
                    "info",
                    "adaptive",
                    &format!(
                        "candidate_start session={session_id} attempt={attempt_id} id={candidate_id} fingerprint={fingerprint} transport={:?}",
                        candidate.transport
                    ),
                );
                context.tried.insert(fingerprint);
                context.probe_transport = Some(candidate.transport);
                context.last_probe = None;
                context.pending_rollback = None;
                let app = app.clone();
                let tx = control_tx.clone();
                let original = context.original.clone();
                let category = context.category;
                let expected_generation = context.expected_generation;
                let task = async_runtime::spawn(async move {
                    let outcome = if let (Some(original), Some(category)) = (original, category) {
                        candidate_runtime::start(
                            &app,
                            &original,
                            expected_generation,
                            category.as_key(),
                            candidate,
                        )
                        .await
                    } else {
                        candidate_runtime::failed(&app, "Adaptive session snapshot отсутствует")
                    };
                    let _ = tx
                        .send(RuntimeEvent::CandidateStartFinished {
                            session_id,
                            attempt_id,
                            candidate_id,
                            outcome,
                        })
                        .await;
                });
                context.tasks.replace_candidate(task);
                None
            }
            RecoveryAction::RunProbes {
                session_id,
                attempt_id,
                candidate,
            } => {
                let tx = control_tx.clone();
                let category = candidate.category;
                let transport = candidate.transport;
                let candidate_id = candidate.candidate_id();
                let generation = context.expected_generation;
                let tuning = search_tuning(app);
                let targets = context.probe_targets.clone();
                let dns_cache = context.dns_cache.clone();
                let evidence = evidence.clone();
                let task = async_runtime::spawn(async move {
                    tokio::time::sleep(tuning.stabilization_delay).await;
                    let series = probe::run_probe_series_for_targets_with_cache(
                        category,
                        transport,
                        &targets,
                        tuning.probe_timeout,
                        tuning.candidate_rounds,
                        tuning.required_successes,
                        tuning.probe_interval,
                        &dns_cache,
                    )
                    .await;
                    let initial_eyes = evidence.snapshot();
                    if series.evaluate(&initial_eyes).is_success() {
                        evidence
                            .wait_for_quiet(
                                generation,
                                tuning.eyes_quiet_window,
                                tuning.eyes_quiet_deadline,
                            )
                            .await;
                    }
                    let _ = tx
                        .send(RuntimeEvent::ProbeFinished {
                            session_id,
                            attempt_id,
                            generation,
                            candidate_id,
                            series,
                        })
                        .await;
                });
                context.tasks.replace_probe(task);
                None
            }
            RecoveryAction::BeginVerification { .. } => {
                // Фаза TemporaryVerification доходит до UI через сопутствующий
                // EmitStatus (model добавляет emit_status рядом с BeginVerification),
                // поэтому отдельное adaptive://verification не эмитим — фронт его не
                // слушает, дублирующий канал только рассинхронил бы контракт.
                None
            }
            RecoveryAction::Rollback {
                session_id,
                attempt_id,
                reason,
            } => {
                evidence.begin(0);
                candidate_generation.store(0, Ordering::SeqCst);
                if context.tasks.rollback_running() || context.pending_rollback.is_some() {
                    continue;
                }
                if context.tasks.candidate_running() {
                    context.pending_rollback = Some(PendingRollback {
                        session_id,
                        attempt_id,
                        reason,
                    });
                    crate::util::emit_log(
                        app,
                        "info",
                        "adaptive",
                        &format!(
                            "rollback_deferred session={session_id} attempt={attempt_id}: candidate spawn still running"
                        ),
                    );
                    continue;
                }
                if matches!(
                    reason,
                    super::model::RollbackReason::CandidateProbeFailed
                        | super::model::RollbackReason::CandidateCrashed
                ) {
                    record_failure(app, context);
                }
                crate::util::emit_log(
                    app,
                    "info",
                    "adaptive",
                    &format!(
                        "rollback_start session={session_id} attempt={attempt_id} reason={reason:?}"
                    ),
                );
                let tuning = search_tuning(app);
                let request = rollback::RollbackRequest {
                    session_id,
                    attempt_id,
                    original: context.original.clone(),
                    expected_generation: context.expected_generation,
                    category: context.category,
                    transport: context.probe_transport,
                    targets: context.probe_targets.clone(),
                    dns_cache: context.dns_cache.clone(),
                    recovery_mode: context.session_mode == Some(SearchSessionMode::Recovery),
                    config: rollback::RollbackConfig {
                        probe_timeout: tuning.probe_timeout,
                        probe_interval: tuning.probe_interval,
                        stabilization_delay: tuning.stabilization_delay,
                        base_recheck_rounds: tuning.base_recheck_rounds,
                        eyes_quiet_window: tuning.eyes_quiet_window,
                        eyes_quiet_deadline: tuning.eyes_quiet_deadline,
                    },
                    evidence: evidence.clone(),
                };
                let tx = control_tx.clone();
                let app = app.clone();
                let task = async_runtime::spawn(async move {
                    let outcome = rollback::run(app, request).await;
                    let _ = tx
                        .send(RuntimeEvent::RollbackFinished {
                            session_id,
                            attempt_id,
                            outcome,
                        })
                        .await;
                });
                context.pending_rollback = None;
                context.tasks.replace_rollback(task);
                None
            }
            RecoveryAction::PersistConfirmed {
                session_id,
                attempt_id,
                candidate,
            } => {
                let candidate_id = candidate.candidate_id();
                let ok = persist_confirmed(app, context, candidate);
                Some(RecoveryEvent::PersistFinished {
                    session_id,
                    attempt_id,
                    candidate_id,
                    ok,
                })
            }
            RecoveryAction::EmitStatus(status) => {
                if status.phase == RecoveryPhase::Applied {
                    candidate_generation.store(0, Ordering::SeqCst);
                    evidence.begin(0);
                }
                if matches!(
                    status.phase,
                    RecoveryPhase::Applied
                        | RecoveryPhase::Exhausted
                        | RecoveryPhase::ProbeUnreliable
                        | RecoveryPhase::QuicTargetsUnavailable
                        | RecoveryPhase::BaseUnhealthy
                        | RecoveryPhase::InternalError
                        | RecoveryPhase::Cancelled
                ) {
                    context.dns_cache = SessionDnsCache::new();
                }
                let _ = app.emit("adaptive://status", &status);
                status_tx.send_replace(status);
                None
            }
        };
        if let Some(event) = follow_up {
            pending.extend(model.step(restamp_event(event, elapsed_ms(started))));
        }
    }
}

fn restamp_event(event: RecoveryEvent, now: u64) -> RecoveryEvent {
    match event {
        RecoveryEvent::ProbeFinished {
            session_id,
            attempt_id,
            candidate_id,
            result,
            ..
        } => RecoveryEvent::ProbeFinished {
            session_id,
            attempt_id,
            candidate_id,
            result,
            now,
        },
        other => other,
    }
}

fn persist_confirmed(
    app: &AppHandle,
    context: &SessionContext,
    candidate: StrategyCandidate,
) -> bool {
    let Some(network_key) = context.network_key.as_deref() else {
        crate::util::emit_log(
            app,
            "error",
            "adaptive",
            "Нет стабильного fingerprint сети — стратегия не сохранена",
        );
        return false;
    };
    let paths = app.state::<AppState>().paths.clone();
    let Some(scope_fingerprint) = context
        .category
        .and_then(|category| category_scope_fingerprint(&paths, category))
    else {
        crate::util::emit_log(
            app,
            "error",
            "adaptive",
            "Не удалось вычислить fingerprint списков — стратегия не сохранена",
        );
        return false;
    };
    let now = unix_secs();
    let probe = probe_summary(context, now);
    let mut cache = AdaptiveStrategyCache::load(&paths);
    cache
        .put_confirmed_scoped(
            network_key,
            context.asn_region.as_deref(),
            &engine_version(),
            candidate,
            Some(&scope_fingerprint),
            now,
            probe,
        )
        .and_then(|_| cache.save(&paths))
        .is_ok()
}

fn record_failure(app: &AppHandle, context: &SessionContext) {
    let Some(network_key) = context.network_key.as_deref() else {
        return;
    };
    let Some(category) = context.category else {
        return;
    };
    let paths = app.state::<AppState>().paths.clone();
    let mut cache = AdaptiveStrategyCache::load(&paths);
    let Some(transport) = context.probe_transport else {
        return;
    };
    if cache.record_failure_for_transport(
        network_key,
        category,
        transport,
        probe_summary(context, unix_secs()),
    ) {
        let _ = cache.save(&paths);
    }
}

async fn reset_saved(app: &AppHandle, category: AdaptiveCategory) {
    let paths = app.state::<AppState>().paths.clone();
    let net = cached_or_resolve_network_identity(app, &paths).await;
    let Some(key) = net.gateway_mac.filter(|value| value != "unknown") else {
        return;
    };
    let mut cache = AdaptiveStrategyCache::load(&paths);
    if cache.reset(&key, category) {
        let _ = cache.save(&paths);
    }

    let snapshot = crate::dpi::runtime_snapshot(app);
    let Some(DpiLaunchSpec::Zapret2 {
        selections,
        mut adaptive_overrides,
    }) = snapshot.launch
    else {
        return;
    };
    let before = adaptive_overrides.len();
    adaptive_overrides.retain(|_, candidate| candidate.category != category);
    if adaptive_overrides.len() == before {
        return;
    }

    let state = app.state::<AppState>();
    let _gate = state.dpi_gate.lock().await;
    if crate::dpi::runtime_snapshot(app).generation != snapshot.generation {
        crate::util::emit_log(
            app,
            "warn",
            "adaptive",
            "Сброс стратегии пропущен: конфигурация Zapret2 уже изменилась",
        );
        return;
    }
    if let Err(error) =
        crate::dpi::start_zapret2_with_overrides(app, &selections, &adaptive_overrides).await
    {
        crate::util::emit_log(
            app,
            "error",
            "adaptive",
            &format!("Не удалось вернуть bundled-профиль после сброса: {error}"),
        );
    }
}

pub(super) fn log_probe_series(
    app: &AppHandle,
    label: &str,
    series: &ProbeSeries,
    result: &super::model::CandidateProbeResult,
) {
    let evidence = serde_json::json!({
        "label": label,
        "series": series,
        "decision": result,
    });
    crate::util::emit_log(app, "info", "adaptive", &evidence.to_string());
}

fn probe_summary(context: &SessionContext, measured_at: u64) -> ProbeSummary {
    let (passed, failed) = context.last_probe.as_ref().map_or((0, 0), |series| {
        series
            .rounds
            .iter()
            .flat_map(|batch| batch.targets.iter())
            .fold((0, 0), |(passed, failed), target| {
                if target.https_ok {
                    (passed + 1, failed)
                } else {
                    (passed, failed + 1)
                }
            })
    });
    ProbeSummary {
        passed,
        failed,
        measured_at,
    }
}

fn zapret2_is_active(app: &AppHandle) -> bool {
    matches!(
        crate::dpi::runtime_snapshot(app).launch,
        Some(DpiLaunchSpec::Zapret2 { .. })
    )
}

fn adaptive_enabled(app: &AppHandle) -> bool {
    app.state::<AppState>()
        .settings
        .lock()
        .map(|settings| settings.adaptive_strategy_enabled)
        .unwrap_or(false)
}

fn category_scope_fingerprint(
    paths: &crate::paths::Paths,
    category: AdaptiveCategory,
) -> Option<String> {
    let files: &[&str] = match category {
        AdaptiveCategory::Discord => &["discord.txt"],
        AdaptiveCategory::YoutubeTwitch => &["youtube_twitch.txt"],
        AdaptiveCategory::Gaming => &["gaming-github.txt", "ipset-gaming.txt"],
    };
    let mut hash = Sha256::new();
    for file_name in files {
        let bytes = std::fs::read(paths.lists_dir().join(file_name)).ok()?;
        hash.update(file_name.as_bytes());
        hash.update([0]);
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    Some(
        hash.finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    )
}
fn engine_version() -> String {
    let (major, minor, patch) = crate::dpi_engine::zapret2::WINWS2_VERSION;
    format!("{major}.{minor}.{patch}")
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u64::MAX as u128) as u64
}

fn runtime_shutdown_complete(
    requested: bool,
    phase: RecoveryPhase,
    candidate_running: bool,
    rollback_running: bool,
) -> bool {
    requested && phase != RecoveryPhase::RollingBack && !candidate_running && !rollback_running
}

fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0)
}

fn category_for_domain(domain: &str) -> Option<AdaptiveCategory> {
    let domain = domain.trim_end_matches('.').to_ascii_lowercase();
    if matches_domain(
        &domain,
        &["youtube.com", "googlevideo.com", "ytimg.com", "youtu.be"],
    ) {
        Some(AdaptiveCategory::YoutubeTwitch)
    } else if matches_domain(
        &domain,
        &[
            "discord.com",
            "discord.gg",
            "discordapp.com",
            "discordapp.net",
        ],
    ) {
        Some(AdaptiveCategory::Discord)
    } else if matches_domain(
        &domain,
        &[
            "github.com",
            "githubusercontent.com",
            "githubassets.com",
            "roblox.com",
            "rbxcdn.com",
            "epicgames.com",
            "ea.com",
            "battle.net",
            "xboxlive.com",
            "deadbydaylight.com",
        ],
    ) {
        Some(AdaptiveCategory::Gaming)
    } else {
        None
    }
}

fn matches_domain(domain: &str, suffixes: &[&str]) -> bool {
    suffixes
        .iter()
        .any(|suffix| domain == *suffix || domain.ends_with(&format!(".{suffix}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_modes_have_distinct_bounded_budgets() {
        let fast = search_tuning_for("fast");
        let balanced = search_tuning_for("balanced");
        let deep = search_tuning_for("deep");
        assert!(fast.candidate_budget < balanced.candidate_budget);
        assert!(balanced.candidate_budget < deep.candidate_budget);
        assert!(fast.probe_timeout < balanced.probe_timeout);
        assert!(balanced.probe_timeout < deep.probe_timeout);
        assert_eq!(search_tuning_for("unknown"), balanced);
        assert!(fast.required_successes <= fast.candidate_rounds);
        assert!(deep.required_successes <= deep.candidate_rounds);
        for tuning in [fast, balanced, deep] {
            assert!(tuning.eyes_quiet_window <= tuning.eyes_quiet_deadline);
            assert!(tuning.eyes_quiet_deadline <= tuning.probe_interval);
        }
    }
    #[test]
    fn domain_mapping_is_suffix_safe() {
        assert_eq!(
            category_for_domain("WWW.YouTube.com"),
            Some(AdaptiveCategory::YoutubeTwitch)
        );
        assert_eq!(
            category_for_domain("r3---sn.googlevideo.com"),
            Some(AdaptiveCategory::YoutubeTwitch)
        );
        assert_eq!(
            category_for_domain("gateway.discord.gg"),
            Some(AdaptiveCategory::Discord)
        );
        assert_eq!(category_for_domain("notdiscord.gg.example"), None);
        assert_eq!(
            category_for_domain("api.github.com"),
            Some(AdaptiveCategory::Gaming)
        );
        assert_eq!(
            category_for_domain("ecsv2.roblox.com"),
            Some(AdaptiveCategory::Gaming)
        );
        assert_eq!(category_for_domain("github.com.evil.example"), None);
    }

    #[test]
    fn passive_detector_requires_repeated_failures_and_resets_on_working() {
        let mut detector = PassiveDetector::default();
        let category = AdaptiveCategory::Discord;
        assert_eq!(detector.observe(category, Verdict::Reset, 1), None);
        assert_eq!(detector.observe(category, Verdict::Reset, 2), None);
        assert_eq!(detector.observe(category, Verdict::Working, 3), None);
        assert_eq!(detector.observe(category, Verdict::Reset, 4), None);
        assert_eq!(detector.observe(category, Verdict::Reset, 5), None);
        assert_eq!(
            detector.observe(category, Verdict::Reset, 6),
            Some(DiagnosisReason::RepeatedReset)
        );
    }

    #[tokio::test]
    async fn observation_flood_does_not_block_control_queue() {
        let (observation_tx, mut observation_rx) = mpsc::channel(1);
        let (control_tx, mut control_rx) = mpsc::channel(1);
        let evidence = Arc::new(EvidenceWindow::default());
        evidence.begin(1);
        let input = AdaptiveInput {
            observation_tx,
            control_tx,
            candidate_generation: Arc::new(AtomicU64::new(0)),
            evidence: evidence.clone(),
        };
        assert!(input.try_observation("discord.com".into(), Verdict::Reset, 1));
        assert!(!input.try_observation("discord.com".into(), Verdict::Reset, 1));
        let snapshot = evidence.snapshot();
        assert_eq!(snapshot.reset_count, 2);
        assert_eq!(snapshot.blackhole_count, 0);
        tokio::time::timeout(Duration::from_millis(50), input.cancel())
            .await
            .expect("control send must not wait for observation queue")
            .unwrap();
        assert!(matches!(
            control_rx.recv().await,
            Some(RuntimeEvent::Cancel)
        ));
        assert!(observation_rx.recv().await.is_some());
    }

    #[tokio::test]
    async fn evidence_quiet_window_restarts_for_current_generation() {
        let evidence = Arc::new(EvidenceWindow::default());
        evidence.begin(7);
        let waiter_evidence = evidence.clone();
        let started = Instant::now();
        let waiter = tokio::spawn(async move {
            waiter_evidence
                .wait_for_quiet(7, Duration::from_millis(30), Duration::from_millis(150))
                .await;
        });

        tokio::time::sleep(Duration::from_millis(20)).await;
        evidence.observe(7, "discord.com", Verdict::Reset);
        tokio::time::timeout(Duration::from_millis(200), waiter)
            .await
            .expect("quiet window exceeded its hard deadline")
            .unwrap();

        assert!(
            started.elapsed() >= Duration::from_millis(45),
            "current-generation evidence must restart the quiet interval"
        );
    }

    #[test]
    fn shutdown_waits_for_owned_side_effect_completion() {
        assert!(!runtime_shutdown_complete(
            true,
            RecoveryPhase::RollingBack,
            false,
            true,
        ));
        assert!(!runtime_shutdown_complete(
            true,
            RecoveryPhase::RollingBack,
            true,
            false,
        ));
        assert!(runtime_shutdown_complete(
            true,
            RecoveryPhase::Cancelled,
            false,
            false,
        ));
        assert!(!runtime_shutdown_complete(
            false,
            RecoveryPhase::Idle,
            false,
            false,
        ));
    }

    #[tokio::test]
    async fn evidence_quiet_window_stops_at_hard_deadline() {
        let evidence = Arc::new(EvidenceWindow::default());
        evidence.begin(9);
        let noisy_evidence = evidence.clone();
        let noise = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(10)).await;
                noisy_evidence.observe(9, "discord.com", Verdict::Working);
            }
        });
        let started = Instant::now();

        evidence
            .wait_for_quiet(9, Duration::from_millis(30), Duration::from_millis(70))
            .await;
        noise.abort();

        assert!(
            started.elapsed() >= Duration::from_millis(60),
            "continuous evidence must keep the quiet window open until its bound"
        );
        assert!(
            started.elapsed() < Duration::from_millis(180),
            "quiet window must remain bounded under continuous evidence"
        );
    }

    #[test]
    fn deferred_rollback_is_released_once_for_matching_completion() {
        let mut pending = Some(PendingRollback {
            session_id: 4,
            attempt_id: 7,
            reason: RollbackReason::UserCancelled,
        });

        assert!(take_pending_rollback(&mut pending, 4, 6).is_none());
        assert!(pending.is_some());
        assert!(matches!(
            take_pending_rollback(&mut pending, 4, 7),
            Some(RecoveryAction::Rollback {
                session_id: 4,
                attempt_id: 7,
                reason: RollbackReason::UserCancelled,
            })
        ));
        assert!(take_pending_rollback(&mut pending, 4, 7).is_none());
    }
    #[test]
    fn quic_failure_enters_recovery_but_dns_failure_does_not() {
        let mut result = crate::adaptive_strategy::model::CandidateProbeResult::default();
        result.transport = StrategyTransport::Quic;
        result.dns_ok = true;
        result.failure_stage = crate::adaptive_strategy::evidence::FailureStage::Quic;
        assert_eq!(
            classify_session_mode(StrategyTransport::Quic, &result),
            Ok(SearchSessionMode::Recovery)
        );

        result.dns_ok = false;
        result.failure_stage = crate::adaptive_strategy::evidence::FailureStage::Dns;
        assert_eq!(
            classify_session_mode(StrategyTransport::Quic, &result),
            Err(PreparationFailure::ProbeUnreliable)
        );
    }

    #[test]
    fn crash_signal_is_generation_scoped() {
        let (observation_tx, _observation_rx) = mpsc::channel(1);
        let (control_tx, _control_rx) = mpsc::channel(1);
        let generation = Arc::new(AtomicU64::new(9));
        let input = AdaptiveInput {
            observation_tx,
            control_tx,
            candidate_generation: generation.clone(),
            evidence: Arc::new(EvidenceWindow::default()),
        };
        assert!(!input.try_candidate_crashed(8));
        assert!(input.try_candidate_crashed(9));
        generation.store(0, Ordering::SeqCst);
        assert!(!input.try_candidate_crashed(9));
    }

    #[test]
    fn probe_completion_guard_rejects_stale_session_attempt_candidate_and_phase() {
        let candidate =
            generator::builtin_baseline_candidates(AdaptiveCategory::YoutubeTwitch)[0].clone();
        let candidate_id = candidate.candidate_id();
        let mut model = RecoveryModel::new(RecoveryCfg::default());
        model.step(RecoveryEvent::DiagnosisConfirmed {
            category: AdaptiveCategory::YoutubeTwitch,
            reason: DiagnosisReason::TlsBlackhole,
        });
        let actions = model.step(RecoveryEvent::UserStart {
            category: AdaptiveCategory::YoutubeTwitch,
            candidates: vec![candidate],
        });
        let RecoveryAction::StartCandidate {
            session_id,
            attempt_id,
            ..
        } = actions[0]
        else {
            panic!("expected candidate start");
        };
        model.step(RecoveryEvent::CandidateStarted {
            session_id,
            attempt_id,
            candidate_id: candidate_id.clone(),
            ok: true,
        });

        let status = model.status();
        assert!(probe_completion_matches_status(
            &status,
            session_id,
            attempt_id,
            &candidate_id,
        ));
        assert!(!probe_completion_matches_status(
            &status,
            session_id + 1,
            attempt_id,
            &candidate_id,
        ));
        assert!(!probe_completion_matches_status(
            &status,
            session_id,
            attempt_id + 1,
            &candidate_id,
        ));
        assert!(!probe_completion_matches_status(
            &status,
            session_id,
            attempt_id,
            "stale-candidate",
        ));

        model.step(RecoveryEvent::UserCancel);
        assert!(!probe_completion_matches_status(
            &model.status(),
            session_id,
            attempt_id,
            &candidate_id,
        ));
    }
}
