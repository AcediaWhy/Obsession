//! Pure fail-closed confirmation policy for a tentative Legacy runtime.
//!
//! The service arms this state only after candidate readiness. It accepts
//! post-arm observer journal entries and bounded HTTPS probe observations, but
//! owns no socket, process or observer handle itself. Production I/O remains
//! outside this module so every timing, fencing and quorum path is deterministic
//! in adverse tests.

#![cfg(windows)]
#![allow(dead_code)]

use std::collections::BTreeSet;

use obsession_runtime_reliability::eyes::flow::{
    ARMED_SILENCE_TIMEOUT_MS, BLACKHOLE_DELIVERY_GRACE_MS,
};
use obsession_runtime_reliability::eyes::Diagnosis;
use obsession_runtime_reliability::legacy_reliability::contracts::{
    ConfirmationFailure, EyeHealthCounters, EyeHealthState, IntentEnvelope,
};
use obsession_runtime_reliability::legacy_reliability::environment_gate::{
    BLACKHOLE_REQUIRED_FLOWS, BLACKHOLE_REQUIRED_TARGETS, RESET_REQUIRED_FLOWS,
    RESET_REQUIRED_TARGETS,
};
use obsession_runtime_reliability::legacy_reliability::manager::{
    ConfirmationFlow, ObserveOnlySnapshot,
};
use obsession_runtime_reliability::legacy_reliability::target_registry::normalize_domain as normalize_registry_domain;

pub(crate) const LEGACY_CONFIRMATION_DEADLINE_MS: u64 = 20_000;
pub(crate) const LEGACY_CONFIRMATION_DELIVERY_GRACE_MS: u64 = BLACKHOLE_DELIVERY_GRACE_MS;
const TRACKER_SETTLE_MS: u64 = 500;
const WEAK_ADVERSE_SETTLE_MS: u64 = LEGACY_CONFIRMATION_DELIVERY_GRACE_MS + TRACKER_SETTLE_MS;
const CLEAN_WINDOW_MS: u64 =
    ARMED_SILENCE_TIMEOUT_MS + LEGACY_CONFIRMATION_DELIVERY_GRACE_MS + TRACKER_SETTLE_MS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LegacyConfirmationBuildError {
    InvalidArm,
    NoExclusiveTargets,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LegacyConfirmationDecision {
    Pending,
    Succeeded,
    Failed(ConfirmationFailure),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LegacyHttpsProbeResult {
    /// Any real HTTP response proves that TLS transport reached the target.
    /// The status code describes application health, not bypass transport.
    HttpResponse {
        status: u16,
    },
    EnvironmentFailure,
    TargetFailure,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LegacyHttpsProbeObservation {
    pub(crate) probe_id: u64,
    pub(crate) domain: String,
    pub(crate) started_at_monotonic_ms: u64,
    pub(crate) finished_at_monotonic_ms: u64,
    pub(crate) result: LegacyHttpsProbeResult,
}

/// Immutable cursor captured immediately after exact candidate readiness.
/// Previous-runtime evidence cannot cross this cursor and confirm a candidate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LegacyConfirmationArm {
    armed_at_monotonic_ms: u64,
    armed_at_sensor_ms: u64,
    after_capture_timestamp: i64,
    after_flow_sequence: u64,
    last_gap_sequence: Option<u64>,
    evidence_epoch: u64,
    counters: EyeHealthCounters,
}

impl LegacyConfirmationArm {
    pub(crate) fn capture(
        snapshot: &ObserveOnlySnapshot,
        envelope: &IntentEnvelope,
        armed_at_monotonic_ms: u64,
    ) -> Result<Self, LegacyConfirmationBuildError> {
        let lane = validate_snapshot_fence(snapshot, envelope)
            .ok_or(LegacyConfirmationBuildError::InvalidArm)?;
        if snapshot.health.state != EyeHealthState::Ready || snapshot.health.receiver_failed {
            return Err(LegacyConfirmationBuildError::InvalidArm);
        }
        Ok(Self {
            armed_at_monotonic_ms,
            armed_at_sensor_ms: snapshot.logical_now_ms,
            after_capture_timestamp: snapshot.logical_now_ms.min(i64::MAX as u64) as i64,
            after_flow_sequence: snapshot.last_accepted_flow_sequence.unwrap_or(0),
            last_gap_sequence: snapshot.last_gap_sequence,
            evidence_epoch: lane.evidence_epoch,
            counters: snapshot.health.counters,
        })
    }

    pub(crate) const fn deadline_at_monotonic_ms(self) -> u64 {
        self.armed_at_monotonic_ms
            .saturating_add(LEGACY_CONFIRMATION_DEADLINE_MS)
    }

    pub(crate) const fn delivery_deadline_at_monotonic_ms(self) -> u64 {
        self.deadline_at_monotonic_ms()
            .saturating_add(LEGACY_CONFIRMATION_DELIVERY_GRACE_MS)
    }

    fn validates(self, snapshot: &ObserveOnlySnapshot, envelope: &IntentEnvelope) -> bool {
        let Some(lane) = validate_snapshot_fence(snapshot, envelope) else {
            return false;
        };
        snapshot.health.state == EyeHealthState::Ready
            && !snapshot.health.receiver_failed
            && snapshot.health.counters.parse_errors == self.counters.parse_errors
            && snapshot.health.counters.queue_drops == self.counters.queue_drops
            && snapshot.last_gap_sequence == self.last_gap_sequence
            && lane.evidence_epoch == self.evidence_epoch
    }
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
    sequence: u64,
    target: String,
    observed_at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct WeakAdverseFlow {
    sequence: u64,
    target: String,
    observed_at_ms: u64,
}

/// Incremental confirmation state resumed by bounded service transaction ticks.
#[derive(Clone, Debug)]
pub(crate) struct LegacyConfirmationWindow {
    envelope: IntentEnvelope,
    arm: LegacyConfirmationArm,
    targets: Vec<String>,
    flow_cursor: u64,
    seen_flow_ids: BTreeSet<u64>,
    successful_probes: Vec<SuccessfulProbe>,
    failed_target_probes: BTreeSet<u64>,
    working_flows: Vec<WorkingFlow>,
    tls_blackhole_flows: Vec<WeakAdverseFlow>,
    reset_flows: Vec<WeakAdverseFlow>,
    last_weak_adverse_at_ms: Option<u64>,
    quorum_reached_at_ms: Option<u64>,
    failure: Option<ConfirmationFailure>,
}

impl LegacyConfirmationWindow {
    pub(crate) fn new(
        envelope: IntentEnvelope,
        snapshot: &ObserveOnlySnapshot,
        candidate_exclusive_targets: impl IntoIterator<Item = impl AsRef<str>>,
        armed_at_monotonic_ms: u64,
    ) -> Result<Self, LegacyConfirmationBuildError> {
        let arm = LegacyConfirmationArm::capture(snapshot, &envelope, armed_at_monotonic_ms)?;
        let mut targets = candidate_exclusive_targets
            .into_iter()
            .filter_map(|target| normalize_domain(target.as_ref()))
            .collect::<Vec<_>>();
        targets.sort_by(|left, right| right.len().cmp(&left.len()).then_with(|| left.cmp(right)));
        targets.dedup();
        if targets.is_empty() {
            return Err(LegacyConfirmationBuildError::NoExclusiveTargets);
        }
        Ok(Self {
            envelope,
            arm,
            targets,
            flow_cursor: arm.after_flow_sequence,
            seen_flow_ids: BTreeSet::new(),
            successful_probes: Vec::new(),
            failed_target_probes: BTreeSet::new(),
            working_flows: Vec::new(),
            tls_blackhole_flows: Vec::new(),
            reset_flows: Vec::new(),
            last_weak_adverse_at_ms: None,
            quorum_reached_at_ms: None,
            failure: None,
        })
    }

    pub(crate) fn arm(&self) -> LegacyConfirmationArm {
        self.arm
    }

    pub(crate) fn targets(&self) -> &[String] {
        &self.targets
    }

    pub(crate) fn observe_probe(
        &mut self,
        probe: LegacyHttpsProbeObservation,
        now_ms: u64,
    ) -> LegacyConfirmationDecision {
        if self.failure.is_some() {
            return self.decision(now_ms, false);
        }
        if probe.probe_id == 0
            || probe.started_at_monotonic_ms < self.arm.armed_at_monotonic_ms
            || probe.finished_at_monotonic_ms < probe.started_at_monotonic_ms
            || probe.finished_at_monotonic_ms > self.arm.deadline_at_monotonic_ms()
        {
            self.failure = Some(ConfirmationFailure::Sensor);
            return self.decision(now_ms, false);
        }
        let Some(target) = self.target_for_domain(&probe.domain).map(str::to_owned) else {
            self.failure = Some(ConfirmationFailure::Target);
            return self.decision(now_ms, false);
        };
        match probe.result {
            LegacyHttpsProbeResult::HttpResponse { .. } => {
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
            LegacyHttpsProbeResult::EnvironmentFailure => {
                self.failure = Some(ConfirmationFailure::Environment);
            }
            LegacyHttpsProbeResult::TargetFailure => {
                // A protected reqwest probe and Discord's Chromium network
                // stack do not have the same TLS fingerprint. Some valid
                // desync strategies therefore reset the synthetic probe while
                // fresh, post-arm Discord connections work. Keep the failure
                // as bounded negative evidence, but do not let it pre-empt
                // exact Eyes observations from the real client.
                if self.envelope.category == "discord" {
                    self.failed_target_probes.insert(probe.probe_id);
                } else {
                    self.failure = Some(ConfirmationFailure::Target);
                }
            }
        }
        self.refresh_quorum();
        self.decision(now_ms, false)
    }

    pub(crate) fn observe_snapshot(
        &mut self,
        snapshot: &ObserveOnlySnapshot,
        now_ms: u64,
    ) -> LegacyConfirmationDecision {
        if now_ms < self.arm.armed_at_monotonic_ms || !self.arm.validates(snapshot, &self.envelope)
        {
            self.failure = Some(ConfirmationFailure::Sensor);
            return self.decision(now_ms, false);
        }
        if snapshot
            .last_evicted_confirmation_flow_sequences
            .get(&self.envelope.category)
            .is_some_and(|evicted| *evicted > self.flow_cursor)
        {
            self.failure = Some(ConfirmationFailure::Sensor);
            return self.decision(now_ms, false);
        }

        let flow_cursor = self.flow_cursor;
        let mut next_cursor = flow_cursor;
        for flow in snapshot
            .confirmation_flows
            .iter()
            .filter(|flow| flow.sequence > flow_cursor)
        {
            if flow.sequence <= next_cursor {
                self.failure = Some(ConfirmationFailure::Sensor);
                break;
            }
            next_cursor = flow.sequence;
            self.observe_flow(flow);
        }
        self.flow_cursor = next_cursor;
        self.refresh_quorum();

        let pending_ingress = snapshot.pending_ingress_control_events.saturating_add(
            snapshot
                .pending_ingress_flow_events
                .get(&self.envelope.category)
                .copied()
                .unwrap_or(0),
        );
        if pending_ingress != 0 && now_ms >= self.arm.delivery_deadline_at_monotonic_ms() {
            self.failure = Some(ConfirmationFailure::Sensor);
        }
        self.decision(now_ms, pending_ingress != 0)
    }

    fn observe_flow(&mut self, flow: &ConfirmationFlow) {
        if self.failure.is_some()
            || flow.sequence <= self.arm.after_flow_sequence
            || flow.category != self.envelope.category
            || flow.lane_generation != self.envelope.expected_lane_generation
        {
            return;
        }
        let (Some(armed_at_sensor_ms), Some(armed_capture_timestamp)) =
            (flow.armed_at_sensor_ms, flow.armed_at_capture_timestamp)
        else {
            return;
        };
        if armed_capture_timestamp <= self.arm.after_capture_timestamp
            || armed_at_sensor_ms > flow.monotonic_ts
        {
            return;
        }
        let Some(target) = normalize_domain(&flow.target) else {
            return;
        };
        if !self.targets.iter().any(|allowed| allowed == &target) {
            return;
        }
        let observed_at_ms = self.sensor_to_process_time(flow.monotonic_ts);
        if observed_at_ms > self.arm.deadline_at_monotonic_ms()
            || !self.seen_flow_ids.insert(flow.flow_id)
        {
            return;
        }
        match flow.diagnosis {
            Diagnosis::Working => self.working_flows.push(WorkingFlow {
                flow_id: flow.flow_id,
                sequence: flow.sequence,
                target,
                observed_at_ms,
            }),
            Diagnosis::TlsBlackhole => {
                self.record_weak_adverse(flow.sequence, target, observed_at_ms, true)
            }
            Diagnosis::TcpReset => {
                self.record_weak_adverse(flow.sequence, target, observed_at_ms, false)
            }
            Diagnosis::HttpBlockPage => self.failure = Some(ConfirmationFailure::Strategy),
            Diagnosis::DnsFailure | Diagnosis::IpUnreachable => {
                self.failure = Some(ConfirmationFailure::Environment)
            }
            Diagnosis::Throttled => self.failure = Some(ConfirmationFailure::Target),
            Diagnosis::TcpBlackhole
            | Diagnosis::QuicBlocked
            | Diagnosis::UdpBlocked
            | Diagnosis::Unknown => {}
        }
    }

    fn record_weak_adverse(
        &mut self,
        sequence: u64,
        target: String,
        observed_at_ms: u64,
        tls_blackhole: bool,
    ) {
        self.last_weak_adverse_at_ms = Some(
            self.last_weak_adverse_at_ms
                .map_or(observed_at_ms, |current| current.max(observed_at_ms)),
        );
        let record = WeakAdverseFlow {
            sequence,
            target,
            observed_at_ms,
        };
        if tls_blackhole {
            self.tls_blackhole_flows.push(record);
        } else {
            self.reset_flows.push(record);
        }
    }

    fn refresh_quorum(&mut self) {
        if self.failure.is_some() {
            return;
        }
        let reached_at = if self.targets.len() == 1 {
            self.single_target_quorum_at()
        } else {
            self.multi_target_quorum_at()
        }
        .or_else(|| self.discord_client_recovery_quorum_at());
        if let Some(reached_at) = reached_at {
            self.quorum_reached_at_ms = Some(
                self.quorum_reached_at_ms
                    .map_or(reached_at, |current| current.min(reached_at)),
            );
        }
    }

    fn single_target_quorum_at(&self) -> Option<u64> {
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
        if probes.len() < 2 || flows.len() < 2 {
            return None;
        }
        let mut completions = self.correspondence_completion_times(target);
        completions.sort_unstable();
        completions.get(1).copied()
    }

    fn multi_target_quorum_at(&self) -> Option<u64> {
        let mut completions = self
            .targets
            .iter()
            .filter_map(|target| {
                self.correspondence_completion_times(target)
                    .into_iter()
                    .min()
            })
            .collect::<Vec<_>>();
        completions.sort_unstable();
        completions.get(1).copied()
    }

    /// Two independent synthetic failures followed by two fresh working
    /// Discord flows prove a TLS-fingerprint mismatch rather than a broken
    /// candidate. This exception is deliberately Discord-only: it relies on
    /// the protected observer's post-arm SNI and generation fencing, never on
    /// the mere presence of a process or cached UI state.
    fn discord_client_recovery_quorum_at(&self) -> Option<u64> {
        if self.envelope.category != "discord" || self.failed_target_probes.len() < 2 {
            return None;
        }
        let mut working = self
            .working_flows
            .iter()
            .map(|flow| (flow.observed_at_ms, flow.flow_id))
            .collect::<Vec<_>>();
        working.sort_unstable();
        working.dedup_by_key(|(_, flow_id)| *flow_id);
        working.get(1).map(|(observed_at_ms, _)| *observed_at_ms)
    }

    fn correspondence_completion_times(&self, target: &str) -> Vec<u64> {
        let mut probes = self
            .successful_probes
            .iter()
            .filter(|probe| probe.target == target)
            .collect::<Vec<_>>();
        probes.sort_by_key(|probe| (probe.finished_at_ms, probe.probe_id));
        let mut used_flows = BTreeSet::new();
        let mut completion_times = Vec::new();
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
                                .saturating_add(LEGACY_CONFIRMATION_DELIVERY_GRACE_MS)
                                .min(self.arm.deadline_at_monotonic_ms())
                        && !used_flows.contains(&flow.flow_id)
                })
                .min_by_key(|flow| (flow.observed_at_ms, flow.flow_id));
            if let Some(flow) = matching {
                used_flows.insert(flow.flow_id);
                completion_times.push(probe.finished_at_ms.max(flow.observed_at_ms));
            }
        }
        completion_times
    }

    fn decision(&self, now_ms: u64, pending_ingress: bool) -> LegacyConfirmationDecision {
        if let Some(failure) = self.failure {
            return LegacyConfirmationDecision::Failed(failure);
        }
        if self.weak_adverse_quorum(
            &self.tls_blackhole_flows,
            BLACKHOLE_REQUIRED_FLOWS as usize,
            BLACKHOLE_REQUIRED_TARGETS as usize,
        ) || self.weak_adverse_quorum(
            &self.reset_flows,
            RESET_REQUIRED_FLOWS as usize,
            RESET_REQUIRED_TARGETS as usize,
        ) {
            return LegacyConfirmationDecision::Failed(ConfirmationFailure::Strategy);
        }

        let clean_anchor = self.quorum_reached_at_ms.map(|reached_at| {
            self.last_weak_recovery_at_ms()
                .map_or(reached_at, |recovered_at| reached_at.max(recovered_at))
        });
        let weak_adverse_settled = self.last_weak_adverse_at_ms.is_none_or(|adverse_at| {
            let settled_at = adverse_at.saturating_add(WEAK_ADVERSE_SETTLE_MS);
            now_ms >= settled_at && settled_at <= self.arm.deadline_at_monotonic_ms()
        });
        if !pending_ingress
            && weak_adverse_settled
            && clean_anchor.is_some_and(|anchor| {
                let clean_at = anchor.saturating_add(CLEAN_WINDOW_MS);
                now_ms >= clean_at && clean_at <= self.arm.deadline_at_monotonic_ms()
            })
        {
            return LegacyConfirmationDecision::Succeeded;
        }
        if now_ms < self.arm.delivery_deadline_at_monotonic_ms() {
            LegacyConfirmationDecision::Pending
        } else if pending_ingress {
            LegacyConfirmationDecision::Failed(ConfirmationFailure::Sensor)
        } else if !self.failed_target_probes.is_empty() {
            LegacyConfirmationDecision::Failed(ConfirmationFailure::Target)
        } else {
            LegacyConfirmationDecision::Failed(ConfirmationFailure::MissingWorkingEvidence)
        }
    }

    fn weak_adverse_quorum(
        &self,
        flows: &[WeakAdverseFlow],
        required_flows: usize,
        required_targets: usize,
    ) -> bool {
        let active = flows
            .iter()
            .filter(|flow| self.working_recovery_at(flow).is_none())
            .collect::<Vec<_>>();
        active.len() >= required_flows
            && active
                .iter()
                .map(|flow| flow.target.as_str())
                .collect::<BTreeSet<_>>()
                .len()
                >= required_targets
    }

    fn working_recovery_at(&self, adverse: &WeakAdverseFlow) -> Option<u64> {
        self.working_flows
            .iter()
            .filter(|working| {
                working.target == adverse.target
                    && (working.observed_at_ms, working.sequence)
                        > (adverse.observed_at_ms, adverse.sequence)
            })
            .map(|working| working.observed_at_ms)
            .max()
    }

    fn last_weak_recovery_at_ms(&self) -> Option<u64> {
        self.tls_blackhole_flows
            .iter()
            .chain(self.reset_flows.iter())
            .filter_map(|flow| self.working_recovery_at(flow))
            .max()
    }

    fn target_for_domain(&self, domain: &str) -> Option<&str> {
        let domain = normalize_domain(domain)?;
        self.targets
            .iter()
            .find(|target| domain_matches_suffix(&domain, target))
            .map(String::as_str)
    }

    fn sensor_to_process_time(&self, sensor_ms: u64) -> u64 {
        self.arm
            .armed_at_monotonic_ms
            .saturating_add(sensor_ms.saturating_sub(self.arm.armed_at_sensor_ms))
    }
}

fn validate_snapshot_fence<'a>(
    snapshot: &'a ObserveOnlySnapshot,
    envelope: &IntentEnvelope,
) -> Option<&'a obsession_runtime_reliability::legacy_reliability::assessment::LaneAssessment> {
    if snapshot.session.closed
        || snapshot.session.session_id != envelope.session_id
        || snapshot.session.sensor_generation != envelope.expected_sensor_generation
        || snapshot.session.target_registry_version != envelope.expected_registry_version
        || snapshot.session.network_fingerprint_at_start != envelope.expected_network_fingerprint
        || snapshot.session.lane_generations.get(&envelope.category)
            != Some(&envelope.expected_lane_generation)
    {
        return None;
    }
    snapshot.lanes.iter().find(|lane| {
        lane.category == envelope.category
            && lane.lane_generation == envelope.expected_lane_generation
    })
}

fn normalize_domain(value: &str) -> Option<String> {
    normalize_registry_domain(value)
}

fn domain_matches_suffix(domain: &str, suffix: &str) -> bool {
    domain == suffix
        || domain
            .strip_suffix(suffix)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use obsession_runtime_reliability::dpi_engine::EngineKind;
    use obsession_runtime_reliability::legacy_reliability::assessment::{
        AssessmentClassification, AssessmentConfidence, EvidenceSummary, LaneAssessment, LanePhase,
    };
    use obsession_runtime_reliability::legacy_reliability::contracts::{
        AttemptId, IntentFence, LaneGeneration, NetworkFingerprint, RegistryVersion,
        SensorGeneration, SessionId,
    };
    use obsession_runtime_reliability::legacy_reliability::manager::{
        AcceptedEventCounters, GapStatus, ObserveOnlyHealthStatus, ObserveOnlySessionStatus,
        RejectedEventCounters,
    };
    use obsession_runtime_reliability::legacy_reliability::policy::PresumedIntent;

    const ARM_PROCESS_MS: u64 = 10_000;
    const ARM_SENSOR_MS: u64 = 1_000;

    fn envelope() -> IntentEnvelope {
        IntentEnvelope::from_fence(
            AttemptId::new(41),
            &IntentFence {
                session_id: SessionId::new(7),
                category: "discord".into(),
                lane_generation: LaneGeneration::new(8),
                sensor_generation: SensorGeneration::new(9),
                registry_version: RegistryVersion::new(12),
                network_fingerprint: NetworkFingerprint::Stable {
                    key: "network".into(),
                },
            },
        )
    }

    fn snapshot() -> ObserveOnlySnapshot {
        let envelope = envelope();
        ObserveOnlySnapshot {
            session: ObserveOnlySessionStatus {
                session_id: envelope.session_id,
                engine: EngineKind::Legacy,
                active_categories: vec![envelope.category.clone()],
                network_fingerprint_at_start: envelope.expected_network_fingerprint.clone(),
                sensor_generation: envelope.expected_sensor_generation,
                target_registry_version: envelope.expected_registry_version,
                lane_generations: BTreeMap::from([(
                    envelope.category.clone(),
                    envelope.expected_lane_generation,
                )]),
                closed: false,
            },
            accepted: AcceptedEventCounters::default(),
            rejected: RejectedEventCounters::default(),
            gaps: GapStatus::default(),
            health: ObserveOnlyHealthStatus {
                state: EyeHealthState::Ready,
                counters: EyeHealthCounters::default(),
                last_reported_state: Some(EyeHealthState::Ready),
                last_reported_counters: Some(EyeHealthCounters::default()),
                receiver_failed: false,
            },
            logical_now_ms: ARM_SENSOR_MS,
            last_gap_sequence: None,
            last_accepted_flow_sequence: Some(10),
            pending_ingress_control_events: 0,
            pending_ingress_flow_events: BTreeMap::new(),
            last_evicted_confirmation_flow_sequences: BTreeMap::new(),
            lanes: vec![LaneAssessment {
                category: envelope.category.clone(),
                lane_generation: envelope.expected_lane_generation,
                phase: LanePhase::Observing,
                classification: AssessmentClassification::AwaitingEvidence,
                confidence: AssessmentConfidence::None,
                evidence: EvidenceSummary::default(),
                working_confirmed_recently: false,
                evidence_epoch: 31,
                assessed_at_ms: ARM_SENSOR_MS,
                cooldown_until_ms: None,
            }],
            presumed_intent: PresumedIntent::default(),
            active_configs: BTreeMap::new(),
            candidate_configs: BTreeMap::new(),
            gate_probe_hosts: BTreeMap::new(),
            confirmation_flows: Vec::new(),
        }
    }

    fn window(targets: &[&str]) -> (LegacyConfirmationWindow, ObserveOnlySnapshot) {
        let snapshot = snapshot();
        let window = LegacyConfirmationWindow::new(
            envelope(),
            &snapshot,
            targets.iter().copied(),
            ARM_PROCESS_MS,
        )
        .unwrap();
        (window, snapshot)
    }

    fn probe(id: u64, target: &str, offset: u64) -> LegacyHttpsProbeObservation {
        LegacyHttpsProbeObservation {
            probe_id: id,
            domain: target.into(),
            started_at_monotonic_ms: ARM_PROCESS_MS + offset,
            finished_at_monotonic_ms: ARM_PROCESS_MS + offset + 50,
            result: LegacyHttpsProbeResult::HttpResponse { status: 204 },
        }
    }

    fn failed_probe(id: u64, target: &str, offset: u64) -> LegacyHttpsProbeObservation {
        LegacyHttpsProbeObservation {
            result: LegacyHttpsProbeResult::TargetFailure,
            ..probe(id, target, offset)
        }
    }

    fn flow(
        sequence: u64,
        flow_id: u64,
        target: &str,
        diagnosis: Diagnosis,
        sensor_offset: u64,
    ) -> ConfirmationFlow {
        ConfirmationFlow {
            sequence,
            category: "discord".into(),
            lane_generation: LaneGeneration::new(8),
            flow_id,
            target: target.into(),
            diagnosis,
            armed_at_sensor_ms: Some(ARM_SENSOR_MS + sensor_offset.saturating_sub(1)),
            armed_at_capture_timestamp: Some((ARM_SENSOR_MS + sensor_offset) as i64),
            monotonic_ts: ARM_SENSOR_MS + sensor_offset,
        }
    }

    fn publish(snapshot: &mut ObserveOnlySnapshot, flows: Vec<ConfirmationFlow>) {
        snapshot.last_accepted_flow_sequence = flows
            .iter()
            .map(|flow| flow.sequence)
            .max()
            .or(snapshot.last_accepted_flow_sequence);
        snapshot.confirmation_flows = flows;
    }

    #[test]
    fn arm_rejects_unhealthy_or_drifted_observer() {
        let mut unhealthy = snapshot();
        unhealthy.health.state = EyeHealthState::Degraded;
        assert_eq!(
            LegacyConfirmationWindow::new(envelope(), &unhealthy, ["one.example"], ARM_PROCESS_MS,)
                .unwrap_err(),
            LegacyConfirmationBuildError::InvalidArm
        );

        let mut drifted = snapshot();
        drifted.session.sensor_generation = SensorGeneration::new(10);
        assert_eq!(
            LegacyConfirmationWindow::new(envelope(), &drifted, ["one.example"], ARM_PROCESS_MS,)
                .unwrap_err(),
            LegacyConfirmationBuildError::InvalidArm
        );
    }

    #[test]
    fn two_targets_need_correlated_https_and_working_then_a_clean_window() {
        let (mut window, mut snapshot) = window(&["one.example", "two.example"]);
        assert_eq!(
            window.observe_probe(probe(1, "one.example", 10), ARM_PROCESS_MS + 60),
            LegacyConfirmationDecision::Pending
        );
        assert_eq!(
            window.observe_probe(probe(2, "two.example", 20), ARM_PROCESS_MS + 70),
            LegacyConfirmationDecision::Pending
        );
        publish(
            &mut snapshot,
            vec![
                flow(11, 101, "one.example", Diagnosis::Working, 80),
                flow(12, 102, "two.example", Diagnosis::Working, 90),
            ],
        );
        assert_eq!(
            window.observe_snapshot(&snapshot, ARM_PROCESS_MS + 100),
            LegacyConfirmationDecision::Pending
        );
        assert_eq!(
            window.observe_snapshot(&snapshot, ARM_PROCESS_MS + 90 + CLEAN_WINDOW_MS),
            LegacyConfirmationDecision::Succeeded
        );
    }

    #[test]
    fn one_target_requires_two_distinct_probes_and_working_flows() {
        let (mut primary, mut snapshot) = window(&["one.example"]);
        primary.observe_probe(probe(1, "one.example", 10), ARM_PROCESS_MS + 60);
        primary.observe_probe(probe(2, "one.example", 20), ARM_PROCESS_MS + 70);
        publish(
            &mut snapshot,
            vec![
                flow(11, 101, "one.example", Diagnosis::Working, 80),
                flow(12, 102, "one.example", Diagnosis::Working, 90),
            ],
        );
        primary.observe_snapshot(&snapshot, ARM_PROCESS_MS + 100);
        assert_eq!(
            primary.observe_snapshot(&snapshot, ARM_PROCESS_MS + 90 + CLEAN_WINDOW_MS),
            LegacyConfirmationDecision::Succeeded
        );

        let (mut duplicate, mut snapshot) = window(&["one.example"]);
        duplicate.observe_probe(probe(1, "one.example", 10), ARM_PROCESS_MS + 60);
        duplicate.observe_probe(probe(1, "one.example", 20), ARM_PROCESS_MS + 70);
        publish(
            &mut snapshot,
            vec![
                flow(11, 101, "one.example", Diagnosis::Working, 80),
                flow(12, 102, "one.example", Diagnosis::Working, 90),
            ],
        );
        duplicate.observe_snapshot(&snapshot, ARM_PROCESS_MS + 100);
        assert_eq!(
            duplicate.observe_snapshot(
                &snapshot,
                ARM_PROCESS_MS
                    + LEGACY_CONFIRMATION_DEADLINE_MS
                    + LEGACY_CONFIRMATION_DELIVERY_GRACE_MS,
            ),
            LegacyConfirmationDecision::Failed(ConfirmationFailure::MissingWorkingEvidence)
        );
    }

    #[test]
    fn discord_client_working_can_override_a_synthetic_tls_fingerprint_mismatch() {
        let (mut window, mut snapshot) = window(&["discord.com", "gateway.discord.gg"]);
        assert_eq!(
            window.observe_probe(failed_probe(1, "discord.com", 10), ARM_PROCESS_MS + 60,),
            LegacyConfirmationDecision::Pending
        );
        assert_eq!(
            window.observe_probe(
                failed_probe(2, "gateway.discord.gg", 20),
                ARM_PROCESS_MS + 70,
            ),
            LegacyConfirmationDecision::Pending
        );
        publish(
            &mut snapshot,
            vec![
                flow(11, 101, "discord.com", Diagnosis::Working, 80),
                flow(12, 102, "gateway.discord.gg", Diagnosis::Working, 90),
            ],
        );
        assert_eq!(
            window.observe_snapshot(&snapshot, ARM_PROCESS_MS + 100),
            LegacyConfirmationDecision::Pending
        );
        assert_eq!(
            window.observe_snapshot(&snapshot, ARM_PROCESS_MS + 90 + CLEAN_WINDOW_MS),
            LegacyConfirmationDecision::Succeeded
        );
    }

    #[test]
    fn synthetic_target_failures_without_real_client_evidence_still_fail_closed() {
        let (mut window, snapshot) = window(&["discord.com"]);
        window.observe_probe(failed_probe(1, "discord.com", 10), ARM_PROCESS_MS + 60);
        window.observe_probe(failed_probe(2, "discord.com", 20), ARM_PROCESS_MS + 70);
        assert_eq!(
            window.observe_snapshot(
                &snapshot,
                ARM_PROCESS_MS
                    + LEGACY_CONFIRMATION_DEADLINE_MS
                    + LEGACY_CONFIRMATION_DELIVERY_GRACE_MS,
            ),
            LegacyConfirmationDecision::Failed(ConfirmationFailure::Target)
        );
    }

    #[test]
    fn pre_arm_or_uncorrelated_working_cannot_confirm_candidate() {
        let (mut window, mut snapshot) = window(&["one.example"]);
        window.observe_probe(probe(1, "one.example", 10), ARM_PROCESS_MS + 60);
        window.observe_probe(probe(2, "one.example", 20), ARM_PROCESS_MS + 70);
        let mut stale = flow(11, 101, "one.example", Diagnosis::Working, 80);
        stale.armed_at_capture_timestamp = Some(ARM_SENSOR_MS as i64);
        publish(
            &mut snapshot,
            vec![
                stale,
                flow(12, 102, "one.example", Diagnosis::Working, 4_000),
            ],
        );
        assert_eq!(
            window.observe_snapshot(
                &snapshot,
                ARM_PROCESS_MS
                    + LEGACY_CONFIRMATION_DEADLINE_MS
                    + LEGACY_CONFIRMATION_DELIVERY_GRACE_MS,
            ),
            LegacyConfirmationDecision::Failed(ConfirmationFailure::MissingWorkingEvidence)
        );
    }

    #[test]
    fn observer_drift_loss_or_stalled_delivery_fails_sensor_closed() {
        for mutation in 0..4 {
            let (mut window, mut snapshot) = window(&["one.example"]);
            match mutation {
                0 => snapshot.health.counters.parse_errors = 1,
                1 => snapshot.last_gap_sequence = Some(12),
                2 => {
                    snapshot
                        .last_evicted_confirmation_flow_sequences
                        .insert("discord".into(), 11);
                }
                3 => {
                    snapshot
                        .pending_ingress_flow_events
                        .insert("discord".into(), 1);
                }
                _ => unreachable!(),
            }
            let now = if mutation == 3 {
                window.arm().delivery_deadline_at_monotonic_ms()
            } else {
                ARM_PROCESS_MS + 100
            };
            assert_eq!(
                window.observe_snapshot(&snapshot, now),
                LegacyConfirmationDecision::Failed(ConfirmationFailure::Sensor)
            );
        }
    }

    #[test]
    fn strong_adverse_diagnoses_keep_failure_attribution_typed() {
        for (diagnosis, expected) in [
            (Diagnosis::HttpBlockPage, ConfirmationFailure::Strategy),
            (Diagnosis::DnsFailure, ConfirmationFailure::Environment),
            (Diagnosis::IpUnreachable, ConfirmationFailure::Environment),
            (Diagnosis::Throttled, ConfirmationFailure::Target),
        ] {
            let (mut window, mut snapshot) = window(&["one.example"]);
            publish(
                &mut snapshot,
                vec![flow(11, 101, "one.example", diagnosis, 80)],
            );
            assert_eq!(
                window.observe_snapshot(&snapshot, ARM_PROCESS_MS + 100),
                LegacyConfirmationDecision::Failed(expected)
            );
        }
    }

    #[test]
    fn unrecovered_weak_adverse_quorum_is_strategy_failure() {
        let (mut window, mut snapshot) = window(&["one.example", "two.example"]);
        publish(
            &mut snapshot,
            vec![
                flow(11, 101, "one.example", Diagnosis::TlsBlackhole, 80),
                flow(12, 102, "two.example", Diagnosis::TlsBlackhole, 90),
            ],
        );
        assert_eq!(
            window.observe_snapshot(&snapshot, ARM_PROCESS_MS + 100),
            LegacyConfirmationDecision::Failed(ConfirmationFailure::Strategy)
        );
    }

    #[test]
    fn probe_transport_failures_and_contract_violations_are_typed() {
        let (mut environment_window, _) = window(&["one.example"]);
        let mut observation = probe(1, "one.example", 10);
        observation.result = LegacyHttpsProbeResult::EnvironmentFailure;
        assert_eq!(
            environment_window.observe_probe(observation, ARM_PROCESS_MS + 60),
            LegacyConfirmationDecision::Failed(ConfirmationFailure::Environment)
        );

        let (mut target_window, snapshot) = window(&["one.example"]);
        assert_eq!(
            target_window.observe_probe(failed_probe(1, "one.example", 10), ARM_PROCESS_MS + 60,),
            LegacyConfirmationDecision::Pending
        );
        assert_eq!(
            target_window.observe_snapshot(
                &snapshot,
                ARM_PROCESS_MS
                    + LEGACY_CONFIRMATION_DEADLINE_MS
                    + LEGACY_CONFIRMATION_DELIVERY_GRACE_MS,
            ),
            LegacyConfirmationDecision::Failed(ConfirmationFailure::Target)
        );

        let (mut foreign_window, _) = window(&["one.example"]);
        assert_eq!(
            foreign_window.observe_probe(probe(1, "foreign.example", 10), ARM_PROCESS_MS + 60,),
            LegacyConfirmationDecision::Failed(ConfirmationFailure::Target)
        );
    }
}
