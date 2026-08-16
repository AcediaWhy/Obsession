//! Service-owned Legacy Eyes, Manager and recovery shell.
//!
//! Every byte used to build the attribution registry comes from the verified
//! Program Files catalog. Raw packets and protected paths remain inside the
//! service; callers receive only a later sanitized status projection.

#![cfg(windows)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use obsession_runtime_protocol::{
    DpiCategory, DpiEngine, DpiSelection, LegacyAssessmentClassification,
    LegacyAssessmentConfidence, LegacyEvidenceSnapshot, LegacyLanePhase, LegacyLaneRuntimeSnapshot,
    LegacyObserverHealth, LegacyRecoveryAttemptPhase, LegacyRecoveryAttemptSnapshot,
    LegacyRecoveryCompletionDisposition, LegacyRecoveryCompletionSnapshot,
    LegacyRecoveryControlsRequest, LegacyRecoveryControlsSnapshot, LegacyRecoveryDisposition,
    LegacyRecoveryMode as ProtocolRecoveryMode, LegacyRecoveryOrigin as ProtocolRecoveryOrigin,
    LegacyRecoveryPhase, LegacyRecoveryProposalSnapshot, LegacyRecoveryRuntimeSnapshot,
    LegacyReliabilityCounters, LegacyReliabilityRuntimeSnapshot, MAX_RELIABILITY_COUNTER,
    MAX_RELIABILITY_LANES,
};
use obsession_runtime_reliability::eyes::flow::SocketAttributionHint;
use obsession_runtime_reliability::eyes::{self, Config, EyesHandle};
use obsession_runtime_reliability::legacy_reliability::adapter::adapt_observation;
use obsession_runtime_reliability::legacy_reliability::assessment::{
    AssessmentClassification, AssessmentConfidence, LanePhase,
};
use obsession_runtime_reliability::legacy_reliability::contracts::{
    EventEnvelope, EyeHealthState, IntentFence, LaneGeneration, LegacySessionContext,
    NetworkFingerprint, ProcessOwner, SensorGeneration, SessionId,
};
use obsession_runtime_reliability::legacy_reliability::environment_gate::{
    EnvironmentGate, GateClassification, GateReport, GateRequest, GateRequestError,
    LocalNetworkSnapshot, ReqwestProbeBackend,
};
use obsession_runtime_reliability::legacy_reliability::ingress::{channel, LegacyIngress};
use obsession_runtime_reliability::legacy_reliability::manager::{
    ObserveOnlyManager, ObserveOnlySnapshot, PreparedEnvironmentGate, ReceiveOutcome,
};
use obsession_runtime_reliability::legacy_reliability::policy::{
    LaneConfigOptions, ObserveOnlyBrain, PresumedIntent,
};
use obsession_runtime_reliability::legacy_reliability::recovery::{
    ApprovalError, ConsiderError, RecoveryAction, RecoveryConfig, RecoveryDecision, RecoveryMode,
    RecoveryOrigin, RecoveryPhase as CoordinatorRecoveryPhase, RecoverySuggestion,
};
use obsession_runtime_reliability::legacy_reliability::recovery_runtime::{
    IncidentObservation, LegacyRecoveryRuntime as CoordinatorRuntime,
};
use obsession_runtime_reliability::legacy_reliability::target_registry::{
    parse_legacy_config, HostlistKind, LegacyConfigRecord, TargetRegistry,
};
use obsession_runtime_reliability::{acknowledge_manager_publication, EyesTeardownTicket};

use crate::legacy_access_activity::LegacyAccessPoll;
use crate::protected_layout::{VerifiedResource, VerifiedRuntimeCatalog};

const MAX_CONFIG_BYTES: u64 = 512 * 1024;
const MAX_HOSTLIST_BYTES: u64 = 16 * 1024 * 1024;
const MAX_SNAPSHOT_BYTES: u64 = 128 * 1024 * 1024;
const MANAGER_POLL_INTERVAL: Duration = Duration::from_millis(250);
const MANAGER_STOP_TIMEOUT: Duration = Duration::from_secs(1);
const EYES_STOP_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_CONFIRMATION_TARGETS: usize = 2;
const ACTIVE_DIAGNOSTIC_INITIAL_DELAY: Duration = Duration::from_secs(15);
const ACTIVE_DIAGNOSTIC_REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);
const ACTIVE_DIAGNOSTIC_RETRY_INTERVAL: Duration = Duration::from_secs(60);
const ACTIVE_DIAGNOSTIC_TTL_MS: u64 = 5 * 60 * 1_000;

#[derive(Debug)]
pub enum LegacyObserverError {
    EngineUnavailable,
    WinDivertUnavailable,
    InvalidResource,
    SnapshotTooLarge,
    InvalidUtf8,
    RegistryInvalid,
    ManagerUnavailable,
    EyesUnavailable,
    GateUnavailable,
    ThreadUnavailable,
}

impl std::fmt::Display for LegacyObserverError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::EngineUnavailable => "verified Legacy engine is unavailable",
            Self::WinDivertUnavailable => "verified Legacy WinDivert.dll is unavailable",
            Self::InvalidResource => "verified Legacy observer resource is invalid",
            Self::SnapshotTooLarge => "Legacy observer registry exceeds its size bound",
            Self::InvalidUtf8 => "Legacy observer text resource is not UTF-8",
            Self::RegistryInvalid => "Legacy observer registry is invalid",
            Self::ManagerUnavailable => "Legacy reliability Manager could not be created",
            Self::EyesUnavailable => "Legacy Eyes could not be started",
            Self::GateUnavailable => "Legacy Environment Gate could not be created",
            Self::ThreadUnavailable => "Legacy Manager worker could not be started",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for LegacyObserverError {}

/// Immutable, startup-validated source for per-generation observer sessions.
pub struct LegacyObserverCatalog {
    records: Vec<LegacyConfigRecord>,
    sources: Vec<VerifiedResource>,
    windivert_dll: VerifiedResource,
}

impl LegacyObserverCatalog {
    pub fn from_catalog(catalog: &VerifiedRuntimeCatalog) -> Result<Self, LegacyObserverError> {
        let engine = catalog
            .engine(DpiEngine::Legacy)
            .ok_or(LegacyObserverError::EngineUnavailable)?;
        let mut text_cache = BTreeMap::<String, String>::new();
        let mut sources = BTreeMap::<String, VerifiedResource>::new();
        let mut records = Vec::with_capacity(engine.strategies().len());
        let mut windivert_dll = None;
        let mut total_bytes = 0u64;

        for strategy in engine.strategies() {
            let artifact = strategy.artifact_resource();
            let config_content = read_text_resource(
                artifact,
                MAX_CONFIG_BYTES,
                &mut total_bytes,
                &mut text_cache,
            )?;
            sources.insert(resource_key(artifact.relative_path()), artifact.clone());

            let parsed = parse_legacy_config(&config_content)
                .map_err(|_| LegacyObserverError::RegistryInvalid)?;
            let mut record = LegacyConfigRecord::new(
                category_name(strategy.category()),
                strategy.strategy_id(),
                config_content,
            );

            for dependency in strategy.dependency_resources() {
                let key = resource_key(dependency.relative_path());
                if dependency.relative_path().file_name().is_some_and(|name| {
                    name.to_string_lossy().eq_ignore_ascii_case("WinDivert.dll")
                }) {
                    windivert_dll.get_or_insert_with(|| dependency.clone());
                }
                sources.entry(key).or_insert_with(|| dependency.clone());
            }

            for reference in parsed
                .hostlists
                .into_iter()
                .filter(|reference| reference.kind != HostlistKind::AutoInclude)
            {
                let dependency = strategy
                    .dependency_resources()
                    .iter()
                    .find(|resource| {
                        resource_key(resource.relative_path())
                            == normalized_reference(&reference.reference)
                    })
                    .ok_or(LegacyObserverError::InvalidResource)?;
                let content = read_text_resource(
                    dependency,
                    MAX_HOSTLIST_BYTES,
                    &mut total_bytes,
                    &mut text_cache,
                )?;
                record = record.with_hostlist(reference.reference, content);
            }
            records.push(record);
        }

        TargetRegistry::from_records(&records).map_err(|_| LegacyObserverError::RegistryInvalid)?;
        let windivert_dll = windivert_dll.ok_or(LegacyObserverError::WinDivertUnavailable)?;
        windivert_dll
            .verify()
            .map_err(|_| LegacyObserverError::InvalidResource)?;

        Ok(Self {
            records,
            sources: sources.into_values().collect(),
            windivert_dll,
        })
    }

    fn prepare_registry(
        &self,
        selections: &[DpiSelection],
    ) -> Result<Arc<TargetRegistry>, LegacyObserverError> {
        for source in &self.sources {
            source
                .verify()
                .map_err(|_| LegacyObserverError::InvalidResource)?;
        }
        let active = selections
            .iter()
            .map(|selection| {
                (
                    category_name(selection.category).to_owned(),
                    selection.strategy_id.clone(),
                )
            })
            .collect::<Vec<_>>();
        TargetRegistry::from_records_with_active_selections(&self.records, active)
            .map(Arc::new)
            .map_err(|_| LegacyObserverError::RegistryInvalid)
    }
}

pub struct ProtectedLegacyObserver {
    generation: u64,
    registry: Arc<TargetRegistry>,
    publication: Arc<Mutex<ObserverPublication>>,
    stop: Arc<AtomicBool>,
    manager_done: mpsc::Receiver<()>,
    manager_thread: Option<JoinHandle<()>>,
    eyes: Option<EyesHandle>,
}

#[derive(Clone)]
struct ObserverPublication {
    revision: u64,
    snapshot: ObserveOnlySnapshot,
}

pub struct ObserverStopResult {
    pub manager_stopped: bool,
    pub eyes_status: obsession_runtime_reliability::EyesTeardownStatus,
    pub pending_eyes: Option<EyesTeardownTicket>,
}

pub struct LegacyObserverRuntime {
    catalog: LegacyObserverCatalog,
    active: Option<ProtectedLegacyObserver>,
    pending_eyes: Option<EyesTeardownTicket>,
}

/// Exact service-private input for one coordinator observation. Raw targets,
/// paths and process identifiers never enter the public runtime snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LegacyRecoveryInput {
    pub(crate) observation: IncidentObservation,
    pub(crate) fence: IntentFence,
    pub(crate) previous: RecoveryConfig,
    pub(crate) candidates: Vec<RecoveryConfig>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LegacyRecoveryScan {
    pub(crate) generation: u64,
    pub(crate) revision: u64,
    pub(crate) session_id: SessionId,
    pub(crate) input: Option<LegacyRecoveryInput>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LegacyRecoveryCandidateContext {
    pub(crate) registry_content_hash: [u8; 32],
    pub(crate) exclusive_target_count: usize,
    pub(crate) lane_generations: BTreeMap<String, LaneGeneration>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LegacyObserverStartPlan {
    runtime_generation: u64,
    session_id: SessionId,
    sensor_generation: SensorGeneration,
    network_fingerprint: NetworkFingerprint,
    lane_generations: BTreeMap<String, LaneGeneration>,
}

impl LegacyObserverStartPlan {
    fn initial(
        generation: u64,
        selections: &[DpiSelection],
        network_fingerprint: NetworkFingerprint,
    ) -> Option<Self> {
        let lane_generations = selections
            .iter()
            .map(|selection| {
                (
                    category_name(selection.category).to_owned(),
                    LaneGeneration::new(generation),
                )
            })
            .collect();
        Self::checked(
            generation,
            SessionId::new(generation),
            SensorGeneration::new(generation),
            network_fingerprint,
            lane_generations,
            selections,
        )
    }

    pub(crate) fn recovery(
        runtime_generation: u64,
        session_id: SessionId,
        sensor_generation: SensorGeneration,
        network_fingerprint: NetworkFingerprint,
        lane_generations: BTreeMap<String, LaneGeneration>,
        selections: &[DpiSelection],
    ) -> Option<Self> {
        if !network_fingerprint.is_stable() {
            return None;
        }
        Self::checked(
            runtime_generation,
            session_id,
            sensor_generation,
            network_fingerprint,
            lane_generations,
            selections,
        )
    }

    fn checked(
        runtime_generation: u64,
        session_id: SessionId,
        sensor_generation: SensorGeneration,
        network_fingerprint: NetworkFingerprint,
        lane_generations: BTreeMap<String, LaneGeneration>,
        selections: &[DpiSelection],
    ) -> Option<Self> {
        let categories = selections
            .iter()
            .map(|selection| category_name(selection.category).to_owned())
            .collect::<BTreeSet<_>>();
        if runtime_generation == 0
            || session_id.get() == 0
            || sensor_generation.get() == 0
            || selections.is_empty()
            || categories.len() != selections.len()
            || categories != lane_generations.keys().cloned().collect()
            || lane_generations
                .values()
                .any(|generation| generation.get() == 0)
        {
            return None;
        }
        Some(Self {
            runtime_generation,
            session_id,
            sensor_generation,
            network_fingerprint,
            lane_generations,
        })
    }

    pub(crate) fn runtime_generation(&self) -> u64 {
        self.runtime_generation
    }

    pub(crate) fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub(crate) fn sensor_generation(&self) -> SensorGeneration {
        self.sensor_generation
    }

    pub(crate) fn lane_generation(&self, category: &str) -> Option<LaneGeneration> {
        self.lane_generations.get(category).copied()
    }

    /// Builds a fresh observer epoch for the previous protected selection.
    /// Neighbor lane generations remain byte-for-byte identical; only the
    /// recovering lane and sensor epoch change.
    pub(crate) fn restore_previous(
        &self,
        category: &str,
        previous_lane_generation: LaneGeneration,
        selections: &[DpiSelection],
    ) -> Option<Self> {
        let mut lane_generations = self.lane_generations.clone();
        if !lane_generations.contains_key(category) || previous_lane_generation.get() == 0 {
            return None;
        }
        lane_generations.insert(category.to_owned(), previous_lane_generation);
        Self::checked(
            self.runtime_generation,
            self.session_id,
            SensorGeneration::new(next_nonzero_epoch(self.sensor_generation.get())),
            self.network_fingerprint.clone(),
            lane_generations,
            selections,
        )
    }
}

/// Service-owned shell around the pure coordinator. All mutating decisions,
/// opaque Assisted proposals and authorization generations remain inside the
/// protected service.
pub(crate) struct LegacyRecoveryRuntime {
    coordinator: CoordinatorRuntime,
    controls: LegacyRecoveryControlsSnapshot,
    started: Instant,
    observed_generation: Option<u64>,
    last_revision: u64,
    last_input: Option<LegacyRecoveryInput>,
    suggestion: Option<RecoverySuggestion>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LegacyRecoveryRuntimeError {
    StaleObserverRevision,
    MissingPreviousOwner,
    CoordinatorRejected(ConsiderError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegacyObserverRuntimeError {
    Busy,
    Conflict,
    StartFailed,
    StopUnresolved,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegacyObserverSnapshotError {
    InvalidCategory,
    InvalidFence,
}

impl LegacyObserverRuntime {
    pub fn from_catalog(catalog: &VerifiedRuntimeCatalog) -> Result<Self, LegacyObserverError> {
        Ok(Self {
            catalog: LegacyObserverCatalog::from_catalog(catalog)?,
            active: None,
            pending_eyes: None,
        })
    }

    pub fn prepare_start(&mut self) -> Result<(), LegacyObserverRuntimeError> {
        self.poll_pending();
        if self.active.is_some() || self.pending_eyes.is_some() {
            Err(LegacyObserverRuntimeError::Busy)
        } else {
            Ok(())
        }
    }

    pub fn start(
        &mut self,
        generation: u64,
        selections: &[DpiSelection],
    ) -> Result<(), LegacyObserverRuntimeError> {
        self.prepare_start()?;
        let observer = ProtectedLegacyObserver::start(&self.catalog, generation, selections)
            .map_err(|_| LegacyObserverRuntimeError::StartFailed)?;
        self.active = Some(observer);
        Ok(())
    }

    pub(crate) fn start_recovery(
        &mut self,
        plan: LegacyObserverStartPlan,
        selections: &[DpiSelection],
    ) -> Result<(), LegacyObserverRuntimeError> {
        self.prepare_start()?;
        let observer = ProtectedLegacyObserver::start_with_plan(&self.catalog, plan, selections)
            .map_err(|_| LegacyObserverRuntimeError::StartFailed)?;
        self.active = Some(observer);
        Ok(())
    }

    pub(crate) fn rebind_generation(
        &mut self,
        expected: u64,
        replacement: u64,
    ) -> Result<(), LegacyObserverRuntimeError> {
        let active = self
            .active
            .as_mut()
            .ok_or(LegacyObserverRuntimeError::Conflict)?;
        if expected == 0 || replacement == 0 || active.generation != expected {
            return Err(LegacyObserverRuntimeError::Conflict);
        }
        active.generation = replacement;
        Ok(())
    }

    pub fn stop(&mut self, generation: u64) -> Result<(), LegacyObserverRuntimeError> {
        self.poll_pending();
        let Some(active) = self.active.as_ref() else {
            return if self.pending_eyes.is_some() {
                Err(LegacyObserverRuntimeError::StopUnresolved)
            } else {
                Ok(())
            };
        };
        if generation == 0 || active.generation() != generation {
            return Err(LegacyObserverRuntimeError::Conflict);
        }
        let result = self.active.take().expect("checked active observer").stop();
        let clean = result.is_clean();
        self.pending_eyes = result.pending_eyes;
        if clean {
            Ok(())
        } else {
            Err(LegacyObserverRuntimeError::StopUnresolved)
        }
    }

    /// Raw service-internal snapshot reserved for the generation-fenced
    /// recovery executor. It must never be returned over IPC directly.
    #[allow(dead_code)]
    pub fn snapshot(&self) -> Option<ObserveOnlySnapshot> {
        self.active.as_ref().map(ProtectedLegacyObserver::snapshot)
    }

    /// Produces the only observer representation allowed to cross IPC. The
    /// conversion intentionally drops raw targets, config identities,
    /// timestamps and all process/resource ownership details.
    pub fn public_snapshot(
        &self,
    ) -> Result<Option<LegacyReliabilityRuntimeSnapshot>, LegacyObserverSnapshotError> {
        let Some(active) = self.active.as_ref() else {
            return Ok(None);
        };
        let generation = active.generation();
        let publication = active.publication();
        sanitize_snapshot(generation, publication.revision, &publication.snapshot).map(Some)
    }

    /// Feeds bounded, exact process-owned SYN sockets into the currently active
    /// Eyes generation. The monitor snapshot carries no domain; this method
    /// resolves one unambiguous target from the generation-fenced registry.
    pub(crate) fn apply_access_poll(&self, poll: &LegacyAccessPoll) {
        if let Some(active) = self.active.as_ref() {
            active.apply_access_poll(poll);
        }
    }

    pub(crate) fn recovery_scan(&self) -> Option<LegacyRecoveryScan> {
        self.active
            .as_ref()
            .map(ProtectedLegacyObserver::recovery_scan)
    }

    pub(crate) fn recovery_candidate_context(
        &self,
        scan: &LegacyRecoveryScan,
        candidate: &RecoveryConfig,
    ) -> Option<LegacyRecoveryCandidateContext> {
        self.active
            .as_ref()?
            .recovery_candidate_context(scan, candidate)
    }

    pub(crate) fn current_fence(&self, category: &str) -> Option<IntentFence> {
        let active = self.active.as_ref()?;
        let publication = active.publication();
        let session = &publication.snapshot.session;
        if session.closed
            || session.session_id.get() == 0
            || session.sensor_generation.get() == 0
            || session.target_registry_version != active.registry.version()
            || !session.network_fingerprint_at_start.is_stable()
        {
            return None;
        }
        Some(IntentFence {
            session_id: session.session_id,
            category: category.to_owned(),
            lane_generation: *session.lane_generations.get(category)?,
            sensor_generation: session.sensor_generation,
            registry_version: session.target_registry_version,
            network_fingerprint: session.network_fingerprint_at_start.clone(),
        })
    }

    fn poll_pending(&mut self) {
        let Some(ticket) = self.pending_eyes.as_mut() else {
            return;
        };
        if ticket.wait_bounded(Duration::ZERO).resolved {
            self.pending_eyes = None;
        }
    }
}

/// Returns a bounded set of targets owned exclusively by the exact candidate
/// currently bound in the service observer. The caller receives no registry or
/// path handle and cannot broaden the capture/probe scope.
pub(crate) fn confirmation_candidate_targets(
    runtime: &LegacyObserverRuntime,
    category: &str,
    candidate: &RecoveryConfig,
) -> Option<Vec<String>> {
    let active = runtime.active.as_ref()?;
    let active_config = active.registry.active_config(category)?;
    let fingerprint = active
        .registry
        .config_fingerprint(category, active_config)
        .ok()?;
    if !active_config.eq_ignore_ascii_case(candidate.config_id())
        || fingerprint.as_hex() != candidate.fingerprint().as_str()
    {
        return None;
    }
    let targets = active
        .registry
        .config_target_suffixes(category, candidate.config_id())
        .ok()?
        .into_iter()
        .take(MAX_CONFIRMATION_TARGETS)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    (!targets.is_empty()).then_some(targets)
}

impl LegacyRecoveryRuntime {
    pub(crate) fn new() -> Self {
        Self {
            coordinator: CoordinatorRuntime::new(RecoveryMode::ObserveOnly),
            controls: LegacyRecoveryControlsSnapshot::observe_only(),
            started: Instant::now(),
            observed_generation: None,
            last_revision: 0,
            last_input: None,
            suggestion: None,
        }
    }

    pub(crate) fn reset(&mut self) {
        if self.observed_generation.is_none() {
            return;
        }
        let controls = self.controls.clone();
        *self = Self::new();
        self.controls = controls;
        self.apply_controls();
    }

    /// Stores and applies one atomic, service-owned control generation.
    /// Existing executor-owned attempts remain responsible for reaching a
    /// safe terminal state; mode/pause/freeze changes govern future attempts
    /// and revoke an unapproved Assisted proposal when required.
    pub(crate) fn set_controls(
        &mut self,
        request: LegacyRecoveryControlsRequest,
    ) -> Result<u64, ()> {
        let frozen = request
            .frozen_categories
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        if frozen.len() != request.frozen_categories.len()
            || frozen.len() > MAX_RELIABILITY_LANES
            || (request.mode != ProtocolRecoveryMode::Automatic && !request.automatic_paused)
        {
            return Err(());
        }
        let frozen_categories = frozen.into_iter().collect::<Vec<_>>();
        if self.controls.mode == request.mode
            && self.controls.automatic_paused == request.automatic_paused
            && self.controls.frozen_categories == frozen_categories
        {
            return Ok(self.controls.control_generation);
        }
        self.controls = LegacyRecoveryControlsSnapshot {
            control_generation: next_nonzero_epoch(self.controls.control_generation),
            mode: request.mode,
            automatic_paused: request.automatic_paused,
            frozen_categories,
        };
        self.last_revision = 0;
        self.last_input = None;
        self.suggestion = None;
        self.apply_controls();
        Ok(self.controls.control_generation)
    }

    fn apply_controls(&mut self) {
        self.coordinator.set_mode(match self.controls.mode {
            ProtocolRecoveryMode::ObserveOnly => RecoveryMode::ObserveOnly,
            ProtocolRecoveryMode::Assisted => RecoveryMode::Assisted,
            ProtocolRecoveryMode::Automatic => RecoveryMode::Automatic,
        });
        self.coordinator
            .set_automatic_paused(self.controls.automatic_paused);
        self.coordinator.set_manual_frozen_categories(
            self.controls
                .frozen_categories
                .iter()
                .copied()
                .map(category_name),
        );
    }

    pub(crate) fn monotonic_now_ms(&self) -> u64 {
        elapsed_ms(self.started)
    }

    pub(crate) fn poll(
        &mut self,
        scan: LegacyRecoveryScan,
        previous_owner: Option<ProcessOwner>,
    ) -> Result<Option<RecoveryAction>, LegacyRecoveryRuntimeError> {
        let now_ms = elapsed_ms(self.started);
        self.poll_at(scan, previous_owner, now_ms)
    }

    fn poll_at(
        &mut self,
        scan: LegacyRecoveryScan,
        previous_owner: Option<ProcessOwner>,
        now_ms: u64,
    ) -> Result<Option<RecoveryAction>, LegacyRecoveryRuntimeError> {
        if self.observed_generation != Some(scan.generation) {
            self.observed_generation = Some(scan.generation);
            self.last_revision = 0;
            self.last_input = None;
            self.suggestion = None;
            self.coordinator.observe_session(scan.session_id);
        } else if scan.revision < self.last_revision {
            self.suggestion = None;
            return Err(LegacyRecoveryRuntimeError::StaleObserverRevision);
        } else if scan.revision == self.last_revision {
            return Ok(None);
        }
        self.last_revision = scan.revision;

        let Some(input) = scan.input else {
            self.last_input = None;
            self.suggestion = None;
            return Ok(None);
        };
        if self.last_input.as_ref() == Some(&input) {
            return Ok(None);
        }
        self.last_input = Some(input.clone());
        self.suggestion = None;

        let previous_owner =
            previous_owner.ok_or(LegacyRecoveryRuntimeError::MissingPreviousOwner)?;
        match self.coordinator.consider_with_control_generation(
            input.observation,
            input.fence,
            input.previous,
            previous_owner,
            input.candidates,
            self.controls.control_generation,
            now_ms,
        ) {
            Ok(RecoveryDecision::ObserveOnly { suggestion }) => {
                self.suggestion = Some(suggestion);
                Ok(None)
            }
            Ok(RecoveryDecision::Automatic { action }) => Ok(Some(action)),
            Ok(RecoveryDecision::Assisted { .. }) => Ok(None),
            Err(
                ConsiderError::Busy
                | ConsiderError::IncidentAlreadyHandled
                | ConsiderError::NoEligibleCandidate
                | ConsiderError::AutomaticPaused
                | ConsiderError::CategoryFrozen
                | ConsiderError::CategoryHalted
                | ConsiderError::AutomaticPacing { .. },
            ) => Ok(None),
            Err(error) => Err(LegacyRecoveryRuntimeError::CoordinatorRejected(error)),
        }
    }

    /// Rebinds the original coordinator-owned incident to an exact retry
    /// `Preflight` action. This is preparation metadata only: the caller must
    /// still prove that the live observer fence equals the action envelope
    /// before resolving a new immutable `PreparedLegacyRecovery`.
    pub(crate) fn preflight_scan_for_action(
        &self,
        live_scan: &LegacyRecoveryScan,
        action: &RecoveryAction,
    ) -> Option<LegacyRecoveryScan> {
        let RecoveryAction::Preflight {
            envelope,
            previous,
            candidate,
            ..
        } = action
        else {
            return None;
        };
        let mut input = self.last_input.clone()?;
        if live_scan.generation == 0
            || live_scan.revision == 0
            || live_scan.session_id != envelope.session_id
            || input.fence.session_id != envelope.session_id
            || input.fence.category != envelope.category
            || input.fence.network_fingerprint != envelope.expected_network_fingerprint
            || &input.previous != previous
            || !input.candidates.iter().any(|known| known == candidate)
        {
            return None;
        }
        input.fence = IntentFence {
            session_id: envelope.session_id,
            category: envelope.category.clone(),
            lane_generation: envelope.expected_lane_generation,
            sensor_generation: envelope.expected_sensor_generation,
            registry_version: envelope.expected_registry_version,
            network_fingerprint: envelope.expected_network_fingerprint.clone(),
        };
        input.observation.session_id = envelope.session_id;
        input.observation.sensor_generation = envelope.expected_sensor_generation;
        input.observation.category = envelope.category.clone();
        input.observation.lane_generation = envelope.expected_lane_generation;
        Some(LegacyRecoveryScan {
            generation: live_scan.generation,
            revision: live_scan.revision,
            session_id: live_scan.session_id,
            input: Some(input),
        })
    }

    pub(crate) fn public_snapshot(&self, observer_active: bool) -> LegacyRecoveryRuntimeSnapshot {
        let status = self.coordinator.status();
        let proposal = status.proposal.as_ref().and_then(|proposal| {
            Some(LegacyRecoveryProposalSnapshot {
                proposal_id: proposal.proposal_id.get(),
                attempt_id: proposal.attempt_id.get(),
                incident_id: proposal.incident_id.get(),
                category: protocol_recovery_category(&proposal.category)?,
                previous_config_id: proposal.previous_config_id.clone(),
                candidate_config_id: proposal.candidate_config_id.clone(),
                expires_at_monotonic_ms: proposal.expires_at_monotonic_ms,
            })
        });
        let active_attempt = status.active_attempt.as_ref().and_then(|attempt| {
            Some(LegacyRecoveryAttemptSnapshot {
                attempt_id: attempt.attempt_id.get(),
                incident_id: attempt.incident_id.get(),
                category: protocol_recovery_category(&attempt.category)?,
                previous_config_id: attempt.previous_config_id.clone(),
                candidate_config_id: attempt.candidate_config_id.clone(),
                origin: protocol_recovery_origin(attempt.origin)?,
                phase: protocol_recovery_attempt_phase(attempt.phase),
                phase_started_at_monotonic_ms: attempt.phase_started_at_monotonic_ms,
            })
        });
        let last_completion = status.last_completion.as_ref().and_then(|completion| {
            Some(LegacyRecoveryCompletionSnapshot {
                attempt_id: completion.attempt_id.get(),
                incident_id: completion.incident_id.get(),
                category: protocol_recovery_category(&completion.category)?,
                previous_config_id: completion.previous_config_id.clone(),
                candidate_config_id: completion.candidate_config_id.clone(),
                origin: protocol_recovery_origin(completion.origin)?,
                phase: protocol_recovery_attempt_phase(completion.phase),
                disposition: match completion.disposition {
                    obsession_runtime_reliability::legacy_reliability::recovery::RecoveryDisposition::CandidateApplied => LegacyRecoveryCompletionDisposition::CandidateApplied,
                    obsession_runtime_reliability::legacy_reliability::recovery::RecoveryDisposition::PreviousPreserved => LegacyRecoveryCompletionDisposition::PreviousPreserved,
                    obsession_runtime_reliability::legacy_reliability::recovery::RecoveryDisposition::RolledBack => LegacyRecoveryCompletionDisposition::RolledBack,
                    obsession_runtime_reliability::legacy_reliability::recovery::RecoveryDisposition::ProcessFailed => LegacyRecoveryCompletionDisposition::ProcessFailed,
                },
                finished_at_monotonic_ms: completion.finished_at_monotonic_ms,
            })
        });
        let (phase, disposition) = if proposal.is_some() {
            (
                LegacyRecoveryPhase::Evaluating,
                LegacyRecoveryDisposition::Pending,
            )
        } else if let Some(attempt) = active_attempt.as_ref() {
            let phase = match attempt.phase {
                LegacyRecoveryAttemptPhase::Preflight => LegacyRecoveryPhase::Evaluating,
                LegacyRecoveryAttemptPhase::RollingBack => LegacyRecoveryPhase::RollingBack,
                LegacyRecoveryAttemptPhase::Stopping
                | LegacyRecoveryAttemptPhase::Starting
                | LegacyRecoveryAttemptPhase::Confirming
                | LegacyRecoveryAttemptPhase::Applied
                | LegacyRecoveryAttemptPhase::ProcessFailed => LegacyRecoveryPhase::Executing,
            };
            (phase, LegacyRecoveryDisposition::Pending)
        } else if let Some(completion) = last_completion.as_ref() {
            match completion.disposition {
                LegacyRecoveryCompletionDisposition::CandidateApplied => (
                    LegacyRecoveryPhase::Idle,
                    LegacyRecoveryDisposition::Succeeded,
                ),
                LegacyRecoveryCompletionDisposition::PreviousPreserved
                | LegacyRecoveryCompletionDisposition::RolledBack => (
                    LegacyRecoveryPhase::Idle,
                    LegacyRecoveryDisposition::RolledBack,
                ),
                LegacyRecoveryCompletionDisposition::ProcessFailed => (
                    LegacyRecoveryPhase::ManualIntervention,
                    LegacyRecoveryDisposition::Halted,
                ),
            }
        } else if observer_active {
            (
                LegacyRecoveryPhase::Idle,
                LegacyRecoveryDisposition::ObserveOnly,
            )
        } else {
            (
                LegacyRecoveryPhase::Disabled,
                LegacyRecoveryDisposition::ObserveOnly,
            )
        };
        LegacyRecoveryRuntimeSnapshot {
            phase,
            disposition,
            controls: self.controls.clone(),
            proposal,
            active_attempt,
            last_completion,
            automatic_pacing_remaining_ms: status
                .automatic_pacing_until_monotonic_ms
                .map(|until| until.saturating_sub(self.monotonic_now_ms()))
                .filter(|remaining| *remaining > 0),
            halted_categories: status
                .halted_categories
                .iter()
                .filter_map(|category| protocol_recovery_category(category))
                .collect(),
            negative_cooldown_count: u16::try_from(status.negative_cooldown_count)
                .unwrap_or(u16::MAX),
        }
    }

    pub(crate) fn approve(
        &mut self,
        approval: obsession_runtime_reliability::legacy_reliability::recovery::AssistedApproval,
        current_fence: &IntentFence,
    ) -> Result<RecoveryAction, ApprovalError> {
        self.approve_at(approval, current_fence, self.monotonic_now_ms())
    }

    fn approve_at(
        &mut self,
        approval: obsession_runtime_reliability::legacy_reliability::recovery::AssistedApproval,
        current_fence: &IntentFence,
        now_ms: u64,
    ) -> Result<RecoveryAction, ApprovalError> {
        self.coordinator.approve(approval, current_fence, now_ms)
    }

    pub(crate) fn apply_executor_result(
        &mut self,
        result: obsession_runtime_reliability::legacy_reliability::contracts::ExecutorResult,
    ) -> Result<
        obsession_runtime_reliability::legacy_reliability::recovery::RecoveryAction,
        obsession_runtime_reliability::legacy_reliability::recovery::TransitionError,
    > {
        self.coordinator.apply_result(result)
    }

    pub(crate) fn force_manual_failure(&mut self, now_ms: u64) -> Option<RecoveryAction> {
        self.coordinator.force_manual_failure(now_ms)
    }

    #[cfg(test)]
    fn suggestion(&self) -> Option<&RecoverySuggestion> {
        self.suggestion.as_ref()
    }
}

impl ObserverStopResult {
    pub fn is_clean(&self) -> bool {
        self.manager_stopped && self.eyes_status.resolved && self.eyes_status.clean
    }
}

impl ProtectedLegacyObserver {
    pub fn start(
        catalog: &LegacyObserverCatalog,
        generation: u64,
        selections: &[DpiSelection],
    ) -> Result<Self, LegacyObserverError> {
        let network = crate::network_identity::snapshot();
        let plan =
            LegacyObserverStartPlan::initial(generation, selections, network.network_fingerprint)
                .ok_or(LegacyObserverError::RegistryInvalid)?;
        Self::start_with_plan(catalog, plan, selections)
    }

    fn start_with_plan(
        catalog: &LegacyObserverCatalog,
        plan: LegacyObserverStartPlan,
        selections: &[DpiSelection],
    ) -> Result<Self, LegacyObserverError> {
        let registry = catalog.prepare_registry(selections)?;
        let port_plan = registry
            .active_capture_plan()
            .map_err(|_| LegacyObserverError::RegistryInvalid)?;
        let categories = selections
            .iter()
            .map(|selection| category_name(selection.category).to_owned())
            .collect::<Vec<_>>();
        let context =
            LegacySessionContext::new(plan.session_id, categories, plan.network_fingerprint);
        let envelope =
            EventEnvelope::new(plan.session_id, plan.sensor_generation, registry.version());
        let (ingress, receiver) = channel();
        let callback_ingress = ingress.clone();
        let manager = ObserveOnlyManager::new_with_registry(
            context,
            plan.sensor_generation,
            Arc::clone(&registry),
            plan.lane_generations.clone(),
            ingress.counters(),
            receiver,
        )
        .map_err(|_| LegacyObserverError::ManagerUnavailable)?;
        let gate = Arc::new(tokio::sync::Mutex::new(
            EnvironmentGate::production().map_err(|_| LegacyObserverError::GateUnavailable)?,
        ));
        let publication = Arc::new(Mutex::new(ObserverPublication {
            revision: 1,
            snapshot: manager.snapshot(),
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let (manager_done_tx, manager_done) = mpsc::sync_channel(1);
        let manager_publication = Arc::clone(&publication);
        let manager_stop = Arc::clone(&stop);
        let manager_thread = std::thread::Builder::new()
            .name("obsession-legacy-manager".into())
            .spawn(move || {
                run_manager(
                    manager,
                    manager_publication,
                    manager_stop,
                    ingress,
                    gate,
                    manager_done_tx,
                )
            })
            .map_err(|_| LegacyObserverError::ThreadUnavailable)?;

        let callback_registry = Arc::clone(&registry);
        let callback_lanes = plan.lane_generations;
        let eyes = match eyes::start_legacy(
            catalog.windivert_dll.path(),
            Config::default(),
            &port_plan,
            callback_ingress.counters(),
            move |observation| {
                if let Ok(adapted) =
                    adapt_observation(observation, envelope, &callback_registry, &callback_lanes)
                {
                    let _ = callback_ingress.try_flow(adapted.event);
                }
            },
        ) {
            Ok(eyes) => eyes,
            Err(_) => {
                stop.store(true, Ordering::Release);
                if manager_done.recv_timeout(MANAGER_STOP_TIMEOUT).is_ok() {
                    let _ = manager_thread.join();
                }
                return Err(LegacyObserverError::EyesUnavailable);
            }
        };

        Ok(Self {
            generation: plan.runtime_generation,
            registry,
            publication,
            stop,
            manager_done,
            manager_thread: Some(manager_thread),
            eyes: Some(eyes),
        })
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    #[allow(dead_code)]
    pub fn snapshot(&self) -> ObserveOnlySnapshot {
        self.publication().snapshot
    }

    fn recovery_scan(&self) -> LegacyRecoveryScan {
        let publication = self.publication();
        LegacyRecoveryScan {
            generation: self.generation,
            revision: publication.revision,
            session_id: publication.snapshot.session.session_id,
            input: prepare_recovery_input(&publication.snapshot, &self.registry),
        }
    }

    fn recovery_candidate_context(
        &self,
        scan: &LegacyRecoveryScan,
        candidate: &RecoveryConfig,
    ) -> Option<LegacyRecoveryCandidateContext> {
        let publication = self.publication();
        let input = scan.input.as_ref()?;
        if self.generation != scan.generation
            || publication.revision != scan.revision
            || publication.snapshot.session.session_id != scan.session_id
            || publication.snapshot.session.target_registry_version != input.fence.registry_version
            || self.registry.version() != input.fence.registry_version
            || !input.candidates.iter().any(|known| known == candidate)
        {
            return None;
        }
        let active_config = self.registry.active_config(&input.fence.category)?;
        let active_fingerprint = self
            .registry
            .config_fingerprint(&input.fence.category, active_config)
            .ok()?;
        if active_config != input.previous.config_id()
            || active_fingerprint.as_hex() != input.previous.fingerprint().as_str()
        {
            return None;
        }
        let tentative = self
            .registry
            .tentative_selection(&input.fence.category, candidate.config_id())
            .ok()?;
        if tentative.candidate_fingerprint().as_hex() != candidate.fingerprint().as_str() {
            return None;
        }
        let exclusive_target_count = tentative.candidate_target_suffixes().len();
        if exclusive_target_count == 0 {
            return None;
        }
        Some(LegacyRecoveryCandidateContext {
            registry_content_hash: *self.registry.content_hash(),
            exclusive_target_count,
            lane_generations: publication.snapshot.session.lane_generations,
        })
    }

    fn publication(&self) -> ObserverPublication {
        self.publication
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn apply_access_poll(&self, poll: &LegacyAccessPoll) {
        let Some(eyes) = self.eyes.as_ref() else {
            return;
        };
        for socket in &poll.sockets {
            let category = category_name(socket.category);
            let targets = self.registry.active_targets_for_category(category);
            let Some(domain) = targets
                .iter()
                .copied()
                .find(|target| *target == "discord.com")
                .or_else(|| targets.first().copied())
            else {
                continue;
            };
            let _ = eyes.try_attribute_socket(SocketAttributionHint {
                key: socket.key,
                domain: domain.to_owned(),
            });
        }
    }

    pub fn stop(mut self) -> ObserverStopResult {
        self.stop_inner()
    }

    fn stop_inner(&mut self) -> ObserverStopResult {
        self.stop.store(true, Ordering::Release);
        let manager_stopped = self.manager_done.recv_timeout(MANAGER_STOP_TIMEOUT).is_ok();
        if manager_stopped {
            if let Some(thread) = self.manager_thread.take() {
                let _ = thread.join();
            }
        } else {
            self.manager_thread.take();
        }

        let mut pending_eyes = self.eyes.take().map(EyesTeardownTicket::begin);
        let eyes_status = pending_eyes.as_mut().map_or(
            obsession_runtime_reliability::EyesTeardownStatus {
                resolved: true,
                clean: true,
            },
            |ticket| ticket.wait_bounded(EYES_STOP_TIMEOUT),
        );
        if eyes_status.resolved {
            pending_eyes = None;
        }
        ObserverStopResult {
            manager_stopped,
            eyes_status,
            pending_eyes,
        }
    }
}

impl Drop for ProtectedLegacyObserver {
    fn drop(&mut self) {
        if self.eyes.is_some() || self.manager_thread.is_some() {
            let _ = self.stop_inner();
        }
    }
}

fn run_manager(
    mut manager: ObserveOnlyManager,
    publication: Arc<Mutex<ObserverPublication>>,
    stop: Arc<AtomicBool>,
    ingress: LegacyIngress,
    gate: Arc<tokio::sync::Mutex<EnvironmentGate<ReqwestProbeBackend>>>,
    done: mpsc::SyncSender<()>,
) {
    let runtime = match build_manager_runtime() {
        Ok(runtime) => runtime,
        Err(_) => {
            let _ = done.send(());
            return;
        }
    };
    let started = Instant::now();
    runtime.block_on(async {
        let mut gate_task: Option<ActiveGateTask> = None;
        let mut active_diagnostics = BTreeMap::new();
        let mut diagnostic_cursor = 0usize;
        let mut next_diagnostic_at = Instant::now() + ACTIVE_DIAGNOSTIC_INITIAL_DELAY;
        loop {
            let now_ms = elapsed_ms(started);
            if stop.load(Ordering::Acquire) {
                if let Some(task) = gate_task.take() {
                    task.join.abort();
                }
                publish_snapshot(&publication, manager.shutdown(now_ms));
                break;
            }
            tokio::select! {
                biased;
                completion = wait_gate_task(&mut gate_task), if gate_task.is_some() => {
                    if let Some(completion) = completion {
                        let active = gate_task.take().expect("completed gate task is present");
                        let now_ms = elapsed_ms(started);
                        match active.purpose {
                            ActiveGatePurpose::Assessment(prepared) => match completion {
                                Ok(Ok(report)) => {
                                    let _ = manager.apply_environment_gate_report(
                                        &prepared,
                                        &report,
                                        now_ms,
                                    );
                                }
                                Ok(Err(GateRequestError::NetworkFingerprintMismatch)) => {
                                    let _ = manager.apply_environment_gate_failure(
                                        &prepared,
                                        AssessmentClassification::SensorUnreliable,
                                        now_ms,
                                    );
                                }
                                Ok(Err(_)) | Err(_) => {
                                    let _ = manager.apply_environment_gate_failure(
                                        &prepared,
                                        AssessmentClassification::UpstreamDegraded,
                                        now_ms,
                                    );
                                }
                            },
                            ActiveGatePurpose::Diagnostic { category } => {
                                let classification = match completion {
                                    Ok(Ok(report)) if report.category == category => {
                                        active_diagnostic_classification(&report)
                                    }
                                    Ok(Ok(_)) | Ok(Err(_)) | Err(_) => {
                                        Some(AssessmentClassification::UpstreamDegraded)
                                    }
                                };
                                update_active_diagnostic(
                                    &mut active_diagnostics,
                                    category,
                                    classification,
                                    now_ms,
                                );
                                next_diagnostic_at =
                                    Instant::now() + ACTIVE_DIAGNOSTIC_REFRESH_INTERVAL;
                            }
                        }
                        publish_projected_snapshot(
                            &publication,
                            manager.snapshot(),
                            &mut active_diagnostics,
                            now_ms,
                        );
                    }
                }
                outcome = tokio::time::timeout(MANAGER_POLL_INTERVAL, manager.recv_next(now_ms)) => {
                    match outcome {
                        Ok(ReceiveOutcome::Event { pending_scope, .. }) => {
                            publish_projected_snapshot(
                                &publication,
                                manager.snapshot(),
                                &mut active_diagnostics,
                                elapsed_ms(started),
                            );
                            acknowledge_manager_publication(&manager, &pending_scope);
                        }
                        Ok(ReceiveOutcome::ReceiverClosed | ReceiveOutcome::ManagerClosed) => {
                            if let Some(task) = gate_task.take() {
                                task.join.abort();
                            }
                            publish_snapshot(&publication, manager.snapshot());
                            break;
                        }
                        Err(_) => {
                            let now_ms = elapsed_ms(started);
                            let snapshot = manager.poll(now_ms);
                            publish_projected_snapshot(
                                &publication,
                                snapshot,
                                &mut active_diagnostics,
                                now_ms,
                            );
                        }
                    }
                }
            }

            if gate_task.is_none() {
                let local = manager_local_network(&manager);
                if let Some(prepared) = manager.take_environment_gate_request(local) {
                    gate_task = Some(spawn_gate_task(Arc::clone(&gate), prepared));
                    publish_projected_snapshot(
                        &publication,
                        manager.snapshot(),
                        &mut active_diagnostics,
                        elapsed_ms(started),
                    );
                } else if Instant::now() >= next_diagnostic_at {
                    let categories = manager
                        .snapshot()
                        .lanes
                        .into_iter()
                        .map(|lane| lane.category)
                        .collect::<Vec<_>>();
                    let diagnostic = next_active_diagnostic(
                        &manager,
                        &categories,
                        &mut diagnostic_cursor,
                        manager_local_network(&manager),
                    );
                    if let Some((category, request)) = diagnostic {
                        gate_task = Some(spawn_diagnostic_gate_task(
                            Arc::clone(&gate),
                            category,
                            request,
                        ));
                        next_diagnostic_at =
                            Instant::now() + ACTIVE_DIAGNOSTIC_REFRESH_INTERVAL;
                    } else {
                        next_diagnostic_at =
                            Instant::now() + ACTIVE_DIAGNOSTIC_RETRY_INTERVAL;
                    }
                }
            }
        }
    });
    drop(ingress);
    let _ = done.send(());
}

fn build_manager_runtime() -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
}

enum ActiveGatePurpose {
    Assessment(PreparedEnvironmentGate),
    Diagnostic { category: String },
}

struct ActiveGateTask {
    purpose: ActiveGatePurpose,
    join: tokio::task::JoinHandle<Result<GateReport, GateRequestError>>,
}

fn spawn_gate_task(
    gate: Arc<tokio::sync::Mutex<EnvironmentGate<ReqwestProbeBackend>>>,
    prepared: PreparedEnvironmentGate,
) -> ActiveGateTask {
    let request = prepared.request.clone();
    spawn_gate_request(gate, ActiveGatePurpose::Assessment(prepared), request)
}

fn spawn_diagnostic_gate_task(
    gate: Arc<tokio::sync::Mutex<EnvironmentGate<ReqwestProbeBackend>>>,
    category: String,
    request: GateRequest,
) -> ActiveGateTask {
    spawn_gate_request(gate, ActiveGatePurpose::Diagnostic { category }, request)
}

fn spawn_gate_request(
    gate: Arc<tokio::sync::Mutex<EnvironmentGate<ReqwestProbeBackend>>>,
    purpose: ActiveGatePurpose,
    mut request: GateRequest,
) -> ActiveGateTask {
    let join = tokio::spawn(async move {
        request.local_network = tokio::task::spawn_blocking(crate::network_identity::snapshot)
            .await
            .unwrap_or(LocalNetworkSnapshot {
                online: false,
                interface_up: false,
                default_route_available: false,
                gateway_reachable: false,
                network_fingerprint: NetworkFingerprint::Unknown,
            });
        gate.lock().await.evaluate(request).await
    });
    ActiveGateTask { purpose, join }
}

async fn wait_gate_task(
    task: &mut Option<ActiveGateTask>,
) -> Option<Result<Result<GateReport, GateRequestError>, tokio::task::JoinError>> {
    Some((&mut task.as_mut()?.join).await)
}

fn manager_local_network(manager: &ObserveOnlyManager) -> LocalNetworkSnapshot {
    let snapshot = manager.snapshot();
    LocalNetworkSnapshot {
        online: true,
        interface_up: true,
        default_route_available: true,
        gateway_reachable: true,
        network_fingerprint: snapshot.session.network_fingerprint_at_start,
    }
}

fn next_active_diagnostic(
    manager: &ObserveOnlyManager,
    categories: &[String],
    cursor: &mut usize,
    local_network: LocalNetworkSnapshot,
) -> Option<(String, GateRequest)> {
    if categories.is_empty() {
        return None;
    }
    for offset in 0..categories.len() {
        let index = cursor.wrapping_add(offset) % categories.len();
        let category = &categories[index];
        if let Some(request) = manager.baseline_gate_request_for(category, local_network.clone()) {
            *cursor = index.wrapping_add(1) % categories.len();
            return Some((category.clone(), request));
        }
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ActiveDiagnostic {
    classification: AssessmentClassification,
    assessed_at_ms: u64,
    expires_at_ms: u64,
}

fn diagnostic_classification(
    classification: GateClassification,
) -> Option<AssessmentClassification> {
    Some(match classification {
        GateClassification::Stable => return None,
        GateClassification::Offline => AssessmentClassification::Offline,
        GateClassification::DnsFailure => AssessmentClassification::DnsFailure,
        GateClassification::UpstreamDegraded => AssessmentClassification::UpstreamDegraded,
        GateClassification::TargetUnavailable => AssessmentClassification::TargetUnavailable,
        GateClassification::ServiceSlow => AssessmentClassification::ServiceSlow,
        // An active request carries no passive Eyes quorum and must never
        // authorize a strategy mutation, even if a backend regression returns
        // an actionable classification.
        GateClassification::DpiSuspected | GateClassification::DpiBlocked => {
            AssessmentClassification::TargetUnavailable
        }
        GateClassification::SensorUnreliable => AssessmentClassification::SensorUnreliable,
    })
}

/// Active diagnostics are presentation-only. If the exact category target
/// returned a valid HTTP response, failures of the neutral connectivity
/// controls must not make that working service look unavailable. Assessment
/// and recovery continue to consume the original fail-closed gate report.
fn active_diagnostic_classification(report: &GateReport) -> Option<AssessmentClassification> {
    let category_is_reachable = report
        .category_targets
        .iter()
        .any(|outcome| outcome.category_target_reachable());
    if category_is_reachable
        && matches!(
            report.classification,
            GateClassification::DnsFailure | GateClassification::UpstreamDegraded
        )
    {
        return None;
    }

    diagnostic_classification(report.classification)
}

fn update_active_diagnostic(
    diagnostics: &mut BTreeMap<String, ActiveDiagnostic>,
    category: String,
    classification: Option<AssessmentClassification>,
    now_ms: u64,
) {
    if let Some(classification) = classification {
        diagnostics.insert(
            category,
            ActiveDiagnostic {
                classification,
                assessed_at_ms: now_ms,
                expires_at_ms: now_ms.saturating_add(ACTIVE_DIAGNOSTIC_TTL_MS),
            },
        );
    } else {
        diagnostics.remove(&category);
    }
}

fn publish_projected_snapshot(
    target: &Mutex<ObserverPublication>,
    value: ObserveOnlySnapshot,
    diagnostics: &mut BTreeMap<String, ActiveDiagnostic>,
    now_ms: u64,
) {
    publish_snapshot(
        target,
        project_active_diagnostics(value, diagnostics, now_ms),
    );
}

fn project_active_diagnostics(
    mut snapshot: ObserveOnlySnapshot,
    diagnostics: &mut BTreeMap<String, ActiveDiagnostic>,
    now_ms: u64,
) -> ObserveOnlySnapshot {
    diagnostics.retain(|_, diagnostic| now_ms <= diagnostic.expires_at_ms);
    for lane in &mut snapshot.lanes {
        let Some(diagnostic) = diagnostics.get(&lane.category) else {
            continue;
        };
        if lane.working_confirmed_recently
            || matches!(
                lane.classification,
                AssessmentClassification::Working
                    | AssessmentClassification::DpiSuspected
                    | AssessmentClassification::DpiBlocked
                    | AssessmentClassification::SensorUnreliable
            )
            || lane.phase == LanePhase::GatePending
        {
            continue;
        }
        lane.phase = if diagnostic.classification == AssessmentClassification::SensorUnreliable {
            LanePhase::SensorUnreliable
        } else {
            LanePhase::Observing
        };
        lane.classification = diagnostic.classification;
        lane.confidence = if diagnostic.classification == AssessmentClassification::SensorUnreliable
        {
            AssessmentConfidence::None
        } else {
            AssessmentConfidence::Medium
        };
        lane.assessed_at_ms = diagnostic.assessed_at_ms;
        lane.cooldown_until_ms = None;
    }

    let options = snapshot
        .lanes
        .iter()
        .map(|lane| {
            (
                lane.category.clone(),
                LaneConfigOptions {
                    active: snapshot.active_configs.get(&lane.category).cloned(),
                    candidates: snapshot
                        .candidate_configs
                        .get(&lane.category)
                        .cloned()
                        .unwrap_or_default(),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    snapshot.presumed_intent = ObserveOnlyBrain::decide_all(&snapshot.lanes, &options, now_ms);
    snapshot
}

fn publish_snapshot(target: &Mutex<ObserverPublication>, value: ObserveOnlySnapshot) {
    let mut target = target
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    target.snapshot = value;
    target.revision = next_nonzero_revision(target.revision);
}

const fn next_nonzero_revision(current: u64) -> u64 {
    let next = current.wrapping_add(1);
    if next == 0 {
        1
    } else {
        next
    }
}

const fn next_nonzero_epoch(current: u64) -> u64 {
    let next = current.wrapping_add(1);
    if next == 0 {
        1
    } else {
        next
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

fn read_text_resource(
    resource: &VerifiedResource,
    per_file_limit: u64,
    total_bytes: &mut u64,
    cache: &mut BTreeMap<String, String>,
) -> Result<String, LegacyObserverError> {
    let key = resource_key(resource.relative_path());
    if let Some(value) = cache.get(&key) {
        return Ok(value.clone());
    }
    resource
        .verify()
        .map_err(|_| LegacyObserverError::InvalidResource)?;
    if resource.size() > per_file_limit {
        return Err(LegacyObserverError::SnapshotTooLarge);
    }
    *total_bytes = total_bytes
        .checked_add(resource.size())
        .filter(|size| *size <= MAX_SNAPSHOT_BYTES)
        .ok_or(LegacyObserverError::SnapshotTooLarge)?;
    let bytes = fs::read(resource.path()).map_err(|_| LegacyObserverError::InvalidResource)?;
    let value = String::from_utf8(bytes).map_err(|_| LegacyObserverError::InvalidUtf8)?;
    cache.insert(key, value.clone());
    Ok(value)
}

fn resource_key(path: &Path) -> String {
    normalized_reference(&path.to_string_lossy())
}

fn normalized_reference(value: &str) -> String {
    value.trim().replace('\\', "/").to_ascii_lowercase()
}

fn category_name(category: DpiCategory) -> &'static str {
    match category {
        DpiCategory::Discord => "discord",
        DpiCategory::YoutubeTwitch => "youtube_twitch",
        DpiCategory::Gaming => "gaming",
        DpiCategory::AtRisk => "atrisk",
        DpiCategory::Universal => "universal",
    }
}

fn protocol_recovery_category(category: &str) -> Option<DpiCategory> {
    match category {
        "discord" => Some(DpiCategory::Discord),
        "youtube_twitch" => Some(DpiCategory::YoutubeTwitch),
        "gaming" => Some(DpiCategory::Gaming),
        "atrisk" | "at_risk" => Some(DpiCategory::AtRisk),
        "universal" => Some(DpiCategory::Universal),
        _ => None,
    }
}

const fn protocol_recovery_origin(origin: RecoveryOrigin) -> Option<ProtocolRecoveryOrigin> {
    match origin {
        RecoveryOrigin::Assisted => Some(ProtocolRecoveryOrigin::Assisted),
        RecoveryOrigin::Automatic { control_generation } if control_generation != 0 => {
            Some(ProtocolRecoveryOrigin::Automatic { control_generation })
        }
        RecoveryOrigin::Automatic { .. } => None,
    }
}

const fn protocol_recovery_attempt_phase(
    phase: CoordinatorRecoveryPhase,
) -> LegacyRecoveryAttemptPhase {
    match phase {
        CoordinatorRecoveryPhase::Preflight => LegacyRecoveryAttemptPhase::Preflight,
        CoordinatorRecoveryPhase::Stopping => LegacyRecoveryAttemptPhase::Stopping,
        CoordinatorRecoveryPhase::Starting => LegacyRecoveryAttemptPhase::Starting,
        CoordinatorRecoveryPhase::Confirming => LegacyRecoveryAttemptPhase::Confirming,
        CoordinatorRecoveryPhase::RollingBack => LegacyRecoveryAttemptPhase::RollingBack,
        CoordinatorRecoveryPhase::Applied => LegacyRecoveryAttemptPhase::Applied,
        CoordinatorRecoveryPhase::ProcessFailed => LegacyRecoveryAttemptPhase::ProcessFailed,
    }
}

fn prepare_recovery_input(
    snapshot: &ObserveOnlySnapshot,
    registry: &TargetRegistry,
) -> Option<LegacyRecoveryInput> {
    if snapshot.session.closed
        || snapshot.health.state != EyeHealthState::Ready
        || snapshot
            .session
            .network_fingerprint_at_start
            .stable_key()
            .is_none()
        || registry.version() != snapshot.session.target_registry_version
    {
        return None;
    }
    let PresumedIntent::SwitchLane {
        category,
        candidate_config,
        ..
    } = &snapshot.presumed_intent
    else {
        return None;
    };
    let lane = snapshot
        .lanes
        .iter()
        .find(|lane| lane.category == *category)?;
    let active_config = snapshot.active_configs.get(category)?;
    if registry.active_config(category)? != active_config {
        return None;
    }
    let active_fingerprint = registry
        .config_fingerprint(category, active_config)
        .ok()?
        .as_hex()
        .to_owned();

    let mut candidate_ids = Vec::new();
    for candidate in std::iter::once(candidate_config).chain(
        snapshot
            .candidate_configs
            .get(category)
            .into_iter()
            .flatten(),
    ) {
        if candidate != active_config && !candidate_ids.contains(candidate) {
            candidate_ids.push(candidate.clone());
        }
    }
    let candidates = candidate_ids
        .into_iter()
        .filter_map(|candidate| {
            let fingerprint = registry
                .config_fingerprint(category, &candidate)
                .ok()?
                .as_hex()
                .to_owned();
            Some(RecoveryConfig::new(candidate, fingerprint))
        })
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return None;
    }

    Some(LegacyRecoveryInput {
        observation: IncidentObservation {
            session_id: snapshot.session.session_id,
            sensor_generation: snapshot.session.sensor_generation,
            category: category.clone(),
            lane_generation: lane.lane_generation,
            evidence_epoch: lane.evidence_epoch,
            classification: lane.classification,
        },
        fence: IntentFence {
            session_id: snapshot.session.session_id,
            category: category.clone(),
            lane_generation: lane.lane_generation,
            sensor_generation: snapshot.session.sensor_generation,
            registry_version: snapshot.session.target_registry_version,
            network_fingerprint: snapshot.session.network_fingerprint_at_start.clone(),
        },
        previous: RecoveryConfig::new(active_config.clone(), active_fingerprint),
        candidates,
    })
}

fn sanitize_snapshot(
    generation: u64,
    revision: u64,
    snapshot: &ObserveOnlySnapshot,
) -> Result<LegacyReliabilityRuntimeSnapshot, LegacyObserverSnapshotError> {
    if generation == 0
        || revision == 0
        || snapshot.session.session_id.get() == 0
        || snapshot.session.sensor_generation.get() == 0
        || snapshot.session.target_registry_version.get() == 0
        || snapshot.session.closed
    {
        return Err(LegacyObserverSnapshotError::InvalidFence);
    }

    let active_categories = snapshot
        .session
        .active_categories
        .iter()
        .map(|category| protocol_category(category))
        .collect::<Result<Vec<_>, _>>()?;
    let lanes = snapshot
        .lanes
        .iter()
        .map(|lane| {
            let project_recent_success = lane.working_confirmed_recently
                && lane.classification == AssessmentClassification::AwaitingEvidence;
            Ok(LegacyLaneRuntimeSnapshot {
                category: protocol_category(&lane.category)?,
                lane_generation: lane.lane_generation.get(),
                phase: if project_recent_success {
                    LegacyLanePhase::Healthy
                } else {
                    protocol_lane_phase(lane.phase)
                },
                classification: if project_recent_success {
                    LegacyAssessmentClassification::Working
                } else {
                    protocol_classification(lane.classification)
                },
                confidence: if project_recent_success {
                    LegacyAssessmentConfidence::High
                } else {
                    protocol_confidence(lane.confidence)
                },
                working_confirmed_recently: false,
                evidence: LegacyEvidenceSnapshot {
                    working_flows: bounded_evidence(lane.evidence.working_flows, 2),
                    working_targets: bounded_evidence(lane.evidence.working_targets, 2),
                    reset_flows: bounded_evidence(lane.evidence.reset_flows, 3),
                    reset_targets: bounded_evidence(lane.evidence.reset_targets, 2),
                    blackhole_flows: bounded_evidence(lane.evidence.blackhole_flows, 2),
                    blackhole_targets: bounded_evidence(lane.evidence.blackhole_targets, 2),
                },
            })
        })
        .collect::<Result<Vec<_>, LegacyObserverSnapshotError>>()?;
    let dropped_events = snapshot
        .gaps
        .producer_dropped_events
        .saturating_add(snapshot.gaps.reported_dropped_events);

    Ok(LegacyReliabilityRuntimeSnapshot {
        generation,
        revision,
        session_id: snapshot.session.session_id.get(),
        sensor_generation: snapshot.session.sensor_generation.get(),
        registry_version: snapshot.session.target_registry_version.get(),
        health: protocol_health(snapshot.health.state),
        active_categories,
        lanes,
        counters: LegacyReliabilityCounters {
            accepted_events: bounded_counter(snapshot.accepted.total_events),
            rejected_events: bounded_counter(snapshot.rejected.total_events),
            dropped_events: bounded_counter(dropped_events),
            packet_count: bounded_counter(snapshot.health.counters.packet_count),
            parse_errors: bounded_counter(snapshot.health.counters.parse_errors),
            queue_drops: bounded_counter(snapshot.health.counters.queue_drops),
        },
        recovery: LegacyRecoveryRuntimeSnapshot::observe_only(),
    })
}

fn protocol_category(category: &str) -> Result<DpiCategory, LegacyObserverSnapshotError> {
    match category {
        "discord" => Ok(DpiCategory::Discord),
        "youtube_twitch" => Ok(DpiCategory::YoutubeTwitch),
        "gaming" => Ok(DpiCategory::Gaming),
        "atrisk" | "at_risk" => Ok(DpiCategory::AtRisk),
        "universal" => Ok(DpiCategory::Universal),
        _ => Err(LegacyObserverSnapshotError::InvalidCategory),
    }
}

const fn protocol_health(state: EyeHealthState) -> LegacyObserverHealth {
    match state {
        EyeHealthState::Ready => LegacyObserverHealth::Ready,
        EyeHealthState::Degraded => LegacyObserverHealth::Degraded,
        EyeHealthState::Blind => LegacyObserverHealth::Blind,
        EyeHealthState::Stopped => LegacyObserverHealth::Stopped,
    }
}

const fn protocol_lane_phase(phase: LanePhase) -> LegacyLanePhase {
    match phase {
        LanePhase::Observing => LegacyLanePhase::Observing,
        LanePhase::Healthy => LegacyLanePhase::Healthy,
        LanePhase::Suspect => LegacyLanePhase::Suspect,
        LanePhase::GatePending => LegacyLanePhase::GatePending,
        LanePhase::BlockedCooldown => LegacyLanePhase::BlockedCooldown,
        LanePhase::SensorUnreliable => LegacyLanePhase::SensorUnreliable,
    }
}

const fn protocol_classification(
    classification: AssessmentClassification,
) -> LegacyAssessmentClassification {
    match classification {
        AssessmentClassification::AwaitingEvidence => {
            LegacyAssessmentClassification::AwaitingEvidence
        }
        AssessmentClassification::Working => LegacyAssessmentClassification::Working,
        AssessmentClassification::Offline => LegacyAssessmentClassification::Offline,
        AssessmentClassification::DnsFailure => LegacyAssessmentClassification::DnsFailure,
        AssessmentClassification::UpstreamDegraded => {
            LegacyAssessmentClassification::UpstreamDegraded
        }
        AssessmentClassification::TargetUnavailable => {
            LegacyAssessmentClassification::TargetUnavailable
        }
        AssessmentClassification::ServiceSlow => LegacyAssessmentClassification::ServiceSlow,
        AssessmentClassification::DpiSuspected => LegacyAssessmentClassification::DpiSuspected,
        AssessmentClassification::DpiBlocked => LegacyAssessmentClassification::DpiBlocked,
        AssessmentClassification::SensorUnreliable => {
            LegacyAssessmentClassification::SensorUnreliable
        }
    }
}

const fn protocol_confidence(confidence: AssessmentConfidence) -> LegacyAssessmentConfidence {
    match confidence {
        AssessmentConfidence::None => LegacyAssessmentConfidence::None,
        AssessmentConfidence::Low => LegacyAssessmentConfidence::Low,
        AssessmentConfidence::Medium => LegacyAssessmentConfidence::Medium,
        AssessmentConfidence::High => LegacyAssessmentConfidence::High,
    }
}

fn bounded_counter(value: u64) -> u64 {
    value.min(MAX_RELIABILITY_COUNTER)
}

fn bounded_evidence(value: u16, maximum: u8) -> u8 {
    value.min(u16::from(maximum)) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use obsession_runtime_reliability::legacy_reliability::contracts::{
        AttemptId, IntentEnvelope, RegistryVersion,
    };
    use obsession_runtime_reliability::legacy_reliability::environment_gate::{
        EndpointProbeOutcome, EndpointProbeStage, GateFence,
    };
    use obsession_runtime_reliability::legacy_reliability::recovery::{
        AssistedApproval, RecoveryOrigin,
    };

    #[test]
    fn manager_runtime_enables_network_io() {
        let runtime = build_manager_runtime().unwrap();
        runtime.block_on(async {
            let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
                .await
                .unwrap();
            assert!(listener.local_addr().unwrap().port() > 0);
        });
    }

    fn observer_snapshot() -> ObserveOnlySnapshot {
        let (ingress, receiver) = channel();
        let manager = ObserveOnlyManager::new(
            LegacySessionContext::new(
                SessionId::new(11),
                vec!["discord".into()],
                NetworkFingerprint::Unknown,
            ),
            SensorGeneration::new(13),
            RegistryVersion::new(17),
            BTreeMap::from([("discord".into(), LaneGeneration::new(19))]),
            ingress.counters(),
            receiver,
        )
        .unwrap();
        manager.snapshot()
    }

    #[test]
    fn active_diagnostic_is_non_actionable_and_respects_passive_state() {
        assert_eq!(
            diagnostic_classification(GateClassification::DpiBlocked),
            Some(AssessmentClassification::TargetUnavailable)
        );
        assert_eq!(diagnostic_classification(GateClassification::Stable), None);

        let mut diagnostics = BTreeMap::new();
        update_active_diagnostic(
            &mut diagnostics,
            "discord".into(),
            Some(AssessmentClassification::TargetUnavailable),
            100,
        );
        let projected = project_active_diagnostics(observer_snapshot(), &mut diagnostics, 101);
        assert_eq!(
            projected.lanes[0].classification,
            AssessmentClassification::TargetUnavailable
        );
        assert!(matches!(
            projected.presumed_intent,
            PresumedIntent::Wait {
                reason: AssessmentClassification::TargetUnavailable
            }
        ));

        let mut working = observer_snapshot();
        working.lanes[0].classification = AssessmentClassification::Working;
        working.lanes[0].phase = LanePhase::Healthy;
        let projected = project_active_diagnostics(working, &mut diagnostics, 102);
        assert_eq!(
            projected.lanes[0].classification,
            AssessmentClassification::Working
        );

        let mut passive_dpi = observer_snapshot();
        passive_dpi.lanes[0].classification = AssessmentClassification::DpiSuspected;
        passive_dpi.lanes[0].phase = LanePhase::Suspect;
        let projected = project_active_diagnostics(passive_dpi, &mut diagnostics, 103);
        assert_eq!(
            projected.lanes[0].classification,
            AssessmentClassification::DpiSuspected
        );
    }

    #[test]
    fn reachable_category_target_suppresses_neutral_probe_failures_for_ux_only() {
        let mut report = GateReport {
            fence: GateFence {
                session_id: SessionId::new(11),
                lane_generation: LaneGeneration::new(19),
                sensor_generation: SensorGeneration::new(13),
                target_registry_version: RegistryVersion::new(17),
                network_fingerprint: NetworkFingerprint::Unknown,
            },
            category: "discord".into(),
            classification: GateClassification::UpstreamDegraded,
            controls: Vec::new(),
            category_targets: vec![EndpointProbeOutcome::http("https://discord.com/", 120, 200)],
            baseline_latency_ms: None,
            slow_threshold_ms: None,
            generated_at_monotonic_ms: 100,
            valid_until_monotonic_ms: 110,
        };

        for classification in [
            GateClassification::DnsFailure,
            GateClassification::UpstreamDegraded,
        ] {
            report.classification = classification;
            assert_eq!(active_diagnostic_classification(&report), None);
        }

        let mut diagnostics = BTreeMap::new();
        update_active_diagnostic(
            &mut diagnostics,
            report.category.clone(),
            active_diagnostic_classification(&report),
            100,
        );
        let projected = project_active_diagnostics(observer_snapshot(), &mut diagnostics, 101);
        assert_eq!(
            projected.lanes[0].classification,
            AssessmentClassification::AwaitingEvidence
        );
        assert!(matches!(
            projected.presumed_intent,
            PresumedIntent::Wait {
                reason: AssessmentClassification::AwaitingEvidence
            }
        ));

        report.category_targets = vec![EndpointProbeOutcome::failed(
            "https://discord.com/",
            EndpointProbeStage::Transport,
            120,
        )];
        assert_eq!(
            active_diagnostic_classification(&report),
            Some(AssessmentClassification::UpstreamDegraded)
        );
    }

    fn recovery_snapshot_and_registry() -> (ObserveOnlySnapshot, TargetRegistry) {
        let records = vec![
            LegacyConfigRecord::new(
                "discord",
                "discord_1.conf",
                "--wf-tcp=443 --hostlist=lists\\discord-1.txt --new\n",
            )
            .with_hostlist("lists\\discord-1.txt", "one.discord.example\n"),
            LegacyConfigRecord::new(
                "discord",
                "discord_2.conf",
                "--wf-tcp=443 --hostlist=lists\\discord-2.txt --new\n",
            )
            .with_hostlist("lists\\discord-2.txt", "two.discord.example\n"),
        ];
        let registry = TargetRegistry::from_records_with_active_selections(
            &records,
            [("discord", "discord_1.conf")],
        )
        .unwrap();
        let mut snapshot = observer_snapshot();
        snapshot.session.network_fingerprint_at_start = NetworkFingerprint::Stable {
            key: "network-fingerprint".into(),
        };
        snapshot.session.target_registry_version = registry.version();
        snapshot.active_configs = BTreeMap::from([("discord".into(), "discord_1.conf".into())]);
        snapshot.candidate_configs = BTreeMap::from([(
            "discord".into(),
            vec!["discord_1.conf".into(), "discord_2.conf".into()],
        )]);
        snapshot.lanes[0].classification = AssessmentClassification::DpiSuspected;
        snapshot.lanes[0].evidence_epoch = 23;
        snapshot.presumed_intent = PresumedIntent::SwitchLane {
            category: "discord".into(),
            candidate_config: "discord_2.conf".into(),
            reason: AssessmentClassification::DpiSuspected,
        };
        (snapshot, registry)
    }

    #[test]
    fn public_projection_keeps_fences_and_drops_backend_only_state() {
        let mut source = observer_snapshot();
        source.lanes[0].working_confirmed_recently = true;
        let projected = sanitize_snapshot(7, 5, &source).unwrap();

        assert_eq!(projected.generation, 7);
        assert_eq!(projected.revision, 5);
        assert_eq!(projected.session_id, 11);
        assert_eq!(projected.sensor_generation, 13);
        assert_eq!(projected.registry_version, 17);
        assert_eq!(projected.active_categories, [DpiCategory::Discord]);
        assert_eq!(projected.lanes.len(), 1);
        assert_eq!(projected.lanes[0].lane_generation, 19);
        assert_eq!(
            projected.lanes[0].classification,
            LegacyAssessmentClassification::Working
        );
        assert_eq!(projected.lanes[0].phase, LegacyLanePhase::Healthy);
        assert_eq!(
            projected.lanes[0].confidence,
            LegacyAssessmentConfidence::High
        );
        assert_eq!(projected.health, LegacyObserverHealth::Ready);
        assert_eq!(
            projected.recovery,
            LegacyRecoveryRuntimeSnapshot::observe_only()
        );
    }

    #[test]
    fn observer_publication_revision_is_nonzero_and_monotonic() {
        let publication = Mutex::new(ObserverPublication {
            revision: 1,
            snapshot: observer_snapshot(),
        });
        publish_snapshot(&publication, observer_snapshot());
        assert_eq!(publication.lock().unwrap().revision, 2);

        publication.lock().unwrap().revision = u64::MAX;
        publish_snapshot(&publication, observer_snapshot());
        assert_eq!(publication.lock().unwrap().revision, 1);
    }

    #[test]
    fn recovery_input_is_exact_private_and_candidate_ordered() {
        let (snapshot, registry) = recovery_snapshot_and_registry();
        let input = prepare_recovery_input(&snapshot, &registry).unwrap();

        assert_eq!(input.fence.session_id, SessionId::new(11));
        assert_eq!(input.fence.sensor_generation, SensorGeneration::new(13));
        assert_eq!(input.fence.lane_generation, LaneGeneration::new(19));
        assert_eq!(input.fence.registry_version, registry.version());
        assert_eq!(input.previous.config_id(), "discord_1.conf");
        assert_eq!(input.candidates.len(), 1);
        assert_eq!(input.candidates[0].config_id(), "discord_2.conf");
        assert_ne!(
            input.previous.fingerprint(),
            input.candidates[0].fingerprint()
        );
    }

    #[test]
    fn service_recovery_runtime_deduplicates_and_never_arms_mutation() {
        let (snapshot, registry) = recovery_snapshot_and_registry();
        let input = prepare_recovery_input(&snapshot, &registry).unwrap();
        let owner = ProcessOwner {
            pid: 41,
            process_start_identity:
                obsession_runtime_reliability::legacy_reliability::contracts::ProcessStartIdentity::new(43),
            config_fingerprint: input.previous.fingerprint().clone(),
            lane_generation: input.fence.lane_generation,
        };
        let mut runtime = LegacyRecoveryRuntime::new();
        let scan = LegacyRecoveryScan {
            generation: 7,
            revision: 1,
            session_id: snapshot.session.session_id,
            input: Some(input.clone()),
        };

        runtime
            .poll_at(scan.clone(), Some(owner.clone()), 10)
            .unwrap();
        let first = runtime.suggestion().unwrap().clone();
        assert_eq!(first.previous_config_id, "discord_1.conf");
        assert_eq!(first.candidate_config_id, "discord_2.conf");

        let mut repeated = scan.clone();
        repeated.revision = 2;
        runtime.poll_at(repeated, Some(owner), 20).unwrap();
        assert_eq!(runtime.suggestion().unwrap().incident_id, first.incident_id);
        assert_eq!(
            runtime.public_snapshot(true),
            LegacyRecoveryRuntimeSnapshot {
                phase: LegacyRecoveryPhase::Idle,
                disposition: LegacyRecoveryDisposition::ObserveOnly,
                controls: LegacyRecoveryControlsSnapshot::observe_only(),
                proposal: None,
                active_attempt: None,
                last_completion: None,
                automatic_pacing_remaining_ms: None,
                halted_categories: Vec::new(),
                negative_cooldown_count: 0,
            }
        );

        let mut cleared = scan;
        cleared.revision = 3;
        cleared.input = None;
        runtime.poll_at(cleared, None, 30).unwrap();
        assert!(runtime.suggestion().is_none());
        assert!(runtime.coordinator.status().active_attempt.is_none());
        assert!(runtime.coordinator.status().proposal.is_none());
    }

    #[test]
    fn service_controls_are_atomic_monotonic_and_drive_the_coordinator() {
        let mut runtime = LegacyRecoveryRuntime::new();
        assert_eq!(
            runtime.public_snapshot(false).controls,
            LegacyRecoveryControlsSnapshot::observe_only()
        );

        let generation = runtime
            .set_controls(LegacyRecoveryControlsRequest {
                mode: ProtocolRecoveryMode::Automatic,
                automatic_paused: false,
                frozen_categories: vec![DpiCategory::Gaming, DpiCategory::Discord],
            })
            .unwrap();
        assert_eq!(generation, 2);
        let controls = runtime.public_snapshot(true).controls;
        assert_eq!(controls.control_generation, generation);
        assert_eq!(controls.mode, ProtocolRecoveryMode::Automatic);
        assert!(!controls.automatic_paused);
        assert_eq!(
            controls.frozen_categories,
            [DpiCategory::Discord, DpiCategory::Gaming]
        );
        assert_eq!(runtime.coordinator.status().mode, RecoveryMode::Automatic);
        assert!(!runtime.coordinator.status().automatic_paused);
        assert_eq!(
            runtime.coordinator.status().manual_frozen_categories,
            ["discord", "gaming"]
        );

        assert_eq!(
            runtime
                .set_controls(LegacyRecoveryControlsRequest {
                    mode: ProtocolRecoveryMode::Automatic,
                    automatic_paused: false,
                    frozen_categories: vec![DpiCategory::Discord, DpiCategory::Gaming],
                })
                .unwrap(),
            generation,
            "an idempotent write must not advance the authorization fence"
        );
        assert!(runtime
            .set_controls(LegacyRecoveryControlsRequest {
                mode: ProtocolRecoveryMode::Assisted,
                automatic_paused: false,
                frozen_categories: Vec::new(),
            })
            .is_err());
    }

    #[test]
    fn assisted_proposal_is_service_owned_and_only_exact_tokens_can_arm_it() {
        let (snapshot, registry) = recovery_snapshot_and_registry();
        let input = prepare_recovery_input(&snapshot, &registry).unwrap();
        let owner = ProcessOwner {
            pid: 41,
            process_start_identity:
                obsession_runtime_reliability::legacy_reliability::contracts::ProcessStartIdentity::new(43),
            config_fingerprint: input.previous.fingerprint().clone(),
            lane_generation: input.fence.lane_generation,
        };
        let mut runtime = LegacyRecoveryRuntime::new();
        runtime
            .set_controls(LegacyRecoveryControlsRequest {
                mode: ProtocolRecoveryMode::Assisted,
                automatic_paused: true,
                frozen_categories: Vec::new(),
            })
            .unwrap();
        let scan = LegacyRecoveryScan {
            generation: 7,
            revision: 1,
            session_id: snapshot.session.session_id,
            input: Some(input.clone()),
        };

        assert!(runtime.poll_at(scan, Some(owner), 10).unwrap().is_none());
        let proposal = runtime.public_snapshot(true).proposal.unwrap();
        assert_eq!(proposal.category, DpiCategory::Discord);
        assert_eq!(proposal.previous_config_id, "discord_1.conf");
        assert_eq!(proposal.candidate_config_id, "discord_2.conf");

        let foreign = AssistedApproval::new(proposal.proposal_id + 1, proposal.attempt_id).unwrap();
        assert_eq!(
            runtime.approve_at(foreign, &input.fence, 20),
            Err(ApprovalError::ProposalMismatch)
        );
        let exact = AssistedApproval::new(proposal.proposal_id, proposal.attempt_id).unwrap();
        let action = runtime.approve_at(exact, &input.fence, 20).unwrap();
        assert!(matches!(
            action,
            RecoveryAction::Preflight {
                origin: RecoveryOrigin::Assisted,
                ..
            }
        ));
        let status = runtime.public_snapshot(true);
        assert!(status.proposal.is_none());
        assert!(status.active_attempt.is_some());
        assert_eq!(status.phase, LegacyRecoveryPhase::Evaluating);
        assert_eq!(status.disposition, LegacyRecoveryDisposition::Pending);
    }

    #[test]
    fn automatic_action_carries_the_exact_service_control_generation() {
        let (snapshot, registry) = recovery_snapshot_and_registry();
        let input = prepare_recovery_input(&snapshot, &registry).unwrap();
        let owner = ProcessOwner {
            pid: 41,
            process_start_identity:
                obsession_runtime_reliability::legacy_reliability::contracts::ProcessStartIdentity::new(43),
            config_fingerprint: input.previous.fingerprint().clone(),
            lane_generation: input.fence.lane_generation,
        };
        let mut runtime = LegacyRecoveryRuntime::new();
        let control_generation = runtime
            .set_controls(LegacyRecoveryControlsRequest {
                mode: ProtocolRecoveryMode::Automatic,
                automatic_paused: false,
                frozen_categories: Vec::new(),
            })
            .unwrap();
        let action = runtime
            .poll_at(
                LegacyRecoveryScan {
                    generation: 7,
                    revision: 1,
                    session_id: snapshot.session.session_id,
                    input: Some(input),
                },
                Some(owner),
                10,
            )
            .unwrap()
            .unwrap();
        assert!(matches!(
            action,
            RecoveryAction::Preflight {
                origin: RecoveryOrigin::Automatic {
                    control_generation: generation
                },
                ..
            } if generation == control_generation
        ));
        assert!(matches!(
            runtime.public_snapshot(true).active_attempt.unwrap().origin,
            ProtocolRecoveryOrigin::Automatic {
                control_generation: generation
            } if generation == control_generation
        ));
        let paused_generation = runtime
            .set_controls(LegacyRecoveryControlsRequest {
                mode: ProtocolRecoveryMode::Automatic,
                automatic_paused: true,
                frozen_categories: vec![DpiCategory::Discord],
            })
            .unwrap();
        assert_ne!(paused_generation, control_generation);
        let paused = runtime.public_snapshot(true);
        assert!(paused.active_attempt.is_some());
        assert!(paused.controls.automatic_paused);
        assert_eq!(paused.controls.frozen_categories, [DpiCategory::Discord]);
        assert!(matches!(
            runtime.force_manual_failure(20),
            Some(RecoveryAction::ManualIntervention { .. })
        ));
        let terminal = runtime.public_snapshot(true);
        assert!(terminal.active_attempt.is_none());
        assert_eq!(terminal.phase, LegacyRecoveryPhase::ManualIntervention);
        assert_eq!(terminal.disposition, LegacyRecoveryDisposition::Halted);
        assert_eq!(
            terminal.last_completion.unwrap().disposition,
            LegacyRecoveryCompletionDisposition::ProcessFailed
        );
    }

    #[test]
    fn retry_preflight_rebinds_only_the_exact_retained_incident() {
        let (snapshot, registry) = recovery_snapshot_and_registry();
        let input = prepare_recovery_input(&snapshot, &registry).unwrap();
        let owner = ProcessOwner {
            pid: 41,
            process_start_identity:
                obsession_runtime_reliability::legacy_reliability::contracts::ProcessStartIdentity::new(43),
            config_fingerprint: input.previous.fingerprint().clone(),
            lane_generation: input.fence.lane_generation,
        };
        let mut runtime = LegacyRecoveryRuntime::new();
        runtime.last_input = Some(input.clone());

        let retry_fence = IntentFence {
            session_id: input.fence.session_id,
            category: input.fence.category.clone(),
            lane_generation: LaneGeneration::new(29),
            sensor_generation: SensorGeneration::new(31),
            registry_version: RegistryVersion::new(37),
            network_fingerprint: input.fence.network_fingerprint.clone(),
        };
        let make_action =
            |fence: &IntentFence, previous: RecoveryConfig, candidate: RecoveryConfig| {
                RecoveryAction::Preflight {
                    envelope: IntentEnvelope::from_fence(AttemptId::new(47), fence),
                    previous,
                    previous_owner: owner.clone(),
                    candidate,
                    origin: RecoveryOrigin::Automatic {
                        control_generation: 53,
                    },
                }
            };
        let action = make_action(
            &retry_fence,
            input.previous.clone(),
            input.candidates[0].clone(),
        );
        let live_scan = LegacyRecoveryScan {
            generation: 59,
            revision: 61,
            session_id: input.fence.session_id,
            input: None,
        };

        let rebound = runtime
            .preflight_scan_for_action(&live_scan, &action)
            .expect("the exact retained retry must be rebound");
        let rebound_input = rebound.input.unwrap();
        assert_eq!(rebound.generation, live_scan.generation);
        assert_eq!(rebound.revision, live_scan.revision);
        assert_eq!(rebound_input.fence, retry_fence);
        assert_eq!(rebound_input.observation.session_id, retry_fence.session_id);
        assert_eq!(
            rebound_input.observation.sensor_generation,
            retry_fence.sensor_generation
        );
        assert_eq!(rebound_input.observation.category, retry_fence.category);
        assert_eq!(
            rebound_input.observation.lane_generation,
            retry_fence.lane_generation
        );

        let foreign_candidate = make_action(
            &retry_fence,
            input.previous.clone(),
            RecoveryConfig::new("discord_3.conf", "foreign-candidate"),
        );
        assert!(runtime
            .preflight_scan_for_action(&live_scan, &foreign_candidate)
            .is_none());

        let previous_drift = make_action(
            &retry_fence,
            RecoveryConfig::new("discord_9.conf", "foreign-previous"),
            input.candidates[0].clone(),
        );
        assert!(runtime
            .preflight_scan_for_action(&live_scan, &previous_drift)
            .is_none());

        for drifted_fence in [
            IntentFence {
                session_id: SessionId::new(67),
                ..retry_fence.clone()
            },
            IntentFence {
                category: "youtube_twitch".into(),
                ..retry_fence.clone()
            },
            IntentFence {
                network_fingerprint: NetworkFingerprint::Stable {
                    key: "different-network".into(),
                },
                ..retry_fence.clone()
            },
        ] {
            let drifted = make_action(
                &drifted_fence,
                input.previous.clone(),
                input.candidates[0].clone(),
            );
            assert!(runtime
                .preflight_scan_for_action(&live_scan, &drifted)
                .is_none());
        }

        let stale_session_scan = LegacyRecoveryScan {
            session_id: SessionId::new(71),
            ..live_scan
        };
        assert!(runtime
            .preflight_scan_for_action(&stale_session_scan, &action)
            .is_none());
    }
}
