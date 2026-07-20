//! Pure, per-category correlation of generation-fenced Legacy evidence.
//!
//! This layer deliberately owns no network client, process handle, cache, or
//! UI emitter. It turns already-attributed TLS flows into bounded lane windows
//! and asks the runtime for an Environment Gate only after a real quorum.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::Serialize;

use crate::eyes::Diagnosis;

use super::contracts::{FlowEvent, LaneGeneration, Transport};

pub const EVIDENCE_WINDOW_MS: u64 = 30_000;
pub const MAX_EVIDENCE_PER_LANE: usize = 2_048;
pub const RESET_FLOW_QUORUM: usize = 3;
pub const RESET_TARGET_QUORUM: usize = 2;
pub const BLACKHOLE_FLOW_QUORUM: usize = 2;
pub const BLACKHOLE_TARGET_QUORUM: usize = 2;
pub const WORKING_FLOW_QUORUM: usize = 2;
pub const WORKING_TARGET_QUORUM: usize = 2;
/// UX-only memory of a confirmed Working quorum. It never contributes to an
/// adverse quorum or authorizes a policy action.
pub const RECENT_WORKING_CONFIRMATION_TTL_MS: u64 = 5 * 60 * 1_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LanePhase {
    Observing,
    Healthy,
    Suspect,
    GatePending,
    BlockedCooldown,
    SensorUnreliable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssessmentClassification {
    AwaitingEvidence,
    Working,
    Offline,
    DnsFailure,
    UpstreamDegraded,
    TargetUnavailable,
    ServiceSlow,
    DpiSuspected,
    DpiBlocked,
    SensorUnreliable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssessmentConfidence {
    None,
    Low,
    Medium,
    High,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceSummary {
    pub working_flows: u16,
    pub working_targets: u16,
    pub reset_flows: u16,
    pub reset_targets: u16,
    pub blackhole_flows: u16,
    pub blackhole_targets: u16,
}

impl EvidenceSummary {
    pub fn reset_quorum(self) -> bool {
        usize::from(self.reset_flows) >= RESET_FLOW_QUORUM
            && usize::from(self.reset_targets) >= RESET_TARGET_QUORUM
    }

    pub fn blackhole_quorum(self) -> bool {
        usize::from(self.blackhole_flows) >= BLACKHOLE_FLOW_QUORUM
            && usize::from(self.blackhole_targets) >= BLACKHOLE_TARGET_QUORUM
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaneAssessment {
    pub category: String,
    pub lane_generation: LaneGeneration,
    pub phase: LanePhase,
    pub classification: AssessmentClassification,
    pub confidence: AssessmentConfidence,
    pub evidence: EvidenceSummary,
    /// Privacy-safe projection; the monotonic confirmation timestamp remains
    /// private to the assessor and is never exposed to UI or local logs.
    pub working_confirmed_recently: bool,
    pub evidence_epoch: u64,
    pub assessed_at_ms: u64,
    pub cooldown_until_ms: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GateTrigger {
    ResetQuorum,
    BlackholeQuorum,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingGateRequest {
    pub gate_id: u64,
    pub category: String,
    pub lane_generation: LaneGeneration,
    pub evidence_epoch: u64,
    pub trigger: GateTrigger,
    pub evidence: EvidenceSummary,
    pub requested_at_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SensorInvalidationReason {
    Health,
    Gap,
    CounterRegression,
    ReceiverFailure,
    EvidenceOverflow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EvidenceKind {
    Working,
    Reset,
    TlsBlackhole,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct EvidencePoint {
    ts_ms: u64,
    flow_id: u64,
    target: String,
    probe_host: String,
    kind: EvidenceKind,
}

#[derive(Clone, Debug)]
struct AppliedGate {
    gate_id: u64,
    evidence_epoch: u64,
    trigger: GateTrigger,
    classification: AssessmentClassification,
    assessed_at_ms: u64,
    valid_until_ms: u64,
    cooldown_until_ms: Option<u64>,
}

#[derive(Clone, Debug)]
struct LaneState {
    generation: LaneGeneration,
    eligible_target_count: usize,
    evidence: VecDeque<EvidencePoint>,
    pending_gate: Option<PendingGateRequest>,
    gate_in_flight: bool,
    applied_gate: Option<AppliedGate>,
    last_working_confirmation_ms: Option<u64>,
}

impl LaneState {
    fn new(generation: LaneGeneration, eligible_target_count: usize) -> Self {
        Self {
            generation,
            eligible_target_count,
            evidence: VecDeque::new(),
            pending_gate: None,
            gate_in_flight: false,
            applied_gate: None,
            last_working_confirmation_ms: None,
        }
    }

    fn prune(&mut self, now_ms: u64) {
        // Do not assume producers can never deliver an older timestamp after
        // a newer one: control-first queue draining and capture scheduling may
        // reorder independent flows. Retain by timestamp instead of popping
        // only the deque front.
        self.evidence
            .retain(|point| now_ms.saturating_sub(point.ts_ms) <= EVIDENCE_WINDOW_MS);
        if self
            .last_working_confirmation_ms
            .is_some_and(|confirmed_at| {
                confirmed_at > now_ms
                    || now_ms.saturating_sub(confirmed_at) > RECENT_WORKING_CONFIRMATION_TTL_MS
            })
        {
            self.last_working_confirmation_ms = None;
        }

        // A completed non-cooldown result belongs only to the evidence window
        // that armed it. Once all adverse evidence expires, return to passive
        // observation instead of preserving an old diagnosis indefinitely.
        let summary = summarize(&self.evidence);
        if !summary.reset_quorum() && !summary.blackhole_quorum() {
            self.pending_gate = None;
            self.gate_in_flight = false;
        }
        let gate_quorum_still_valid = summary.reset_quorum() || summary.blackhole_quorum();
        let applied_gate_is_stale = self.applied_gate.as_ref().is_some_and(|gate| {
            let cooldown_active = gate
                .cooldown_until_ms
                .is_some_and(|until_ms| now_ms < until_ms);
            !cooldown_active && (!gate_quorum_still_valid || now_ms > gate.valid_until_ms)
        });
        if applied_gate_is_stale {
            self.applied_gate = None;
        }
    }
}

/// Pure state holder for every active Legacy category.
pub struct LaneAssessor {
    lanes: BTreeMap<String, LaneState>,
    evidence_epoch: u64,
    next_gate_id: u64,
    sensor_reliable: bool,
    invalidation_reason: Option<SensorInvalidationReason>,
}

impl LaneAssessor {
    pub fn new<I, C>(lanes: I) -> Self
    where
        I: IntoIterator<Item = (C, LaneGeneration, usize)>,
        C: Into<String>,
    {
        Self {
            lanes: lanes
                .into_iter()
                .map(|(category, generation, eligible_target_count)| {
                    (
                        category.into(),
                        LaneState::new(generation, eligible_target_count),
                    )
                })
                .collect(),
            evidence_epoch: 1,
            next_gate_id: 1,
            sensor_reliable: true,
            invalidation_reason: None,
        }
    }

    pub const fn evidence_epoch(&self) -> u64 {
        self.evidence_epoch
    }

    pub const fn sensor_reliable(&self) -> bool {
        self.sensor_reliable
    }

    pub const fn invalidation_reason(&self) -> Option<SensorInvalidationReason> {
        self.invalidation_reason
    }

    /// Records one event that already passed session/sensor/registry/lane
    /// fencing and was re-attributed to the exact active registry target.
    /// Returns `true` only when the policy-visible lane state changed.
    pub fn observe_flow(
        &mut self,
        category: &str,
        target: &str,
        flow: &FlowEvent,
        now_ms: u64,
    ) -> bool {
        if !self.sensor_reliable || flow.transport != Transport::Tls {
            return false;
        }
        let Some(kind) = evidence_kind(flow.diagnosis) else {
            return false;
        };
        let Some(lane) = self.lanes.get_mut(category) else {
            return false;
        };
        if flow.lane_generation != Some(lane.generation) {
            return false;
        }
        lane.prune(now_ms);
        if now_ms.saturating_sub(flow.monotonic_ts) > EVIDENCE_WINDOW_MS
            || lane
                .evidence
                .iter()
                .any(|point| point.flow_id == flow.flow_id)
        {
            return false;
        }
        if lane.evidence.len() >= MAX_EVIDENCE_PER_LANE {
            self.invalidate(SensorInvalidationReason::EvidenceOverflow);
            return true;
        }

        if matches!(kind, EvidenceKind::Reset | EvidenceKind::TlsBlackhole) {
            lane.last_working_confirmation_ms = None;
        }
        let before = summarize(&lane.evidence);
        lane.evidence.push_back(EvidencePoint {
            ts_ms: flow.monotonic_ts,
            flow_id: flow.flow_id,
            target: target.to_owned(),
            probe_host: flow.domain.clone(),
            kind,
        });
        let mut after = summarize(&lane.evidence);
        if kind == EvidenceKind::Working && working_quorum(after, lane.eligible_target_count) {
            lane.last_working_confirmation_ms = Some(now_ms);
            lane.evidence
                .retain(|point| point.kind == EvidenceKind::Working);
            lane.pending_gate = None;
            lane.gate_in_flight = false;
            lane.applied_gate = None;
            after = summarize(&lane.evidence);
        }
        let changed = before != after;
        self.arm_gate_if_needed(category, now_ms);
        changed
    }

    /// Invalidates every lane at once. A Gap or unhealthy sensor makes the
    /// correlation interval unknowable; retaining a partial lane window would
    /// turn missing events into false confidence.
    pub fn invalidate(&mut self, reason: SensorInvalidationReason) {
        self.evidence_epoch = next_nonzero(self.evidence_epoch);
        self.sensor_reliable = false;
        self.invalidation_reason = Some(reason);
        for lane in self.lanes.values_mut() {
            lane.evidence.clear();
            lane.pending_gate = None;
            lane.gate_in_flight = false;
            lane.applied_gate = None;
            lane.last_working_confirmation_ms = None;
        }
    }

    /// Rearms collection after the Manager has independently observed its
    /// required clean health window. No old evidence is restored.
    pub fn mark_sensor_ready(&mut self) {
        self.sensor_reliable = true;
        self.invalidation_reason = None;
    }

    pub fn take_gate_request(&mut self) -> Option<PendingGateRequest> {
        self.lanes.values_mut().find_map(|lane| {
            if lane.gate_in_flight {
                return None;
            }
            let request = lane.pending_gate.clone()?;
            lane.gate_in_flight = true;
            Some(request)
        })
    }

    pub fn poll(&mut self, now_ms: u64) {
        let categories = self.lanes.keys().cloned().collect::<Vec<_>>();
        for lane in self.lanes.values_mut() {
            lane.prune(now_ms);
        }
        for category in categories {
            self.arm_gate_if_needed(&category, now_ms);
        }
    }

    /// Exact recently observed SNI hosts for distinct registry targets. These
    /// values stay inside the Gate request and are never projected to UI or
    /// the privacy-safe reliability log. Using a real observed host avoids
    /// probing a registry suffix whose apex has no DNS record.
    pub fn adverse_probe_hosts(&self, category: &str, now_ms: u64, limit: usize) -> Vec<String> {
        let Some(lane) = self.lanes.get(category) else {
            return Vec::new();
        };
        let mut targets = BTreeSet::new();
        let mut hosts = Vec::new();
        for point in lane.evidence.iter().rev().filter(|point| {
            now_ms.saturating_sub(point.ts_ms) <= EVIDENCE_WINDOW_MS
                && matches!(point.kind, EvidenceKind::Reset | EvidenceKind::TlsBlackhole)
        }) {
            if targets.insert(point.target.as_str()) {
                hosts.push(point.probe_host.clone());
                if hosts.len() >= limit {
                    break;
                }
            }
        }
        hosts.reverse();
        hosts
    }

    /// Applies a typed Gate result only to the exact evidence epoch and lane
    /// generation that requested it. Stale async results have zero effect.
    pub fn apply_gate_result(
        &mut self,
        request: &PendingGateRequest,
        classification: AssessmentClassification,
        assessed_at_ms: u64,
        valid_until_ms: u64,
        cooldown_until_ms: Option<u64>,
    ) -> bool {
        if !self.sensor_reliable || request.evidence_epoch != self.evidence_epoch {
            return false;
        }
        let Some(lane) = self.lanes.get_mut(&request.category) else {
            return false;
        };
        if lane.generation != request.lane_generation {
            return false;
        }
        if !lane.gate_in_flight
            || lane
                .pending_gate
                .as_ref()
                .is_none_or(|pending| pending.gate_id != request.gate_id)
        {
            return false;
        }
        if assessed_at_ms > valid_until_ms {
            lane.gate_in_flight = false;
            return false;
        }
        lane.prune(assessed_at_ms);
        let summary = summarize(&lane.evidence);
        let trigger_still_valid = match request.trigger {
            GateTrigger::ResetQuorum => summary.reset_quorum(),
            GateTrigger::BlackholeQuorum => summary.blackhole_quorum(),
        };
        if !trigger_still_valid {
            lane.pending_gate = None;
            lane.gate_in_flight = false;
            return false;
        }
        lane.pending_gate = None;
        lane.gate_in_flight = false;
        lane.applied_gate = Some(AppliedGate {
            gate_id: request.gate_id,
            evidence_epoch: request.evidence_epoch,
            trigger: request.trigger,
            classification,
            assessed_at_ms,
            valid_until_ms,
            cooldown_until_ms,
        });
        true
    }

    /// Replaces an exact rejected asynchronous request with a fresh attempt.
    /// Evidence identity, incident time and fences stay unchanged, while the
    /// gate id advances. Manager independently stamps every actual dispatch.
    pub fn release_gate_request(&mut self, request: &PendingGateRequest, retry_at_ms: u64) -> bool {
        if !self.sensor_reliable || request.evidence_epoch != self.evidence_epoch {
            return false;
        }

        let evidence = {
            let Some(lane) = self.lanes.get_mut(&request.category) else {
                return false;
            };
            lane.prune(retry_at_ms);
            if lane.generation != request.lane_generation
                || !lane.gate_in_flight
                || lane
                    .pending_gate
                    .as_ref()
                    .is_none_or(|pending| pending.gate_id != request.gate_id)
            {
                return false;
            }

            let evidence = summarize(&lane.evidence);
            let trigger_still_valid = match request.trigger {
                GateTrigger::ResetQuorum => evidence.reset_quorum(),
                GateTrigger::BlackholeQuorum => evidence.blackhole_quorum(),
            };
            if !trigger_still_valid {
                lane.pending_gate = None;
                lane.gate_in_flight = false;
                return false;
            }
            evidence
        };

        let gate_id = self.next_gate_id;
        self.next_gate_id = next_nonzero(self.next_gate_id);
        let lane = self
            .lanes
            .get_mut(&request.category)
            .expect("validated gate lane remains present");
        lane.pending_gate = Some(PendingGateRequest {
            gate_id,
            category: request.category.clone(),
            lane_generation: request.lane_generation,
            evidence_epoch: request.evidence_epoch,
            trigger: request.trigger,
            evidence,
            // Preserve the incident time for correlation. Manager stamps the
            // actual GateRequest separately at dispatch.
            requested_at_ms: request.requested_at_ms,
        });
        lane.gate_in_flight = false;
        true
    }

    pub fn snapshots(&self, now_ms: u64) -> Vec<LaneAssessment> {
        let sensor_reliable = self.sensor_reliable;
        let evidence_epoch = self.evidence_epoch;
        self.lanes
            .iter()
            .map(|(category, lane)| {
                assess_lane(category, lane, evidence_epoch, sensor_reliable, now_ms)
            })
            .collect()
    }

    fn arm_gate_if_needed(&mut self, category: &str, now_ms: u64) {
        let Some(lane) = self.lanes.get_mut(category) else {
            return;
        };
        let summary = summarize(&lane.evidence);
        let trigger = if summary.blackhole_quorum() {
            Some(GateTrigger::BlackholeQuorum)
        } else if summary.reset_quorum() {
            Some(GateTrigger::ResetQuorum)
        } else {
            None
        };
        let Some(trigger) = trigger else {
            return;
        };

        let already_handled = lane.pending_gate.as_ref().is_some_and(|request| {
            request.evidence_epoch == self.evidence_epoch && request.trigger == trigger
        }) || lane.applied_gate.as_ref().is_some_and(|gate| {
            gate.evidence_epoch == self.evidence_epoch
                && gate.trigger == trigger
                && (now_ms <= gate.valid_until_ms
                    || gate
                        .cooldown_until_ms
                        .is_some_and(|until_ms| now_ms < until_ms))
                && match trigger {
                    GateTrigger::ResetQuorum => summary.reset_quorum(),
                    GateTrigger::BlackholeQuorum => summary.blackhole_quorum(),
                }
        });
        if already_handled {
            return;
        }

        let gate_id = self.next_gate_id;
        self.next_gate_id = next_nonzero(self.next_gate_id);
        lane.pending_gate = Some(PendingGateRequest {
            gate_id,
            category: category.to_owned(),
            lane_generation: lane.generation,
            evidence_epoch: self.evidence_epoch,
            trigger,
            evidence: summary,
            requested_at_ms: now_ms,
        });
        lane.gate_in_flight = false;
    }
}

fn evidence_kind(diagnosis: Diagnosis) -> Option<EvidenceKind> {
    match diagnosis {
        Diagnosis::Working => Some(EvidenceKind::Working),
        Diagnosis::TcpReset => Some(EvidenceKind::Reset),
        Diagnosis::TlsBlackhole => Some(EvidenceKind::TlsBlackhole),
        Diagnosis::DnsFailure
        | Diagnosis::TcpBlackhole
        | Diagnosis::QuicBlocked
        | Diagnosis::UdpBlocked
        | Diagnosis::HttpBlockPage
        | Diagnosis::Throttled
        | Diagnosis::IpUnreachable
        | Diagnosis::Unknown => None,
    }
}

fn summarize(points: &VecDeque<EvidencePoint>) -> EvidenceSummary {
    summarize_iter(points.iter())
}

fn summarize_at(points: &VecDeque<EvidencePoint>, now_ms: u64) -> EvidenceSummary {
    summarize_iter(
        points
            .iter()
            .filter(|point| now_ms.saturating_sub(point.ts_ms) <= EVIDENCE_WINDOW_MS),
    )
}

fn summarize_iter<'a>(points: impl IntoIterator<Item = &'a EvidencePoint>) -> EvidenceSummary {
    let mut working_flows = BTreeSet::new();
    let mut working_targets = BTreeSet::new();
    let mut reset_flows = BTreeSet::new();
    let mut reset_targets = BTreeSet::new();
    let mut blackhole_flows = BTreeSet::new();
    let mut blackhole_targets = BTreeSet::new();

    for point in points {
        let (flows, targets) = match point.kind {
            EvidenceKind::Working => (&mut working_flows, &mut working_targets),
            EvidenceKind::Reset => (&mut reset_flows, &mut reset_targets),
            EvidenceKind::TlsBlackhole => (&mut blackhole_flows, &mut blackhole_targets),
        };
        flows.insert(point.flow_id);
        targets.insert(point.target.as_str());
    }

    EvidenceSummary {
        working_flows: bounded_u16(working_flows.len()),
        working_targets: bounded_u16(working_targets.len()),
        reset_flows: bounded_u16(reset_flows.len()),
        reset_targets: bounded_u16(reset_targets.len()),
        blackhole_flows: bounded_u16(blackhole_flows.len()),
        blackhole_targets: bounded_u16(blackhole_targets.len()),
    }
}

fn assess_lane(
    category: &str,
    lane: &LaneState,
    evidence_epoch: u64,
    sensor_reliable: bool,
    now_ms: u64,
) -> LaneAssessment {
    let evidence = summarize_at(&lane.evidence, now_ms);
    let gate_quorum_still_valid = evidence.reset_quorum() || evidence.blackhole_quorum();
    let (phase, classification, confidence, cooldown_until_ms) = if !sensor_reliable {
        (
            LanePhase::SensorUnreliable,
            AssessmentClassification::SensorUnreliable,
            AssessmentConfidence::None,
            None,
        )
    } else if let Some(gate) = lane.applied_gate.as_ref().filter(|gate| {
        gate.cooldown_until_ms.is_some_and(|until| now_ms < until)
            || (now_ms <= gate.valid_until_ms && gate_quorum_still_valid)
    }) {
        let cooldown_active = gate.cooldown_until_ms.is_some_and(|until| now_ms < until);
        (
            if gate.classification == AssessmentClassification::SensorUnreliable {
                LanePhase::SensorUnreliable
            } else if cooldown_active {
                LanePhase::BlockedCooldown
            } else if matches!(
                gate.classification,
                AssessmentClassification::DpiSuspected
                    | AssessmentClassification::DpiBlocked
                    | AssessmentClassification::AwaitingEvidence
            ) {
                LanePhase::Suspect
            } else {
                LanePhase::Observing
            },
            gate.classification,
            confidence_for_classification(gate.classification),
            gate.cooldown_until_ms.filter(|until| now_ms < *until),
        )
    } else if lane.pending_gate.is_some() && gate_quorum_still_valid {
        (
            LanePhase::GatePending,
            AssessmentClassification::AwaitingEvidence,
            AssessmentConfidence::Medium,
            None,
        )
    } else if evidence.blackhole_flows > 0 || evidence.reset_flows > 0 {
        (
            LanePhase::Suspect,
            AssessmentClassification::AwaitingEvidence,
            AssessmentConfidence::Low,
            None,
        )
    } else {
        if working_quorum(evidence, lane.eligible_target_count) {
            (
                LanePhase::Healthy,
                AssessmentClassification::Working,
                AssessmentConfidence::High,
                None,
            )
        } else {
            (
                LanePhase::Observing,
                AssessmentClassification::AwaitingEvidence,
                AssessmentConfidence::None,
                None,
            )
        }
    };

    LaneAssessment {
        category: category.to_owned(),
        lane_generation: lane.generation,
        phase,
        classification,
        confidence,
        evidence,
        working_confirmed_recently: working_confirmation_is_recent(
            lane.last_working_confirmation_ms,
            now_ms,
        ),
        evidence_epoch,
        assessed_at_ms: lane
            .applied_gate
            .as_ref()
            .map_or(now_ms, |gate| gate.assessed_at_ms),
        cooldown_until_ms,
    }
}

fn working_confirmation_is_recent(confirmed_at_ms: Option<u64>, now_ms: u64) -> bool {
    confirmed_at_ms.is_some_and(|confirmed_at| {
        confirmed_at <= now_ms
            && now_ms.saturating_sub(confirmed_at) <= RECENT_WORKING_CONFIRMATION_TTL_MS
    })
}

fn working_quorum(evidence: EvidenceSummary, eligible_target_count: usize) -> bool {
    let required_targets = if eligible_target_count <= 1 {
        1
    } else {
        WORKING_TARGET_QUORUM
    };
    usize::from(evidence.working_flows) >= WORKING_FLOW_QUORUM
        && usize::from(evidence.working_targets) >= required_targets
}

const fn confidence_for_classification(
    classification: AssessmentClassification,
) -> AssessmentConfidence {
    match classification {
        AssessmentClassification::DpiBlocked => AssessmentConfidence::High,
        AssessmentClassification::DpiSuspected
        | AssessmentClassification::Offline
        | AssessmentClassification::DnsFailure
        | AssessmentClassification::UpstreamDegraded
        | AssessmentClassification::TargetUnavailable
        | AssessmentClassification::ServiceSlow => AssessmentConfidence::Medium,
        AssessmentClassification::Working => AssessmentConfidence::High,
        AssessmentClassification::AwaitingEvidence => AssessmentConfidence::Low,
        AssessmentClassification::SensorUnreliable => AssessmentConfidence::None,
    }
}

fn bounded_u16(value: usize) -> u16 {
    value.min(usize::from(u16::MAX)) as u16
}

const fn next_nonzero(value: u64) -> u64 {
    let next = value.wrapping_add(1);
    if next == 0 {
        1
    } else {
        next
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use super::*;
    use crate::legacy_reliability::contracts::{
        EventEnvelope, RegistryVersion, SensorGeneration, SessionId,
    };

    const LANE: LaneGeneration = LaneGeneration::new(7);

    fn assessor(targets: usize) -> LaneAssessor {
        LaneAssessor::new([("video", LANE, targets)])
    }

    fn flow(id: u64, ts: u64, diagnosis: Diagnosis) -> FlowEvent {
        FlowEvent::new(
            EventEnvelope::new(
                SessionId::new(1),
                SensorGeneration::new(2),
                RegistryVersion::new(3),
            ),
            Some("video".into()),
            Some(LANE),
            id,
            "redacted.test",
            IpAddr::V4(Ipv4Addr::new(203, 0, 113, 8)),
            Transport::Tls,
            diagnosis,
            "typed",
            ts,
        )
        .unwrap()
    }

    fn observe(assessor: &mut LaneAssessor, id: u64, target: &str, ts: u64, diagnosis: Diagnosis) {
        assert!(assessor.observe_flow("video", target, &flow(id, ts, diagnosis), ts));
    }

    #[test]
    fn reset_requires_three_flows_and_two_registry_targets() {
        let mut assessor = assessor(2);
        observe(&mut assessor, 1, "one.test", 1, Diagnosis::TcpReset);
        observe(&mut assessor, 2, "one.test", 2, Diagnosis::TcpReset);
        observe(&mut assessor, 3, "one.test", 3, Diagnosis::TcpReset);
        assert!(assessor.take_gate_request().is_none());

        observe(&mut assessor, 4, "two.test", 4, Diagnosis::TcpReset);
        let request = assessor.take_gate_request().unwrap();
        assert_eq!(request.trigger, GateTrigger::ResetQuorum);
        assert_eq!(request.evidence.reset_flows, 4);
        assert_eq!(request.evidence.reset_targets, 2);
    }

    #[test]
    fn blackhole_requires_distinct_flows_and_targets() {
        let mut assessor = assessor(2);
        observe(&mut assessor, 1, "one.test", 1, Diagnosis::TlsBlackhole);
        assert!(!assessor.observe_flow(
            "video",
            "two.test",
            &flow(1, 2, Diagnosis::TlsBlackhole),
            2,
        ));
        assert!(assessor.take_gate_request().is_none());

        observe(&mut assessor, 2, "two.test", 3, Diagnosis::TlsBlackhole);
        assert_eq!(
            assessor.take_gate_request().unwrap().trigger,
            GateTrigger::BlackholeQuorum
        );
    }

    #[test]
    fn working_needs_quorum_and_silence_is_not_healthy() {
        let mut two_targets = assessor(2);
        let initial = two_targets.snapshots(0).remove(0);
        assert_eq!(initial.phase, LanePhase::Observing);
        assert!(!initial.working_confirmed_recently);
        observe(&mut two_targets, 1, "one.test", 1, Diagnosis::Working);
        observe(&mut two_targets, 2, "two.test", 2, Diagnosis::Working);
        let confirmed = two_targets.snapshots(2).remove(0);
        assert_eq!(confirmed.phase, LanePhase::Healthy);
        assert!(confirmed.working_confirmed_recently);

        let mut one_target = assessor(1);
        observe(&mut one_target, 1, "only.test", 1, Diagnosis::Working);
        observe(&mut one_target, 2, "only.test", 2, Diagnosis::Working);
        assert_eq!(one_target.snapshots(2)[0].phase, LanePhase::Healthy);
    }

    #[test]
    fn working_confirmation_outlives_policy_window_but_expires_after_five_minutes() {
        let mut assessor = assessor(2);
        observe(&mut assessor, 1, "one.test", 1, Diagnosis::Working);
        observe(&mut assessor, 2, "two.test", 2, Diagnosis::Working);

        let after_policy_window = EVIDENCE_WINDOW_MS + 3;
        assessor.poll(after_policy_window);
        let recent = assessor.snapshots(after_policy_window).remove(0);
        assert_eq!(recent.phase, LanePhase::Observing);
        assert_eq!(
            recent.classification,
            AssessmentClassification::AwaitingEvidence
        );
        assert_eq!(recent.evidence, EvidenceSummary::default());
        assert!(recent.working_confirmed_recently);
        assert!(assessor.take_gate_request().is_none());

        let ttl_boundary = 2 + RECENT_WORKING_CONFIRMATION_TTL_MS;
        assessor.poll(ttl_boundary);
        assert!(assessor.snapshots(ttl_boundary)[0].working_confirmed_recently);
        assessor.poll(ttl_boundary + 1);
        assert!(!assessor.snapshots(ttl_boundary + 1)[0].working_confirmed_recently);
    }

    #[test]
    fn adverse_incident_and_sensor_invalidation_clear_working_confirmation() {
        for diagnosis in [Diagnosis::TcpReset, Diagnosis::TlsBlackhole] {
            let mut assessor = assessor(2);
            observe(&mut assessor, 1, "one.test", 1, Diagnosis::Working);
            observe(&mut assessor, 2, "two.test", 2, Diagnosis::Working);
            assert!(assessor.snapshots(2)[0].working_confirmed_recently);

            observe(&mut assessor, 3, "one.test", 3, diagnosis);
            assert!(!assessor.snapshots(3)[0].working_confirmed_recently);
        }

        for reason in [
            SensorInvalidationReason::Gap,
            SensorInvalidationReason::Health,
            SensorInvalidationReason::ReceiverFailure,
        ] {
            let mut assessor = assessor(2);
            observe(&mut assessor, 1, "one.test", 1, Diagnosis::Working);
            observe(&mut assessor, 2, "two.test", 2, Diagnosis::Working);
            assessor.invalidate(reason);
            assert!(!assessor.snapshots(3)[0].working_confirmed_recently);
        }
    }

    #[test]
    fn boundary_is_inclusive_and_older_evidence_expires() {
        let mut assessor = assessor(2);
        observe(&mut assessor, 1, "one.test", 0, Diagnosis::TcpReset);
        observe(
            &mut assessor,
            2,
            "two.test",
            EVIDENCE_WINDOW_MS,
            Diagnosis::TcpReset,
        );
        observe(
            &mut assessor,
            3,
            "two.test",
            EVIDENCE_WINDOW_MS,
            Diagnosis::TcpReset,
        );
        assert!(assessor.take_gate_request().is_some());

        let snapshot = assessor.snapshots(EVIDENCE_WINDOW_MS + 1).remove(0);
        assert_eq!(snapshot.evidence.reset_flows, 2);
        assert_eq!(snapshot.phase, LanePhase::Suspect);
    }

    #[test]
    fn out_of_order_old_flow_cannot_survive_behind_a_newer_one() {
        let mut assessor = assessor(2);
        observe(&mut assessor, 1, "one.test", 20_000, Diagnosis::TcpReset);
        assert!(assessor.observe_flow(
            "video",
            "two.test",
            &flow(2, 10_000, Diagnosis::TcpReset),
            20_000,
        ));

        assessor.poll(45_000);
        let snapshot = assessor.snapshots(45_000).remove(0);
        assert_eq!(snapshot.evidence.reset_flows, 1);
        assert_eq!(snapshot.evidence.reset_targets, 1);
    }

    #[test]
    fn non_tls_and_syn_blackhole_never_arm_policy() {
        let mut assessor = assessor(2);
        let mut tcp = flow(1, 1, Diagnosis::TcpReset);
        tcp.transport = Transport::Tcp;
        assert!(!assessor.observe_flow("video", "one.test", &tcp, 1));
        assert!(!assessor.observe_flow(
            "video",
            "one.test",
            &flow(2, 2, Diagnosis::TcpBlackhole),
            2,
        ));
        assert_eq!(
            assessor.snapshots(2)[0].evidence,
            EvidenceSummary::default()
        );
    }

    #[test]
    fn sensor_invalidation_clears_windows_and_rejects_stale_gate() {
        let mut assessor = assessor(2);
        for (id, target) in [(1, "one.test"), (2, "two.test"), (3, "two.test")] {
            observe(&mut assessor, id, target, id, Diagnosis::TcpReset);
        }
        let request = assessor.take_gate_request().unwrap();
        assessor.invalidate(SensorInvalidationReason::Gap);
        assert_eq!(
            assessor.snapshots(10)[0].classification,
            AssessmentClassification::SensorUnreliable
        );
        assert!(!assessor.apply_gate_result(
            &request,
            AssessmentClassification::DpiSuspected,
            10,
            20,
            None,
        ));

        assessor.mark_sensor_ready();
        assert_eq!(assessor.snapshots(11)[0].phase, LanePhase::Observing);
    }

    #[test]
    fn expired_blackhole_cooldown_does_not_suppress_a_future_incident() {
        let mut assessor = assessor(2);
        observe(&mut assessor, 1, "one.test", 1, Diagnosis::TlsBlackhole);
        observe(&mut assessor, 2, "two.test", 2, Diagnosis::TlsBlackhole);
        let first = assessor.take_gate_request().unwrap();
        assert!(assessor.apply_gate_result(
            &first,
            AssessmentClassification::DpiBlocked,
            3,
            13,
            Some(300_003),
        ));

        assessor.poll(300_004);
        observe(
            &mut assessor,
            3,
            "one.test",
            300_005,
            Diagnosis::TlsBlackhole,
        );
        observe(
            &mut assessor,
            4,
            "two.test",
            300_006,
            Diagnosis::TlsBlackhole,
        );
        let second = assessor.take_gate_request().unwrap();
        assert_ne!(second.gate_id, first.gate_id);
    }

    #[test]
    fn blackhole_quorum_escalates_an_applied_reset_assessment() {
        let mut assessor = assessor(2);
        for (id, target) in [(1, "one.test"), (2, "two.test"), (3, "two.test")] {
            observe(&mut assessor, id, target, id, Diagnosis::TcpReset);
        }
        let reset = assessor.take_gate_request().unwrap();
        assert!(assessor.apply_gate_result(
            &reset,
            AssessmentClassification::DpiSuspected,
            4,
            14,
            None,
        ));

        observe(&mut assessor, 4, "one.test", 5, Diagnosis::TlsBlackhole);
        observe(&mut assessor, 5, "two.test", 6, Diagnosis::TlsBlackhole);
        let blackhole = assessor.take_gate_request().unwrap();
        assert_eq!(blackhole.trigger, GateTrigger::BlackholeQuorum);
        assert_ne!(blackhole.gate_id, reset.gate_id);
    }

    #[test]
    fn working_quorum_cancels_a_blocked_incident() {
        let mut assessor = assessor(2);
        observe(&mut assessor, 1, "one.test", 1, Diagnosis::TlsBlackhole);
        observe(&mut assessor, 2, "two.test", 2, Diagnosis::TlsBlackhole);
        let request = assessor.take_gate_request().unwrap();
        assert!(assessor.apply_gate_result(
            &request,
            AssessmentClassification::DpiBlocked,
            3,
            13,
            Some(300_003),
        ));

        observe(&mut assessor, 3, "one.test", 4, Diagnosis::Working);
        observe(&mut assessor, 4, "two.test", 5, Diagnosis::Working);
        let snapshot = assessor.snapshots(5).remove(0);
        assert_eq!(snapshot.phase, LanePhase::Healthy);
        assert_eq!(snapshot.classification, AssessmentClassification::Working);
        assert_eq!(snapshot.evidence.blackhole_flows, 0);
    }

    #[test]
    fn expired_gate_result_rearms_while_adverse_quorum_is_still_current() {
        let mut assessor = assessor(2);
        for (id, target) in [(1, "one.test"), (2, "two.test"), (3, "two.test")] {
            observe(&mut assessor, id, target, id, Diagnosis::TcpReset);
        }
        let first = assessor.take_gate_request().unwrap();
        assert!(assessor.apply_gate_result(
            &first,
            AssessmentClassification::DpiSuspected,
            4,
            14,
            None,
        ));
        assert_eq!(
            assessor.snapshots(14)[0].classification,
            AssessmentClassification::DpiSuspected
        );

        assessor.poll(15);
        let snapshot = assessor.snapshots(15).remove(0);
        assert_eq!(snapshot.phase, LanePhase::GatePending);
        assert_eq!(
            snapshot.classification,
            AssessmentClassification::AwaitingEvidence
        );
        let retry = assessor.take_gate_request().unwrap();
        assert_ne!(retry.gate_id, first.gate_id);
    }

    #[test]
    fn rejected_inflight_request_is_released_for_immediate_retry() {
        let mut assessor = assessor(2);
        for (id, target) in [(1, "one.test"), (2, "two.test"), (3, "two.test")] {
            observe(&mut assessor, id, target, id, Diagnosis::TcpReset);
        }
        let request = assessor.take_gate_request().unwrap();
        assert!(assessor.take_gate_request().is_none());
        assert!(assessor.release_gate_request(&request, 10));
        assert!(!assessor.release_gate_request(&request, 11));

        let retry = assessor.take_gate_request().unwrap();
        assert_ne!(retry.gate_id, request.gate_id);
        assert_eq!(retry.requested_at_ms, request.requested_at_ms);
        assert_eq!(retry.evidence_epoch, request.evidence_epoch);
        assert_eq!(retry.lane_generation, request.lane_generation);
    }
}
