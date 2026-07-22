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
pub const WORKING_RECOVERY_HYSTERESIS_MS: u64 = 10_000;

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

    fn blackhole_gate_quorum(self) -> bool {
        usize::from(self.blackhole_flows) >= BLACKHOLE_FLOW_QUORUM && self.blackhole_targets > 0
    }

    fn has_gate_quorum(self) -> bool {
        self.reset_quorum() || self.blackhole_gate_quorum()
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
    /// UX-only last-known-good memory for this exact lane generation. Despite
    /// the legacy wire name, it is intentionally sticky until a contrary
    /// evidence quorum or sensor invalidation and never authorizes a policy
    /// action.
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
    working_recovery_started_at_ms: Option<u64>,
}

impl AppliedGate {
    fn requires_working_recovery(&self) -> bool {
        matches!(
            self.classification,
            AssessmentClassification::DpiSuspected | AssessmentClassification::DpiBlocked
        )
    }

    fn cooldown_active(&self, now_ms: u64) -> bool {
        self.cooldown_until_ms
            .is_some_and(|until_ms| now_ms < until_ms)
    }

    fn working_recovery_complete(&self, now_ms: u64) -> bool {
        self.working_recovery_started_at_ms
            .is_some_and(|started_at_ms| {
                now_ms >= started_at_ms.saturating_add(WORKING_RECOVERY_HYSTERESIS_MS)
            })
    }

    fn recovered_by_working(&self, now_ms: u64) -> bool {
        self.requires_working_recovery()
            && self.working_recovery_complete(now_ms)
            && !self.cooldown_active(now_ms)
    }

    fn remains_authoritative(&self, gate_quorum_still_valid: bool, now_ms: u64) -> bool {
        if self.cooldown_active(now_ms) {
            return true;
        }
        if self.requires_working_recovery() {
            return !self.recovered_by_working(now_ms);
        }
        gate_quorum_still_valid && now_ms <= self.valid_until_ms
    }

    fn blocks_new_gate(&self, now_ms: u64) -> bool {
        self.cooldown_active(now_ms)
            || (self.requires_working_recovery() && !self.working_recovery_complete(now_ms))
    }
}

#[derive(Clone, Debug)]
struct LaneState {
    generation: LaneGeneration,
    eligible_target_count: usize,
    evidence: VecDeque<EvidencePoint>,
    pending_gate: Option<PendingGateRequest>,
    gate_in_flight: bool,
    applied_gate: Option<AppliedGate>,
    /// Sticky, display-only last-known-good bit for this exact assessor lane.
    /// Policy still uses only the bounded evidence window and classification.
    unrefuted_working_confirmation: bool,
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
            unrefuted_working_confirmation: false,
        }
    }

    fn prune(&mut self, now_ms: u64) {
        // Do not assume producers can never deliver an older timestamp after
        // a newer one: control-first queue draining and capture scheduling may
        // reorder independent flows. Retain by timestamp instead of popping
        // only the deque front.
        let preserve_adverse_during_recovery = self.applied_gate.as_ref().is_some_and(|gate| {
            gate.requires_working_recovery()
                && gate.working_recovery_started_at_ms.is_some()
                && !gate.recovered_by_working(now_ms)
        });
        self.evidence.retain(|point| {
            now_ms.saturating_sub(point.ts_ms) <= EVIDENCE_WINDOW_MS
                || (preserve_adverse_during_recovery && point.kind != EvidenceKind::Working)
        });
        // Transient environment/site results belong only to the evidence
        // window that armed them. Confirmed DPI verdicts are different: they
        // remain latched through silence and expire only after Working
        // hysteresis (and any explicit caller-supplied cooldown).
        let summary = summarize(&self.evidence);
        if !summary.has_gate_quorum() {
            self.pending_gate = None;
            self.gate_in_flight = false;
        }
        let gate_quorum_still_valid = summary.has_gate_quorum();
        let recovered_by_working = self
            .applied_gate
            .as_ref()
            .is_some_and(|gate| gate.recovered_by_working(now_ms));
        let applied_gate_is_stale = self
            .applied_gate
            .as_ref()
            .is_some_and(|gate| !gate.remains_authoritative(gate_quorum_still_valid, now_ms));
        if applied_gate_is_stale {
            if recovered_by_working {
                // Publish recovery as one state transition: the old adverse
                // quorum remains visible for the entire clean interval, then
                // disappears in the same mutation that releases the DPI gate.
                self.evidence
                    .retain(|point| point.kind == EvidenceKind::Working);
                self.pending_gate = None;
                self.gate_in_flight = false;
            }
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
        if now_ms.saturating_sub(flow.monotonic_ts) > EVIDENCE_WINDOW_MS
            || lane
                .evidence
                .iter()
                .any(|point| point.flow_id == flow.flow_id)
        {
            return false;
        }
        if kind == EvidenceKind::Working
            && lane.applied_gate.as_ref().is_some_and(|gate| {
                gate.requires_working_recovery() && flow.monotonic_ts <= gate.assessed_at_ms
            })
        {
            // Control-first draining may deliver a flow after the Gate even
            // though Eyes observed it before the verdict. Such a delayed
            // success cannot count as post-Gate recovery evidence.
            lane.prune(now_ms);
            return false;
        }
        // Do this before time-based pruning: an accepted adverse event that
        // arrives on the hysteresis boundary must interrupt the old recovery,
        // not let pruning briefly publish Healthy first.
        if kind != EvidenceKind::Working {
            if let Some(gate) = lane
                .applied_gate
                .as_mut()
                .filter(|gate| gate.requires_working_recovery())
            {
                gate.working_recovery_started_at_ms = None;
            }
        }
        lane.prune(now_ms);
        if lane.evidence.len() >= MAX_EVIDENCE_PER_LANE {
            self.invalidate(SensorInvalidationReason::EvidenceOverflow);
            return true;
        }

        let before = summarize(&lane.evidence);
        lane.evidence.push_back(EvidencePoint {
            ts_ms: flow.monotonic_ts,
            flow_id: flow.flow_id,
            target: target.to_owned(),
            probe_host: flow.domain.clone(),
            kind,
        });
        if kind != EvidenceKind::Working
            && lane
                .applied_gate
                .as_ref()
                .is_some_and(|gate| gate.requires_working_recovery())
        {
            // Recovery needs a new Working quorum after every adverse signal;
            // flows observed before the interruption cannot complete the new
            // clean interval.
            lane.evidence
                .retain(|point| point.kind != EvidenceKind::Working);
        }
        let mut after = summarize(&lane.evidence);
        if kind == EvidenceKind::Working
            && working_quorum(after, lane.eligible_target_count)
            && !has_unrecovered_adverse_target(&lane.evidence)
        {
            lane.unrefuted_working_confirmation = true;
            lane.pending_gate = None;
            lane.gate_in_flight = false;
            if let Some(gate) = lane
                .applied_gate
                .as_mut()
                .filter(|gate| gate.requires_working_recovery())
            {
                // The first complete Working quorum starts one clean interval.
                // More successful flows do not keep pushing its deadline out.
                gate.working_recovery_started_at_ms.get_or_insert(now_ms);
            } else {
                // Non-DPI Gate results remain immediately refutable by a fresh
                // Working quorum.
                lane.evidence
                    .retain(|point| point.kind == EvidenceKind::Working);
                lane.applied_gate = None;
            }
            after = summarize(&lane.evidence);
        } else if after.reset_quorum() || after.blackhole_gate_quorum() {
            // Two independent blackhole flows are enough to refute the
            // display-only session confirmation and ask the active Gate for a
            // diagnosis. High-confidence DpiBlocked still requires distinct
            // targets inside the Gate classifier.
            lane.unrefuted_working_confirmation = false;
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
            lane.unrefuted_working_confirmation = false;
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
            GateTrigger::BlackholeQuorum => summary.blackhole_gate_quorum(),
        };
        if !trigger_still_valid {
            lane.pending_gate = None;
            lane.gate_in_flight = false;
            return false;
        }
        lane.pending_gate = None;
        lane.gate_in_flight = false;
        if matches!(
            classification,
            AssessmentClassification::DpiSuspected | AssessmentClassification::DpiBlocked
        ) {
            // Recovery evidence must be strictly newer than the Gate verdict.
            // Otherwise successes captured before the adverse incident could
            // start (or nearly complete) the clean hysteresis interval.
            lane.evidence
                .retain(|point| point.kind != EvidenceKind::Working);
        }
        lane.applied_gate = Some(AppliedGate {
            gate_id: request.gate_id,
            evidence_epoch: request.evidence_epoch,
            trigger: request.trigger,
            classification,
            assessed_at_ms,
            valid_until_ms,
            cooldown_until_ms,
            working_recovery_started_at_ms: None,
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
                GateTrigger::BlackholeQuorum => evidence.blackhole_gate_quorum(),
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
        let trigger = if summary.blackhole_gate_quorum() {
            Some(GateTrigger::BlackholeQuorum)
        } else if summary.reset_quorum() {
            Some(GateTrigger::ResetQuorum)
        } else {
            None
        };
        let Some(trigger) = trigger else {
            return;
        };

        // An applied DPI verdict owns the lane until a full Working quorum and
        // ten-second clean interval recover it. This is lane-wide: switching
        // from blackhole evidence to resets cannot bypass the hysteresis (or an
        // explicit cooldown supplied by a caller).
        let applied_gate_blocks = lane
            .applied_gate
            .as_ref()
            .is_some_and(|gate| gate.blocks_new_gate(now_ms));
        let already_handled = applied_gate_blocks
            || lane.pending_gate.as_ref().is_some_and(|request| {
                request.evidence_epoch == self.evidence_epoch && request.trigger == trigger
            })
            || lane.applied_gate.as_ref().is_some_and(|gate| {
                gate.evidence_epoch == self.evidence_epoch
                    && gate.trigger == trigger
                    && (now_ms <= gate.valid_until_ms
                        || gate
                            .cooldown_until_ms
                            .is_some_and(|until_ms| now_ms < until_ms))
                    && match trigger {
                        GateTrigger::ResetQuorum => summary.reset_quorum(),
                        GateTrigger::BlackholeQuorum => summary.blackhole_gate_quorum(),
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

fn has_unrecovered_adverse_target(points: &VecDeque<EvidencePoint>) -> bool {
    let mut latest_working = BTreeMap::<&str, u64>::new();
    let mut latest_adverse = BTreeMap::<&str, u64>::new();

    for point in points {
        let timestamps = if point.kind == EvidenceKind::Working {
            &mut latest_working
        } else {
            &mut latest_adverse
        };
        timestamps
            .entry(point.target.as_str())
            .and_modify(|timestamp| *timestamp = (*timestamp).max(point.ts_ms))
            .or_insert(point.ts_ms);
    }

    latest_adverse.iter().any(|(target, adverse_at_ms)| {
        latest_working
            .get(target)
            .is_none_or(|working_at_ms| working_at_ms <= adverse_at_ms)
    })
}

fn assess_lane(
    category: &str,
    lane: &LaneState,
    evidence_epoch: u64,
    sensor_reliable: bool,
    now_ms: u64,
) -> LaneAssessment {
    let evidence = summarize_at(&lane.evidence, now_ms);
    let gate_quorum_still_valid = evidence.has_gate_quorum();
    let (phase, classification, confidence, cooldown_until_ms) = if !sensor_reliable {
        (
            LanePhase::SensorUnreliable,
            AssessmentClassification::SensorUnreliable,
            AssessmentConfidence::None,
            None,
        )
    } else if let Some(gate) = lane
        .applied_gate
        .as_ref()
        .filter(|gate| gate.remains_authoritative(gate_quorum_still_valid, now_ms))
    {
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
        working_confirmed_recently: lane.unrefuted_working_confirmation,
        evidence_epoch,
        assessed_at_ms: lane
            .applied_gate
            .as_ref()
            .map_or(now_ms, |gate| gate.assessed_at_ms),
        cooldown_until_ms,
    }
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
    fn working_confirmation_outlives_policy_window_for_the_lane_generation() {
        let mut assessor = assessor(2);
        observe(&mut assessor, 1, "one.test", 1, Diagnosis::Working);
        observe(&mut assessor, 2, "two.test", 2, Diagnosis::Working);

        let after_policy_window = EVIDENCE_WINDOW_MS + 3;
        assessor.poll(after_policy_window);
        let confirmed_in_session = assessor.snapshots(after_policy_window).remove(0);
        assert_eq!(confirmed_in_session.phase, LanePhase::Observing);
        assert_eq!(
            confirmed_in_session.classification,
            AssessmentClassification::AwaitingEvidence
        );
        assert_eq!(confirmed_in_session.evidence, EvidenceSummary::default());
        assert!(confirmed_in_session.working_confirmed_recently);
        assert!(assessor.take_gate_request().is_none());

        let long_silence = 24 * 60 * 60 * 1_000;
        assessor.poll(long_silence);
        let after_long_silence = assessor.snapshots(long_silence).remove(0);
        assert_eq!(
            after_long_silence.classification,
            AssessmentClassification::AwaitingEvidence
        );
        assert!(after_long_silence.working_confirmed_recently);

        observe(
            &mut assessor,
            3,
            "one.test",
            long_silence + 1,
            Diagnosis::Working,
        );
        let partial_quorum = assessor.snapshots(long_silence + 1).remove(0);
        assert_eq!(
            partial_quorum.classification,
            AssessmentClassification::AwaitingEvidence
        );
        assert_eq!(partial_quorum.confidence, AssessmentConfidence::None);
        assert_eq!(partial_quorum.evidence.working_flows, 1);
        assert_eq!(partial_quorum.evidence.working_targets, 1);
        assert!(partial_quorum.working_confirmed_recently);

        let next_generation =
            LaneAssessor::new([("video", LaneGeneration::new(LANE.get() + 1), 2)]);
        assert!(!next_generation.snapshots(long_silence)[0].working_confirmed_recently);
    }

    #[test]
    fn subquorum_adverse_evidence_keeps_working_confirmation() {
        let mut blackhole = assessor(2);
        observe(&mut blackhole, 1, "one.test", 1, Diagnosis::Working);
        observe(&mut blackhole, 2, "two.test", 2, Diagnosis::Working);
        observe(&mut blackhole, 3, "one.test", 3, Diagnosis::TlsBlackhole);
        observe(&mut blackhole, 4, "one.test", 4, Diagnosis::TlsBlackhole);
        let same_target_timeouts = blackhole.snapshots(4).remove(0);
        assert_eq!(
            same_target_timeouts.confidence,
            AssessmentConfidence::Medium
        );
        assert_eq!(same_target_timeouts.evidence.blackhole_flows, 2);
        assert_eq!(same_target_timeouts.evidence.blackhole_targets, 1);
        assert!(!same_target_timeouts.working_confirmed_recently);
        assert_eq!(
            blackhole.take_gate_request().unwrap().trigger,
            GateTrigger::BlackholeQuorum
        );

        let mut reset = assessor(2);
        observe(&mut reset, 1, "one.test", 1, Diagnosis::Working);
        observe(&mut reset, 2, "two.test", 2, Diagnosis::Working);
        observe(&mut reset, 3, "one.test", 3, Diagnosis::TcpReset);
        observe(&mut reset, 4, "two.test", 4, Diagnosis::TcpReset);
        let two_target_resets = reset.snapshots(4).remove(0);
        assert_eq!(two_target_resets.evidence.reset_flows, 2);
        assert_eq!(two_target_resets.evidence.reset_targets, 2);
        assert!(two_target_resets.working_confirmed_recently);
        assert!(reset.take_gate_request().is_none());
    }

    #[test]
    fn working_on_peer_targets_cannot_erase_an_unrecovered_blackhole() {
        let mut assessor = assessor(3);
        observe(&mut assessor, 1, "broken.test", 1, Diagnosis::TlsBlackhole);
        observe(&mut assessor, 2, "broken.test", 2, Diagnosis::TlsBlackhole);
        observe(&mut assessor, 3, "peer-one.test", 3, Diagnosis::Working);
        observe(&mut assessor, 4, "peer-two.test", 4, Diagnosis::Working);

        let still_broken = assessor.snapshots(4).remove(0);
        assert_eq!(still_broken.phase, LanePhase::GatePending);
        assert_eq!(
            still_broken.classification,
            AssessmentClassification::AwaitingEvidence
        );
        assert_eq!(still_broken.evidence.blackhole_flows, 2);
        assert_eq!(still_broken.evidence.blackhole_targets, 1);
        assert_eq!(still_broken.evidence.working_targets, 2);
        assert_eq!(
            assessor.take_gate_request().unwrap().trigger,
            GateTrigger::BlackholeQuorum
        );

        observe(&mut assessor, 5, "broken.test", 5, Diagnosis::Working);
        let recovered = assessor.snapshots(5).remove(0);
        assert_eq!(recovered.phase, LanePhase::Healthy);
        assert_eq!(recovered.classification, AssessmentClassification::Working);
        assert_eq!(recovered.evidence.blackhole_flows, 0);
        assert!(assessor.take_gate_request().is_none());
    }

    #[test]
    fn adverse_quorum_clears_working_confirmation_until_working_recovers() {
        let mut blackhole = assessor(2);
        observe(&mut blackhole, 1, "one.test", 1, Diagnosis::Working);
        observe(&mut blackhole, 2, "two.test", 2, Diagnosis::Working);
        observe(&mut blackhole, 3, "one.test", 3, Diagnosis::TlsBlackhole);
        observe(&mut blackhole, 4, "two.test", 4, Diagnosis::TlsBlackhole);
        assert!(!blackhole.snapshots(4)[0].working_confirmed_recently);
        assert_eq!(
            blackhole.take_gate_request().unwrap().trigger,
            GateTrigger::BlackholeQuorum
        );
        blackhole.poll(EVIDENCE_WINDOW_MS + 5);
        assert!(!blackhole.snapshots(EVIDENCE_WINDOW_MS + 5)[0].working_confirmed_recently);

        observe(
            &mut blackhole,
            5,
            "one.test",
            EVIDENCE_WINDOW_MS + 6,
            Diagnosis::Working,
        );
        observe(
            &mut blackhole,
            6,
            "two.test",
            EVIDENCE_WINDOW_MS + 7,
            Diagnosis::Working,
        );
        assert!(blackhole.snapshots(EVIDENCE_WINDOW_MS + 7)[0].working_confirmed_recently);

        let mut reset = assessor(2);
        observe(&mut reset, 1, "one.test", 1, Diagnosis::Working);
        observe(&mut reset, 2, "two.test", 2, Diagnosis::Working);
        observe(&mut reset, 3, "one.test", 3, Diagnosis::TcpReset);
        observe(&mut reset, 4, "two.test", 4, Diagnosis::TcpReset);
        observe(&mut reset, 5, "one.test", 5, Diagnosis::TcpReset);
        assert!(!reset.snapshots(5)[0].working_confirmed_recently);
        assert_eq!(
            reset.take_gate_request().unwrap().trigger,
            GateTrigger::ResetQuorum
        );
    }

    #[test]
    fn sensor_invalidation_clears_working_confirmation() {
        for reason in [
            SensorInvalidationReason::Gap,
            SensorInvalidationReason::Health,
            SensorInvalidationReason::CounterRegression,
            SensorInvalidationReason::ReceiverFailure,
            SensorInvalidationReason::EvidenceOverflow,
        ] {
            let mut assessor = assessor(2);
            observe(&mut assessor, 1, "one.test", 1, Diagnosis::Working);
            observe(&mut assessor, 2, "two.test", 2, Diagnosis::Working);
            assessor.invalidate(reason);
            assert!(!assessor.snapshots(3)[0].working_confirmed_recently);
            assessor.mark_sensor_ready();
            assert!(!assessor.snapshots(4)[0].working_confirmed_recently);
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
    fn silence_and_cooldown_expiry_do_not_clear_an_applied_dpi_incident() {
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

        assessor.poll(300_004);
        let after_silence = assessor.snapshots(300_004).remove(0);
        assert_eq!(
            after_silence.classification,
            AssessmentClassification::DpiBlocked
        );
        assert_eq!(after_silence.cooldown_until_ms, None);
        assert_eq!(after_silence.evidence, EvidenceSummary::default());
        assert!(assessor.take_gate_request().is_none());
    }

    #[test]
    fn a_cross_trigger_cannot_replace_an_applied_dpi_assessment() {
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
        let snapshot = assessor.snapshots(6).remove(0);
        assert_eq!(
            snapshot.classification,
            AssessmentClassification::DpiSuspected
        );
        assert_eq!(snapshot.evidence.blackhole_targets, 2);
        assert!(assessor.take_gate_request().is_none());
    }

    #[test]
    fn applied_dpi_requires_a_ten_second_clean_working_interval() {
        for (diagnosis, classification, flows) in [
            (
                Diagnosis::TlsBlackhole,
                AssessmentClassification::DpiBlocked,
                2,
            ),
            (
                Diagnosis::TcpReset,
                AssessmentClassification::DpiSuspected,
                3,
            ),
        ] {
            let mut assessor = assessor(2);
            for id in 1..=flows {
                let target = if id == 1 { "one.test" } else { "two.test" };
                observe(&mut assessor, id, target, id, diagnosis);
            }
            let request = assessor.take_gate_request().unwrap();
            assert!(assessor.apply_gate_result(
                &request,
                classification,
                flows + 1,
                flows + 10_001,
                None,
            ));

            observe(&mut assessor, 10, "one.test", 10, Diagnosis::Working);
            observe(&mut assessor, 11, "two.test", 11, Diagnosis::Working);
            let recovering = assessor.snapshots(11).remove(0);
            assert_eq!(recovering.classification, classification);
            assert_eq!(recovering.evidence.working_flows, 2);
            assert_eq!(
                recovering.evidence.reset_flows,
                if classification == AssessmentClassification::DpiSuspected {
                    3
                } else {
                    0
                }
            );
            assert_eq!(
                recovering.evidence.blackhole_flows,
                if classification == AssessmentClassification::DpiBlocked {
                    2
                } else {
                    0
                }
            );

            assessor.poll(11 + WORKING_RECOVERY_HYSTERESIS_MS - 1);
            assert_eq!(assessor.snapshots(10_010)[0].classification, classification);
            assessor.poll(11 + WORKING_RECOVERY_HYSTERESIS_MS);
            let recovered = assessor
                .snapshots(11 + WORKING_RECOVERY_HYSTERESIS_MS)
                .remove(0);
            assert_eq!(recovered.phase, LanePhase::Healthy);
            assert_eq!(recovered.classification, AssessmentClassification::Working);
            assert_eq!(recovered.evidence.reset_flows, 0);
            assert_eq!(recovered.evidence.blackhole_flows, 0);
            assert_eq!(recovered.evidence.working_flows, 2);
            assert!(assessor.take_gate_request().is_none());
        }
    }

    #[test]
    fn applied_dpi_does_not_reuse_working_evidence_from_before_the_gate() {
        let mut assessor = assessor(2);
        observe(&mut assessor, 1, "one.test", 1, Diagnosis::Working);
        observe(&mut assessor, 2, "two.test", 2, Diagnosis::Working);
        observe(&mut assessor, 3, "one.test", 3, Diagnosis::TlsBlackhole);
        observe(&mut assessor, 4, "two.test", 4, Diagnosis::TlsBlackhole);
        let request = assessor.take_gate_request().unwrap();

        assert!(assessor.apply_gate_result(
            &request,
            AssessmentClassification::DpiBlocked,
            5,
            10_005,
            None,
        ));
        assert_eq!(assessor.snapshots(5)[0].evidence.working_flows, 0);

        let delayed_one = flow(7, 4, Diagnosis::Working);
        let delayed_two = flow(8, 5, Diagnosis::Working);
        assert!(!assessor.observe_flow("video", "one.test", &delayed_one, 6));
        assert!(!assessor.observe_flow("video", "two.test", &delayed_two, 7));
        assert_eq!(assessor.snapshots(7)[0].evidence.working_flows, 0);

        observe(&mut assessor, 5, "one.test", 6, Diagnosis::Working);
        assessor.poll(6 + WORKING_RECOVERY_HYSTERESIS_MS);
        assert_eq!(
            assessor.snapshots(6 + WORKING_RECOVERY_HYSTERESIS_MS)[0].classification,
            AssessmentClassification::DpiBlocked
        );

        observe(
            &mut assessor,
            6,
            "two.test",
            7 + WORKING_RECOVERY_HYSTERESIS_MS,
            Diagnosis::Working,
        );
        assessor.poll(7 + WORKING_RECOVERY_HYSTERESIS_MS * 2 - 1);
        assert_eq!(
            assessor.snapshots(7 + WORKING_RECOVERY_HYSTERESIS_MS * 2 - 1)[0].classification,
            AssessmentClassification::DpiBlocked
        );
        assessor.poll(7 + WORKING_RECOVERY_HYSTERESIS_MS * 2);
        assert_eq!(
            assessor.snapshots(7 + WORKING_RECOVERY_HYSTERESIS_MS * 2)[0].classification,
            AssessmentClassification::Working
        );
    }

    #[test]
    fn any_adverse_flow_restarts_the_clean_working_interval() {
        let mut assessor = assessor(2);
        observe(&mut assessor, 1, "one.test", 1, Diagnosis::TlsBlackhole);
        observe(&mut assessor, 2, "two.test", 2, Diagnosis::TlsBlackhole);
        let request = assessor.take_gate_request().unwrap();
        assert!(assessor.apply_gate_result(
            &request,
            AssessmentClassification::DpiBlocked,
            3,
            10_003,
            None,
        ));

        observe(&mut assessor, 3, "one.test", 4, Diagnosis::Working);
        observe(&mut assessor, 4, "two.test", 5, Diagnosis::Working);
        observe(&mut assessor, 5, "one.test", 10_004, Diagnosis::TcpReset);
        assessor.poll(20_004);
        let interrupted = assessor.snapshots(20_004).remove(0);
        assert_eq!(
            interrupted.classification,
            AssessmentClassification::DpiBlocked
        );
        assert_eq!(interrupted.evidence.working_flows, 0);
        assert!(assessor.take_gate_request().is_none());

        observe(&mut assessor, 6, "one.test", 20_005, Diagnosis::Working);
        observe(&mut assessor, 7, "two.test", 20_006, Diagnosis::Working);
        assessor.poll(30_005);
        assert_eq!(
            assessor.snapshots(30_005)[0].classification,
            AssessmentClassification::DpiBlocked
        );
        assessor.poll(30_006);
        assert_eq!(
            assessor.snapshots(30_006)[0].classification,
            AssessmentClassification::Working
        );
    }

    #[test]
    fn adverse_delivery_on_the_clean_deadline_interrupts_before_pruning() {
        let mut assessor = assessor(2);
        observe(&mut assessor, 1, "one.test", 1, Diagnosis::TlsBlackhole);
        observe(&mut assessor, 2, "two.test", 2, Diagnosis::TlsBlackhole);
        let request = assessor.take_gate_request().unwrap();
        assert!(assessor.apply_gate_result(
            &request,
            AssessmentClassification::DpiBlocked,
            3,
            10_003,
            None,
        ));
        observe(&mut assessor, 3, "one.test", 4, Diagnosis::Working);
        observe(&mut assessor, 4, "two.test", 5, Diagnosis::Working);

        let late_adverse = flow(5, 10_004, Diagnosis::TcpReset);
        assert!(assessor.observe_flow("video", "one.test", &late_adverse, 10_005));
        let snapshot = assessor.snapshots(10_005).remove(0);
        assert_eq!(
            snapshot.classification,
            AssessmentClassification::DpiBlocked
        );
        assert_eq!(snapshot.evidence.working_flows, 0);
        assert_eq!(snapshot.evidence.reset_flows, 1);
    }

    #[test]
    fn working_quorum_immediately_clears_a_non_dpi_gate_result() {
        let mut assessor = assessor(2);
        for (id, target) in [(1, "one.test"), (2, "two.test"), (3, "two.test")] {
            observe(&mut assessor, id, target, id, Diagnosis::TcpReset);
        }
        let request = assessor.take_gate_request().unwrap();
        assert!(assessor.apply_gate_result(
            &request,
            AssessmentClassification::TargetUnavailable,
            4,
            14,
            None,
        ));

        observe(&mut assessor, 4, "one.test", 5, Diagnosis::Working);
        observe(&mut assessor, 5, "two.test", 6, Diagnosis::Working);
        let snapshot = assessor.snapshots(6).remove(0);
        assert_eq!(snapshot.phase, LanePhase::Healthy);
        assert_eq!(snapshot.classification, AssessmentClassification::Working);
        assert!(assessor.take_gate_request().is_none());
    }

    #[test]
    fn explicit_cooldown_and_dpi_hysteresis_are_both_lane_wide() {
        let mut assessor = assessor(2);
        observe(&mut assessor, 1, "one.test", 1, Diagnosis::TlsBlackhole);
        observe(&mut assessor, 2, "two.test", 2, Diagnosis::TlsBlackhole);
        let blocked = assessor.take_gate_request().unwrap();
        assert!(assessor.apply_gate_result(
            &blocked,
            AssessmentClassification::DpiBlocked,
            3,
            13,
            Some(300_003),
        ));

        for (id, target, at) in [
            (3, "one.test", 299_990),
            (4, "two.test", 299_991),
            (5, "two.test", 299_992),
        ] {
            observe(&mut assessor, id, target, at, Diagnosis::TcpReset);
        }
        assert!(assessor.take_gate_request().is_none());
        assert_eq!(
            assessor.snapshots(300_002)[0].phase,
            LanePhase::BlockedCooldown
        );

        assessor.poll(300_003);
        assert_eq!(
            assessor.snapshots(300_003)[0].classification,
            AssessmentClassification::DpiBlocked
        );
        assert!(assessor.take_gate_request().is_none());
    }

    #[test]
    fn expired_non_dpi_gate_result_rearms_while_adverse_quorum_is_current() {
        let mut assessor = assessor(2);
        for (id, target) in [(1, "one.test"), (2, "two.test"), (3, "two.test")] {
            observe(&mut assessor, id, target, id, Diagnosis::TcpReset);
        }
        let first = assessor.take_gate_request().unwrap();
        assert!(assessor.apply_gate_result(
            &first,
            AssessmentClassification::TargetUnavailable,
            4,
            14,
            None,
        ));
        assert_eq!(
            assessor.snapshots(14)[0].classification,
            AssessmentClassification::TargetUnavailable
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
