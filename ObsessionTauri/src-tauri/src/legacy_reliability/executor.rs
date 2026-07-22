//! Fenced, category-scoped Legacy recovery executor.
//!
//! The coordinator in [`super::recovery`] decides *what* may happen. This
//! module owns the stricter execution boundary: immutable registry preflight,
//! a fresh Environment Gate, exact process/fence checks before every effect,
//! candidate confirmation, and exact rollback. Platform effects are injected
//! through [`ScopedExecutorBackend`], keeping the policy and adverse paths
//! deterministic in unit tests.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Manager};

use crate::eyes::Diagnosis;
use crate::state::{AppState, LegacyCategoryRuntimeSnapshot, LegacyProcessOwner};
use crate::util::LockExt;

use super::contracts::{
    AttemptId, ConfirmationFailure, EventEnvelope, ExecutorOutcome, ExecutorResult, ExecutorStage,
    EyeEvent, EyeHealthCounters, EyeHealthState, FlowEvent, IntentEnvelope, IntentFence,
    LaneGeneration, NetworkFingerprint, ProcessOwner, SessionId, Transport,
};
use super::environment_gate::{
    EndpointProbeBackend, EnvironmentGate, GateClassification, GateFence, GateReport, GateRequest,
    GateRequestError, LocalNetworkSnapshot, PassiveEvidenceSummary, SensorSnapshot,
    GATE_REPORT_TTL,
};
use super::manager::{ConfirmationFlow, ObserveOnlySnapshot};
use super::recovery::{RecoveryAction, RecoveryConfig, RecoveryOrigin};
use super::target_registry::{ConfigLookupError, TargetRegistry};

pub const CONFIRMATION_DEADLINE_MS: u64 = 20_000;
pub const CONFIRMATION_CLEAN_WINDOW_MS: u64 = 5_000;
pub const CONFIRMATION_DELIVERY_MARGIN_MS: u64 = 2_000;

type BackendFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Clone, Debug, PartialEq, Eq)]
struct SelectionIdentity {
    config_name: String,
    fingerprint: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RegistryImage {
    content_hash: [u8; 32],
    selections: BTreeMap<String, SelectionIdentity>,
}

impl RegistryImage {
    fn capture(
        registry: &TargetRegistry,
        selections: impl IntoIterator<Item = (String, String)>,
    ) -> Result<Self, RegistryPreflightError> {
        let mut identities = BTreeMap::new();
        for (category, config_name) in selections {
            let fingerprint = registry
                .config_fingerprint(&category, &config_name)
                .map_err(RegistryPreflightError::Lookup)?;
            if identities
                .insert(
                    category.clone(),
                    SelectionIdentity {
                        config_name,
                        fingerprint: fingerprint.as_hex().to_owned(),
                    },
                )
                .is_some()
            {
                return Err(RegistryPreflightError::DuplicateCategory(category));
            }
        }
        Ok(Self {
            content_hash: *registry.content_hash(),
            selections: identities,
        })
    }

    fn selection_pairs(&self) -> Vec<(String, String)> {
        self.selections
            .iter()
            .map(|(category, identity)| (category.clone(), identity.config_name.clone()))
            .collect()
    }

    fn verify_active_registry(
        &self,
        registry: &TargetRegistry,
        require_complete_content_hash: bool,
    ) -> Result<(), RegistryPreflightError> {
        if require_complete_content_hash && registry.content_hash() != &self.content_hash {
            return Err(RegistryPreflightError::ContentChanged);
        }
        let active = registry
            .active_selections()
            .map(|(category, config_name)| (category.to_owned(), config_name.to_owned()))
            .collect::<Vec<_>>();
        let current = Self::capture(registry, active)?;
        if current.selections != self.selections
            || (require_complete_content_hash && current.content_hash != self.content_hash)
        {
            return Err(RegistryPreflightError::SelectionChanged);
        }
        Ok(())
    }
}

/// Immutable original/candidate registry fence retained for one attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScopedRegistryPlan {
    category: String,
    previous: RegistryImage,
    candidate: RegistryImage,
    previous_config_id: String,
    candidate_config_id: String,
    candidate_targets: Vec<String>,
}

impl ScopedRegistryPlan {
    pub fn prepare(
        registry: &TargetRegistry,
        category: &str,
        previous: &RecoveryConfig,
        candidate: &RecoveryConfig,
    ) -> Result<Self, RegistryPreflightError> {
        let active_config = registry
            .active_config(category)
            .ok_or_else(|| RegistryPreflightError::MissingActiveCategory(category.to_owned()))?;
        if !active_config.eq_ignore_ascii_case(previous.config_id()) {
            return Err(RegistryPreflightError::PreviousConfigChanged {
                expected: previous.config_id().to_owned(),
                actual: active_config.to_owned(),
            });
        }

        let previous_registry_fingerprint = registry
            .config_fingerprint(category, active_config)
            .map_err(RegistryPreflightError::Lookup)?;
        if previous_registry_fingerprint.as_hex() != previous.fingerprint().as_str() {
            return Err(RegistryPreflightError::PreviousFingerprintChanged);
        }

        let tentative = registry
            .tentative_selection(category, candidate.config_id())
            .map_err(RegistryPreflightError::Lookup)?;
        if tentative.candidate_fingerprint().as_hex() != candidate.fingerprint().as_str() {
            return Err(RegistryPreflightError::CandidateFingerprintChanged);
        }

        let previous_selections = registry
            .active_selections()
            .map(|(category, config_name)| (category.to_owned(), config_name.to_owned()))
            .collect::<Vec<_>>();
        let candidate_selections = tentative
            .selections()
            .map(|(category, config_name)| (category.to_owned(), config_name.to_owned()))
            .collect::<Vec<_>>();
        let previous_image = RegistryImage::capture(registry, previous_selections)?;
        let candidate_image = RegistryImage::capture(registry, candidate_selections)?;

        let mut candidate_targets = tentative
            .candidate_target_suffixes()
            .into_iter()
            .filter_map(normalize_domain)
            .collect::<Vec<_>>();
        candidate_targets.sort();
        candidate_targets.dedup();
        if candidate_targets.is_empty() {
            return Err(RegistryPreflightError::NoExclusiveCandidateTargets);
        }

        Ok(Self {
            category: tentative.category().to_owned(),
            previous: previous_image,
            candidate: candidate_image,
            previous_config_id: active_config.to_owned(),
            candidate_config_id: tentative.config_name().to_owned(),
            candidate_targets,
        })
    }

    pub fn category(&self) -> &str {
        &self.category
    }

    pub fn previous_config_id(&self) -> &str {
        &self.previous_config_id
    }

    pub fn candidate_config_id(&self) -> &str {
        &self.candidate_config_id
    }

    pub fn candidate_targets(&self) -> &[String] {
        &self.candidate_targets
    }

    pub fn previous_selections(&self) -> Vec<(String, String)> {
        self.previous.selection_pairs()
    }

    pub fn candidate_selections(&self) -> Vec<(String, String)> {
        self.candidate.selection_pairs()
    }

    pub fn verify_previous_registry(
        &self,
        registry: &TargetRegistry,
    ) -> Result<(), RegistryPreflightError> {
        // A candidate file can change while it is being tested. Exact rollback
        // is still allowed when the previous config and all active neighbors
        // retain their own fingerprints; a changed unused candidate must not
        // strand the lane in ProcessFailed.
        self.previous.verify_active_registry(registry, false)
    }

    pub fn verify_candidate_registry(
        &self,
        registry: &TargetRegistry,
    ) -> Result<(), RegistryPreflightError> {
        self.candidate.verify_active_registry(registry, true)?;
        let tentative = registry
            .tentative_selection(&self.category, &self.candidate_config_id)
            .map_err(RegistryPreflightError::Lookup)?;
        let mut targets = tentative
            .candidate_target_suffixes()
            .into_iter()
            .filter_map(normalize_domain)
            .collect::<Vec<_>>();
        targets.sort();
        targets.dedup();
        if targets != self.candidate_targets {
            return Err(RegistryPreflightError::CandidateTargetsChanged);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegistryPreflightError {
    Lookup(ConfigLookupError),
    MissingActiveCategory(String),
    DuplicateCategory(String),
    PreviousConfigChanged { expected: String, actual: String },
    PreviousFingerprintChanged,
    CandidateFingerprintChanged,
    ContentChanged,
    SelectionChanged,
    CandidateTargetsChanged,
    NoExclusiveCandidateTargets,
}

impl fmt::Display for RegistryPreflightError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lookup(error) => write!(formatter, "Legacy config lookup failed: {error}"),
            Self::MissingActiveCategory(category) => {
                write!(formatter, "Legacy category {category:?} is not active")
            }
            Self::DuplicateCategory(category) => {
                write!(formatter, "Legacy category {category:?} is selected twice")
            }
            Self::PreviousConfigChanged { expected, actual } => write!(
                formatter,
                "previous Legacy config changed from {expected:?} to {actual:?}"
            ),
            Self::PreviousFingerprintChanged => {
                formatter.write_str("previous Legacy config fingerprint changed")
            }
            Self::CandidateFingerprintChanged => {
                formatter.write_str("candidate Legacy config fingerprint changed")
            }
            Self::ContentChanged => formatter.write_str("Legacy registry content hash changed"),
            Self::SelectionChanged => {
                formatter.write_str("Legacy candidate or neighboring selection changed")
            }
            Self::CandidateTargetsChanged => {
                formatter.write_str("Legacy candidate target whitelist changed")
            }
            Self::NoExclusiveCandidateTargets => {
                formatter.write_str("Legacy candidate has no unambiguous confirmation targets")
            }
        }
    }
}

impl Error for RegistryPreflightError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreflightGateError {
    InvalidRequest,
    StaleFence,
    StaleReport,
    Environment,
    Target,
    Sensor,
    IncidentNotActionable,
}

impl PreflightGateError {
    pub const fn failure_reason(self) -> ConfirmationFailure {
        match self {
            Self::Target => ConfirmationFailure::Target,
            Self::Sensor | Self::StaleFence | Self::StaleReport => ConfirmationFailure::Sensor,
            Self::InvalidRequest | Self::Environment | Self::IncidentNotActionable => {
                ConfirmationFailure::Environment
            }
        }
    }
}

/// Evaluates the shared process-local gate and rejects any report that is not
/// fresh for the exact pre-observer fence. The caller still re-reads the live
/// fence immediately before restarting Eyes/Manager.
pub async fn evaluate_fresh_environment_gate<B>(
    gate: &mut EnvironmentGate<B>,
    request: GateRequest,
    expected: &IntentEnvelope,
) -> Result<GateReport, PreflightGateError>
where
    B: EndpointProbeBackend + 'static,
{
    let expected_gate = gate_fence(expected);
    if request.fence != expected_gate {
        return Err(PreflightGateError::StaleFence);
    }
    let report = gate
        .evaluate(request)
        .await
        .map_err(|_: GateRequestError| PreflightGateError::InvalidRequest)?;
    authorize_gate_report(&report, &expected_gate, report.generated_at_monotonic_ms)?;
    Ok(report)
}

pub fn authorize_gate_report(
    report: &GateReport,
    expected: &GateFence,
    now_ms: u64,
) -> Result<(), PreflightGateError> {
    if !report.is_fresh(now_ms, expected) {
        return Err(PreflightGateError::StaleReport);
    }
    match report.classification {
        GateClassification::DpiSuspected | GateClassification::DpiBlocked => Ok(()),
        GateClassification::Stable => Err(PreflightGateError::IncidentNotActionable),
        GateClassification::Offline
        | GateClassification::DnsFailure
        | GateClassification::UpstreamDegraded => Err(PreflightGateError::Environment),
        GateClassification::TargetUnavailable | GateClassification::ServiceSlow => {
            Err(PreflightGateError::Target)
        }
        GateClassification::SensorUnreliable => Err(PreflightGateError::Sensor),
    }
}

fn authorize_current_passive_quorum(
    report: &GateReport,
    expected_evidence_epoch: u64,
    current_evidence_epoch: u64,
    current_evidence: PassiveEvidenceSummary,
) -> Result<(), PreflightGateError> {
    if current_evidence_epoch != expected_evidence_epoch {
        return Err(PreflightGateError::StaleFence);
    }
    match report.classification {
        GateClassification::DpiSuspected
            if current_evidence.has_reset_quorum()
                || current_evidence.has_blackhole_gate_quorum() =>
        {
            Ok(())
        }
        GateClassification::DpiBlocked if current_evidence.has_blackhole_quorum() => Ok(()),
        GateClassification::DpiSuspected | GateClassification::DpiBlocked => {
            Err(PreflightGateError::IncidentNotActionable)
        }
        _ => Err(PreflightGateError::IncidentNotActionable),
    }
}

fn lane_passive_evidence(lane: &super::assessment::LaneAssessment) -> PassiveEvidenceSummary {
    PassiveEvidenceSummary {
        reset_after_client_hello_flows: u32::from(lane.evidence.reset_flows),
        reset_targets: u32::from(lane.evidence.reset_targets),
        confirmed_tls_blackhole_flows: u32::from(lane.evidence.blackhole_flows),
        blackhole_targets: u32::from(lane.evidence.blackhole_targets),
    }
}

fn gate_fence(envelope: &IntentEnvelope) -> GateFence {
    GateFence {
        session_id: envelope.session_id,
        lane_generation: envelope.expected_lane_generation,
        sensor_generation: envelope.expected_sensor_generation,
        target_registry_version: envelope.expected_registry_version,
        network_fingerprint: envelope.expected_network_fingerprint.clone(),
    }
}

fn normalize_domain(value: &str) -> Option<String> {
    let normalized = value.trim().trim_end_matches('.').to_ascii_lowercase();
    (!normalized.is_empty()).then_some(normalized)
}

fn domain_matches_suffix(domain: &str, suffix: &str) -> bool {
    domain == suffix
        || domain
            .strip_suffix(suffix)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpsProbeObservation {
    pub probe_id: u64,
    pub domain: String,
    pub started_at_monotonic_ms: u64,
    pub finished_at_monotonic_ms: u64,
    pub result: HttpsProbeResult,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpsProbeResult {
    /// Any response below 500 proves that the TLS request reached its target;
    /// authentication and rate-limit responses remain valid reachability.
    HttpResponse {
        status: u16,
    },
    EnvironmentFailure,
    TargetFailure,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmationDecision {
    Pending,
    Succeeded,
    Failed(ConfirmationFailure),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SuccessfulProbe {
    probe_id: u64,
    target: String,
    started_at_ms: u64,
    finished_at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct WorkingFlow {
    flow_id: u64,
    target: String,
    observed_at_ms: u64,
}

/// Cursor captured immediately after candidate readiness. The confirmation
/// backend accepts only journal entries after this point, so Working produced
/// by the still-running previous process during observer preflight cannot
/// confirm the candidate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConfirmationArm {
    pub armed_at_monotonic_ms: u64,
    pub armed_at_sensor_ms: u64,
    pub after_flow_sequence: u64,
    pub last_gap_sequence: Option<u64>,
    pub evidence_epoch: u64,
    pub health: EyeHealthState,
    pub counters: EyeHealthCounters,
}

impl ConfirmationArm {
    pub fn from_snapshot(
        snapshot: &ObserveOnlySnapshot,
        envelope: &IntentEnvelope,
        armed_at_monotonic_ms: u64,
    ) -> Result<Self, ConfirmationArmError> {
        validate_snapshot_fence(snapshot, envelope)?;
        let lane = snapshot
            .lanes
            .iter()
            .find(|lane| lane.category == envelope.category)
            .ok_or(ConfirmationArmError::MissingLane)?;
        if lane.lane_generation != envelope.expected_lane_generation {
            return Err(ConfirmationArmError::FenceChanged);
        }
        if snapshot.health.state != EyeHealthState::Ready {
            return Err(ConfirmationArmError::SensorUnreliable);
        }
        Ok(Self {
            armed_at_monotonic_ms,
            armed_at_sensor_ms: snapshot.logical_now_ms,
            after_flow_sequence: snapshot.last_accepted_flow_sequence.unwrap_or(0),
            last_gap_sequence: snapshot.last_gap_sequence,
            evidence_epoch: lane.evidence_epoch,
            health: snapshot.health.state,
            counters: snapshot.health.counters,
        })
    }

    pub fn validate_snapshot(
        self,
        snapshot: &ObserveOnlySnapshot,
        envelope: &IntentEnvelope,
    ) -> Result<(), ConfirmationArmError> {
        validate_snapshot_fence(snapshot, envelope)?;
        let lane = snapshot
            .lanes
            .iter()
            .find(|lane| lane.category == envelope.category)
            .ok_or(ConfirmationArmError::MissingLane)?;
        if lane.lane_generation != envelope.expected_lane_generation {
            return Err(ConfirmationArmError::FenceChanged);
        }
        if snapshot.health.state != EyeHealthState::Ready
            || snapshot.health.counters.parse_errors > self.counters.parse_errors
            || snapshot.health.counters.queue_drops > self.counters.queue_drops
            || snapshot.last_gap_sequence != self.last_gap_sequence
            || lane.evidence_epoch != self.evidence_epoch
        {
            return Err(ConfirmationArmError::SensorUnreliable);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmationArmError {
    FenceChanged,
    MissingLane,
    SensorUnreliable,
}

fn validate_snapshot_fence(
    snapshot: &ObserveOnlySnapshot,
    envelope: &IntentEnvelope,
) -> Result<(), ConfirmationArmError> {
    if snapshot.session.closed
        || snapshot.session.session_id != envelope.session_id
        || snapshot.session.sensor_generation != envelope.expected_sensor_generation
        || snapshot.session.target_registry_version != envelope.expected_registry_version
    {
        Err(ConfirmationArmError::FenceChanged)
    } else {
        Ok(())
    }
}

/// Pure evaluator for the candidate's bounded 20-second confirmation window.
///
/// A successful HTTPS response is insufficient by itself: the same registry
/// target must also produce exact-generation `Working` evidence. Two registry
/// targets are required when available. A candidate with one eligible target
/// instead needs two distinct probes and two distinct Working flows.
#[derive(Clone, Debug)]
pub struct ConfirmationEvaluator {
    envelope: EventEnvelope,
    category: String,
    lane_generation: LaneGeneration,
    targets: Vec<String>,
    started_at_ms: u64,
    started_at_sensor_ms: u64,
    deadline_at_ms: u64,
    initial_counters: EyeHealthCounters,
    health_state: EyeHealthState,
    after_flow_sequence: u64,
    successful_probes: Vec<SuccessfulProbe>,
    working_flows: Vec<WorkingFlow>,
    quorum_reached_at_ms: Option<u64>,
    failure: Option<ConfirmationFailure>,
}

impl ConfirmationEvaluator {
    pub fn new(
        envelope: &IntentEnvelope,
        candidate_targets: impl IntoIterator<Item = impl AsRef<str>>,
        started_at_ms: u64,
        initial_health: EyeHealthState,
        initial_counters: EyeHealthCounters,
    ) -> Result<Self, ConfirmationBuildError> {
        Self::new_armed(
            envelope,
            candidate_targets,
            started_at_ms,
            ConfirmationArm {
                armed_at_monotonic_ms: started_at_ms,
                armed_at_sensor_ms: started_at_ms,
                after_flow_sequence: 0,
                last_gap_sequence: None,
                evidence_epoch: 0,
                health: initial_health,
                counters: initial_counters,
            },
        )
    }

    pub fn new_armed(
        envelope: &IntentEnvelope,
        candidate_targets: impl IntoIterator<Item = impl AsRef<str>>,
        _started_at_ms: u64,
        arm: ConfirmationArm,
    ) -> Result<Self, ConfirmationBuildError> {
        let mut targets = candidate_targets
            .into_iter()
            .filter_map(|target| normalize_domain(target.as_ref()))
            .collect::<Vec<_>>();
        targets.sort_by(|left, right| right.len().cmp(&left.len()).then_with(|| left.cmp(right)));
        targets.dedup();
        if targets.is_empty() {
            return Err(ConfirmationBuildError::NoTargets);
        }

        let started_at_ms = arm.armed_at_monotonic_ms;
        Ok(Self {
            envelope: EventEnvelope::new(
                envelope.session_id,
                envelope.expected_sensor_generation,
                envelope.expected_registry_version,
            ),
            category: envelope.category.clone(),
            lane_generation: envelope.expected_lane_generation,
            targets,
            started_at_ms,
            started_at_sensor_ms: arm.armed_at_sensor_ms,
            deadline_at_ms: started_at_ms.saturating_add(CONFIRMATION_DEADLINE_MS),
            initial_counters: arm.counters,
            health_state: arm.health,
            after_flow_sequence: arm.after_flow_sequence,
            successful_probes: Vec::new(),
            working_flows: Vec::new(),
            quorum_reached_at_ms: None,
            failure: (!matches!(arm.health, EyeHealthState::Ready))
                .then_some(ConfirmationFailure::Sensor),
        })
    }

    pub const fn deadline_at_monotonic_ms(&self) -> u64 {
        self.deadline_at_ms
    }

    pub fn observe_probe(&mut self, probe: HttpsProbeObservation) -> ConfirmationDecision {
        if self.failure.is_some()
            || probe.started_at_monotonic_ms < self.started_at_ms
            || probe.finished_at_monotonic_ms < probe.started_at_monotonic_ms
            || probe.finished_at_monotonic_ms > self.deadline_at_ms
        {
            return self.decision(probe.finished_at_monotonic_ms);
        }
        let Some(target) = self.target_for_domain(&probe.domain).map(str::to_owned) else {
            return self.decision(probe.finished_at_monotonic_ms);
        };
        match probe.result {
            HttpsProbeResult::HttpResponse { status } if status < 500 => {
                if !self
                    .successful_probes
                    .iter()
                    .any(|existing| existing.probe_id == probe.probe_id)
                {
                    self.successful_probes.push(SuccessfulProbe {
                        probe_id: probe.probe_id,
                        target,
                        started_at_ms: probe.started_at_monotonic_ms,
                        finished_at_ms: probe.finished_at_monotonic_ms,
                    });
                }
            }
            HttpsProbeResult::HttpResponse { .. } | HttpsProbeResult::TargetFailure => {
                self.failure = Some(ConfirmationFailure::Target);
            }
            HttpsProbeResult::EnvironmentFailure => {
                self.failure = Some(ConfirmationFailure::Environment);
            }
        }
        self.refresh_quorum(probe.finished_at_monotonic_ms);
        self.decision(probe.finished_at_monotonic_ms)
    }

    pub fn observe_eye(&mut self, event: &EyeEvent, observed_at_ms: u64) -> ConfirmationDecision {
        if self.failure.is_some() || event.envelope() != self.envelope {
            return self.decision(observed_at_ms);
        }
        match event {
            EyeEvent::Flow(flow) => self.observe_flow(flow),
            EyeEvent::Health(health) => {
                self.health_state = health.state;
                if !matches!(health.state, EyeHealthState::Ready)
                    || health.counters.parse_errors > self.initial_counters.parse_errors
                    || health.counters.queue_drops > self.initial_counters.queue_drops
                {
                    self.failure = Some(ConfirmationFailure::Sensor);
                }
            }
            EyeEvent::Gap(gap) => {
                let sensor_deadline = self
                    .started_at_sensor_ms
                    .saturating_add(CONFIRMATION_DEADLINE_MS);
                let intersects =
                    gap.to_ts >= self.started_at_sensor_ms && gap.from_ts <= sensor_deadline;
                if intersects {
                    self.failure = Some(ConfirmationFailure::Sensor);
                }
            }
        }
        self.refresh_quorum(observed_at_ms);
        self.decision(observed_at_ms)
    }

    pub fn observe_confirmation_flow(
        &mut self,
        flow: &ConfirmationFlow,
        observed_at_ms: u64,
    ) -> ConfirmationDecision {
        if self.failure.is_some()
            || flow.sequence <= self.after_flow_sequence
            || flow.category != self.category
            || flow.lane_generation != self.lane_generation
            || flow.monotonic_ts < self.started_at_sensor_ms
        {
            return self.decision(observed_at_ms);
        }
        let Some(target) = normalize_domain(&flow.target) else {
            return self.decision(observed_at_ms);
        };
        if !self.targets.iter().any(|allowed| allowed == &target) {
            return self.decision(observed_at_ms);
        }
        let process_ts = self.sensor_to_process_time(flow.monotonic_ts);
        if process_ts > self.deadline_at_ms {
            return self.decision(observed_at_ms);
        }
        self.observe_diagnosis(flow.flow_id, target, flow.diagnosis, process_ts);
        self.refresh_quorum(observed_at_ms);
        self.decision(observed_at_ms)
    }

    pub fn poll(&mut self, now_ms: u64) -> ConfirmationDecision {
        self.refresh_quorum(now_ms);
        self.decision(now_ms)
    }

    fn observe_flow(&mut self, flow: &FlowEvent) {
        if flow.category.as_deref() != Some(self.category.as_str())
            || flow.lane_generation != Some(self.lane_generation)
            || flow.transport != Transport::Tls
            || flow.monotonic_ts < self.started_at_sensor_ms
        {
            return;
        }
        let Some(target) = self.target_for_domain(&flow.domain).map(str::to_owned) else {
            return;
        };
        let process_ts = self.sensor_to_process_time(flow.monotonic_ts);
        if process_ts <= self.deadline_at_ms {
            self.observe_diagnosis(flow.flow_id, target, flow.diagnosis, process_ts);
        }
    }

    fn observe_diagnosis(
        &mut self,
        flow_id: u64,
        target: String,
        diagnosis: Diagnosis,
        monotonic_ts: u64,
    ) {
        match diagnosis {
            Diagnosis::Working => {
                if !self
                    .working_flows
                    .iter()
                    .any(|existing| existing.flow_id == flow_id)
                {
                    self.working_flows.push(WorkingFlow {
                        flow_id,
                        target,
                        observed_at_ms: monotonic_ts,
                    });
                }
            }
            Diagnosis::TcpReset | Diagnosis::TlsBlackhole | Diagnosis::HttpBlockPage => {
                self.failure = Some(ConfirmationFailure::Strategy);
            }
            Diagnosis::DnsFailure | Diagnosis::IpUnreachable => {
                self.failure = Some(ConfirmationFailure::Environment);
            }
            Diagnosis::Throttled => self.failure = Some(ConfirmationFailure::Target),
            Diagnosis::TcpBlackhole
            | Diagnosis::QuicBlocked
            | Diagnosis::UdpBlocked
            | Diagnosis::Unknown => {}
        }
    }

    fn target_for_domain(&self, domain: &str) -> Option<&str> {
        let domain = normalize_domain(domain)?;
        self.targets
            .iter()
            .find(|target| domain_matches_suffix(&domain, target))
            .map(String::as_str)
    }

    fn sensor_to_process_time(&self, sensor_ms: u64) -> u64 {
        self.started_at_ms
            .saturating_add(sensor_ms.saturating_sub(self.started_at_sensor_ms))
    }

    fn refresh_quorum(&mut self, now_ms: u64) {
        if self.failure.is_some() || self.quorum_reached_at_ms.is_some() {
            return;
        }
        let reached = if self.targets.len() == 1 {
            self.single_target_quorum()
        } else {
            self.multi_target_quorum()
        };
        if reached {
            self.quorum_reached_at_ms = Some(now_ms.min(self.deadline_at_ms));
        }
    }

    fn single_target_quorum(&self) -> bool {
        let target = &self.targets[0];
        let probes = self
            .successful_probes
            .iter()
            .filter(|probe| &probe.target == target)
            .map(|probe| probe.probe_id)
            .collect::<BTreeSet<_>>();
        let flows = self
            .working_flows
            .iter()
            .filter(|flow| &flow.target == target)
            .map(|flow| flow.flow_id)
            .collect::<BTreeSet<_>>();
        probes.len() >= 2 && flows.len() >= 2 && self.correspondence_count(target) >= 2
    }

    fn multi_target_quorum(&self) -> bool {
        self.targets
            .iter()
            .filter(|target| self.correspondence_count(target) >= 1)
            .take(2)
            .count()
            >= 2
    }

    fn correspondence_count(&self, target: &str) -> usize {
        let mut probes = self
            .successful_probes
            .iter()
            .filter(|probe| probe.target == target)
            .collect::<Vec<_>>();
        probes.sort_by_key(|probe| (probe.finished_at_ms, probe.probe_id));
        let mut used_flows = BTreeSet::new();
        let mut matches = 0usize;
        for probe in probes {
            let matching = self
                .working_flows
                .iter()
                .filter(|flow| {
                    flow.target == target
                        && flow.observed_at_ms >= probe.started_at_ms
                        && flow.observed_at_ms
                            <= probe
                                .finished_at_ms
                                .saturating_add(CONFIRMATION_DELIVERY_MARGIN_MS)
                                .min(self.deadline_at_ms)
                        && !used_flows.contains(&flow.flow_id)
                })
                .min_by_key(|flow| (flow.observed_at_ms, flow.flow_id));
            if let Some(flow) = matching {
                used_flows.insert(flow.flow_id);
                matches += 1;
            }
        }
        matches
    }

    fn decision(&self, now_ms: u64) -> ConfirmationDecision {
        if let Some(failure) = self.failure {
            return ConfirmationDecision::Failed(failure);
        }
        if matches!(self.health_state, EyeHealthState::Ready)
            && self.quorum_reached_at_ms.is_some_and(|reached_at| {
                now_ms >= reached_at.saturating_add(CONFIRMATION_CLEAN_WINDOW_MS)
                    && reached_at.saturating_add(CONFIRMATION_CLEAN_WINDOW_MS)
                        <= self.deadline_at_ms
            })
        {
            return ConfirmationDecision::Succeeded;
        }
        if now_ms >= self.deadline_at_ms {
            ConfirmationDecision::Failed(ConfirmationFailure::MissingWorkingEvidence)
        } else {
            ConfirmationDecision::Pending
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmationBuildError {
    NoTargets,
}

impl fmt::Display for ConfirmationBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("candidate confirmation requires at least one registry target")
    }
}

impl Error for ConfirmationBuildError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendFailure {
    pub reason: ConfirmationFailure,
    pub message: String,
    pub requires_manual_intervention: bool,
}

impl BackendFailure {
    pub fn new(reason: ConfirmationFailure, message: impl Into<String>) -> Self {
        Self {
            reason,
            message: message.into(),
            requires_manual_intervention: false,
        }
    }

    pub fn manual(reason: ConfirmationFailure, message: impl Into<String>) -> Self {
        Self {
            reason,
            message: message.into(),
            requires_manual_intervention: true,
        }
    }

    fn registry(error: impl fmt::Display) -> Self {
        Self::new(ConfirmationFailure::Sensor, error.to_string())
    }
}

impl fmt::Display for BackendFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for BackendFailure {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopLaneResult {
    Stopped,
    /// The exact previous process is still owned and alive. An indeterminate
    /// timeout must be returned as an error, never as `TimedOutPreserved`.
    TimedOutPreserved,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartLaneError {
    /// Spawn/readiness actually tested the candidate executable/config and is
    /// therefore eligible for the coordinator's negative cooldown.
    Readiness(String),
    /// Environment, ownership, or sensor invalidation did not test strategy.
    Aborted(BackendFailure),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaneInstallKind {
    Candidate,
    Rollback,
    CrashRetry,
}

#[derive(Clone, Debug)]
pub struct ObserverReplacementPlan {
    pub session_id: SessionId,
    pub network_fingerprint: NetworkFingerprint,
    pub registry: Arc<TargetRegistry>,
    pub lane_generations: BTreeMap<String, LaneGeneration>,
    pub category: String,
    pub target_state: ObserverTargetState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObserverTargetState {
    Present(ProcessOwner),
    Absent,
}

#[derive(Clone, Debug)]
pub struct StartLaneRequest {
    pub envelope: IntentEnvelope,
    pub config: RecoveryConfig,
    pub lane_generation: LaneGeneration,
    pub install_kind: LaneInstallKind,
    pub previous_owner: ProcessOwner,
    pub failed_candidate: Option<ProcessOwner>,
    pub neighbor_owners: BTreeMap<String, ProcessOwner>,
}

#[derive(Clone, Debug)]
pub struct StartedLane {
    pub owner: ProcessOwner,
    pub confirmation_arm: ConfirmationArm,
}

#[derive(Clone, Debug)]
pub struct ConfirmationRequest {
    pub envelope: IntentEnvelope,
    pub candidate: RecoveryConfig,
    pub owner: ProcessOwner,
    pub arm: ConfirmationArm,
    pub candidate_targets: Vec<String>,
    pub deadline_ms: u64,
    pub clean_window_ms: u64,
}

#[derive(Clone, Debug)]
pub struct CommitRequest {
    pub envelope: IntentEnvelope,
    pub candidate: RecoveryConfig,
    pub owner: ProcessOwner,
    pub neighbor_owners: BTreeMap<String, ProcessOwner>,
    pub active_selections: Vec<(String, String)>,
}

/// Platform boundary for the executor. Implementations must keep every
/// method bounded and must not weaken PID start-identity checks to PID-only.
pub trait ScopedExecutorBackend: Send {
    fn monotonic_ms(&self) -> u64;
    /// Revalidates Phase 4 operator controls. Automatic actions carry a
    /// generation that becomes permanently stale after any mode/pause/freeze
    /// transition; Assisted actions are unaffected.
    fn authorize_recovery<'a>(
        &'a mut self,
        _origin: RecoveryOrigin,
        _category: String,
    ) -> BackendFuture<'a, Result<(), BackendFailure>> {
        Box::pin(async { Ok(()) })
    }
    fn current_fence<'a>(
        &'a mut self,
        category: String,
    ) -> BackendFuture<'a, Result<IntentFence, BackendFailure>>;
    fn current_registry(&self) -> Result<Arc<TargetRegistry>, BackendFailure>;
    fn lane_generations(&self) -> Result<BTreeMap<String, LaneGeneration>, BackendFailure>;
    fn exact_process_owners(&self) -> Result<BTreeMap<String, ProcessOwner>, BackendFailure>;
    fn owns_exact_process(&self, category: &str, owner: &ProcessOwner) -> bool;
    /// Returns true only after the backend has consumed an exact supervisor
    /// exit event for this owner. Plain absence from in-memory state is not
    /// enough because another actor could have corrupted or replaced the lane.
    fn exact_process_exit_observed<'a>(
        &'a mut self,
        _category: &'a str,
        _owner: &'a ProcessOwner,
    ) -> BackendFuture<'a, Result<bool, BackendFailure>> {
        Box::pin(async { Ok(false) })
    }

    fn reload_registry<'a>(
        &'a mut self,
        selections: Vec<(String, String)>,
    ) -> BackendFuture<'a, Result<Arc<TargetRegistry>, BackendFailure>>;

    /// Validates the exact generated config for one target without touching
    /// runtime files. Preflight invokes this for both candidate and rollback
    /// selections before the first observer/process mutation.
    fn preflight_scoped_launch<'a>(
        &'a mut self,
        selections: Vec<(String, String)>,
        category: String,
        config_file: String,
    ) -> BackendFuture<'a, Result<(), BackendFailure>>;

    fn fresh_environment_gate<'a>(
        &'a mut self,
        envelope: &'a IntentEnvelope,
    ) -> BackendFuture<'a, Result<GateReport, BackendFailure>>;

    fn replace_observer<'a>(
        &'a mut self,
        plan: ObserverReplacementPlan,
    ) -> BackendFuture<'a, Result<IntentFence, BackendFailure>>;

    fn stop_lane<'a>(
        &'a mut self,
        category: &'a str,
        owner: &'a ProcessOwner,
    ) -> BackendFuture<'a, Result<StopLaneResult, BackendFailure>>;

    fn start_lane<'a>(
        &'a mut self,
        request: StartLaneRequest,
    ) -> BackendFuture<'a, Result<StartedLane, StartLaneError>>;

    fn confirm_candidate<'a>(
        &'a mut self,
        request: ConfirmationRequest,
    ) -> BackendFuture<'a, Result<ConfirmationDecision, BackendFailure>>;

    /// Persists selected config/cache only after confirmation. In-memory lane
    /// ownership may already point at the candidate before this call.
    fn commit_candidate<'a>(
        &'a mut self,
        request: CommitRequest,
    ) -> BackendFuture<'a, Result<(), BackendFailure>>;
}

#[derive(Clone, Debug)]
struct ActiveExecution {
    attempt_id: AttemptId,
    original_envelope: IntentEnvelope,
    current_envelope: IntentEnvelope,
    original_lane_generations: BTreeMap<String, LaneGeneration>,
    neighbor_owners: BTreeMap<String, ProcessOwner>,
    plan: ScopedRegistryPlan,
    previous: RecoveryConfig,
    previous_owner: ProcessOwner,
    candidate: RecoveryConfig,
    origin: RecoveryOrigin,
    candidate_owner: Option<ProcessOwner>,
    confirmation_arm: Option<ConfirmationArm>,
    gate_valid_until_process_ms: u64,
    manual_after_rollback: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExecutorRunError {
    Busy,
    NoPreparedAttempt,
    AttemptMismatch,
    ActionMismatch,
}

impl fmt::Display for ExecutorRunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Busy => "a scoped Legacy attempt is already active",
            Self::NoPreparedAttempt => "no scoped Legacy attempt was prepared",
            Self::AttemptMismatch => "recovery action belongs to another attempt",
            Self::ActionMismatch => "recovery action does not match prepared immutable state",
        })
    }
}

impl AppScopedExecutorBackend {
    async fn run_confirmation(
        &mut self,
        request: ConfirmationRequest,
    ) -> Result<ConfirmationDecision, BackendFailure> {
        let mut evaluator = ConfirmationEvaluator::new_armed(
            &request.envelope,
            request.candidate_targets.iter(),
            request.arm.armed_at_monotonic_ms,
            request.arm,
        )
        .map_err(BackendFailure::registry)?;
        let deadline = request
            .arm
            .armed_at_monotonic_ms
            .saturating_add(request.deadline_ms.min(CONFIRMATION_DEADLINE_MS));

        let probe_targets = if request.candidate_targets.len() == 1 {
            vec![
                request.candidate_targets[0].clone(),
                request.candidate_targets[0].clone(),
            ]
        } else {
            request
                .candidate_targets
                .iter()
                .take(2)
                .cloned()
                .collect::<Vec<_>>()
        };
        let mut probes = tokio::task::JoinSet::new();
        for (index, target) in probe_targets.into_iter().enumerate() {
            probes.spawn(run_https_confirmation_probe(
                self.app.clone(),
                index as u64 + 1,
                target,
                Duration::from_millis(request.deadline_ms),
            ));
        }
        let mut remaining_probes = probes.len();
        let mut probe_transport_failed = false;
        let mut last_flow_sequence = request.arm.after_flow_sequence;
        let mut candidate_exit = self.candidate_exit.take();

        loop {
            let now_ms = self.monotonic_ms();
            let snapshot = self.manager_snapshot()?;
            if request
                .arm
                .validate_snapshot(&snapshot, &request.envelope)
                .is_err()
            {
                if let Some(exit) = candidate_exit.take() {
                    self.candidate_exit = Some(exit);
                }
                probes.abort_all();
                return Ok(ConfirmationDecision::Failed(ConfirmationFailure::Sensor));
            }
            let flow_cursor = last_flow_sequence;
            for flow in snapshot
                .confirmation_flows
                .iter()
                .filter(|flow| flow.sequence > flow_cursor)
            {
                last_flow_sequence = last_flow_sequence.max(flow.sequence);
                let decision = evaluator.observe_confirmation_flow(flow, now_ms);
                if decision != ConfirmationDecision::Pending {
                    if let Some(exit) = candidate_exit.take() {
                        self.candidate_exit = Some(exit);
                    }
                    probes.abort_all();
                    return Ok(decision);
                }
            }

            let decision = evaluator.poll(now_ms);
            if decision != ConfirmationDecision::Pending {
                let decision = if decision
                    == ConfirmationDecision::Failed(ConfirmationFailure::MissingWorkingEvidence)
                    && probe_transport_failed
                {
                    let local = Self::local_network().await;
                    if !local.online
                        || !local.interface_up
                        || !local.default_route_available
                        || !local.gateway_reachable
                        || local.fingerprint != request.envelope.expected_network_fingerprint
                    {
                        ConfirmationDecision::Failed(ConfirmationFailure::Environment)
                    } else {
                        ConfirmationDecision::Failed(ConfirmationFailure::Target)
                    }
                } else {
                    decision
                };
                if let Some(exit) = candidate_exit.take() {
                    self.candidate_exit = Some(exit);
                }
                probes.abort_all();
                return Ok(decision);
            }
            if now_ms >= deadline {
                continue;
            }

            tokio::select! {
                biased;
                exit = async {
                    match candidate_exit.as_mut() {
                        Some(receiver) => Some(receiver.await),
                        None => std::future::pending().await,
                    }
                } => {
                    match exit {
                        Some(Ok(event)) => {
                            probes.abort_all();
                            let owner = crate::dpi::recovery_process_owner(&event.owner);
                            return if owner == request.owner && !event.intentional {
                                self.retired_candidate_owner = Some(owner);
                                Ok(ConfirmationDecision::Failed(ConfirmationFailure::Strategy))
                            } else {
                                Err(BackendFailure::new(
                                    ConfirmationFailure::Sensor,
                                    "candidate process exit owner was stale or intentional",
                                ))
                            };
                        }
                        Some(Err(_)) => candidate_exit = None,
                        None => unreachable!("disabled exit branch never resolves"),
                    }
                }
                joined = probes.join_next(), if remaining_probes > 0 => {
                    remaining_probes = remaining_probes.saturating_sub(1);
                    match joined {
                        Some(Ok(ProbeTaskResult::Observed(observation))) => {
                            let decision = evaluator.observe_probe(observation);
                            if decision != ConfirmationDecision::Pending {
                                if let Some(exit) = candidate_exit.take() {
                                    self.candidate_exit = Some(exit);
                                }
                                probes.abort_all();
                                return Ok(decision);
                            }
                        }
                        Some(Ok(ProbeTaskResult::TransportFailed)) => {
                            probe_transport_failed = true;
                        }
                        Some(Err(error)) => {
                            if let Some(exit) = candidate_exit.take() {
                                self.candidate_exit = Some(exit);
                            }
                            probes.abort_all();
                            return Err(BackendFailure::new(
                                ConfirmationFailure::Sensor,
                                format!("confirmation probe task failed: {error}"),
                            ));
                        }
                        None => remaining_probes = 0,
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(100)) => {}
            }
        }
    }
}

enum ProbeTaskResult {
    Observed(HttpsProbeObservation),
    TransportFailed,
}

async fn run_https_confirmation_probe(
    app: AppHandle,
    probe_id: u64,
    domain: String,
    timeout: Duration,
) -> ProbeTaskResult {
    let started_at_monotonic_ms = app.state::<AppState>().legacy_monotonic_ms();
    let url = format!("https://{domain}/?obsession_recovery_probe={probe_id}");
    let response = match reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(timeout)
        .timeout(timeout)
        .build()
    {
        Ok(client) => client.get(url).send().await,
        Err(_) => return ProbeTaskResult::TransportFailed,
    };
    let finished_at_monotonic_ms = app.state::<AppState>().legacy_monotonic_ms();
    match response {
        Ok(response) => ProbeTaskResult::Observed(HttpsProbeObservation {
            probe_id,
            domain,
            started_at_monotonic_ms,
            finished_at_monotonic_ms,
            result: HttpsProbeResult::HttpResponse {
                status: response.status().as_u16(),
            },
        }),
        Err(_) => ProbeTaskResult::TransportFailed,
    }
}

#[derive(Debug)]
pub enum ScopedRecoveryRunError {
    Backend(BackendFailure),
    Executor(ExecutorRunError),
    Coordinator(super::recovery::TransitionError),
    TooManyTransitions,
}

impl fmt::Display for ScopedRecoveryRunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Backend(error) => write!(formatter, "Legacy executor backend failed: {error}"),
            Self::Executor(error) => write!(formatter, "Legacy executor rejected action: {error}"),
            Self::Coordinator(error) => {
                write!(formatter, "Legacy recovery transition rejected: {error:?}")
            }
            Self::TooManyTransitions => {
                formatter.write_str("Legacy recovery exceeded its bounded transition count")
            }
        }
    }
}

impl Error for ScopedRecoveryRunError {}

/// Runs a backend-owned Assisted or Automatic attempt to a terminal coordinator
/// action while holding the same DPI operation gate. The caller supplies only
/// the initial `Preflight` action produced by the recovery coordinator.
pub async fn run_scoped_recovery(
    app: &AppHandle,
    initial_action: RecoveryAction,
) -> Result<RecoveryAction, ScopedRecoveryRunError> {
    if !matches!(initial_action, RecoveryAction::Preflight { .. }) {
        return Err(ScopedRecoveryRunError::Executor(
            ExecutorRunError::ActionMismatch,
        ));
    }
    let state = app.state::<AppState>();
    let _dpi_gate = state.dpi_gate.lock().await;
    let backend = match AppScopedExecutorBackend::new(app.clone()) {
        Ok(backend) => backend,
        Err(_) => {
            let rejected = runner_failure_result(&initial_action, state.legacy_monotonic_ms())
                .ok_or(ScopedRecoveryRunError::Executor(
                    ExecutorRunError::ActionMismatch,
                ))?;
            let terminal_result = { state.legacy_recovery.lock_recover().apply_result(rejected) };
            let terminal = match terminal_result {
                Ok(terminal) => terminal,
                Err(error) => state
                    .legacy_recovery
                    .lock_recover()
                    .force_manual_failure(state.legacy_monotonic_ms())
                    .ok_or(ScopedRecoveryRunError::Coordinator(error))?,
            };
            super::status::refresh_recovery_overlay(app);
            return Ok(terminal);
        }
    };
    let mut executor = ScopedExecutor::new(backend);
    let mut action = initial_action;
    let mut pending_cache_failure = None;
    for _ in 0..12 {
        let executed_action = action.clone();
        let result = match executor.execute(action).await {
            Ok(result) => result,
            Err(error) => {
                let _ = executor.emergency_safe_state().await;
                let terminal = state
                    .legacy_recovery
                    .lock_recover()
                    .force_manual_failure(state.legacy_monotonic_ms())
                    .ok_or(ScopedRecoveryRunError::Executor(error))?;
                persist_candidate_failure(app, pending_cache_failure.take()).await;
                super::status::refresh_recovery_overlay(app);
                return Ok(terminal);
            }
        };
        if let Some(failure) = candidate_cache_failure(&executed_action, &result) {
            pending_cache_failure = Some(failure);
        }
        let next_action = { state.legacy_recovery.lock_recover().apply_result(result) };
        action = match next_action {
            Ok(action) => action,
            Err(error) => {
                let _ = executor.emergency_safe_state().await;
                let terminal = state
                    .legacy_recovery
                    .lock_recover()
                    .force_manual_failure(state.legacy_monotonic_ms())
                    .ok_or(ScopedRecoveryRunError::Coordinator(error))?;
                persist_candidate_failure(app, pending_cache_failure.take()).await;
                super::status::refresh_recovery_overlay(app);
                return Ok(terminal);
            }
        };
        super::status::refresh_recovery_overlay(app);
        if matches!(
            action,
            RecoveryAction::Complete { .. } | RecoveryAction::ManualIntervention { .. }
        ) {
            persist_candidate_failure(app, pending_cache_failure.take()).await;
            return Ok(action);
        }
    }
    let _ = executor.emergency_safe_state().await;
    let terminal = {
        state
            .legacy_recovery
            .lock_recover()
            .force_manual_failure(state.legacy_monotonic_ms())
    };
    if let Some(terminal) = terminal {
        persist_candidate_failure(app, pending_cache_failure.take()).await;
        super::status::refresh_recovery_overlay(app);
        Ok(terminal)
    } else {
        Err(ScopedRecoveryRunError::TooManyTransitions)
    }
}

#[derive(Clone, Debug)]
struct PendingCandidateCacheFailure {
    envelope: IntentEnvelope,
    candidate: RecoveryConfig,
    kind: super::cache::FailureKind,
    reason: &'static str,
}

fn candidate_cache_failure(
    action: &RecoveryAction,
    result: &ExecutorResult,
) -> Option<PendingCandidateCacheFailure> {
    let (envelope, candidate) = match action {
        RecoveryAction::StartCandidate {
            envelope,
            candidate,
            ..
        }
        | RecoveryAction::ConfirmCandidate {
            envelope,
            candidate,
            ..
        }
        | RecoveryAction::CommitCandidate {
            envelope,
            candidate,
            ..
        } => (envelope, candidate),
        _ => return None,
    };
    if &result.envelope != envelope {
        return None;
    }
    let (kind, reason) = match &result.outcome {
        ExecutorOutcome::StartFailed { .. } => {
            (super::cache::FailureKind::Readiness, "readiness_failure")
        }
        ExecutorOutcome::ConfirmationFailed {
            reason: ConfirmationFailure::Strategy,
            ..
        }
        | ExecutorOutcome::ExecutionAborted {
            reason: ConfirmationFailure::Strategy,
            ..
        } => (super::cache::FailureKind::Strategy, "strategy_failure"),
        ExecutorOutcome::Exited {
            intentional: false, ..
        } => (super::cache::FailureKind::Readiness, "unexpected_exit"),
        _ => return None,
    };
    Some(PendingCandidateCacheFailure {
        envelope: envelope.clone(),
        candidate: candidate.clone(),
        kind,
        reason,
    })
}

async fn persist_candidate_failure(app: &AppHandle, failure: Option<PendingCandidateCacheFailure>) {
    let Some(failure) = failure else {
        return;
    };
    // Rollback may take long enough for the user to move to another Wi-Fi.
    // Session-start identity alone is therefore insufficient for a persisted
    // cooldown: resolve the local network again immediately before the write.
    let local = AppScopedExecutorBackend::local_network().await;
    if !cache_network_fence_matches(&local, &failure.envelope.expected_network_fingerprint) {
        return;
    }
    let state = app.state::<AppState>();
    let snapshot = state
        .legacy_manager
        .lock_recover()
        .as_ref()
        .map(|manager| manager.snapshot());
    let Some(snapshot) = snapshot.filter(|snapshot| {
        !snapshot.session.closed
            && snapshot.session.session_id == failure.envelope.session_id
            && snapshot.session.network_fingerprint_at_start
                == failure.envelope.expected_network_fingerprint
    }) else {
        return;
    };
    let Some(stable_network) = snapshot.session.network_fingerprint_at_start.stable_key() else {
        return;
    };
    let fingerprint_is_exact = state
        .legacy_manager
        .lock_recover()
        .as_ref()
        .and_then(|manager| {
            manager
                .registry()
                .config_fingerprint(&failure.envelope.category, failure.candidate.config_id())
                .ok()
                .map(|fingerprint| fingerprint.as_hex() == failure.candidate.fingerprint().as_str())
        })
        .unwrap_or(false);
    if !fingerprint_is_exact {
        return;
    }
    let paths = state.paths.clone();
    let result = state.legacy_trust_cache.lock_recover().record_failure(
        &paths,
        super::cache::FailureRecord {
            candidate: super::cache::CandidateIdentity {
                stable_network_key: stable_network,
                category: &failure.envelope.category,
                config_id: failure.candidate.config_id(),
                config_fingerprint: failure.candidate.fingerprint().as_str(),
            },
            kind: failure.kind,
            reason: failure.reason,
            failed_at: unix_now_secs(),
        },
    );
    if let Err(error) = result {
        crate::util::emit_log(
            app,
            "warn",
            "legacy-reliability",
            &format!("Legacy candidate cooldown was not persisted: {error}"),
        );
    }
}

fn runner_failure_result(action: &RecoveryAction, now_ms: u64) -> Option<ExecutorResult> {
    let (envelope, outcome) = match action {
        RecoveryAction::Preflight { envelope, .. } => {
            (envelope.clone(), ExecutorOutcome::PreflightRejected)
        }
        RecoveryAction::StopPrevious { envelope, .. } => (
            envelope.clone(),
            ExecutorOutcome::ExecutionAborted {
                stage: ExecutorStage::Stop,
                reason: ConfirmationFailure::Sensor,
            },
        ),
        RecoveryAction::StartCandidate { envelope, .. } => (
            envelope.clone(),
            ExecutorOutcome::ExecutionAborted {
                stage: ExecutorStage::Start,
                reason: ConfirmationFailure::Sensor,
            },
        ),
        RecoveryAction::ConfirmCandidate { envelope, .. } => (
            envelope.clone(),
            ExecutorOutcome::ExecutionAborted {
                stage: ExecutorStage::Confirmation,
                reason: ConfirmationFailure::Sensor,
            },
        ),
        RecoveryAction::CommitCandidate { envelope, .. } => (
            envelope.clone(),
            ExecutorOutcome::ExecutionAborted {
                stage: ExecutorStage::Commit,
                reason: ConfirmationFailure::Sensor,
            },
        ),
        RecoveryAction::RollbackPrevious {
            envelope, previous, ..
        } => (
            envelope.clone(),
            ExecutorOutcome::RollbackFailed {
                previous_fingerprint: previous.fingerprint().clone(),
            },
        ),
        RecoveryAction::Complete { .. } | RecoveryAction::ManualIntervention { .. } => return None,
    };
    Some(result(envelope, outcome, now_ms))
}

impl Error for ExecutorRunError {}

/// Single-attempt executor driven by the pure [`super::recovery::RecoveryCoordinator`].
pub struct ScopedExecutor<B> {
    backend: B,
    active: Option<ActiveExecution>,
    last_failure: Option<BackendFailure>,
}

impl<B> ScopedExecutor<B>
where
    B: ScopedExecutorBackend,
{
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            active: None,
            last_failure: None,
        }
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }

    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    pub fn last_failure(&self) -> Option<&BackendFailure> {
        self.last_failure.as_ref()
    }

    async fn emergency_safe_state(&mut self) -> bool {
        let Some(active) = self.active.clone() else {
            return false;
        };
        let safe = if self
            .backend
            .owns_exact_process(&active.original_envelope.category, &active.previous_owner)
        {
            self.restore_previous_observer(&active).await.is_ok()
        } else {
            let rollback_generation =
                next_lane_generation(active.current_envelope.expected_lane_generation);
            self.perform_rollback(&active, &active.current_envelope, rollback_generation)
                .await
                .is_ok()
        };
        self.active = None;
        safe
    }

    pub async fn execute(
        &mut self,
        action: RecoveryAction,
    ) -> Result<ExecutorResult, ExecutorRunError> {
        match action {
            RecoveryAction::Preflight {
                envelope,
                previous,
                previous_owner,
                candidate,
                origin,
            } => {
                self.execute_preflight(origin, envelope, previous, previous_owner, candidate)
                    .await
            }
            RecoveryAction::StopPrevious {
                envelope,
                previous,
                previous_owner,
                origin,
            } => {
                self.execute_stop(origin, envelope, previous, previous_owner)
                    .await
            }
            RecoveryAction::StartCandidate {
                envelope,
                candidate,
                candidate_lane_generation,
            } => {
                self.execute_start(envelope, candidate, candidate_lane_generation)
                    .await
            }
            RecoveryAction::ConfirmCandidate {
                envelope,
                candidate,
                owner,
            } => self.execute_confirmation(envelope, candidate, owner).await,
            RecoveryAction::CommitCandidate {
                envelope,
                candidate,
                owner,
            } => self.execute_commit(envelope, candidate, owner).await,
            RecoveryAction::RollbackPrevious {
                envelope,
                previous,
                previous_lane_generation,
            } => {
                self.execute_rollback(envelope, previous, previous_lane_generation)
                    .await
            }
            RecoveryAction::Complete { .. } | RecoveryAction::ManualIntervention { .. } => {
                Err(ExecutorRunError::ActionMismatch)
            }
        }
    }

    async fn execute_preflight(
        &mut self,
        origin: RecoveryOrigin,
        envelope: IntentEnvelope,
        previous: RecoveryConfig,
        previous_owner: ProcessOwner,
        candidate: RecoveryConfig,
    ) -> Result<ExecutorResult, ExecutorRunError> {
        if self.active.is_some() {
            return Err(ExecutorRunError::Busy);
        }
        if let Err(failure) = self
            .backend
            .authorize_recovery(origin, envelope.category.clone())
            .await
        {
            return Ok(self.preflight_rejected(envelope, failure));
        }
        let old_fence = match self.exact_current_fence(&envelope).await {
            Ok(fence) => fence,
            Err(failure) => return Ok(self.preflight_rejected(envelope, failure)),
        };
        if !previous_owner.owns(previous.fingerprint(), envelope.expected_lane_generation) {
            return Ok(self.preflight_rejected(
                envelope,
                BackendFailure::new(
                    ConfirmationFailure::Sensor,
                    "approved previous owner does not match config/lane fence",
                ),
            ));
        }
        if !self
            .backend
            .owns_exact_process(&envelope.category, &previous_owner)
        {
            self.last_failure = Some(BackendFailure::new(
                ConfirmationFailure::Sensor,
                "previous Legacy process disappeared before preflight",
            ));
            return Ok(result(
                envelope,
                ExecutorOutcome::PreviousProcessMissing {
                    previous_fingerprint: previous.fingerprint().clone(),
                },
                self.backend.monotonic_ms(),
            ));
        }

        let current_registry = match self.backend.current_registry() {
            Ok(registry) => registry,
            Err(failure) => return Ok(self.preflight_rejected(envelope, failure)),
        };
        if current_registry.version() != envelope.expected_registry_version {
            return Ok(self.preflight_rejected(
                envelope,
                BackendFailure::new(
                    ConfirmationFailure::Sensor,
                    "current TargetRegistry version does not match approval fence",
                ),
            ));
        }
        let plan = match ScopedRegistryPlan::prepare(
            &current_registry,
            &envelope.category,
            &previous,
            &candidate,
        ) {
            Ok(plan) => plan,
            Err(error) => {
                return Ok(self.preflight_rejected(envelope, BackendFailure::registry(error)))
            }
        };
        let mut initial_owners = match self.backend.exact_process_owners() {
            Ok(owners) => owners,
            Err(failure) => return Ok(self.preflight_rejected(envelope, failure)),
        };
        let selected_categories = plan
            .previous_selections()
            .into_iter()
            .map(|(category, _)| category)
            .collect::<BTreeSet<_>>();
        if initial_owners.keys().cloned().collect::<BTreeSet<_>>() != selected_categories
            || initial_owners.get(plan.category()) != Some(&previous_owner)
        {
            if !self
                .backend
                .owns_exact_process(&envelope.category, &previous_owner)
            {
                return Ok(result(
                    envelope,
                    ExecutorOutcome::PreviousProcessMissing {
                        previous_fingerprint: previous.fingerprint().clone(),
                    },
                    self.backend.monotonic_ms(),
                ));
            }
            return Ok(self.preflight_rejected(
                envelope,
                BackendFailure::new(
                    ConfirmationFailure::Sensor,
                    "exact Legacy process set does not match registry selections",
                ),
            ));
        }
        initial_owners.remove(plan.category());
        let neighbor_owners = initial_owners;
        let original_lane_generations = match self.backend.lane_generations() {
            Ok(generations) => generations,
            Err(failure) => return Ok(self.preflight_rejected(envelope, failure)),
        };
        if original_lane_generations.get(plan.category())
            != Some(&envelope.expected_lane_generation)
        {
            return Ok(self.preflight_rejected(
                envelope,
                BackendFailure::new(
                    ConfirmationFailure::Sensor,
                    "Manager lane generation changed before preflight",
                ),
            ));
        }

        let candidate_registry = match self
            .backend
            .reload_registry(plan.candidate_selections())
            .await
        {
            Ok(registry) => registry,
            Err(failure) => return Ok(self.preflight_rejected(envelope, failure)),
        };
        if let Err(error) = plan.verify_candidate_registry(&candidate_registry) {
            return Ok(self.preflight_rejected(envelope, BackendFailure::registry(error)));
        }
        if candidate_registry.version() == envelope.expected_registry_version {
            return Ok(self.preflight_rejected(
                envelope,
                BackendFailure::new(
                    ConfirmationFailure::Sensor,
                    "tentative candidate did not produce a new registry version",
                ),
            ));
        }

        for (selections, config_file) in [
            (plan.previous_selections(), previous.config_id().to_owned()),
            (
                plan.candidate_selections(),
                candidate.config_id().to_owned(),
            ),
        ] {
            if let Err(failure) = self
                .backend
                .preflight_scoped_launch(selections, envelope.category.clone(), config_file)
                .await
            {
                return Ok(self.preflight_rejected(envelope, failure));
            }
        }

        let report = match self.backend.fresh_environment_gate(&envelope).await {
            Ok(report) => report,
            Err(failure) => return Ok(self.preflight_rejected(envelope, failure)),
        };
        if let Err(error) = authorize_gate_report(
            &report,
            &gate_fence(&envelope),
            report.generated_at_monotonic_ms,
        ) {
            return Ok(self.preflight_rejected(
                envelope,
                BackendFailure::new(error.failure_reason(), format!("gate rejected: {error:?}")),
            ));
        }

        // Gate probes and registry I/O are asynchronous. Re-read the complete
        // pre-observer fence immediately before the first mutation.
        if let Err(failure) = self.exact_current_fence(&envelope).await {
            if !self
                .backend
                .owns_exact_process(&envelope.category, &previous_owner)
            {
                return Ok(result(
                    envelope,
                    ExecutorOutcome::PreviousProcessMissing {
                        previous_fingerprint: previous.fingerprint().clone(),
                    },
                    self.backend.monotonic_ms(),
                ));
            }
            return Ok(self.preflight_rejected(envelope, failure));
        }
        let process_set_is_exact = self.backend.exact_process_owners().is_ok_and(|mut owners| {
            let target = owners.remove(&envelope.category);
            target.as_ref() == Some(&previous_owner) && owners == neighbor_owners
        });
        if !process_set_is_exact {
            if !self
                .backend
                .owns_exact_process(&envelope.category, &previous_owner)
            {
                return Ok(result(
                    envelope,
                    ExecutorOutcome::PreviousProcessMissing {
                        previous_fingerprint: previous.fingerprint().clone(),
                    },
                    self.backend.monotonic_ms(),
                ));
            }
            return Ok(self.preflight_rejected(
                envelope,
                BackendFailure::new(
                    ConfirmationFailure::Sensor,
                    "previous process identity changed during gate preflight",
                ),
            ));
        }

        let candidate_lane_generation = next_lane_generation(envelope.expected_lane_generation);
        let mut candidate_lane_generations = original_lane_generations.clone();
        candidate_lane_generations.insert(plan.category().to_owned(), candidate_lane_generation);
        let observer_plan = ObserverReplacementPlan {
            session_id: envelope.session_id,
            network_fingerprint: envelope.expected_network_fingerprint.clone(),
            registry: Arc::clone(&candidate_registry),
            lane_generations: candidate_lane_generations,
            category: plan.category().to_owned(),
            target_state: ObserverTargetState::Present(previous_owner.clone()),
        };
        if let Err(failure) = self
            .backend
            .authorize_recovery(origin, envelope.category.clone())
            .await
        {
            return Ok(self.preflight_rejected(envelope, failure));
        }
        let refreshed_fence = match self.backend.replace_observer(observer_plan).await {
            Ok(fence) => fence,
            Err(failure) => return Ok(self.preflight_rejected(envelope, failure)),
        };
        let refresh_is_exact = refreshed_fence.session_id == old_fence.session_id
            && refreshed_fence.category == old_fence.category
            && refreshed_fence.network_fingerprint == old_fence.network_fingerprint
            && refreshed_fence.lane_generation == candidate_lane_generation
            && refreshed_fence.sensor_generation != old_fence.sensor_generation
            && refreshed_fence.registry_version == candidate_registry.version()
            && refreshed_fence.registry_version != old_fence.registry_version;
        let current_envelope = IntentEnvelope::from_fence(envelope.attempt_id, &refreshed_fence);
        let mut temporary = ActiveExecution {
            attempt_id: envelope.attempt_id,
            original_envelope: envelope.clone(),
            current_envelope: current_envelope.clone(),
            original_lane_generations,
            neighbor_owners,
            plan,
            previous,
            previous_owner,
            candidate,
            origin,
            candidate_owner: None,
            confirmation_arm: None,
            gate_valid_until_process_ms: 0,
            manual_after_rollback: false,
        };
        if !refresh_is_exact {
            let failure = BackendFailure::new(
                ConfirmationFailure::Sensor,
                "observer restart returned an invalid refreshed fence",
            );
            self.last_failure = Some(failure.clone());
            if !self
                .backend
                .owns_exact_process(&envelope.category, &temporary.previous_owner)
            {
                return Ok(result(
                    envelope,
                    ExecutorOutcome::PreviousProcessMissing {
                        previous_fingerprint: temporary.previous.fingerprint().clone(),
                    },
                    self.backend.monotonic_ms(),
                ));
            }
            if let Err(restore_failure) = self.restore_previous_observer(&temporary).await {
                self.last_failure = Some(restore_failure);
                return Ok(result(
                    envelope,
                    ExecutorOutcome::RollbackFailed {
                        previous_fingerprint: temporary.previous.fingerprint().clone(),
                    },
                    self.backend.monotonic_ms(),
                ));
            }
            return Ok(result(
                envelope,
                ExecutorOutcome::ExecutionAborted {
                    stage: ExecutorStage::Preflight,
                    reason: failure.reason,
                },
                self.backend.monotonic_ms(),
            ));
        }

        // The transactional observer restart intentionally waits through its
        // degrading Gap/clean window, so the old Gate report has expired. A
        // second Gate on the new sensor/registry/lane fence is mandatory.
        let refreshed_gate = self.backend.fresh_environment_gate(&current_envelope).await;
        let refreshed_gate_failure = match refreshed_gate {
            Ok(report) => match authorize_gate_report(
                &report,
                &gate_fence(&current_envelope),
                report.generated_at_monotonic_ms,
            ) {
                Ok(()) => {
                    temporary.gate_valid_until_process_ms = self
                        .backend
                        .monotonic_ms()
                        .saturating_add(duration_millis(GATE_REPORT_TTL));
                    None
                }
                Err(error) => Some(BackendFailure::new(
                    error.failure_reason(),
                    format!("refreshed Environment Gate rejected: {error:?}"),
                )),
            },
            Err(failure) => Some(failure),
        };
        if let Some(failure) = refreshed_gate_failure {
            self.last_failure = Some(failure.clone());
            if !self
                .backend
                .owns_exact_process(&envelope.category, &temporary.previous_owner)
            {
                return Ok(result(
                    envelope,
                    ExecutorOutcome::PreviousProcessMissing {
                        previous_fingerprint: temporary.previous.fingerprint().clone(),
                    },
                    self.backend.monotonic_ms(),
                ));
            }
            if let Err(restore_failure) = self.restore_previous_observer(&temporary).await {
                self.last_failure = Some(restore_failure);
                return Ok(result(
                    envelope,
                    ExecutorOutcome::RollbackFailed {
                        previous_fingerprint: temporary.previous.fingerprint().clone(),
                    },
                    self.backend.monotonic_ms(),
                ));
            }
            return Ok(result(
                envelope,
                ExecutorOutcome::ExecutionAborted {
                    stage: ExecutorStage::Preflight,
                    reason: failure.reason,
                },
                self.backend.monotonic_ms(),
            ));
        }
        if let Err(failure) = self
            .verify_candidate_effect(
                &temporary,
                &current_envelope,
                Some(&temporary.previous_owner),
            )
            .await
        {
            self.last_failure = Some(failure.clone());
            if !self
                .backend
                .owns_exact_process(&envelope.category, &temporary.previous_owner)
            {
                return Ok(result(
                    envelope,
                    ExecutorOutcome::PreviousProcessMissing {
                        previous_fingerprint: temporary.previous.fingerprint().clone(),
                    },
                    self.backend.monotonic_ms(),
                ));
            }
            if let Err(restore_failure) = self.restore_previous_observer(&temporary).await {
                self.last_failure = Some(restore_failure);
                return Ok(result(
                    envelope,
                    ExecutorOutcome::RollbackFailed {
                        previous_fingerprint: temporary.previous.fingerprint().clone(),
                    },
                    self.backend.monotonic_ms(),
                ));
            }
            return Ok(result(
                envelope,
                ExecutorOutcome::ExecutionAborted {
                    stage: ExecutorStage::Preflight,
                    reason: failure.reason,
                },
                self.backend.monotonic_ms(),
            ));
        }

        self.active = Some(temporary);
        Ok(result(
            envelope,
            ExecutorOutcome::PreflightPassed { refreshed_fence },
            self.backend.monotonic_ms(),
        ))
    }

    async fn execute_stop(
        &mut self,
        origin: RecoveryOrigin,
        envelope: IntentEnvelope,
        previous: RecoveryConfig,
        previous_owner: ProcessOwner,
    ) -> Result<ExecutorResult, ExecutorRunError> {
        let active = self.active_for(&envelope)?.clone();
        if active.previous != previous
            || active.previous_owner != previous_owner
            || active.origin != origin
        {
            return Err(ExecutorRunError::ActionMismatch);
        }
        if !self
            .backend
            .owns_exact_process(&envelope.category, &previous_owner)
        {
            self.active = None;
            if self.verify_process_set(&active, None).is_ok() {
                if let Err(failure) = self.restore_previous_observer_absent(&active).await {
                    self.last_failure = Some(failure);
                }
                return Ok(result(
                    envelope,
                    ExecutorOutcome::PreviousProcessMissing {
                        previous_fingerprint: previous.fingerprint().clone(),
                    },
                    self.backend.monotonic_ms(),
                ));
            }
            return Ok(result(
                envelope,
                ExecutorOutcome::RollbackFailed {
                    previous_fingerprint: previous.fingerprint().clone(),
                },
                self.backend.monotonic_ms(),
            ));
        }
        let stop_authorization = async {
            let mut gate_deadline = active.gate_valid_until_process_ms;
            if self.backend.monotonic_ms() > gate_deadline {
                gate_deadline = self.refresh_stop_gate_deadline(&envelope).await?;
            }
            self.verify_candidate_effect(&active, &envelope, Some(&previous_owner))
                .await?;
            if self.backend.monotonic_ms() > gate_deadline {
                gate_deadline = self.refresh_stop_gate_deadline(&envelope).await?;
                self.verify_candidate_effect(&active, &envelope, Some(&previous_owner))
                    .await?;
            }
            if self.backend.monotonic_ms() > gate_deadline {
                return Err(BackendFailure::new(
                    ConfirmationFailure::Environment,
                    "fresh Environment Gate expired before the scoped stop",
                ));
            }
            self.backend
                .authorize_recovery(origin, envelope.category.clone())
                .await?;
            Ok::<(), BackendFailure>(())
        }
        .await;
        if let Err(failure) = stop_authorization {
            self.last_failure = Some(failure.clone());
            self.active = None;
            if !self
                .backend
                .owns_exact_process(&envelope.category, &previous_owner)
            {
                if self.verify_process_set(&active, None).is_ok() {
                    if let Err(restore_failure) =
                        self.restore_previous_observer_absent(&active).await
                    {
                        self.last_failure = Some(restore_failure);
                    }
                    return Ok(result(
                        envelope,
                        ExecutorOutcome::PreviousProcessMissing {
                            previous_fingerprint: previous.fingerprint().clone(),
                        },
                        self.backend.monotonic_ms(),
                    ));
                }
                return Ok(result(
                    envelope,
                    ExecutorOutcome::RollbackFailed {
                        previous_fingerprint: previous.fingerprint().clone(),
                    },
                    self.backend.monotonic_ms(),
                ));
            }
            if let Err(restore_failure) = self.restore_previous_observer(&active).await {
                self.last_failure = Some(restore_failure);
                return Ok(result(
                    envelope,
                    ExecutorOutcome::RollbackFailed {
                        previous_fingerprint: previous.fingerprint().clone(),
                    },
                    self.backend.monotonic_ms(),
                ));
            }
            return Ok(result(
                envelope,
                ExecutorOutcome::ExecutionAborted {
                    stage: ExecutorStage::Stop,
                    reason: failure.reason,
                },
                self.backend.monotonic_ms(),
            ));
        }

        match self
            .backend
            .stop_lane(&envelope.category, &previous_owner)
            .await
        {
            Ok(StopLaneResult::Stopped) => Ok(result(
                envelope,
                ExecutorOutcome::Stopped {
                    previous: previous_owner,
                },
                self.backend.monotonic_ms(),
            )),
            Ok(StopLaneResult::TimedOutPreserved) => {
                let restore = self.restore_previous_observer(&active).await;
                self.active = None;
                if let Err(failure) = restore {
                    self.last_failure = Some(failure.clone());
                    return Ok(result(
                        envelope,
                        ExecutorOutcome::RollbackFailed {
                            previous_fingerprint: previous.fingerprint().clone(),
                        },
                        self.backend.monotonic_ms(),
                    ));
                }
                Ok(result(
                    envelope,
                    ExecutorOutcome::StopTimedOut {
                        previous: previous_owner,
                    },
                    self.backend.monotonic_ms(),
                ))
            }
            Err(failure) => {
                self.last_failure = Some(failure.clone());
                self.active = None;
                if self.verify_process_set(&active, None).is_ok() {
                    if let Err(restore_failure) =
                        self.restore_previous_observer_absent(&active).await
                    {
                        self.last_failure = Some(restore_failure);
                    }
                    Ok(result(
                        envelope,
                        ExecutorOutcome::PreviousProcessMissing {
                            previous_fingerprint: previous.fingerprint().clone(),
                        },
                        self.backend.monotonic_ms(),
                    ))
                } else {
                    if self
                        .backend
                        .owns_exact_process(&envelope.category, &previous_owner)
                    {
                        if let Err(restore_failure) = self.restore_previous_observer(&active).await
                        {
                            self.last_failure = Some(restore_failure);
                        }
                    }
                    Ok(result(
                        envelope,
                        ExecutorOutcome::RollbackFailed {
                            previous_fingerprint: previous.fingerprint().clone(),
                        },
                        self.backend.monotonic_ms(),
                    ))
                }
            }
        }
    }

    async fn execute_start(
        &mut self,
        envelope: IntentEnvelope,
        candidate: RecoveryConfig,
        candidate_lane_generation: LaneGeneration,
    ) -> Result<ExecutorResult, ExecutorRunError> {
        let active = self.active_for(&envelope)?.clone();
        if active.candidate != candidate
            || candidate_lane_generation != envelope.expected_lane_generation
        {
            return Err(ExecutorRunError::ActionMismatch);
        }
        if let Err(failure) = self.verify_candidate_effect(&active, &envelope, None).await {
            self.last_failure = Some(failure.clone());
            return Ok(result(
                envelope,
                ExecutorOutcome::ExecutionAborted {
                    stage: ExecutorStage::Start,
                    reason: failure.reason,
                },
                self.backend.monotonic_ms(),
            ));
        }

        let request = StartLaneRequest {
            envelope: envelope.clone(),
            config: candidate.clone(),
            lane_generation: candidate_lane_generation,
            install_kind: LaneInstallKind::Candidate,
            previous_owner: active.previous_owner.clone(),
            failed_candidate: None,
            neighbor_owners: active.neighbor_owners.clone(),
        };
        match self.backend.start_lane(request).await {
            Ok(started)
                if started
                    .owner
                    .owns(candidate.fingerprint(), candidate_lane_generation)
                    && self
                        .backend
                        .owns_exact_process(&envelope.category, &started.owner) =>
            {
                let active = self
                    .active
                    .as_mut()
                    .expect("active execution checked above");
                active.candidate_owner = Some(started.owner.clone());
                active.confirmation_arm = Some(started.confirmation_arm);
                Ok(result(
                    envelope,
                    ExecutorOutcome::Ready {
                        candidate: started.owner,
                    },
                    self.backend.monotonic_ms(),
                ))
            }
            Ok(_) => {
                let failure = BackendFailure::new(
                    ConfirmationFailure::Sensor,
                    "candidate startup returned a mismatched process owner",
                );
                self.last_failure = Some(failure.clone());
                Ok(result(
                    envelope,
                    ExecutorOutcome::ExecutionAborted {
                        stage: ExecutorStage::Start,
                        reason: failure.reason,
                    },
                    self.backend.monotonic_ms(),
                ))
            }
            Err(StartLaneError::Readiness(message)) => {
                self.last_failure =
                    Some(BackendFailure::new(ConfirmationFailure::Strategy, message));
                Ok(result(
                    envelope,
                    ExecutorOutcome::StartFailed {
                        candidate_fingerprint: candidate.fingerprint().clone(),
                    },
                    self.backend.monotonic_ms(),
                ))
            }
            Err(StartLaneError::Aborted(failure)) => {
                self.last_failure = Some(failure.clone());
                Ok(result(
                    envelope,
                    ExecutorOutcome::ExecutionAborted {
                        stage: ExecutorStage::Start,
                        reason: failure.reason,
                    },
                    self.backend.monotonic_ms(),
                ))
            }
        }
    }

    async fn execute_confirmation(
        &mut self,
        envelope: IntentEnvelope,
        candidate: RecoveryConfig,
        owner: ProcessOwner,
    ) -> Result<ExecutorResult, ExecutorRunError> {
        let active = self.active_for(&envelope)?.clone();
        if active.candidate != candidate || active.candidate_owner.as_ref() != Some(&owner) {
            return Err(ExecutorRunError::ActionMismatch);
        }
        if let Err(failure) = self
            .verify_candidate_effect(&active, &envelope, Some(&owner))
            .await
        {
            self.last_failure = Some(failure.clone());
            return Ok(result(
                envelope,
                ExecutorOutcome::ExecutionAborted {
                    stage: ExecutorStage::Confirmation,
                    reason: failure.reason,
                },
                self.backend.monotonic_ms(),
            ));
        }

        let request = ConfirmationRequest {
            envelope: envelope.clone(),
            candidate,
            owner: owner.clone(),
            arm: active
                .confirmation_arm
                .ok_or(ExecutorRunError::ActionMismatch)?,
            candidate_targets: active.plan.candidate_targets().to_vec(),
            deadline_ms: CONFIRMATION_DEADLINE_MS,
            clean_window_ms: CONFIRMATION_CLEAN_WINDOW_MS,
        };
        match self.backend.confirm_candidate(request).await {
            Ok(ConfirmationDecision::Succeeded) => Ok(result(
                envelope,
                ExecutorOutcome::ConfirmationSucceeded { candidate: owner },
                self.backend.monotonic_ms(),
            )),
            Ok(ConfirmationDecision::Failed(reason)) => Ok(result(
                envelope,
                ExecutorOutcome::ConfirmationFailed {
                    candidate: owner,
                    reason,
                },
                self.backend.monotonic_ms(),
            )),
            Ok(ConfirmationDecision::Pending) => {
                let failure = BackendFailure::new(
                    ConfirmationFailure::Sensor,
                    "confirmation backend returned before its bounded deadline",
                );
                self.last_failure = Some(failure.clone());
                Ok(result(
                    envelope,
                    ExecutorOutcome::ExecutionAborted {
                        stage: ExecutorStage::Confirmation,
                        reason: failure.reason,
                    },
                    self.backend.monotonic_ms(),
                ))
            }
            Err(failure) => {
                self.last_failure = Some(failure.clone());
                Ok(result(
                    envelope,
                    ExecutorOutcome::ExecutionAborted {
                        stage: ExecutorStage::Confirmation,
                        reason: failure.reason,
                    },
                    self.backend.monotonic_ms(),
                ))
            }
        }
    }

    async fn execute_commit(
        &mut self,
        envelope: IntentEnvelope,
        candidate: RecoveryConfig,
        owner: ProcessOwner,
    ) -> Result<ExecutorResult, ExecutorRunError> {
        let active = self.active_for(&envelope)?.clone();
        if active.candidate != candidate || active.candidate_owner.as_ref() != Some(&owner) {
            return Err(ExecutorRunError::ActionMismatch);
        }
        if let Err(failure) = self
            .verify_candidate_effect(&active, &envelope, Some(&owner))
            .await
        {
            self.last_failure = Some(failure.clone());
            return Ok(result(
                envelope,
                ExecutorOutcome::ExecutionAborted {
                    stage: ExecutorStage::Commit,
                    reason: failure.reason,
                },
                self.backend.monotonic_ms(),
            ));
        }

        let request = CommitRequest {
            envelope: envelope.clone(),
            candidate,
            owner: owner.clone(),
            neighbor_owners: active.neighbor_owners.clone(),
            active_selections: active.plan.candidate_selections(),
        };
        match self.backend.commit_candidate(request).await {
            Ok(()) => {
                self.active = None;
                Ok(result(
                    envelope,
                    ExecutorOutcome::CandidateCommitted { candidate: owner },
                    self.backend.monotonic_ms(),
                ))
            }
            Err(failure) => {
                if failure.requires_manual_intervention {
                    if let Some(active) = self.active.as_mut() {
                        active.manual_after_rollback = true;
                    }
                }
                self.last_failure = Some(failure);
                Ok(result(
                    envelope,
                    ExecutorOutcome::CommitFailed { candidate: owner },
                    self.backend.monotonic_ms(),
                ))
            }
        }
    }

    async fn execute_rollback(
        &mut self,
        envelope: IntentEnvelope,
        previous: RecoveryConfig,
        previous_lane_generation: LaneGeneration,
    ) -> Result<ExecutorResult, ExecutorRunError> {
        let active = self.active_for(&envelope)?.clone();
        if active.previous != previous {
            return Err(ExecutorRunError::ActionMismatch);
        }

        let rollback = self
            .perform_rollback(&active, &envelope, previous_lane_generation)
            .await;
        self.active = None;
        match rollback {
            Ok(owner) if !active.manual_after_rollback => Ok(result(
                envelope,
                ExecutorOutcome::RolledBack { previous: owner },
                self.backend.monotonic_ms(),
            )),
            Ok(_) => Ok(result(
                envelope,
                ExecutorOutcome::RollbackFailed {
                    previous_fingerprint: previous.fingerprint().clone(),
                },
                self.backend.monotonic_ms(),
            )),
            Err(failure) => {
                self.last_failure = Some(failure);
                Ok(result(
                    envelope,
                    ExecutorOutcome::RollbackFailed {
                        previous_fingerprint: previous.fingerprint().clone(),
                    },
                    self.backend.monotonic_ms(),
                ))
            }
        }
    }

    async fn perform_rollback(
        &mut self,
        active: &ActiveExecution,
        envelope: &IntentEnvelope,
        previous_lane_generation: LaneGeneration,
    ) -> Result<ProcessOwner, BackendFailure> {
        // Candidate may be absent after a readiness failure. When it is
        // installed, exact owner fencing is mandatory before its bounded stop.
        if let Some(candidate_owner) = active.candidate_owner.as_ref() {
            self.exact_current_cleanup_fence(envelope).await?;
            if self
                .backend
                .owns_exact_process(&envelope.category, candidate_owner)
            {
                self.verify_process_set(active, Some(candidate_owner))?;
                match self
                    .backend
                    .stop_lane(&envelope.category, candidate_owner)
                    .await?
                {
                    StopLaneResult::Stopped => {}
                    StopLaneResult::TimedOutPreserved => {
                        return Err(BackendFailure::new(
                            ConfirmationFailure::Sensor,
                            "candidate could not be stopped for exact rollback",
                        ));
                    }
                }
            } else if self
                .backend
                .exact_process_exit_observed(&envelope.category, candidate_owner)
                .await?
            {
                // The exact supervisor event retires only this PID + creation
                // identity. Re-check the complete set so a replacement owner
                // or a neighbor race still fails closed.
                self.verify_process_set(active, None)?;
            } else {
                return Err(BackendFailure::new(
                    ConfirmationFailure::Sensor,
                    "candidate disappeared without an exact supervisor exit event",
                ));
            }
        } else {
            self.exact_current_cleanup_fence(envelope).await?;
            self.verify_process_set(active, None)?;
        }

        let previous_registry = self
            .backend
            .reload_registry(active.plan.previous_selections())
            .await?;
        active
            .plan
            .verify_previous_registry(&previous_registry)
            .map_err(BackendFailure::registry)?;

        // A network change is one of the reasons rollback is required, so it
        // must not make cleanup impossible. All executor-owned epochs and
        // process identities remain exact; only the live network value may
        // differ from the attempt fence.
        self.exact_current_cleanup_fence(envelope).await?;
        let mut rollback_lanes = self.backend.lane_generations()?;
        rollback_lanes.insert(envelope.category.clone(), previous_lane_generation);
        let rollback_plan = ObserverReplacementPlan {
            session_id: envelope.session_id,
            network_fingerprint: envelope.expected_network_fingerprint.clone(),
            registry: Arc::clone(&previous_registry),
            lane_generations: rollback_lanes,
            category: envelope.category.clone(),
            target_state: ObserverTargetState::Absent,
        };
        let rollback_fence = self.backend.replace_observer(rollback_plan).await?;
        if rollback_fence.session_id != envelope.session_id
            || rollback_fence.category != envelope.category
            || rollback_fence.network_fingerprint != envelope.expected_network_fingerprint
            || rollback_fence.lane_generation != previous_lane_generation
            || rollback_fence.sensor_generation == envelope.expected_sensor_generation
            || rollback_fence.registry_version != previous_registry.version()
            || rollback_fence.registry_version == envelope.expected_registry_version
        {
            return Err(BackendFailure::new(
                ConfirmationFailure::Sensor,
                "rollback observer returned an invalid refreshed fence",
            ));
        }
        let rollback_envelope = IntentEnvelope::from_fence(envelope.attempt_id, &rollback_fence);

        // Exact disk/content and live fence check immediately before starting
        // the previous process. Nothing persisted by candidate confirmation is
        // reused as an authority for rollback.
        self.exact_current_cleanup_fence(&rollback_envelope).await?;
        self.verify_process_set(active, None)?;
        let reloaded = self
            .backend
            .reload_registry(active.plan.previous_selections())
            .await?;
        active
            .plan
            .verify_previous_registry(&reloaded)
            .map_err(BackendFailure::registry)?;
        if reloaded.version() != rollback_envelope.expected_registry_version {
            return Err(BackendFailure::new(
                ConfirmationFailure::Sensor,
                "rollback registry version changed immediately before start",
            ));
        }

        let request = StartLaneRequest {
            envelope: rollback_envelope,
            config: active.previous.clone(),
            lane_generation: previous_lane_generation,
            install_kind: LaneInstallKind::Rollback,
            previous_owner: active.previous_owner.clone(),
            failed_candidate: active.candidate_owner.clone(),
            neighbor_owners: active.neighbor_owners.clone(),
        };
        let restored = self
            .backend
            .start_lane(request)
            .await
            .map_err(|error| match error {
                StartLaneError::Readiness(message) => {
                    BackendFailure::new(ConfirmationFailure::Strategy, message)
                }
                StartLaneError::Aborted(failure) => failure,
            })?;
        if !restored
            .owner
            .owns(active.previous.fingerprint(), previous_lane_generation)
            || self
                .verify_process_set(active, Some(&restored.owner))
                .is_err()
        {
            return Err(BackendFailure::new(
                ConfirmationFailure::Sensor,
                "restored process owner does not match exact rollback identity",
            ));
        }
        Ok(restored.owner)
    }

    async fn verify_candidate_effect(
        &mut self,
        active: &ActiveExecution,
        envelope: &IntentEnvelope,
        owner: Option<&ProcessOwner>,
    ) -> Result<(), BackendFailure> {
        self.exact_current_fence(envelope).await?;
        let current_registry = self.backend.current_registry()?;
        active
            .plan
            .verify_candidate_registry(&current_registry)
            .map_err(BackendFailure::registry)?;
        if current_registry.version() != envelope.expected_registry_version {
            return Err(BackendFailure::new(
                ConfirmationFailure::Sensor,
                "live candidate registry does not match exact action fence",
            ));
        }

        let reloaded = self
            .backend
            .reload_registry(active.plan.candidate_selections())
            .await?;
        active
            .plan
            .verify_candidate_registry(&reloaded)
            .map_err(BackendFailure::registry)?;
        if reloaded.version() != envelope.expected_registry_version {
            return Err(BackendFailure::new(
                ConfirmationFailure::Sensor,
                "candidate registry changed immediately before process effect",
            ));
        }
        self.verify_process_set(active, owner)?;
        Ok(())
    }

    fn verify_process_set(
        &self,
        active: &ActiveExecution,
        target: Option<&ProcessOwner>,
    ) -> Result<(), BackendFailure> {
        let mut owners = self.backend.exact_process_owners()?;
        let current_target = owners.remove(&active.current_envelope.category);
        if current_target.as_ref() != target || owners != active.neighbor_owners {
            return Err(BackendFailure::new(
                ConfirmationFailure::Sensor,
                "Legacy target or neighboring process ownership changed",
            ));
        }
        Ok(())
    }

    async fn restore_previous_observer(
        &mut self,
        active: &ActiveExecution,
    ) -> Result<IntentFence, BackendFailure> {
        self.restore_previous_observer_with_target(
            active,
            ObserverTargetState::Present(active.previous_owner.clone()),
        )
        .await
    }

    async fn restore_previous_observer_absent(
        &mut self,
        active: &ActiveExecution,
    ) -> Result<IntentFence, BackendFailure> {
        self.restore_previous_observer_with_target(active, ObserverTargetState::Absent)
            .await
    }

    async fn restore_previous_observer_with_target(
        &mut self,
        active: &ActiveExecution,
        target_state: ObserverTargetState,
    ) -> Result<IntentFence, BackendFailure> {
        match &target_state {
            ObserverTargetState::Present(owner) => {
                self.verify_process_set(active, Some(owner))?;
            }
            ObserverTargetState::Absent => self.verify_process_set(active, None)?,
        }
        let registry = self
            .backend
            .reload_registry(active.plan.previous_selections())
            .await?;
        active
            .plan
            .verify_previous_registry(&registry)
            .map_err(BackendFailure::registry)?;
        let plan = ObserverReplacementPlan {
            session_id: active.original_envelope.session_id,
            network_fingerprint: active
                .original_envelope
                .expected_network_fingerprint
                .clone(),
            registry: Arc::clone(&registry),
            lane_generations: active.original_lane_generations.clone(),
            category: active.original_envelope.category.clone(),
            target_state,
        };
        let restored = self.backend.replace_observer(plan).await?;
        if restored.session_id != active.original_envelope.session_id
            || restored.category != active.original_envelope.category
            || restored.network_fingerprint != active.original_envelope.expected_network_fingerprint
            || restored.lane_generation != active.original_envelope.expected_lane_generation
            || restored.registry_version != registry.version()
        {
            return Err(BackendFailure::new(
                ConfirmationFailure::Sensor,
                "failed to restore the previous observer fence",
            ));
        }
        Ok(restored)
    }

    async fn exact_current_fence(
        &mut self,
        envelope: &IntentEnvelope,
    ) -> Result<IntentFence, BackendFailure> {
        let current = self
            .backend
            .current_fence(envelope.category.clone())
            .await?;
        if envelope.matches_fence(&current) {
            Ok(current)
        } else {
            Err(BackendFailure::new(
                fence_failure_reason(envelope, &current),
                "Legacy recovery fence changed",
            ))
        }
    }

    async fn refresh_stop_gate_deadline(
        &mut self,
        envelope: &IntentEnvelope,
    ) -> Result<u64, BackendFailure> {
        let report = self.backend.fresh_environment_gate(envelope).await?;
        authorize_gate_report(
            &report,
            &gate_fence(envelope),
            report.generated_at_monotonic_ms,
        )
        .map_err(|error| {
            BackendFailure::new(
                error.failure_reason(),
                format!("last-moment Environment Gate rejected stop: {error:?}"),
            )
        })?;
        Ok(self
            .backend
            .monotonic_ms()
            .saturating_add(duration_millis(GATE_REPORT_TTL)))
    }

    async fn exact_current_cleanup_fence(
        &mut self,
        envelope: &IntentEnvelope,
    ) -> Result<IntentFence, BackendFailure> {
        let current = self
            .backend
            .current_fence(envelope.category.clone())
            .await?;
        if cleanup_fence_matches(envelope, &current) {
            Ok(current)
        } else {
            Err(BackendFailure::new(
                ConfirmationFailure::Sensor,
                "Legacy rollback fence changed outside the live network identity",
            ))
        }
    }

    fn active_for(&self, envelope: &IntentEnvelope) -> Result<&ActiveExecution, ExecutorRunError> {
        let active = self
            .active
            .as_ref()
            .ok_or(ExecutorRunError::NoPreparedAttempt)?;
        if active.attempt_id != envelope.attempt_id {
            return Err(ExecutorRunError::AttemptMismatch);
        }
        if active.current_envelope != *envelope {
            return Err(ExecutorRunError::ActionMismatch);
        }
        Ok(active)
    }

    fn preflight_rejected(
        &mut self,
        envelope: IntentEnvelope,
        failure: BackendFailure,
    ) -> ExecutorResult {
        self.last_failure = Some(failure);
        result(
            envelope,
            ExecutorOutcome::PreflightRejected,
            self.backend.monotonic_ms(),
        )
    }
}

fn fence_failure_reason(expected: &IntentEnvelope, current: &IntentFence) -> ConfirmationFailure {
    if expected.expected_network_fingerprint != current.network_fingerprint {
        ConfirmationFailure::Environment
    } else {
        ConfirmationFailure::Sensor
    }
}

fn cleanup_fence_matches(expected: &IntentEnvelope, current: &IntentFence) -> bool {
    expected.session_id == current.session_id
        && expected.category == current.category
        && expected.expected_lane_generation == current.lane_generation
        && expected.expected_sensor_generation == current.sensor_generation
        && expected.expected_registry_version == current.registry_version
}

fn result(
    envelope: IntentEnvelope,
    outcome: ExecutorOutcome,
    completed_at_monotonic_ms: u64,
) -> ExecutorResult {
    ExecutorResult {
        envelope,
        outcome,
        completed_at_monotonic_ms,
    }
}

fn next_lane_generation(current: LaneGeneration) -> LaneGeneration {
    let next = current.get().wrapping_add(1);
    LaneGeneration::new(if next == 0 { 1 } else { next })
}

fn duration_millis(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

/// Thin production bridge used by the Windows backend. It deliberately calls
/// the Eyes/Manager-only restart helper and derives the refreshed fence from
/// the returned Manager snapshot; no winws process is touched here.
pub(crate) async fn replace_legacy_observer(
    app: &AppHandle,
    expected_dpi_generation: u64,
    plan: ObserverReplacementPlan,
) -> Result<IntentFence, String> {
    let category = plan.category.clone();
    let target_expectation = match plan.target_state {
        ObserverTargetState::Present(owner) => {
            crate::dpi::LegacyObserverTargetExpectation::Present(owner)
        }
        ObserverTargetState::Absent => crate::dpi::LegacyObserverTargetExpectation::Absent,
    };
    let snapshot = crate::dpi::replace_legacy_observer_locked(
        app,
        expected_dpi_generation,
        plan.session_id,
        plan.network_fingerprint,
        plan.registry,
        plan.lane_generations,
        target_expectation,
    )
    .await?;
    intent_fence_from_snapshot(&snapshot, &category)
        .ok_or_else(|| "Legacy observer snapshot does not contain the target lane".to_owned())
}

/// One bounded same-config retry for a Legacy lane crash. The DPI gate orders
/// it after any in-flight assisted attempt: an already replaced owner becomes
/// `AlreadyHandled`, while a post-commit/post-rollback crash is retried.
pub(crate) enum CrashRetryRunOutcome {
    Restarted(ProcessOwner),
    AlreadyHandled,
    Deferred,
}

async fn replace_crash_retry_observer_after_preflight<B>(
    backend: &mut B,
    selections: Vec<(String, String)>,
    config_file: String,
    observer_plan: ObserverReplacementPlan,
) -> Result<IntentFence, BackendFailure>
where
    B: ScopedExecutorBackend,
{
    backend
        .preflight_scoped_launch(selections, observer_plan.category.clone(), config_file)
        .await?;
    backend.replace_observer(observer_plan).await
}

pub(crate) async fn retry_crashed_legacy_lane(
    app: &AppHandle,
    event: &crate::dpi::LegacyProcessExit,
) -> Result<CrashRetryRunOutcome, String> {
    if event.intentional {
        return Err("intentional Legacy stop is not retryable".into());
    }
    let state = app.state::<AppState>();
    let _dpi_gate = state.dpi_gate.lock().await;
    if state
        .shutting_down
        .load(std::sync::atomic::Ordering::SeqCst)
    {
        return Err("application is shutting down".into());
    }
    {
        let mut recovery = state.legacy_recovery.lock_recover();
        let status = recovery.status();
        if status.active_attempt.is_some() {
            return Ok(CrashRetryRunOutcome::Deferred);
        }
        if status.proposal.is_some() {
            recovery.cancel_pending();
        }
    }
    super::status::refresh_recovery_overlay(app);

    let mut backend = AppScopedExecutorBackend::new(app.clone()).map_err(|error| error.message)?;
    if event.owner.runtime_generation != backend.runtime_generation {
        return Ok(CrashRetryRunOutcome::AlreadyHandled);
    }
    if backend
        .exact_process_owners()
        .map_err(|error| error.message)?
        .contains_key(&event.owner.category)
    {
        return Ok(CrashRetryRunOutcome::AlreadyHandled);
    }
    let snapshot = backend.manager_snapshot().map_err(|error| error.message)?;
    if snapshot.session.closed
        || snapshot.session.lane_generations.get(&event.owner.category)
            != Some(&event.owner.lane_generation)
    {
        return Ok(CrashRetryRunOutcome::AlreadyHandled);
    }
    let registry = backend.current_registry().map_err(|error| error.message)?;
    let active_config = registry
        .active_config(&event.owner.category)
        .ok_or_else(|| "crashed category is absent from TargetRegistry".to_owned())?
        .to_owned();
    let active_fingerprint = registry
        .config_fingerprint(&event.owner.category, &active_config)
        .map_err(|error| error.to_string())?;
    if !active_config.eq_ignore_ascii_case(&event.owner.config_file)
        || active_fingerprint.as_hex() != event.owner.config_fingerprint
    {
        return Err("crashed config no longer matches immutable registry content".into());
    }

    let selections = registry
        .active_selections()
        .map(|(category, config)| (category.to_owned(), config.to_owned()))
        .collect::<Vec<_>>();
    let reloaded = backend
        .reload_registry(selections.clone())
        .await
        .map_err(|error| error.message)?;
    if reloaded.version() != registry.version()
        || reloaded.content_hash() != registry.content_hash()
    {
        return Err("Legacy registry content changed after the process crash".into());
    }

    let neighbors = backend
        .exact_process_owners()
        .map_err(|error| error.message)?;
    if neighbors.contains_key(&event.owner.category) {
        return Err("crashed Legacy category was already replaced".into());
    }
    let expected_neighbor_categories = snapshot
        .session
        .active_categories
        .iter()
        .filter(|category| *category != &event.owner.category)
        .cloned()
        .collect::<BTreeSet<_>>();
    if neighbors.keys().cloned().collect::<BTreeSet<_>>() != expected_neighbor_categories {
        return Err("neighboring Legacy process set changed after the crash".into());
    }

    let retry_lane_generation = next_lane_generation(event.owner.lane_generation);
    let mut lane_generations = snapshot.session.lane_generations.clone();
    lane_generations.insert(event.owner.category.clone(), retry_lane_generation);
    let observer_plan = ObserverReplacementPlan {
        session_id: snapshot.session.session_id,
        network_fingerprint: snapshot.session.network_fingerprint_at_start.clone(),
        registry: Arc::clone(&reloaded),
        lane_generations,
        category: event.owner.category.clone(),
        target_state: ObserverTargetState::Absent,
    };
    let retry_fence = replace_crash_retry_observer_after_preflight(
        &mut backend,
        selections,
        active_config,
        observer_plan,
    )
    .await
    .map_err(|error| error.message)?;
    if retry_fence.session_id != snapshot.session.session_id
        || retry_fence.category != event.owner.category
        || retry_fence.lane_generation != retry_lane_generation
        || retry_fence.sensor_generation == snapshot.session.sensor_generation
        || retry_fence.registry_version != reloaded.version()
    {
        return Err("crash retry observer returned an invalid refreshed fence".into());
    }

    backend.previous_anchor = Some(LegacyCategoryRuntimeSnapshot {
        owner: event.owner.clone(),
        selection_index: event.selection_index,
    });
    let previous_owner = recovery_process_owner_from_legacy(&event.owner);
    let config = RecoveryConfig::new(
        event.owner.config_file.clone(),
        super::contracts::ConfigFingerprint::new(event.owner.config_fingerprint.clone()),
    );
    let request = StartLaneRequest {
        envelope: IntentEnvelope::from_fence(AttemptId::new(u64::MAX), &retry_fence),
        config: config.clone(),
        lane_generation: retry_lane_generation,
        install_kind: LaneInstallKind::CrashRetry,
        previous_owner,
        failed_candidate: None,
        neighbor_owners: neighbors.clone(),
    };
    let started = backend
        .start_lane(request)
        .await
        .map_err(|error| match error {
            StartLaneError::Readiness(message) => message,
            StartLaneError::Aborted(failure) => failure.message,
        })?;
    let installed_is_exact = started
        .owner
        .owns(config.fingerprint(), retry_lane_generation)
        && backend.owns_exact_process(&event.owner.category, &started.owner)
        && backend
            .exact_process_owners()
            .map_err(|error| error.message)?
            .into_iter()
            .filter(|(category, _)| category != &event.owner.category)
            .collect::<BTreeMap<_, _>>()
            == neighbors;
    if !installed_is_exact {
        let _ = backend
            .stop_lane(&event.owner.category, &started.owner)
            .await;
        return Err("same-config crash retry installed an invalid process owner".into());
    }
    Ok(CrashRetryRunOutcome::Restarted(started.owner))
}

fn recovery_process_owner_from_legacy(owner: &LegacyProcessOwner) -> ProcessOwner {
    crate::dpi::recovery_process_owner(owner)
}

pub fn intent_fence_from_snapshot(
    snapshot: &ObserveOnlySnapshot,
    category: &str,
) -> Option<IntentFence> {
    let lane_generation = snapshot.session.lane_generations.get(category).copied()?;
    Some(IntentFence {
        session_id: snapshot.session.session_id,
        category: category.to_owned(),
        lane_generation,
        sensor_generation: snapshot.session.sensor_generation,
        registry_version: snapshot.session.target_registry_version,
        network_fingerprint: snapshot.session.network_fingerprint_at_start.clone(),
    })
}

/// Production backend for one transaction. Construct it only while holding
/// `AppState::dpi_gate`; [`run_scoped_recovery`] enforces that lifetime.
pub struct AppScopedExecutorBackend {
    app: AppHandle,
    runtime_generation: u64,
    previous_anchor: Option<LegacyCategoryRuntimeSnapshot>,
    failed_candidate_anchor: Option<LegacyCategoryRuntimeSnapshot>,
    candidate_exit: Option<tokio::sync::oneshot::Receiver<crate::dpi::LegacyProcessExit>>,
    retired_candidate_owner: Option<ProcessOwner>,
    unresolved_pending_owner: Option<LegacyProcessOwner>,
}

impl AppScopedExecutorBackend {
    pub fn new(app: AppHandle) -> Result<Self, BackendFailure> {
        let runtime_generation = app.state::<AppState>().dpi.lock_recover().generation;
        if runtime_generation == 0 {
            return Err(BackendFailure::new(
                ConfirmationFailure::Sensor,
                "Legacy runtime generation is not active",
            ));
        }
        Ok(Self {
            app,
            runtime_generation,
            previous_anchor: None,
            failed_candidate_anchor: None,
            candidate_exit: None,
            retired_candidate_owner: None,
            unresolved_pending_owner: None,
        })
    }

    fn manager_snapshot(&self) -> Result<ObserveOnlySnapshot, BackendFailure> {
        self.app
            .state::<AppState>()
            .legacy_manager
            .lock_recover()
            .as_ref()
            .map(|manager| manager.snapshot())
            .ok_or_else(|| {
                BackendFailure::new(ConfirmationFailure::Sensor, "Legacy Manager is not active")
            })
    }

    fn manager_gate(&self) -> Result<Arc<tokio::sync::Mutex<EnvironmentGate>>, BackendFailure> {
        self.app
            .state::<AppState>()
            .legacy_manager
            .lock_recover()
            .as_ref()
            .and_then(|manager| manager.environment_gate())
            .ok_or_else(|| {
                BackendFailure::new(
                    ConfirmationFailure::Environment,
                    "process-local Environment Gate is unavailable",
                )
            })
    }

    fn snapshot_anchor(
        &self,
        category: &str,
        expected: &ProcessOwner,
    ) -> Result<LegacyCategoryRuntimeSnapshot, BackendFailure> {
        let snapshot = self
            .app
            .state::<AppState>()
            .dpi
            .lock_recover()
            .snapshot_legacy_category(category)
            .map_err(BackendFailure::registry)?;
        if crate::dpi::recovery_process_owner(&snapshot.owner) != *expected {
            return Err(BackendFailure::new(
                ConfirmationFailure::Sensor,
                "Legacy process PID/start identity no longer matches",
            ));
        }
        Ok(snapshot)
    }

    fn capture_arm(&self, envelope: &IntentEnvelope) -> Result<ConfirmationArm, BackendFailure> {
        let snapshot = self.manager_snapshot()?;
        validate_snapshot_fence(&snapshot, envelope).map_err(|error| {
            BackendFailure::new(
                ConfirmationFailure::Sensor,
                format!("candidate confirmation fence is stale: {error:?}"),
            )
        })?;
        let lane = snapshot
            .lanes
            .iter()
            .find(|lane| lane.category == envelope.category)
            .ok_or_else(|| {
                BackendFailure::new(
                    ConfirmationFailure::Sensor,
                    "candidate lane missing from Manager snapshot",
                )
            })?;
        if lane.lane_generation != envelope.expected_lane_generation {
            return Err(BackendFailure::new(
                ConfirmationFailure::Sensor,
                "candidate lane generation changed before confirmation arm",
            ));
        }
        Ok(ConfirmationArm {
            armed_at_monotonic_ms: self.monotonic_ms(),
            armed_at_sensor_ms: snapshot.logical_now_ms,
            after_flow_sequence: snapshot.last_accepted_flow_sequence.unwrap_or(0),
            last_gap_sequence: snapshot.last_gap_sequence,
            evidence_epoch: lane.evidence_epoch,
            health: snapshot.health.state,
            counters: snapshot.health.counters,
        })
    }

    async fn abort_pending_lane(
        &mut self,
        pending: crate::dpi::PendingLegacyLane,
        context: &str,
    ) -> Result<(), BackendFailure> {
        let owner = pending.process().exact_legacy_owner().ok_or_else(|| {
            BackendFailure::new(
                ConfirmationFailure::Sensor,
                format!("{context}; pending process has no exact owner"),
            )
        })?;
        match pending.abort().await {
            Ok(()) => Ok(()),
            Err(error) => {
                self.unresolved_pending_owner = Some(owner);
                Err(BackendFailure::new(
                    ConfirmationFailure::Sensor,
                    format!("{context}; exact pending cleanup failed: {error}"),
                ))
            }
        }
    }

    async fn ensure_pending_lane_retired(&mut self) -> Result<(), BackendFailure> {
        let Some(owner) = self.unresolved_pending_owner.take() else {
            return Ok(());
        };
        match crate::dpi::stop_uninstalled_legacy_owner_bounded(&owner).await {
            Ok(()) => Ok(()),
            Err(error) => {
                self.unresolved_pending_owner = Some(owner);
                Err(BackendFailure::new(
                    ConfirmationFailure::Sensor,
                    format!(
                        "uninstalled candidate is still not proven stopped; rollback spawn is forbidden: {error}"
                    ),
                ))
            }
        }
    }

    async fn local_network() -> crate::netid::LocalNetworkIdentity {
        crate::netid::resolve_local_read_only().await
    }
}

impl ScopedExecutorBackend for AppScopedExecutorBackend {
    fn monotonic_ms(&self) -> u64 {
        self.app.state::<AppState>().legacy_monotonic_ms()
    }

    fn authorize_recovery<'a>(
        &'a mut self,
        origin: RecoveryOrigin,
        category: String,
    ) -> BackendFuture<'a, Result<(), BackendFailure>> {
        Box::pin(async move {
            let RecoveryOrigin::Automatic { control_generation } = origin else {
                return Ok(());
            };
            let state = self.app.state::<AppState>();
            // `mutate_settings` changes the authorization fields and bumps the
            // dedicated revision while holding this same settings lock. Read
            // both under it so pause -> resume cannot expose new settings with
            // an old revision snapshot and revive this queued action.
            let settings = state.settings.lock_recover();
            if state
                .shutting_down
                .load(std::sync::atomic::Ordering::SeqCst)
                || state.legacy_automation_revision.current() != control_generation
            {
                return Err(BackendFailure::new(
                    ConfirmationFailure::Sensor,
                    "Automatic authorization generation changed",
                ));
            }
            let authorized = settings.dpi_engine == "legacy"
                && settings.legacy_reliability_mode == "automatic"
                && !settings.legacy_automatic_paused
                && !settings
                    .legacy_reliability_frozen_categories
                    .iter()
                    .any(|frozen| frozen == &category);
            if authorized {
                Ok(())
            } else {
                Err(BackendFailure::new(
                    ConfirmationFailure::Sensor,
                    "Automatic recovery was paused, frozen, downgraded, or left Legacy",
                ))
            }
        })
    }

    fn current_fence<'a>(
        &'a mut self,
        category: String,
    ) -> BackendFuture<'a, Result<IntentFence, BackendFailure>> {
        Box::pin(async move {
            let snapshot = self.manager_snapshot()?;
            if !self
                .app
                .state::<AppState>()
                .dpi
                .lock_recover()
                .is_current_generation(self.runtime_generation)
            {
                return Err(BackendFailure::new(
                    ConfirmationFailure::Sensor,
                    "DPI runtime generation changed",
                ));
            }
            let local = Self::local_network().await;
            let mut fence = intent_fence_from_snapshot(&snapshot, &category).ok_or_else(|| {
                BackendFailure::new(
                    ConfirmationFailure::Sensor,
                    "category is absent from current Manager fence",
                )
            })?;
            fence.network_fingerprint = local.fingerprint;
            Ok(fence)
        })
    }

    fn current_registry(&self) -> Result<Arc<TargetRegistry>, BackendFailure> {
        self.app
            .state::<AppState>()
            .legacy_manager
            .lock_recover()
            .as_ref()
            .map(|manager| manager.registry())
            .ok_or_else(|| {
                BackendFailure::new(
                    ConfirmationFailure::Sensor,
                    "Legacy Manager registry is unavailable",
                )
            })
    }

    fn lane_generations(&self) -> Result<BTreeMap<String, LaneGeneration>, BackendFailure> {
        Ok(self.manager_snapshot()?.session.lane_generations)
    }

    fn exact_process_owners(&self) -> Result<BTreeMap<String, ProcessOwner>, BackendFailure> {
        let state = self.app.state::<AppState>();
        let dpi = state.dpi.lock_recover();
        if !dpi.is_current_generation(self.runtime_generation) {
            return Err(BackendFailure::new(
                ConfirmationFailure::Sensor,
                "DPI runtime generation changed while reading process owners",
            ));
        }
        let categories = match dpi.active_launch.as_ref() {
            Some(crate::state::DpiLaunchSpec::Legacy { selections }) => selections
                .iter()
                .map(|(category, _)| category.clone())
                .collect::<Vec<_>>(),
            Some(crate::state::DpiLaunchSpec::Zapret2 { .. }) => {
                return Err(BackendFailure::new(
                    ConfirmationFailure::Sensor,
                    "DPI runtime changed to Zapret2",
                ));
            }
            None if dpi.procs.is_empty() => Vec::new(),
            None => {
                return Err(BackendFailure::new(
                    ConfirmationFailure::Sensor,
                    "Legacy runtime lost its exact launch snapshot",
                ));
            }
        };
        let mut owners = BTreeMap::new();
        for category in categories {
            let snapshot = dpi
                .snapshot_legacy_category(&category)
                .map_err(BackendFailure::registry)?;
            owners.insert(
                category,
                crate::dpi::recovery_process_owner(&snapshot.owner),
            );
        }
        Ok(owners)
    }

    fn owns_exact_process(&self, category: &str, owner: &ProcessOwner) -> bool {
        self.snapshot_anchor(category, owner).is_ok()
    }

    fn exact_process_exit_observed<'a>(
        &'a mut self,
        category: &'a str,
        owner: &'a ProcessOwner,
    ) -> BackendFuture<'a, Result<bool, BackendFailure>> {
        Box::pin(async move {
            if self.retired_candidate_owner.as_ref() == Some(owner) {
                return Ok(true);
            }
            if self.owns_exact_process(category, owner) {
                return Ok(false);
            }
            let Some(mut receiver) = self.candidate_exit.take() else {
                return Ok(false);
            };
            match tokio::time::timeout(Duration::from_millis(500), &mut receiver).await {
                Ok(Ok(event)) => {
                    let observed = crate::dpi::recovery_process_owner(&event.owner);
                    if observed != *owner || event.intentional {
                        return Err(BackendFailure::new(
                            ConfirmationFailure::Sensor,
                            "candidate exit event did not match the exact unexpected owner",
                        ));
                    }
                    self.retired_candidate_owner = Some(observed);
                    Ok(true)
                }
                Ok(Err(_)) => Ok(false),
                Err(_) => {
                    self.candidate_exit = Some(receiver);
                    Ok(false)
                }
            }
        })
    }

    fn reload_registry<'a>(
        &'a mut self,
        selections: Vec<(String, String)>,
    ) -> BackendFuture<'a, Result<Arc<TargetRegistry>, BackendFailure>> {
        let paths = self.app.state::<AppState>().paths.clone();
        Box::pin(async move {
            tauri::async_runtime::spawn_blocking(move || {
                super::registry_loader::load_target_registry(&paths, &selections)
                    .map(Arc::new)
                    .map_err(BackendFailure::registry)
            })
            .await
            .map_err(|error| {
                BackendFailure::new(
                    ConfirmationFailure::Sensor,
                    format!("Legacy registry worker failed: {error}"),
                )
            })?
        })
    }

    fn preflight_scoped_launch<'a>(
        &'a mut self,
        selections: Vec<(String, String)>,
        category: String,
        config_file: String,
    ) -> BackendFuture<'a, Result<(), BackendFailure>> {
        let base = self.app.state::<AppState>().paths.base_dir.clone();
        Box::pin(async move {
            tauri::async_runtime::spawn_blocking(move || {
                super::autohost_isolation::preflight_scoped_launch_from_disk(
                    &base,
                    &selections,
                    &category,
                    &config_file,
                )
                .map_err(|error| {
                    BackendFailure::new(
                        ConfirmationFailure::Sensor,
                        format!(
                            "Legacy scoped isolation preflight failed for {category}/{config_file}: {error}"
                        ),
                    )
                })
            })
            .await
            .map_err(|error| {
                BackendFailure::new(
                    ConfirmationFailure::Sensor,
                    format!("Legacy scoped isolation preflight worker failed: {error}"),
                )
            })?
        })
    }

    fn fresh_environment_gate<'a>(
        &'a mut self,
        envelope: &'a IntentEnvelope,
    ) -> BackendFuture<'a, Result<GateReport, BackendFailure>> {
        Box::pin(async move {
            let snapshot = self.manager_snapshot()?;
            let lane = snapshot
                .lanes
                .iter()
                .find(|lane| lane.category == envelope.category)
                .ok_or_else(|| {
                    BackendFailure::new(
                        ConfirmationFailure::Sensor,
                        "Gate lane is absent from Manager snapshot",
                    )
                })?;
            let observed_targets = snapshot
                .gate_probe_hosts
                .get(&envelope.category)
                .cloned()
                .unwrap_or_default();
            let targets = if !observed_targets.is_empty() {
                observed_targets
            } else {
                self.current_registry()?
                    .active_targets_for_category(&envelope.category)
                    .into_iter()
                    .take(super::environment_gate::MAX_CATEGORY_TARGETS)
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            };
            if targets.is_empty() {
                return Err(BackendFailure::new(
                    ConfirmationFailure::Target,
                    "fresh Environment Gate has no exact incident targets",
                ));
            }
            let gate_session = snapshot.session.session_id;
            let gate_sensor = snapshot.session.sensor_generation;
            let gate_registry = snapshot.session.target_registry_version;
            let gate_lane = lane.lane_generation;
            let gate_evidence_epoch = lane.evidence_epoch;
            let gate_gap_sequence = snapshot.last_gap_sequence;
            let gate_counters = snapshot.health.counters;
            let local = Self::local_network().await;
            let local_network = LocalNetworkSnapshot {
                online: local.online,
                interface_up: local.interface_up,
                default_route_available: local.default_route_available,
                gateway_reachable: local.gateway_reachable,
                network_fingerprint: local.fingerprint,
            };
            let request = GateRequest {
                fence: gate_fence(envelope),
                category: envelope.category.clone(),
                category_targets: targets
                    .into_iter()
                    .take(super::environment_gate::MAX_CATEGORY_TARGETS)
                    .collect(),
                local_network,
                sensor: SensorSnapshot {
                    state: snapshot.health.state,
                    has_intersecting_gap: matches!(
                        lane.classification,
                        super::assessment::AssessmentClassification::SensorUnreliable
                    ),
                },
                passive_evidence: lane_passive_evidence(lane),
                requested_at_monotonic_ms: snapshot.logical_now_ms,
            };
            let gate = self.manager_gate()?;
            let baseline_clock_ms = self.monotonic_ms();
            let mut gate = gate.lock().await;
            let expected_gate = gate_fence(envelope);
            let report = gate
                .evaluate_with_baseline_clock(request, baseline_clock_ms)
                .await
                .map_err(|error| {
                    BackendFailure::new(
                        ConfirmationFailure::Environment,
                        format!("fresh Environment Gate request failed: {error}"),
                    )
                })?;
            authorize_gate_report(&report, &expected_gate, report.generated_at_monotonic_ms)
                .map_err(|error| {
                    BackendFailure::new(
                        error.failure_reason(),
                        format!("fresh Environment Gate rejected attempt: {error:?}"),
                    )
                })?;
            let current = self.manager_snapshot()?;
            let current_lane = current
                .lanes
                .iter()
                .find(|lane| lane.category == envelope.category)
                .ok_or_else(|| {
                    BackendFailure::new(
                        ConfirmationFailure::Sensor,
                        "Gate lane disappeared while probes were running",
                    )
                })?;
            if current.session.closed
                || current.session.session_id != gate_session
                || current.session.sensor_generation != gate_sensor
                || current.session.target_registry_version != gate_registry
                || current_lane.lane_generation != gate_lane
                || current.last_gap_sequence != gate_gap_sequence
                || current.health.state != EyeHealthState::Ready
                || current.health.counters.parse_errors > gate_counters.parse_errors
                || current.health.counters.queue_drops > gate_counters.queue_drops
            {
                return Err(BackendFailure::new(
                    ConfirmationFailure::Sensor,
                    "sensor/evidence fence changed during fresh Environment Gate",
                ));
            }
            authorize_current_passive_quorum(
                &report,
                gate_evidence_epoch,
                current_lane.evidence_epoch,
                lane_passive_evidence(current_lane),
            )
            .map_err(|error| {
                BackendFailure::new(
                    error.failure_reason(),
                    format!("fresh Environment Gate passive evidence rejected: {error:?}"),
                )
            })?;
            Ok(report)
        })
    }

    fn replace_observer<'a>(
        &'a mut self,
        plan: ObserverReplacementPlan,
    ) -> BackendFuture<'a, Result<IntentFence, BackendFailure>> {
        Box::pin(async move {
            replace_legacy_observer(&self.app, self.runtime_generation, plan)
                .await
                .map_err(|error| BackendFailure::new(ConfirmationFailure::Sensor, error))
        })
    }

    fn stop_lane<'a>(
        &'a mut self,
        category: &'a str,
        owner: &'a ProcessOwner,
    ) -> BackendFuture<'a, Result<StopLaneResult, BackendFailure>> {
        Box::pin(async move {
            let snapshot = self.snapshot_anchor(category, owner)?;
            let is_previous = self.previous_anchor.is_none();
            match crate::dpi::stop_scoped_legacy_lane_locked(&self.app, &snapshot).await {
                Ok(stopped) => {
                    if crate::dpi::recovery_process_owner(&stopped) != *owner {
                        return Err(BackendFailure::new(
                            ConfirmationFailure::Sensor,
                            "scoped stop returned a different process owner",
                        ));
                    }
                    if is_previous {
                        self.previous_anchor = Some(snapshot);
                    } else {
                        self.failed_candidate_anchor = Some(snapshot);
                    }
                    Ok(StopLaneResult::Stopped)
                }
                Err(error) => {
                    let os_identity_matches = crate::dpi_supervisor::capture_process_identity(
                        owner.pid,
                    )
                    .is_some_and(|identity| identity.get() == owner.process_start_identity.get());
                    if self.owns_exact_process(category, owner) && os_identity_matches {
                        Ok(StopLaneResult::TimedOutPreserved)
                    } else {
                        Err(BackendFailure::new(
                            ConfirmationFailure::Sensor,
                            format!("scoped Legacy stop became indeterminate: {error}"),
                        ))
                    }
                }
            }
        })
    }

    fn start_lane<'a>(
        &'a mut self,
        request: StartLaneRequest,
    ) -> BackendFuture<'a, Result<StartedLane, StartLaneError>> {
        Box::pin(async move {
            self.ensure_pending_lane_retired()
                .await
                .map_err(StartLaneError::Aborted)?;
            let pending = crate::dpi::spawn_scoped_legacy_lane_locked(
                &self.app,
                &request.envelope.category,
                request.config.config_id(),
                self.runtime_generation,
                request.lane_generation,
                request.config.fingerprint(),
            )
            .await
            .map_err(classify_scoped_start_error)?;

            let pending_is_exact = pending.process().exact_legacy_owner().is_some_and(|owner| {
                owner.category == request.envelope.category
                    && owner.lane_generation == request.lane_generation
                    && owner.config_fingerprint == request.config.fingerprint().as_str()
            });
            if !pending_is_exact {
                let mismatch = BackendFailure::new(
                    ConfirmationFailure::Sensor,
                    "pending scoped process owner does not match start request",
                );
                self.abort_pending_lane(pending, &mismatch.message)
                    .await
                    .map_err(StartLaneError::Aborted)?;
                return Err(StartLaneError::Aborted(mismatch));
            }

            let preinstall_check = async {
                let current = self
                    .current_fence(request.envelope.category.clone())
                    .await?;
                let fence_matches = match request.install_kind {
                    LaneInstallKind::Candidate => request.envelope.matches_fence(&current),
                    LaneInstallKind::Rollback | LaneInstallKind::CrashRetry => {
                        cleanup_fence_matches(&request.envelope, &current)
                    }
                };
                if !fence_matches {
                    return Err(BackendFailure::new(
                        if request.install_kind == LaneInstallKind::Candidate {
                            fence_failure_reason(&request.envelope, &current)
                        } else {
                            ConfirmationFailure::Sensor
                        },
                        "Legacy fence changed during scoped readiness",
                    ));
                }
                let owners = self.exact_process_owners()?;
                if owners != request.neighbor_owners {
                    return Err(BackendFailure::new(
                        ConfirmationFailure::Sensor,
                        "neighbor process ownership changed during scoped readiness",
                    ));
                }
                let live_registry = self.current_registry()?;
                let live_fingerprint = live_registry
                    .config_fingerprint(&request.envelope.category, request.config.config_id())
                    .map_err(BackendFailure::registry)?;
                if live_registry.version() != request.envelope.expected_registry_version
                    || live_registry
                        .active_config(&request.envelope.category)
                        .is_none_or(|config| {
                            !config.eq_ignore_ascii_case(request.config.config_id())
                        })
                    || live_fingerprint.as_hex() != request.config.fingerprint().as_str()
                {
                    return Err(BackendFailure::new(
                        ConfirmationFailure::Sensor,
                        "candidate registry changed during scoped readiness",
                    ));
                }
                let selections = live_registry
                    .active_selections()
                    .map(|(category, config)| (category.to_owned(), config.to_owned()))
                    .collect::<Vec<_>>();
                let reloaded = self.reload_registry(selections).await?;
                if reloaded.version() != live_registry.version()
                    || reloaded.content_hash() != live_registry.content_hash()
                {
                    return Err(BackendFailure::new(
                        ConfirmationFailure::Sensor,
                        "candidate content hash changed during scoped readiness",
                    ));
                }
                Ok::<(), BackendFailure>(())
            }
            .await;
            if let Err(failure) = preinstall_check {
                self.abort_pending_lane(pending, &failure.message)
                    .await
                    .map_err(StartLaneError::Aborted)?;
                return Err(StartLaneError::Aborted(failure));
            }
            let previous = match self.previous_anchor.clone() {
                Some(previous) => previous,
                None => {
                    let failure = BackendFailure::new(
                        ConfirmationFailure::Sensor,
                        "exact previous rollback anchor is missing",
                    );
                    self.abort_pending_lane(pending, &failure.message)
                        .await
                        .map_err(StartLaneError::Aborted)?;
                    return Err(StartLaneError::Aborted(failure));
                }
            };
            let failed_candidate = self.failed_candidate_anchor.clone();
            let install = match request.install_kind {
                LaneInstallKind::Candidate => pending.install_candidate(&previous),
                LaneInstallKind::Rollback => {
                    if let Some(failed) = failed_candidate.as_ref() {
                        pending.install_rollback(&previous, failed)
                    } else {
                        pending.install_candidate(&previous)
                    }
                }
                LaneInstallKind::CrashRetry => pending.install_crash_retry(&previous),
            };
            let installed = match install {
                Ok(installed) => installed,
                Err(failure) => {
                    let (pending, message) = failure.into_parts();
                    self.abort_pending_lane(pending, &message)
                        .await
                        .map_err(StartLaneError::Aborted)?;
                    return Err(StartLaneError::Aborted(BackendFailure::new(
                        ConfirmationFailure::Sensor,
                        message,
                    )));
                }
            };
            let owner = crate::dpi::recovery_process_owner(&installed.owner);
            let confirmation_arm = self
                .capture_arm(&request.envelope)
                .unwrap_or(ConfirmationArm {
                    armed_at_monotonic_ms: self.monotonic_ms(),
                    armed_at_sensor_ms: 0,
                    after_flow_sequence: 0,
                    last_gap_sequence: None,
                    evidence_epoch: 0,
                    health: EyeHealthState::Blind,
                    counters: EyeHealthCounters::default(),
                });
            if request.install_kind == LaneInstallKind::Candidate {
                self.candidate_exit = Some(installed.exit);
            }
            Ok(StartedLane {
                owner,
                confirmation_arm,
            })
        })
    }

    fn confirm_candidate<'a>(
        &'a mut self,
        request: ConfirmationRequest,
    ) -> BackendFuture<'a, Result<ConfirmationDecision, BackendFailure>> {
        Box::pin(async move { self.run_confirmation(request).await })
    }

    fn commit_candidate<'a>(
        &'a mut self,
        request: CommitRequest,
    ) -> BackendFuture<'a, Result<(), BackendFailure>> {
        Box::pin(async move {
            if !self.owns_exact_process(&request.envelope.category, &request.owner) {
                return Err(BackendFailure::new(
                    ConfirmationFailure::Sensor,
                    "candidate owner changed before settings commit",
                ));
            }
            let paths = self.app.state::<AppState>().paths.clone();
            let base_dir = paths.base_dir.clone();
            let previous_selection = {
                let state = self.app.state::<AppState>();
                let mut settings = state.settings.lock_recover();
                let previous = settings
                    .selected_configs
                    .get(&request.envelope.category)
                    .cloned();
                let mut next = settings.clone();
                next.selected_configs.insert(
                    request.envelope.category.clone(),
                    request.candidate.config_id().to_owned(),
                );
                next.save(&base_dir).map_err(|error| {
                    BackendFailure::new(
                        ConfirmationFailure::Environment,
                        format!("failed to persist confirmed Legacy selection: {error}"),
                    )
                })?;
                *settings = next;
                state.settings_revision.bump();
                previous
            };

            let post_save_fence = self.current_fence(request.envelope.category.clone()).await;
            let fence_matches = post_save_fence
                .as_ref()
                .is_ok_and(|current| request.envelope.matches_fence(current));
            let state = self.app.state::<AppState>();
            let mut expected_owners = request.neighbor_owners.clone();
            expected_owners.insert(request.envelope.category.clone(), request.owner.clone());
            let process_set_matches = {
                let mut dpi = state.dpi.lock_recover();
                let owners_match = dpi.is_current_generation(self.runtime_generation)
                    && dpi.procs.len() == expected_owners.len()
                    && expected_owners.iter().all(|(category, expected)| {
                        dpi.snapshot_legacy_category(category)
                            .ok()
                            .is_some_and(|snapshot| {
                                crate::dpi::recovery_process_owner(&snapshot.owner) == *expected
                            })
                    });
                let expected_selections = request
                    .active_selections
                    .iter()
                    .cloned()
                    .collect::<BTreeMap<_, _>>();
                let selections_match = matches!(
                    dpi.active_launch.as_ref(),
                    Some(crate::state::DpiLaunchSpec::Legacy { selections })
                        if selections.len() == expected_selections.len()
                            && selections.iter().all(|(category, config)| {
                                expected_selections.get(category) == Some(config)
                            })
                );
                if owners_match && selections_match {
                    if fence_matches {
                        dpi.last_legacy_selection = request.active_selections.clone();
                    }
                    true
                } else {
                    false
                }
            };
            if !fence_matches || !process_set_matches {
                let restore_result = {
                    let mut settings = state.settings.lock_recover();
                    if settings
                        .selected_configs
                        .get(&request.envelope.category)
                        .is_some_and(|config| {
                            config.eq_ignore_ascii_case(request.candidate.config_id())
                        })
                    {
                        let mut restored = settings.clone();
                        match previous_selection {
                            Some(previous) => {
                                restored
                                    .selected_configs
                                    .insert(request.envelope.category.clone(), previous);
                            }
                            None => {
                                restored.selected_configs.remove(&request.envelope.category);
                            }
                        }
                        restored.save(&base_dir).map(|()| {
                            *settings = restored;
                            state.settings_revision.bump();
                        })
                    } else {
                        Ok(())
                    }
                };
                if let Err(restore_error) = restore_result {
                    return Err(BackendFailure::manual(
                        ConfirmationFailure::Environment,
                        format!(
                            "commit fence changed and selected-config restore failed: {restore_error}"
                        ),
                    ));
                }
                return Err(match post_save_fence {
                    Err(failure) => failure,
                    Ok(current) => BackendFailure::new(
                        if fence_matches {
                            ConfirmationFailure::Sensor
                        } else {
                            fence_failure_reason(&request.envelope, &current)
                        },
                        if fence_matches {
                            "candidate or neighboring process changed during settings commit"
                        } else {
                            "candidate or network fence changed during settings commit"
                        },
                    ),
                });
            }
            let cache_fingerprint_is_exact = self
                .current_registry()
                .ok()
                .and_then(|registry| {
                    registry
                        .config_fingerprint(
                            &request.envelope.category,
                            request.candidate.config_id(),
                        )
                        .ok()
                        .map(|fingerprint| {
                            fingerprint.as_hex() == request.candidate.fingerprint().as_str()
                        })
                })
                .unwrap_or(false);
            // Selection/process commit is already successful at this point.
            // A network change only suppresses trust memory; it must never
            // roll back the confirmed candidate.
            let cache_local_network = Self::local_network().await;
            if let Some(stable_network) = request
                .envelope
                .expected_network_fingerprint
                .stable_key()
                .filter(|_| {
                    cache_fingerprint_is_exact
                        && cache_network_fence_matches(
                            &cache_local_network,
                            &request.envelope.expected_network_fingerprint,
                        )
                })
            {
                let state = self.app.state::<AppState>();
                let session_key = format!(
                    "{:032x}:{}",
                    state.legacy_cache_boot_nonce,
                    request.envelope.session_id.get()
                );
                let confirmed_at = unix_now_secs();
                let cache_result = state.legacy_trust_cache.lock_recover().record_confirmation(
                    &paths,
                    super::cache::ConfirmationRecord {
                        candidate: super::cache::CandidateIdentity {
                            stable_network_key: stable_network,
                            category: &request.envelope.category,
                            config_id: request.candidate.config_id(),
                            config_fingerprint: request.candidate.fingerprint().as_str(),
                        },
                        session_key: &session_key,
                        confirmed_at,
                    },
                );
                if let Err(error) = cache_result {
                    crate::util::emit_log(
                        &self.app,
                        "warn",
                        "legacy-reliability",
                        &format!(
                            "Confirmed candidate kept, but Legacy trust cache was not updated: {error}"
                        ),
                    );
                }
            }
            Ok(())
        })
    }
}

fn cache_network_fence_matches(
    local: &crate::netid::LocalNetworkIdentity,
    expected: &NetworkFingerprint,
) -> bool {
    expected.stable_key().is_some()
        && local.online
        && local.interface_up
        && local.default_route_available
        && local.gateway_reachable
        && &local.fingerprint == expected
}

fn unix_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn classify_scoped_start_error(error: String) -> StartLaneError {
    let lower = error.to_ascii_lowercase();
    if lower.contains("runtime changed")
        || lower.contains("generation changed")
        || lower.contains("still occupied")
        || lower.contains("app is shutting down")
        || lower.contains("приложение завершает работу")
        || lower.contains("process start identity")
    {
        StartLaneError::Aborted(BackendFailure::new(ConfirmationFailure::Sensor, error))
    } else {
        StartLaneError::Readiness(error)
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use super::*;
    use crate::legacy_reliability::contracts::{
        ConfigFingerprint, EyeHealthCounters, GapEvent, ProcessStartIdentity, RegistryVersion,
        SensorGeneration,
    };
    use crate::legacy_reliability::recovery::{
        IncidentId, RecoveryCoordinator, RecoveryDecision, RecoveryDisposition, RecoveryMode,
        RecoveryRequest,
    };
    use crate::legacy_reliability::target_registry::LegacyConfigRecord;

    fn record(
        category: &str,
        config_name: &str,
        list_name: &str,
        targets: &str,
    ) -> LegacyConfigRecord {
        LegacyConfigRecord::new(
            category,
            config_name,
            format!("--wf-tcp=443 --hostlist=lists/{list_name}"),
        )
        .with_hostlist(format!("lists/{list_name}"), targets)
    }

    fn records() -> Vec<LegacyConfigRecord> {
        vec![
            record(
                "video",
                "video_1.conf",
                "video-old.txt",
                "old.video.example\nshared.video.example\n",
            ),
            record(
                "video",
                "video_2.conf",
                "video-new.txt",
                "one.video.example\ntwo.video.example\nshared.video.example\n",
            ),
            record("chat", "chat_1.conf", "chat.txt", "chat.example\n"),
        ]
    }

    fn registry(
        records: &[LegacyConfigRecord],
        selections: Vec<(String, String)>,
    ) -> Arc<TargetRegistry> {
        Arc::new(TargetRegistry::from_records_with_active_selections(records, selections).unwrap())
    }

    fn old_selections() -> Vec<(String, String)> {
        vec![
            ("chat".into(), "chat_1.conf".into()),
            ("video".into(), "video_1.conf".into()),
        ]
    }

    fn recovery_config(registry: &TargetRegistry, category: &str, config: &str) -> RecoveryConfig {
        RecoveryConfig::new(
            config,
            registry
                .config_fingerprint(category, config)
                .unwrap()
                .as_hex(),
        )
    }

    fn network() -> NetworkFingerprint {
        NetworkFingerprint::Stable {
            key: "stable-test-network".into(),
        }
    }

    fn preflight_gate_fence() -> GateFence {
        GateFence {
            session_id: SessionId::new(7),
            lane_generation: LaneGeneration::new(3),
            sensor_generation: SensorGeneration::new(5),
            target_registry_version: RegistryVersion::new(11),
            network_fingerprint: network(),
        }
    }

    fn preflight_gate_report(classification: GateClassification, fence: GateFence) -> GateReport {
        GateReport {
            fence,
            category: "video".into(),
            classification,
            controls: Vec::new(),
            category_targets: Vec::new(),
            baseline_latency_ms: Some(100),
            slow_threshold_ms: Some(1_600),
            generated_at_monotonic_ms: 100,
            valid_until_monotonic_ms: 10_100,
        }
    }

    #[test]
    fn preflight_authorizes_a_fresh_exact_dpi_blocked_report() {
        let fence = preflight_gate_fence();
        let report = preflight_gate_report(GateClassification::DpiBlocked, fence.clone());
        assert_eq!(authorize_gate_report(&report, &fence, 100), Ok(()));
    }

    #[test]
    fn preflight_keeps_a_confirmed_single_target_blackhole_actionable() {
        let epoch = 17;
        let report =
            preflight_gate_report(GateClassification::DpiSuspected, preflight_gate_fence());
        let current = PassiveEvidenceSummary {
            confirmed_tls_blackhole_flows: 2,
            blackhole_targets: 1,
            ..PassiveEvidenceSummary::default()
        };

        assert_eq!(
            authorize_current_passive_quorum(&report, epoch, epoch, current),
            Ok(())
        );
    }

    #[test]
    fn fresh_gate_rejects_expired_or_cleared_quorum_without_evidence_epoch_change() {
        let unchanged_epoch = 17;
        let cases = [
            (
                GateClassification::DpiSuspected,
                PassiveEvidenceSummary {
                    reset_after_client_hello_flows: 3,
                    reset_targets: 2,
                    ..PassiveEvidenceSummary::default()
                },
                PassiveEvidenceSummary {
                    reset_after_client_hello_flows: 2,
                    reset_targets: 2,
                    ..PassiveEvidenceSummary::default()
                },
            ),
            (
                GateClassification::DpiBlocked,
                PassiveEvidenceSummary {
                    confirmed_tls_blackhole_flows: 2,
                    blackhole_targets: 2,
                    ..PassiveEvidenceSummary::default()
                },
                PassiveEvidenceSummary::default(),
            ),
        ];

        for (classification, initial_quorum, current_evidence) in cases {
            let report = preflight_gate_report(classification, preflight_gate_fence());
            assert_eq!(
                authorize_current_passive_quorum(
                    &report,
                    unchanged_epoch,
                    unchanged_epoch,
                    initial_quorum,
                ),
                Ok(())
            );
            assert_eq!(
                authorize_current_passive_quorum(
                    &report,
                    unchanged_epoch,
                    unchanged_epoch,
                    current_evidence,
                ),
                Err(PreflightGateError::IncidentNotActionable)
            );
        }
    }

    #[test]
    fn preflight_rejects_environment_target_sensor_and_stale_reports() {
        let fence = preflight_gate_fence();
        let recovered = preflight_gate_report(GateClassification::Stable, fence.clone());
        assert_eq!(
            authorize_gate_report(&recovered, &fence, 100),
            Err(PreflightGateError::IncidentNotActionable)
        );
        for classification in [
            GateClassification::Offline,
            GateClassification::DnsFailure,
            GateClassification::UpstreamDegraded,
        ] {
            let report = preflight_gate_report(classification, fence.clone());
            assert_eq!(
                authorize_gate_report(&report, &fence, 100),
                Err(PreflightGateError::Environment)
            );
        }
        for classification in [
            GateClassification::TargetUnavailable,
            GateClassification::ServiceSlow,
        ] {
            let report = preflight_gate_report(classification, fence.clone());
            assert_eq!(
                authorize_gate_report(&report, &fence, 100),
                Err(PreflightGateError::Target)
            );
        }
        let blind = preflight_gate_report(GateClassification::SensorUnreliable, fence.clone());
        assert_eq!(
            authorize_gate_report(&blind, &fence, 100),
            Err(PreflightGateError::Sensor)
        );

        let expired = preflight_gate_report(GateClassification::DpiBlocked, fence.clone());
        assert_eq!(
            authorize_gate_report(&expired, &fence, 10_101),
            Err(PreflightGateError::StaleReport)
        );
        let mut wrong_fence = fence.clone();
        wrong_fence.sensor_generation = SensorGeneration::new(6);
        let mismatched = preflight_gate_report(GateClassification::DpiBlocked, wrong_fence);
        assert_eq!(
            authorize_gate_report(&mismatched, &fence, 100),
            Err(PreflightGateError::StaleReport)
        );
    }

    fn process_owner(
        pid: u32,
        identity: u64,
        fingerprint: &ConfigFingerprint,
        lane: u64,
    ) -> ProcessOwner {
        ProcessOwner {
            pid,
            process_start_identity: ProcessStartIdentity::new(identity),
            config_fingerprint: fingerprint.clone(),
            lane_generation: LaneGeneration::new(lane),
        }
    }

    fn confirmation_envelope() -> IntentEnvelope {
        IntentEnvelope {
            session_id: SessionId::new(11),
            attempt_id: AttemptId::new(9),
            category: "video".into(),
            expected_lane_generation: LaneGeneration::new(8),
            expected_sensor_generation: SensorGeneration::new(4),
            expected_registry_version: RegistryVersion::new(6),
            expected_network_fingerprint: network(),
        }
    }

    fn confirmation_flow(
        sequence: u64,
        flow_id: u64,
        target: &str,
        diagnosis: Diagnosis,
        sensor_ms: u64,
    ) -> ConfirmationFlow {
        ConfirmationFlow {
            sequence,
            category: "video".into(),
            lane_generation: LaneGeneration::new(8),
            flow_id,
            target: target.into(),
            diagnosis,
            monotonic_ts: sensor_ms,
        }
    }

    fn arm() -> ConfirmationArm {
        ConfirmationArm {
            armed_at_monotonic_ms: 10_000,
            armed_at_sensor_ms: 500,
            after_flow_sequence: 100,
            last_gap_sequence: None,
            evidence_epoch: 3,
            health: EyeHealthState::Ready,
            counters: EyeHealthCounters::default(),
        }
    }

    fn probe(id: u64, domain: &str, start: u64, finish: u64) -> HttpsProbeObservation {
        HttpsProbeObservation {
            probe_id: id,
            domain: domain.into(),
            started_at_monotonic_ms: start,
            finished_at_monotonic_ms: finish,
            result: HttpsProbeResult::HttpResponse { status: 204 },
        }
    }

    #[test]
    fn registry_plan_fences_candidate_neighbors_and_shared_same_category_target() {
        let records = records();
        let active = registry(&records, old_selections());
        let previous = recovery_config(&active, "video", "video_1.conf");
        let candidate = recovery_config(&active, "video", "video_2.conf");
        let plan = ScopedRegistryPlan::prepare(&active, "video", &previous, &candidate).unwrap();

        assert_eq!(plan.previous_selections(), old_selections());
        assert!(plan
            .candidate_selections()
            .contains(&("chat".into(), "chat_1.conf".into())));
        assert!(plan
            .candidate_targets()
            .contains(&"shared.video.example".to_owned()));

        let candidate_registry = registry(&records, plan.candidate_selections());
        plan.verify_candidate_registry(&candidate_registry).unwrap();

        let mut changed = records.clone();
        changed[1].config_content.push(' ');
        let changed = registry(&changed, plan.candidate_selections());
        assert_eq!(
            plan.verify_candidate_registry(&changed),
            Err(RegistryPreflightError::ContentChanged)
        );
    }

    #[test]
    fn confirmation_requires_two_corresponding_targets_and_clean_window() {
        let envelope = confirmation_envelope();
        let mut evaluator = ConfirmationEvaluator::new_armed(
            &envelope,
            ["one.video.example", "two.video.example"],
            10_000,
            arm(),
        )
        .unwrap();

        assert_eq!(
            evaluator.observe_confirmation_flow(
                &confirmation_flow(100, 1, "one.video.example", Diagnosis::Working, 600),
                10_100,
            ),
            ConfirmationDecision::Pending,
            "pre-arm sequence must not confirm the candidate"
        );
        evaluator.observe_probe(probe(1, "one.video.example", 10_100, 10_500));
        evaluator.observe_probe(probe(2, "two.video.example", 10_200, 10_600));
        evaluator.observe_confirmation_flow(
            &confirmation_flow(101, 11, "one.video.example", Diagnosis::Working, 650),
            10_650,
        );
        assert_eq!(
            evaluator.observe_confirmation_flow(
                &confirmation_flow(102, 12, "two.video.example", Diagnosis::Working, 700),
                10_700,
            ),
            ConfirmationDecision::Pending
        );
        assert_eq!(evaluator.poll(15_699), ConfirmationDecision::Pending);
        assert_eq!(evaluator.poll(15_700), ConfirmationDecision::Succeeded);
    }

    #[test]
    fn single_target_needs_two_distinct_probes_and_flows_with_delivery_margin() {
        let envelope = confirmation_envelope();
        let mut evaluator =
            ConfirmationEvaluator::new_armed(&envelope, ["only.video.example"], 10_000, arm())
                .unwrap();
        evaluator.observe_probe(probe(1, "only.video.example", 10_100, 10_300));
        evaluator.observe_probe(probe(2, "only.video.example", 10_400, 10_600));
        evaluator.observe_confirmation_flow(
            &confirmation_flow(101, 1, "only.video.example", Diagnosis::Working, 700),
            10_700,
        );
        // Sensor delivery occurs after the HTTP future, but inside the bounded
        // delivery margin and remains corresponding to the second probe.
        evaluator.observe_confirmation_flow(
            &confirmation_flow(102, 2, "only.video.example", Diagnosis::Working, 1_900),
            11_900,
        );
        assert_eq!(evaluator.poll(16_900), ConfirmationDecision::Succeeded);
    }

    #[test]
    fn confirmation_failure_classification_preserves_non_strategy_cases() {
        let envelope = confirmation_envelope();
        let mut adverse =
            ConfirmationEvaluator::new_armed(&envelope, ["one.video.example"], 10_000, arm())
                .unwrap();
        assert_eq!(
            adverse.observe_confirmation_flow(
                &confirmation_flow(101, 1, "one.video.example", Diagnosis::TcpReset, 600),
                10_100,
            ),
            ConfirmationDecision::Failed(ConfirmationFailure::Strategy)
        );

        let mut sensor =
            ConfirmationEvaluator::new_armed(&envelope, ["one.video.example"], 10_000, arm())
                .unwrap();
        let gap = EyeEvent::Gap(GapEvent {
            envelope: EventEnvelope::new(
                envelope.session_id,
                envelope.expected_sensor_generation,
                envelope.expected_registry_version,
            ),
            from_ts: 550,
            to_ts: 600,
            dropped_events: 1,
        });
        assert_eq!(
            sensor.observe_eye(&gap, 10_100),
            ConfirmationDecision::Failed(ConfirmationFailure::Sensor)
        );

        let mut missing =
            ConfirmationEvaluator::new_armed(&envelope, ["one.video.example"], 10_000, arm())
                .unwrap();
        assert_eq!(
            missing.poll(30_000),
            ConfirmationDecision::Failed(ConfirmationFailure::MissingWorkingEvidence)
        );
        assert!(!ConfirmationFailure::Sensor.penalizes_candidate());
        assert!(ConfirmationFailure::Strategy.penalizes_candidate());
    }

    #[test]
    fn cache_network_fence_requires_healthy_exact_stable_identity() {
        let expected = network();
        let healthy = crate::netid::LocalNetworkIdentity {
            online: true,
            interface_up: true,
            default_route_available: true,
            gateway_reachable: true,
            fingerprint: expected.clone(),
        };
        assert!(cache_network_fence_matches(&healthy, &expected));

        let mut offline = healthy.clone();
        offline.online = false;
        assert!(!cache_network_fence_matches(&offline, &expected));

        let mut changed = healthy;
        changed.fingerprint = NetworkFingerprint::Stable {
            key: "other-network".into(),
        };
        assert!(!cache_network_fence_matches(&changed, &expected));
        assert!(!cache_network_fence_matches(
            &changed,
            &NetworkFingerprint::Unstable {
                reason: "incomplete".into()
            }
        ));
    }

    #[derive(Clone, Debug)]
    struct ExecutorFixture {
        records: Vec<LegacyConfigRecord>,
        registry: Arc<TargetRegistry>,
        fence: IntentFence,
        previous: RecoveryConfig,
        candidate: RecoveryConfig,
        previous_owner: ProcessOwner,
        neighbor_owner: ProcessOwner,
    }

    impl ExecutorFixture {
        fn new() -> Self {
            let records = records();
            let registry = registry(&records, old_selections());
            let previous = recovery_config(&registry, "video", "video_1.conf");
            let candidate = recovery_config(&registry, "video", "video_2.conf");
            let neighbor = recovery_config(&registry, "chat", "chat_1.conf");
            let previous_owner = process_owner(41, 401, previous.fingerprint(), 7);
            let neighbor_owner = process_owner(42, 402, neighbor.fingerprint(), 4);
            Self {
                records,
                fence: IntentFence {
                    session_id: SessionId::new(11),
                    category: "video".into(),
                    lane_generation: LaneGeneration::new(7),
                    sensor_generation: SensorGeneration::new(3),
                    registry_version: registry.version(),
                    network_fingerprint: network(),
                },
                registry,
                previous,
                candidate,
                previous_owner,
                neighbor_owner,
            }
        }

        fn backend(&self) -> FakeBackend {
            FakeBackend {
                now_ms: 1,
                records: self.records.clone(),
                registry: Arc::clone(&self.registry),
                fence: self.fence.clone(),
                lanes: BTreeMap::from([
                    ("video".into(), LaneGeneration::new(7)),
                    ("chat".into(), LaneGeneration::new(4)),
                ]),
                processes: BTreeMap::from([
                    ("video".into(), self.previous_owner.clone()),
                    ("chat".into(), self.neighbor_owner.clone()),
                ]),
                next_pid: 100,
                gate_classification: GateClassification::DpiSuspected,
                fail_isolation_preflight_for: None,
                confirmation: ConfirmationDecision::Succeeded,
                change_network_on_confirmation: false,
                crash_candidate_on_confirmation: false,
                retired_candidate: None,
                stop_result: StopLaneResult::Stopped,
                reload_count: 0,
                tamper_from_reload: None,
                persisted: None,
                commit_failure: None,
                automatic_authorization_calls: 0,
                deny_automatic_authorization_at: None,
                authorized_control_generation: None,
                revoke_automatic_after_stop: false,
                automatic_authorization_revoked: false,
                calls: Vec::new(),
            }
        }

        fn coordinator(&self) -> (RecoveryCoordinator, RecoveryAction) {
            let mut coordinator = RecoveryCoordinator::new(RecoveryMode::Assisted);
            let decision = coordinator
                .consider(
                    RecoveryRequest::new(
                        IncidentId::new(1),
                        self.fence.clone(),
                        self.previous.clone(),
                        self.previous_owner.clone(),
                        vec![self.candidate.clone()],
                    ),
                    1,
                )
                .unwrap();
            assert!(matches!(decision, RecoveryDecision::Assisted { .. }));
            let approval = coordinator.proposal().unwrap().approval();
            let action = coordinator.approve(approval, &self.fence, 2).unwrap();
            (coordinator, action)
        }

        fn automatic_coordinator(
            &self,
            control_generation: u64,
        ) -> (RecoveryCoordinator, RecoveryAction) {
            let mut coordinator = RecoveryCoordinator::new(RecoveryMode::Automatic);
            let decision = coordinator
                .consider(
                    RecoveryRequest::new_with_control_generation(
                        IncidentId::new(1),
                        self.fence.clone(),
                        self.previous.clone(),
                        self.previous_owner.clone(),
                        vec![self.candidate.clone()],
                        control_generation,
                    ),
                    1,
                )
                .unwrap();
            let RecoveryDecision::Automatic { action } = decision else {
                panic!("automatic fixture must arm a direct preflight");
            };
            (coordinator, action)
        }
    }

    struct FakeBackend {
        now_ms: u64,
        records: Vec<LegacyConfigRecord>,
        registry: Arc<TargetRegistry>,
        fence: IntentFence,
        lanes: BTreeMap<String, LaneGeneration>,
        processes: BTreeMap<String, ProcessOwner>,
        next_pid: u32,
        gate_classification: GateClassification,
        fail_isolation_preflight_for: Option<String>,
        confirmation: ConfirmationDecision,
        change_network_on_confirmation: bool,
        crash_candidate_on_confirmation: bool,
        retired_candidate: Option<ProcessOwner>,
        stop_result: StopLaneResult,
        reload_count: usize,
        tamper_from_reload: Option<usize>,
        persisted: Option<Vec<(String, String)>>,
        commit_failure: Option<BackendFailure>,
        automatic_authorization_calls: usize,
        deny_automatic_authorization_at: Option<usize>,
        authorized_control_generation: Option<u64>,
        revoke_automatic_after_stop: bool,
        automatic_authorization_revoked: bool,
        calls: Vec<String>,
    }

    impl FakeBackend {
        fn advance(&mut self) {
            self.now_ms += 1;
        }

        fn built_registry(
            &self,
            selections: Vec<(String, String)>,
            tampered: bool,
        ) -> Arc<TargetRegistry> {
            let mut records = self.records.clone();
            if tampered {
                records
                    .iter_mut()
                    .find(|record| record.config_name == "video_2.conf")
                    .unwrap()
                    .config_content
                    .push(' ');
            }
            registry(&records, selections)
        }
    }

    impl ScopedExecutorBackend for FakeBackend {
        fn monotonic_ms(&self) -> u64 {
            self.now_ms
        }

        fn authorize_recovery<'a>(
            &'a mut self,
            origin: RecoveryOrigin,
            _category: String,
        ) -> BackendFuture<'a, Result<(), BackendFailure>> {
            let RecoveryOrigin::Automatic { control_generation } = origin else {
                return Box::pin(async { Ok(()) });
            };
            self.automatic_authorization_calls += 1;
            let call = self.automatic_authorization_calls;
            self.calls.push(format!("authorize:{call}"));
            let denied = self.automatic_authorization_revoked
                || self
                    .deny_automatic_authorization_at
                    .is_some_and(|denied_at| denied_at == call)
                || self
                    .authorized_control_generation
                    .is_some_and(|current| current != control_generation);
            Box::pin(async move {
                if denied {
                    Err(BackendFailure::new(
                        ConfirmationFailure::Sensor,
                        "automatic authorization was revoked",
                    ))
                } else {
                    Ok(())
                }
            })
        }

        fn current_fence<'a>(
            &'a mut self,
            _category: String,
        ) -> BackendFuture<'a, Result<IntentFence, BackendFailure>> {
            let fence = self.fence.clone();
            Box::pin(async move { Ok(fence) })
        }

        fn current_registry(&self) -> Result<Arc<TargetRegistry>, BackendFailure> {
            Ok(Arc::clone(&self.registry))
        }

        fn lane_generations(&self) -> Result<BTreeMap<String, LaneGeneration>, BackendFailure> {
            Ok(self.lanes.clone())
        }

        fn exact_process_owners(&self) -> Result<BTreeMap<String, ProcessOwner>, BackendFailure> {
            Ok(self.processes.clone())
        }

        fn owns_exact_process(&self, category: &str, owner: &ProcessOwner) -> bool {
            self.processes.get(category) == Some(owner)
        }

        fn exact_process_exit_observed<'a>(
            &'a mut self,
            _category: &'a str,
            owner: &'a ProcessOwner,
        ) -> BackendFuture<'a, Result<bool, BackendFailure>> {
            let observed = self.retired_candidate.as_ref() == Some(owner);
            Box::pin(async move { Ok(observed) })
        }

        fn reload_registry<'a>(
            &'a mut self,
            selections: Vec<(String, String)>,
        ) -> BackendFuture<'a, Result<Arc<TargetRegistry>, BackendFailure>> {
            self.reload_count += 1;
            let tampered = self
                .tamper_from_reload
                .is_some_and(|from| self.reload_count >= from);
            let registry = self.built_registry(selections, tampered);
            Box::pin(async move { Ok(registry) })
        }

        fn preflight_scoped_launch<'a>(
            &'a mut self,
            _selections: Vec<(String, String)>,
            category: String,
            config_file: String,
        ) -> BackendFuture<'a, Result<(), BackendFailure>> {
            self.calls
                .push(format!("isolation:{category}/{config_file}"));
            let failure = (self.fail_isolation_preflight_for.as_deref()
                == Some(config_file.as_str()))
            .then(|| {
                BackendFailure::new(
                    ConfirmationFailure::Sensor,
                    format!("simulated isolation preflight failure for {config_file}"),
                )
            });
            Box::pin(async move { failure.map_or(Ok(()), Err) })
        }

        fn fresh_environment_gate<'a>(
            &'a mut self,
            envelope: &'a IntentEnvelope,
        ) -> BackendFuture<'a, Result<GateReport, BackendFailure>> {
            self.calls.push("gate".into());
            let now = self.now_ms;
            let gate = gate_fence(envelope);
            let classification = self.gate_classification;
            Box::pin(async move {
                Ok(GateReport {
                    fence: gate,
                    category: envelope.category.clone(),
                    classification,
                    controls: Vec::new(),
                    category_targets: Vec::new(),
                    baseline_latency_ms: None,
                    slow_threshold_ms: None,
                    generated_at_monotonic_ms: now,
                    valid_until_monotonic_ms: now + 10_000,
                })
            })
        }

        fn replace_observer<'a>(
            &'a mut self,
            plan: ObserverReplacementPlan,
        ) -> BackendFuture<'a, Result<IntentFence, BackendFailure>> {
            let target_matches = match &plan.target_state {
                ObserverTargetState::Present(owner) => {
                    self.processes.get(&plan.category) == Some(owner)
                }
                ObserverTargetState::Absent => !self.processes.contains_key(&plan.category),
            };
            if !target_matches {
                return Box::pin(async {
                    Err(BackendFailure::new(
                        ConfirmationFailure::Sensor,
                        "fake observer target expectation failed",
                    ))
                });
            }
            let candidate = plan
                .registry
                .active_config(&plan.category)
                .unwrap_or_default()
                .to_owned();
            self.calls.push(format!("observer:{candidate}"));
            self.registry = plan.registry;
            self.lanes = plan.lane_generations;
            self.fence.sensor_generation = next_sensor(self.fence.sensor_generation);
            self.fence.registry_version = self.registry.version();
            self.fence.lane_generation = self.lanes[&plan.category];
            let mut fence = self.fence.clone();
            // Production observer snapshots retain the session's start
            // fingerprint; current_fence independently overlays the live one.
            fence.network_fingerprint = plan.network_fingerprint;
            Box::pin(async move { Ok(fence) })
        }

        fn stop_lane<'a>(
            &'a mut self,
            category: &'a str,
            owner: &'a ProcessOwner,
        ) -> BackendFuture<'a, Result<StopLaneResult, BackendFailure>> {
            self.calls.push(format!("stop:{category}"));
            let result = self.stop_result;
            if result == StopLaneResult::Stopped && self.processes.get(category) == Some(owner) {
                self.processes.remove(category);
            }
            if self.revoke_automatic_after_stop {
                self.automatic_authorization_revoked = true;
            }
            Box::pin(async move { Ok(result) })
        }

        fn start_lane<'a>(
            &'a mut self,
            request: StartLaneRequest,
        ) -> BackendFuture<'a, Result<StartedLane, StartLaneError>> {
            let label = match request.install_kind {
                LaneInstallKind::Candidate => "candidate",
                LaneInstallKind::Rollback => "rollback",
                LaneInstallKind::CrashRetry => "crash-retry",
            };
            self.calls.push(format!("start:{label}"));
            assert_eq!(self.processes, request.neighbor_owners);
            self.next_pid += 1;
            let owner = process_owner(
                self.next_pid,
                u64::from(self.next_pid) + 1_000,
                request.config.fingerprint(),
                request.lane_generation.get(),
            );
            self.processes
                .insert(request.envelope.category.clone(), owner.clone());
            let arm = ConfirmationArm {
                armed_at_monotonic_ms: self.now_ms,
                armed_at_sensor_ms: self.now_ms,
                after_flow_sequence: 0,
                last_gap_sequence: None,
                evidence_epoch: 1,
                health: EyeHealthState::Ready,
                counters: EyeHealthCounters::default(),
            };
            Box::pin(async move {
                Ok(StartedLane {
                    owner,
                    confirmation_arm: arm,
                })
            })
        }

        fn confirm_candidate<'a>(
            &'a mut self,
            request: ConfirmationRequest,
        ) -> BackendFuture<'a, Result<ConfirmationDecision, BackendFailure>> {
            self.calls.push("confirm".into());
            if self.change_network_on_confirmation {
                self.fence.network_fingerprint = NetworkFingerprint::Stable {
                    key: "changed-test-network".into(),
                };
            }
            if self.crash_candidate_on_confirmation
                && self.processes.get(&request.envelope.category) == Some(&request.owner)
            {
                self.retired_candidate = self.processes.remove(&request.envelope.category);
            }
            let decision = self.confirmation;
            Box::pin(async move { Ok(decision) })
        }

        fn commit_candidate<'a>(
            &'a mut self,
            request: CommitRequest,
        ) -> BackendFuture<'a, Result<(), BackendFailure>> {
            self.calls.push("commit".into());
            let mut neighbors = self.processes.clone();
            neighbors.remove(&request.envelope.category);
            assert_eq!(neighbors, request.neighbor_owners);
            if let Some(failure) = self.commit_failure.clone() {
                return Box::pin(async move { Err(failure) });
            }
            self.persisted = Some(request.active_selections);
            Box::pin(async { Ok(()) })
        }
    }

    fn next_sensor(sensor: SensorGeneration) -> SensorGeneration {
        SensorGeneration::new(sensor.get() + 1)
    }

    async fn drive(
        executor: &mut ScopedExecutor<FakeBackend>,
        coordinator: &mut RecoveryCoordinator,
        mut action: RecoveryAction,
    ) -> RecoveryAction {
        for _ in 0..12 {
            executor.backend_mut().advance();
            let result = executor.execute(action).await.unwrap();
            action = coordinator.apply_result(result).unwrap();
            if matches!(
                action,
                RecoveryAction::Complete { .. } | RecoveryAction::ManualIntervention { .. }
            ) {
                return action;
            }
        }
        panic!("recovery did not terminate")
    }

    fn disposition(action: &RecoveryAction) -> RecoveryDisposition {
        match action {
            RecoveryAction::Complete { completion }
            | RecoveryAction::ManualIntervention { completion } => completion.disposition,
            _ => panic!("not terminal"),
        }
    }

    #[tokio::test]
    async fn crash_retry_isolation_preflight_precedes_and_can_block_observer_mutation() {
        let fixture = ExecutorFixture::new();
        let observer_plan = ObserverReplacementPlan {
            session_id: fixture.fence.session_id,
            network_fingerprint: fixture.fence.network_fingerprint.clone(),
            registry: Arc::clone(&fixture.registry),
            lane_generations: BTreeMap::from([
                ("video".into(), LaneGeneration::new(8)),
                ("chat".into(), LaneGeneration::new(4)),
            ]),
            category: "video".into(),
            target_state: ObserverTargetState::Absent,
        };
        let config_file = fixture.previous.config_id().to_owned();

        let mut blocked = fixture.backend();
        blocked.processes.remove("video");
        blocked.fail_isolation_preflight_for = Some(config_file.clone());
        let failure = replace_crash_retry_observer_after_preflight(
            &mut blocked,
            old_selections(),
            config_file.clone(),
            observer_plan.clone(),
        )
        .await
        .unwrap_err();
        assert_eq!(failure.reason, ConfirmationFailure::Sensor);
        assert_eq!(blocked.calls, ["isolation:video/video_1.conf"]);

        let mut backend = fixture.backend();
        backend.processes.remove("video");
        replace_crash_retry_observer_after_preflight(
            &mut backend,
            old_selections(),
            config_file,
            observer_plan,
        )
        .await
        .unwrap();
        assert_eq!(
            backend.calls,
            ["isolation:video/video_1.conf", "observer:video_1.conf"]
        );
    }

    #[tokio::test]
    async fn scoped_isolation_failure_preserves_previous_lane_before_any_mutation() {
        let fixture = ExecutorFixture::new();
        let (mut coordinator, action) = fixture.coordinator();
        let mut backend = fixture.backend();
        backend.fail_isolation_preflight_for = Some(fixture.candidate.config_id().to_owned());
        let mut executor = ScopedExecutor::new(backend);

        let terminal = drive(&mut executor, &mut coordinator, action).await;

        assert_eq!(
            disposition(&terminal),
            RecoveryDisposition::PreviousPreserved
        );
        assert_eq!(
            executor.backend().processes.get("video"),
            Some(&fixture.previous_owner)
        );
        assert!(executor
            .backend()
            .calls
            .iter()
            .any(|call| call == "isolation:video/video_2.conf"));
        assert!(!executor.backend().calls.iter().any(|call| {
            call == "gate" || call.starts_with("observer:") || call.starts_with("stop:")
        }));
    }

    #[test]
    fn unexpected_candidate_exit_is_persisted_as_readiness_not_strategy() {
        let fixture = ExecutorFixture::new();
        let envelope = IntentEnvelope::from_fence(AttemptId::new(91), &fixture.fence);
        let candidate_owner = process_owner(99, 999, fixture.candidate.fingerprint(), 8);
        let action = RecoveryAction::ConfirmCandidate {
            envelope: envelope.clone(),
            candidate: fixture.candidate.clone(),
            owner: candidate_owner.clone(),
        };
        let failure = candidate_cache_failure(
            &action,
            &ExecutorResult {
                envelope,
                outcome: ExecutorOutcome::Exited {
                    process: candidate_owner,
                    intentional: false,
                },
                completed_at_monotonic_ms: 10,
            },
        )
        .unwrap();
        assert_eq!(failure.kind, super::super::cache::FailureKind::Readiness);
        assert_eq!(failure.reason, "unexpected_exit");
    }

    #[tokio::test]
    async fn stable_preflight_preserves_the_lane_before_observer_or_stop() {
        let fixture = ExecutorFixture::new();
        let (mut coordinator, action) = fixture.coordinator();
        let mut backend = fixture.backend();
        backend.gate_classification = GateClassification::Stable;
        let mut executor = ScopedExecutor::new(backend);

        let terminal = drive(&mut executor, &mut coordinator, action).await;
        assert_eq!(
            disposition(&terminal),
            RecoveryDisposition::PreviousPreserved
        );
        assert_eq!(
            executor.backend().processes.get("video"),
            Some(&fixture.previous_owner)
        );
        assert!(executor.backend().calls.iter().any(|call| call == "gate"));
        assert!(!executor.backend().calls.iter().any(|call| {
            call.starts_with("observer:") || call.starts_with("stop:") || call.starts_with("start:")
        }));
        assert_eq!(coordinator.status().negative_cooldown_count, 0);
    }

    #[tokio::test]
    async fn automatic_revoke_before_observer_mutation_preserves_every_process() {
        let fixture = ExecutorFixture::new();
        let (mut coordinator, action) = fixture.automatic_coordinator(9);
        let mut backend = fixture.backend();
        backend.deny_automatic_authorization_at = Some(2);
        let mut executor = ScopedExecutor::new(backend);

        let terminal = drive(&mut executor, &mut coordinator, action).await;
        assert_eq!(
            disposition(&terminal),
            RecoveryDisposition::PreviousPreserved
        );
        assert_eq!(
            executor.backend().processes.get("video"),
            Some(&fixture.previous_owner)
        );
        assert_eq!(
            executor.backend().processes.get("chat"),
            Some(&fixture.neighbor_owner)
        );
        assert!(!executor
            .backend()
            .calls
            .iter()
            .any(|call| call.starts_with("observer:") || call.starts_with("stop:")));
        assert_eq!(coordinator.status().negative_cooldown_count, 0);
    }

    #[tokio::test]
    async fn automatic_revoke_after_observer_replacement_restores_before_stop() {
        let fixture = ExecutorFixture::new();
        let (mut coordinator, action) = fixture.automatic_coordinator(9);
        let mut backend = fixture.backend();
        backend.deny_automatic_authorization_at = Some(3);
        let mut executor = ScopedExecutor::new(backend);

        let terminal = drive(&mut executor, &mut coordinator, action).await;
        assert_eq!(
            disposition(&terminal),
            RecoveryDisposition::PreviousPreserved
        );
        assert_eq!(
            executor.backend().processes.get("video"),
            Some(&fixture.previous_owner)
        );
        assert_eq!(
            executor.backend().registry.active_config("video"),
            Some("video_1.conf")
        );
        assert!(executor
            .backend()
            .calls
            .iter()
            .any(|call| call == "observer:video_2.conf"));
        assert!(executor
            .backend()
            .calls
            .iter()
            .any(|call| call == "observer:video_1.conf"));
        assert!(!executor
            .backend()
            .calls
            .iter()
            .any(|call| call == "stop:video"));
        assert_eq!(coordinator.status().negative_cooldown_count, 0);
    }

    #[tokio::test]
    async fn automatic_revoke_after_stop_boundary_finishes_the_safe_transaction() {
        let fixture = ExecutorFixture::new();
        let (mut coordinator, action) = fixture.automatic_coordinator(9);
        let mut backend = fixture.backend();
        backend.revoke_automatic_after_stop = true;
        let mut executor = ScopedExecutor::new(backend);

        let terminal = drive(&mut executor, &mut coordinator, action).await;
        assert_eq!(
            disposition(&terminal),
            RecoveryDisposition::CandidateApplied
        );
        assert!(executor.backend().automatic_authorization_revoked);
        assert_eq!(executor.backend().automatic_authorization_calls, 3);
        assert!(executor
            .backend()
            .calls
            .iter()
            .any(|call| call == "start:candidate"));
        assert_eq!(
            executor.backend().processes.get("chat"),
            Some(&fixture.neighbor_owner)
        );
    }

    #[tokio::test]
    async fn stale_automatic_control_generation_never_reaches_observer_or_stop() {
        let fixture = ExecutorFixture::new();
        let (mut coordinator, action) = fixture.automatic_coordinator(7);
        let mut backend = fixture.backend();
        // Models kill -> resume: current controls are authorized again, but on
        // a newer generation than the queued action.
        backend.authorized_control_generation = Some(9);
        let mut executor = ScopedExecutor::new(backend);

        let terminal = drive(&mut executor, &mut coordinator, action).await;
        assert_eq!(
            disposition(&terminal),
            RecoveryDisposition::PreviousPreserved
        );
        assert_eq!(executor.backend().automatic_authorization_calls, 1);
        assert!(!executor
            .backend()
            .calls
            .iter()
            .any(|call| call.starts_with("observer:") || call.starts_with("stop:")));
    }

    #[tokio::test]
    async fn coordinator_and_executor_apply_one_lane_without_touching_neighbor() {
        let fixture = ExecutorFixture::new();
        let (mut coordinator, action) = fixture.coordinator();
        let mut executor = ScopedExecutor::new(fixture.backend());

        let terminal = drive(&mut executor, &mut coordinator, action).await;
        assert_eq!(
            disposition(&terminal),
            RecoveryDisposition::CandidateApplied
        );
        assert_eq!(
            executor.backend().processes.get("chat"),
            Some(&fixture.neighbor_owner)
        );
        assert_eq!(
            executor.backend().persisted.as_ref().unwrap(),
            &vec![
                ("chat".into(), "chat_1.conf".into()),
                ("video".into(), "video_2.conf".into())
            ]
        );
        let calls = &executor.backend().calls;
        let first_gate = calls.iter().position(|call| call == "gate").unwrap();
        let observer = calls
            .iter()
            .position(|call| call == "observer:video_2.conf")
            .unwrap();
        let second_gate = calls
            .iter()
            .enumerate()
            .find(|(index, call)| *index > observer && *call == "gate")
            .map(|(index, _)| index)
            .unwrap();
        let stop = calls.iter().position(|call| call == "stop:video").unwrap();
        assert!(first_gate < observer && observer < second_gate && second_gate < stop);
    }

    #[tokio::test]
    async fn sensor_confirmation_failure_rolls_back_without_negative_cooldown() {
        let fixture = ExecutorFixture::new();
        let (mut coordinator, action) = fixture.coordinator();
        let mut backend = fixture.backend();
        backend.confirmation = ConfirmationDecision::Failed(ConfirmationFailure::Sensor);
        let mut executor = ScopedExecutor::new(backend);

        let terminal = drive(&mut executor, &mut coordinator, action).await;
        assert_eq!(disposition(&terminal), RecoveryDisposition::RolledBack);
        assert!(executor.backend().persisted.is_none());
        assert_eq!(
            executor.backend().processes["video"].config_fingerprint,
            *fixture.previous.fingerprint()
        );
        assert_eq!(
            coordinator
                .cooldown_until(
                    &network(),
                    "video",
                    fixture.candidate.fingerprint(),
                    executor.backend().now_ms,
                )
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn network_change_during_confirmation_still_restores_exact_previous() {
        let fixture = ExecutorFixture::new();
        let (mut coordinator, action) = fixture.coordinator();
        let mut backend = fixture.backend();
        backend.confirmation = ConfirmationDecision::Failed(ConfirmationFailure::Environment);
        backend.change_network_on_confirmation = true;
        let mut executor = ScopedExecutor::new(backend);

        let terminal = drive(&mut executor, &mut coordinator, action).await;
        assert_eq!(disposition(&terminal), RecoveryDisposition::RolledBack);
        assert_eq!(
            executor.backend().processes["video"].config_fingerprint,
            *fixture.previous.fingerprint()
        );
        assert!(executor.backend().persisted.is_none());
        assert_eq!(
            executor.backend().fence.network_fingerprint,
            NetworkFingerprint::Stable {
                key: "changed-test-network".into()
            }
        );
    }

    #[tokio::test]
    async fn exact_candidate_crash_during_confirmation_still_rolls_back() {
        let fixture = ExecutorFixture::new();
        let (mut coordinator, action) = fixture.coordinator();
        let mut backend = fixture.backend();
        backend.confirmation = ConfirmationDecision::Failed(ConfirmationFailure::Strategy);
        backend.crash_candidate_on_confirmation = true;
        let mut executor = ScopedExecutor::new(backend);

        let terminal = drive(&mut executor, &mut coordinator, action).await;
        assert_eq!(disposition(&terminal), RecoveryDisposition::RolledBack);
        assert_eq!(
            executor.backend().processes["video"].config_fingerprint,
            *fixture.previous.fingerprint()
        );
        assert_eq!(
            executor.backend().processes.get("chat"),
            Some(&fixture.neighbor_owner)
        );
    }

    #[tokio::test]
    async fn unrecoverable_settings_restore_is_manual_even_after_process_rollback() {
        let fixture = ExecutorFixture::new();
        let (mut coordinator, action) = fixture.coordinator();
        let mut backend = fixture.backend();
        backend.commit_failure = Some(BackendFailure::manual(
            ConfirmationFailure::Environment,
            "settings restore failed",
        ));
        let mut executor = ScopedExecutor::new(backend);

        let terminal = drive(&mut executor, &mut coordinator, action).await;
        assert_eq!(disposition(&terminal), RecoveryDisposition::ProcessFailed);
        assert!(matches!(
            terminal,
            RecoveryAction::ManualIntervention { .. }
        ));
        assert_eq!(
            executor.backend().processes["video"].config_fingerprint,
            *fixture.previous.fingerprint()
        );
    }

    #[tokio::test]
    async fn strategy_failure_rolls_back_and_penalizes_only_candidate() {
        let fixture = ExecutorFixture::new();
        let (mut coordinator, action) = fixture.coordinator();
        let mut backend = fixture.backend();
        backend.confirmation = ConfirmationDecision::Failed(ConfirmationFailure::Strategy);
        let mut executor = ScopedExecutor::new(backend);

        let terminal = drive(&mut executor, &mut coordinator, action).await;
        assert_eq!(disposition(&terminal), RecoveryDisposition::RolledBack);
        assert!(coordinator
            .cooldown_until(
                &network(),
                "video",
                fixture.candidate.fingerprint(),
                executor.backend().now_ms,
            )
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn stop_timeout_restores_previous_observer_and_never_starts_candidate() {
        let fixture = ExecutorFixture::new();
        let (mut coordinator, action) = fixture.coordinator();
        let mut backend = fixture.backend();
        backend.stop_result = StopLaneResult::TimedOutPreserved;
        let mut executor = ScopedExecutor::new(backend);

        let terminal = drive(&mut executor, &mut coordinator, action).await;
        assert_eq!(
            disposition(&terminal),
            RecoveryDisposition::PreviousPreserved
        );
        assert_eq!(
            executor.backend().processes["video"],
            fixture.previous_owner
        );
        assert_eq!(
            executor.backend().registry.active_config("video"),
            Some("video_1.conf")
        );
        assert!(!executor
            .backend()
            .calls
            .iter()
            .any(|call| call.starts_with("start:")));
    }

    #[tokio::test]
    async fn candidate_content_change_before_commit_still_exactly_rolls_back_previous() {
        let fixture = ExecutorFixture::new();
        let (mut coordinator, action) = fixture.coordinator();
        let mut backend = fixture.backend();
        // Preflight, refreshed preflight, stop, start, and confirmation each
        // re-read immutable content. The sixth read occurs at commit.
        backend.tamper_from_reload = Some(6);
        let mut executor = ScopedExecutor::new(backend);

        let terminal = drive(&mut executor, &mut coordinator, action).await;
        assert_eq!(disposition(&terminal), RecoveryDisposition::RolledBack);
        assert_eq!(
            executor.backend().processes["video"].config_fingerprint,
            *fixture.previous.fingerprint()
        );
        assert!(executor.backend().persisted.is_none());
    }

    #[tokio::test]
    async fn missing_previous_process_is_manual_not_previous_preserved() {
        let fixture = ExecutorFixture::new();
        let (mut coordinator, action) = fixture.coordinator();
        let mut backend = fixture.backend();
        backend.processes.remove("video");
        let mut executor = ScopedExecutor::new(backend);

        let terminal = drive(&mut executor, &mut coordinator, action).await;
        assert_eq!(disposition(&terminal), RecoveryDisposition::ProcessFailed);
        assert!(matches!(
            terminal,
            RecoveryAction::ManualIntervention { .. }
        ));
    }

    #[tokio::test]
    async fn previous_crash_after_observer_swap_is_manual_not_preserved() {
        let fixture = ExecutorFixture::new();
        let (mut coordinator, action) = fixture.coordinator();
        let mut executor = ScopedExecutor::new(fixture.backend());

        executor.backend_mut().advance();
        let preflight = executor.execute(action).await.unwrap();
        let stop_action = coordinator.apply_result(preflight).unwrap();
        executor.backend_mut().processes.remove("video");
        executor.backend_mut().advance();
        let missing = executor.execute(stop_action).await.unwrap();
        assert!(matches!(
            missing.outcome,
            ExecutorOutcome::PreviousProcessMissing { .. }
        ));
        let terminal = coordinator.apply_result(missing).unwrap();
        assert_eq!(disposition(&terminal), RecoveryDisposition::ProcessFailed);
        assert!(matches!(
            terminal,
            RecoveryAction::ManualIntervention { .. }
        ));
    }

    #[tokio::test]
    async fn expired_second_gate_is_refreshed_immediately_before_stop() {
        let fixture = ExecutorFixture::new();
        let (mut coordinator, action) = fixture.coordinator();
        let mut executor = ScopedExecutor::new(fixture.backend());

        executor.backend_mut().advance();
        let preflight = executor.execute(action).await.unwrap();
        let stop_action = coordinator.apply_result(preflight).unwrap();
        assert_eq!(
            executor
                .backend()
                .calls
                .iter()
                .filter(|call| call.as_str() == "gate")
                .count(),
            2
        );
        executor.backend_mut().now_ms += 20_000;
        let stopped = executor.execute(stop_action).await.unwrap();
        assert!(matches!(stopped.outcome, ExecutorOutcome::Stopped { .. }));
        assert_eq!(
            executor
                .backend()
                .calls
                .iter()
                .filter(|call| call.as_str() == "gate")
                .count(),
            3
        );
    }

    #[test]
    fn label_boundary_matching_rejects_suffix_lookalikes() {
        assert!(domain_matches_suffix(
            "www.one.video.example",
            "one.video.example"
        ));
        assert!(!domain_matches_suffix(
            "notone.video.example",
            "one.video.example"
        ));
        let _ = IpAddr::V4(Ipv4Addr::LOCALHOST);
    }
}
