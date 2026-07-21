//! Observe-only assessment state for generation-fenced Legacy reliability.
//!
//! Manager correlates typed evidence and accepts fenced Environment Gate
//! reports. It cannot mutate cache state, manage processes, or execute the
//! presumed intents returned by the pure Phase 2 policy.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use serde::Serialize;

use crate::dpi_engine::EngineKind;

use super::assessment::{
    AssessmentClassification, LaneAssessment, LaneAssessor, PendingGateRequest,
    SensorInvalidationReason,
};
use super::contracts::{
    EventEnvelope, EyeEvent, EyeHealthCounters, EyeHealthState, FlowEvent, GapEvent,
    LaneGeneration, LegacySessionContext, NetworkFingerprint, RegistryVersion, SensorGeneration,
    SessionId,
};
use super::environment_gate::{
    GateClassification, GateFence, GateReport, LocalNetworkSnapshot, PassiveEvidenceSummary,
    SensorSnapshot, GATE_REPORT_TTL,
};
use super::health::{
    AtomicHealthCounters, HealthPoll, HealthTracker, PendingGap, HEALTH_CLEAN_WINDOW_MS,
};
use super::ingress::{
    AcceptedScope, FenceBuildError, FenceRejection, LegacyIngressReceiver, SessionFence,
};
use super::policy::{LaneConfigOptions, ObserveOnlyBrain, PresumedIntent};
use super::target_registry::{Attribution, TargetRegistry};

/// Immutable session identity plus the mutable epochs fenced by this manager.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ObserveOnlySessionStatus {
    pub session_id: SessionId,
    pub engine: EngineKind,
    pub active_categories: Vec<String>,
    pub network_fingerprint_at_start: NetworkFingerprint,
    pub sensor_generation: SensorGeneration,
    pub target_registry_version: RegistryVersion,
    pub lane_generations: BTreeMap<String, LaneGeneration>,
    pub closed: bool,
}

/// Accepted event totals. These are observations, never recovery evidence or
/// a statement that a category is healthy.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct AcceptedEventCounters {
    pub total_events: u64,
    pub attributed_flows: u64,
    pub diagnostic_flows: u64,
    pub health_events: u64,
    pub gap_events: u64,
}

/// Rejections are separated by fence dimension so stale producers are visible
/// without allowing their payloads to affect accepted observations.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct RejectedEventCounters {
    pub total_events: u64,
    pub closed: u64,
    pub session: u64,
    pub sensor_generation: u64,
    pub registry_version: u64,
    pub unknown_category: u64,
    pub lane_generation: u64,
}

impl RejectedEventCounters {
    fn record(&mut self, reason: RejectionReason) {
        self.total_events = self.total_events.saturating_add(1);
        let counter = match reason {
            RejectionReason::Closed => &mut self.closed,
            RejectionReason::Session => &mut self.session,
            RejectionReason::SensorGeneration => &mut self.sensor_generation,
            RejectionReason::RegistryVersion => &mut self.registry_version,
            RejectionReason::UnknownCategory => &mut self.unknown_category,
            RejectionReason::LaneGeneration => &mut self.lane_generation,
        };
        *counter = counter.saturating_add(1);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectionReason {
    Closed,
    Session,
    SensorGeneration,
    RegistryVersion,
    UnknownCategory,
    LaneGeneration,
}

impl From<&FenceRejection> for RejectionReason {
    fn from(rejection: &FenceRejection) -> Self {
        match rejection {
            FenceRejection::Closed => Self::Closed,
            FenceRejection::Session { .. } => Self::Session,
            FenceRejection::SensorGeneration { .. } => Self::SensorGeneration,
            FenceRejection::RegistryVersion { .. } => Self::RegistryVersion,
            FenceRejection::UnknownCategory { .. } => Self::UnknownCategory,
            FenceRejection::LaneGeneration { .. } => Self::LaneGeneration,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GapSource {
    /// Queue loss reconstructed from the producer-owned atomic counters.
    ProducerCounters,
    /// A typed Gap event accepted through the control queue.
    ReportedEvent,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ObservedGap {
    /// Manager-local ordering. A later accepted Flow always has a larger value.
    pub sequence: u64,
    pub source: GapSource,
    pub from_ts: u64,
    pub to_ts: u64,
    pub dropped_events: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct GapStatus {
    pub total_gaps: u64,
    pub producer_counter_gaps: u64,
    pub reported_event_gaps: u64,
    pub producer_dropped_events: u64,
    pub reported_dropped_events: u64,
    pub last: Option<ObservedGap>,
}

/// Sensor health is intentionally distinct from lane health. Phase 1 has no
/// `Healthy` lane assessment and therefore cannot trigger recovery.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ObserveOnlyHealthStatus {
    pub state: EyeHealthState,
    pub counters: EyeHealthCounters,
    pub last_reported_state: Option<EyeHealthState>,
    pub last_reported_counters: Option<EyeHealthCounters>,
    pub receiver_failed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ObserveOnlySnapshot {
    pub session: ObserveOnlySessionStatus,
    pub accepted: AcceptedEventCounters,
    pub rejected: RejectedEventCounters,
    pub gaps: GapStatus,
    pub health: ObserveOnlyHealthStatus,
    pub logical_now_ms: u64,
    pub last_gap_sequence: Option<u64>,
    pub last_accepted_flow_sequence: Option<u64>,
    pub lanes: Vec<LaneAssessment>,
    pub presumed_intent: PresumedIntent,
    pub active_configs: BTreeMap<String, String>,
    /// Backend-only candidate order used by Assisted proposal reconciliation.
    /// It is intentionally omitted from JSON diagnostics and public status.
    #[serde(skip_serializing)]
    pub candidate_configs: BTreeMap<String, Vec<String>>,
    /// Exact recently observed SNI hosts for a fresh Gate retry. Raw hosts must
    /// never cross the backend boundary or enter the reliability log.
    #[serde(skip_serializing)]
    pub gate_probe_hosts: BTreeMap<String, Vec<String>>,
    /// Backend-only bounded journal used to arm confirmation after candidate
    /// readiness. Sequence fencing prevents Working emitted by the previous
    /// process during observer preflight from confirming the candidate.
    #[serde(skip_serializing)]
    pub confirmation_flows: Vec<ConfirmationFlow>,
}

pub const MAX_CONFIRMATION_FLOWS: usize = 512;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfirmationFlow {
    pub sequence: u64,
    pub category: String,
    pub lane_generation: LaneGeneration,
    pub flow_id: u64,
    pub target: String,
    pub diagnosis: crate::eyes::Diagnosis,
    pub monotonic_ts: u64,
}

/// Immutable, fully fenced request paired with the assessor token that must be
/// returned when its asynchronous Gate report completes.
#[derive(Clone, Debug)]
pub struct PreparedEnvironmentGate {
    pub pending: PendingGateRequest,
    pub request: super::environment_gate::GateRequest,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AcceptedEvent {
    AttributedFlow {
        category: String,
        lane_generation: LaneGeneration,
        sequence: u64,
    },
    DiagnosticFlow {
        sequence: u64,
    },
    Health {
        sequence: u64,
    },
    Gap {
        sequence: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "disposition", rename_all = "snake_case")]
pub enum EventDisposition {
    Accepted { event: AcceptedEvent },
    Rejected { reason: RejectionReason },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum ReceiveOutcome {
    Event { disposition: EventDisposition },
    ReceiverClosed,
    ManagerClosed,
}

/// Manager-owned observe-only state for one immutable Legacy session.
pub struct ObserveOnlyManager {
    context: LegacySessionContext,
    sensor_generation: SensorGeneration,
    registry_version: RegistryVersion,
    lane_generations: BTreeMap<String, LaneGeneration>,
    fence: SessionFence,
    counters: Arc<AtomicHealthCounters>,
    receiver: LegacyIngressReceiver,
    health_tracker: HealthTracker,
    health_counters: EyeHealthCounters,
    last_reported_health_state: Option<EyeHealthState>,
    last_reported_health_counters: Option<EyeHealthCounters>,
    reported_terminal_state: Option<EyeHealthState>,
    last_external_gap_at_ms: Option<u64>,
    accepted: AcceptedEventCounters,
    rejected: RejectedEventCounters,
    gaps: GapStatus,
    logical_now_ms: u64,
    observation_sequence: u64,
    last_gap_sequence: Option<u64>,
    last_accepted_flow_sequence: Option<u64>,
    confirmation_flows: VecDeque<ConfirmationFlow>,
    receiver_failed: bool,
    registry: Option<Arc<TargetRegistry>>,
    assessor: LaneAssessor,
    lane_config_options: BTreeMap<String, LaneConfigOptions>,
    last_assessment_unreliable_at_ms: Option<u64>,
}

impl ObserveOnlyManager {
    pub fn new(
        context: LegacySessionContext,
        sensor_generation: SensorGeneration,
        registry_version: RegistryVersion,
        lane_generations: BTreeMap<String, LaneGeneration>,
        counters: Arc<AtomicHealthCounters>,
        receiver: LegacyIngressReceiver,
    ) -> Result<Self, FenceBuildError> {
        Self::new_at(
            context,
            sensor_generation,
            registry_version,
            lane_generations,
            counters,
            receiver,
            0,
        )
    }

    /// Alternate constructor for deterministic tests or runtimes whose logical
    /// clock does not start at zero.
    pub fn new_at(
        context: LegacySessionContext,
        sensor_generation: SensorGeneration,
        registry_version: RegistryVersion,
        lane_generations: BTreeMap<String, LaneGeneration>,
        counters: Arc<AtomicHealthCounters>,
        receiver: LegacyIngressReceiver,
        started_at_ms: u64,
    ) -> Result<Self, FenceBuildError> {
        Self::build_at(
            context,
            sensor_generation,
            registry_version,
            lane_generations,
            counters,
            receiver,
            started_at_ms,
            None,
        )
    }

    /// Production constructor. Manager keeps the exact immutable registry used
    /// by Eyes so different SNI names cannot masquerade as independent targets.
    pub fn new_with_registry(
        context: LegacySessionContext,
        sensor_generation: SensorGeneration,
        registry: Arc<TargetRegistry>,
        lane_generations: BTreeMap<String, LaneGeneration>,
        counters: Arc<AtomicHealthCounters>,
        receiver: LegacyIngressReceiver,
    ) -> Result<Self, FenceBuildError> {
        Self::build_at(
            context,
            sensor_generation,
            registry.version(),
            lane_generations,
            counters,
            receiver,
            0,
            Some(registry),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn build_at(
        context: LegacySessionContext,
        sensor_generation: SensorGeneration,
        registry_version: RegistryVersion,
        lane_generations: BTreeMap<String, LaneGeneration>,
        counters: Arc<AtomicHealthCounters>,
        receiver: LegacyIngressReceiver,
        started_at_ms: u64,
        registry: Option<Arc<TargetRegistry>>,
    ) -> Result<Self, FenceBuildError> {
        let fence = SessionFence::new(
            context.clone(),
            sensor_generation,
            registry_version,
            lane_generations.clone(),
        )?;

        let assessor = LaneAssessor::new(lane_generations.iter().map(|(category, generation)| {
            let target_count = registry.as_ref().map_or(0, |registry| {
                registry.active_targets_for_category(category).len()
            });
            (category.clone(), *generation, target_count)
        }));
        let lane_config_options = registry.as_ref().map_or_else(BTreeMap::new, |registry| {
            lane_generations
                .keys()
                .map(|category| {
                    (
                        category.clone(),
                        LaneConfigOptions {
                            active: registry.active_config(category).map(str::to_owned),
                            candidates: registry
                                .candidate_configs_for_category(category)
                                .into_iter()
                                .map(str::to_owned)
                                .collect(),
                        },
                    )
                })
                .collect()
        });

        Ok(Self {
            context,
            sensor_generation,
            registry_version,
            lane_generations,
            fence,
            counters,
            receiver,
            health_tracker: HealthTracker::new(started_at_ms),
            health_counters: EyeHealthCounters::default(),
            last_reported_health_state: None,
            last_reported_health_counters: None,
            reported_terminal_state: None,
            last_external_gap_at_ms: None,
            accepted: AcceptedEventCounters::default(),
            rejected: RejectedEventCounters::default(),
            gaps: GapStatus::default(),
            logical_now_ms: started_at_ms,
            observation_sequence: 0,
            last_gap_sequence: None,
            last_accepted_flow_sequence: None,
            confirmation_flows: VecDeque::new(),
            receiver_failed: false,
            registry,
            assessor,
            lane_config_options,
            last_assessment_unreliable_at_ms: None,
        })
    }

    /// Polls the out-of-band health counters at a caller-supplied logical time.
    /// Any producer-side Gap is committed before this method returns.
    pub fn poll(&mut self, now_ms: u64) -> ObserveOnlySnapshot {
        self.poll_health(now_ms);
        self.snapshot()
    }

    /// Processes one already-owned event without any external side effects.
    /// Producer-side health is polled first, preserving Gap-before-Flow order.
    pub fn process_event(&mut self, now_ms: u64, event: EyeEvent) -> EventDisposition {
        self.poll_health(now_ms);
        self.process_event_after_health(event)
    }

    /// Nonblocking, control-first processing for deterministic runtimes/tests.
    pub fn try_process_next(&mut self, now_ms: u64) -> Option<EventDisposition> {
        self.poll_health(now_ms);
        let event = self
            .receiver
            .try_recv_control()
            .or_else(|| self.receiver.try_recv_flow().map(EyeEvent::Flow))?;
        Some(self.process_event_after_health(event))
    }

    /// Waits for one ingress item. Queue health is sampled both before and
    /// after receive so an out-of-band drop is ordered before a returned Flow.
    pub async fn recv_next(&mut self, now_ms: u64) -> ReceiveOutcome {
        self.poll_health(now_ms);

        if self.fence.is_closed() {
            let pending = self
                .receiver
                .try_recv_control()
                .or_else(|| self.receiver.try_recv_flow().map(EyeEvent::Flow));
            return match pending {
                Some(event) => ReceiveOutcome::Event {
                    disposition: self.process_event_after_health(event),
                },
                None => ReceiveOutcome::ManagerClosed,
            };
        }

        match self.receiver.recv().await {
            Some(event) => {
                self.poll_health(now_ms);
                ReceiveOutcome::Event {
                    disposition: self.process_event_after_health(event),
                }
            }
            None => {
                self.close_after_receiver_failure(now_ms);
                ReceiveOutcome::ReceiverClosed
            }
        }
    }

    /// Intentional teardown. The fence is closed before health or any other
    /// mutable state is transitioned, so a concurrent late event cannot pass.
    pub fn shutdown(&mut self, now_ms: u64) -> ObserveOnlySnapshot {
        self.fence.close();
        self.logical_now_ms = self.logical_now_ms.max(now_ms);
        let poll = self
            .health_tracker
            .stop_intentionally(self.logical_now_ms, self.counters.snapshot());
        self.absorb_health_poll(poll);
        self.reported_terminal_state = Some(EyeHealthState::Stopped);
        self.last_reported_health_state = Some(EyeHealthState::Stopped);
        self.mark_assessment_unreliable(SensorInvalidationReason::Health);
        self.snapshot()
    }

    pub fn snapshot(&self) -> ObserveOnlySnapshot {
        let lanes = self.assessor.snapshots(self.logical_now_ms);
        let presumed_intent =
            ObserveOnlyBrain::decide_all(&lanes, &self.lane_config_options, self.logical_now_ms);
        let active_configs = self
            .lane_config_options
            .iter()
            .filter_map(|(category, options)| {
                options
                    .active
                    .as_ref()
                    .map(|active| (category.clone(), active.clone()))
            })
            .collect();
        let candidate_configs = self
            .lane_config_options
            .iter()
            .map(|(category, options)| (category.clone(), options.candidates.clone()))
            .collect();
        let gate_probe_hosts = self
            .lane_generations
            .keys()
            .map(|category| {
                (
                    category.clone(),
                    self.assessor.adverse_probe_hosts(
                        category,
                        self.logical_now_ms,
                        super::environment_gate::MAX_CATEGORY_TARGETS,
                    ),
                )
            })
            .collect();
        ObserveOnlySnapshot {
            session: ObserveOnlySessionStatus {
                session_id: self.context.session_id(),
                engine: self.context.engine(),
                active_categories: self.context.active_categories().to_vec(),
                network_fingerprint_at_start: self.context.network_fingerprint_at_start().clone(),
                sensor_generation: self.sensor_generation,
                target_registry_version: self.registry_version,
                lane_generations: self.lane_generations.clone(),
                closed: self.fence.is_closed(),
            },
            accepted: self.accepted.clone(),
            rejected: self.rejected.clone(),
            gaps: self.gaps.clone(),
            health: ObserveOnlyHealthStatus {
                state: self.effective_health_state(),
                counters: self.health_counters,
                last_reported_state: self.last_reported_health_state,
                last_reported_counters: self.last_reported_health_counters,
                receiver_failed: self.receiver_failed,
            },
            logical_now_ms: self.logical_now_ms,
            last_gap_sequence: self.last_gap_sequence,
            last_accepted_flow_sequence: self.last_accepted_flow_sequence,
            lanes,
            presumed_intent,
            active_configs,
            candidate_configs,
            gate_probe_hosts,
            confirmation_flows: self.confirmation_flows.iter().cloned().collect(),
        }
    }

    /// Builds one asynchronous Environment Gate request after passive quorum.
    /// The local snapshot is collected by runtime without borrowing Manager.
    pub fn take_environment_gate_request(
        &mut self,
        local_network: LocalNetworkSnapshot,
    ) -> Option<PreparedEnvironmentGate> {
        self.sync_assessment_health();
        let pending = self.assessor.take_gate_request()?;
        let registry = self.registry.as_ref()?;
        let observed_hosts = self.assessor.adverse_probe_hosts(
            &pending.category,
            self.logical_now_ms,
            super::environment_gate::MAX_CATEGORY_TARGETS,
        );
        let category_targets = if observed_hosts.len() >= 2 {
            observed_hosts
        } else {
            registry
                .active_targets_for_category(&pending.category)
                .into_iter()
                .take(super::environment_gate::MAX_CATEGORY_TARGETS)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        if category_targets.is_empty() {
            let _ = self.assessor.apply_gate_result(
                &pending,
                AssessmentClassification::TargetUnavailable,
                self.logical_now_ms,
                gate_report_valid_until(self.logical_now_ms),
                None,
            );
            return None;
        }

        let fence = self.gate_fence(pending.lane_generation);
        let request = super::environment_gate::GateRequest {
            fence,
            category: pending.category.clone(),
            category_targets,
            local_network,
            sensor: SensorSnapshot {
                state: self.effective_health_state(),
                has_intersecting_gap: !self.assessor.sensor_reliable(),
            },
            passive_evidence: passive_evidence(pending.evidence),
            // PendingGateRequest keeps the incident time. Gate report TTL is
            // anchored to actual dispatch so a lane queued behind another
            // bounded Gate does not start out stale.
            requested_at_monotonic_ms: self.logical_now_ms,
        };
        Some(PreparedEnvironmentGate { pending, request })
    }

    /// A clean startup sample teaches only the in-memory control latency
    /// baseline. Its report is never applied as a lane assessment.
    pub fn baseline_gate_request(
        &self,
        local_network: LocalNetworkSnapshot,
    ) -> Option<super::environment_gate::GateRequest> {
        let registry = self.registry.as_ref()?;
        let (category, generation) = self.lane_generations.iter().next()?;
        let category_targets = registry
            .active_targets_for_category(category)
            .into_iter()
            .take(super::environment_gate::MAX_CATEGORY_TARGETS)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if category_targets.is_empty() {
            return None;
        }
        Some(super::environment_gate::GateRequest {
            fence: self.gate_fence(*generation),
            category: category.clone(),
            category_targets,
            local_network,
            sensor: SensorSnapshot {
                state: self.effective_health_state(),
                has_intersecting_gap: !self.assessor.sensor_reliable(),
            },
            passive_evidence: PassiveEvidenceSummary::default(),
            requested_at_monotonic_ms: self.logical_now_ms,
        })
    }

    pub fn apply_environment_gate_report(
        &mut self,
        prepared: &PreparedEnvironmentGate,
        report: &GateReport,
        now_ms: u64,
    ) -> bool {
        self.poll_health(now_ms);
        if report.category != prepared.pending.category
            || !report.is_fresh(now_ms, &prepared.request.fence)
            || report.fence != self.gate_fence(prepared.pending.lane_generation)
        {
            let _ = self
                .assessor
                .release_gate_request(&prepared.pending, now_ms);
            return false;
        }
        let classification = assessment_classification(report.classification);
        self.assessor.apply_gate_result(
            &prepared.pending,
            classification,
            now_ms,
            report.valid_until_monotonic_ms,
            // A confirmed DPI incident is held by the assessor's Working
            // hysteresis. Long-lived pacing and failed-candidate cooldowns
            // belong to the recovery coordinator/cache, not the display lane.
            None,
        )
    }

    pub fn apply_environment_gate_failure(
        &mut self,
        prepared: &PreparedEnvironmentGate,
        classification: AssessmentClassification,
        now_ms: u64,
    ) -> bool {
        self.poll_health(now_ms);
        self.assessor.apply_gate_result(
            &prepared.pending,
            classification,
            now_ms,
            gate_report_valid_until(now_ms),
            None,
        )
    }

    fn gate_fence(&self, lane_generation: LaneGeneration) -> GateFence {
        GateFence::from_envelope(
            EventEnvelope::new(
                self.context.session_id(),
                self.sensor_generation,
                self.registry_version,
            ),
            lane_generation,
            self.context.network_fingerprint_at_start().clone(),
        )
    }

    fn process_event_after_health(&mut self, event: EyeEvent) -> EventDisposition {
        let scope = match self.fence.check(&event) {
            Ok(scope) => scope,
            Err(rejection) => {
                let reason = RejectionReason::from(&rejection);
                self.rejected.record(reason);
                return EventDisposition::Rejected { reason };
            }
        };

        match (scope, event) {
            (
                AcceptedScope::AttributedFlow {
                    category,
                    lane_generation,
                },
                EyeEvent::Flow(flow),
            ) => {
                self.logical_now_ms = self.logical_now_ms.max(flow.monotonic_ts);
                let confirmation_target =
                    self.observe_attributed_flow(&category, lane_generation, &flow);
                let sequence = self.next_sequence();
                if let Some(target) = confirmation_target {
                    self.record_confirmation_flow(
                        sequence,
                        &category,
                        lane_generation,
                        target,
                        &flow,
                    );
                }
                self.accepted.total_events = self.accepted.total_events.saturating_add(1);
                self.accepted.attributed_flows = self.accepted.attributed_flows.saturating_add(1);
                self.last_accepted_flow_sequence = Some(sequence);
                EventDisposition::Accepted {
                    event: AcceptedEvent::AttributedFlow {
                        category,
                        lane_generation,
                        sequence,
                    },
                }
            }
            (AcceptedScope::DiagnosticFlow, EyeEvent::Flow(flow)) => {
                self.logical_now_ms = self.logical_now_ms.max(flow.monotonic_ts);
                let sequence = self.next_sequence();
                self.accepted.total_events = self.accepted.total_events.saturating_add(1);
                self.accepted.diagnostic_flows = self.accepted.diagnostic_flows.saturating_add(1);
                self.last_accepted_flow_sequence = Some(sequence);
                EventDisposition::Accepted {
                    event: AcceptedEvent::DiagnosticFlow { sequence },
                }
            }
            (AcceptedScope::Sensor, EyeEvent::Health(health)) => {
                if let Some(event_ts) = health.counters.last_event_ts {
                    self.logical_now_ms = self.logical_now_ms.max(event_ts);
                }
                let sequence = self.next_sequence();
                self.accepted.total_events = self.accepted.total_events.saturating_add(1);
                self.accepted.health_events = self.accepted.health_events.saturating_add(1);
                self.accept_reported_health(health.state, health.counters);
                self.sync_assessment_health();
                EventDisposition::Accepted {
                    event: AcceptedEvent::Health { sequence },
                }
            }
            (AcceptedScope::Sensor, EyeEvent::Gap(gap)) => {
                self.logical_now_ms = self.logical_now_ms.max(gap.from_ts.max(gap.to_ts));
                let sequence = self.next_sequence();
                self.accepted.total_events = self.accepted.total_events.saturating_add(1);
                self.accepted.gap_events = self.accepted.gap_events.saturating_add(1);
                self.record_gap(GapSource::ReportedEvent, gap, sequence);
                EventDisposition::Accepted {
                    event: AcceptedEvent::Gap { sequence },
                }
            }
            // SessionFence and EyeEvent are designed so these combinations are
            // unreachable. Treating them as diagnostic bugs would add a new
            // side effect, so keep the invariant local and explicit.
            (AcceptedScope::Sensor, EyeEvent::Flow(_))
            | (AcceptedScope::DiagnosticFlow, EyeEvent::Health(_) | EyeEvent::Gap(_))
            | (AcceptedScope::AttributedFlow { .. }, EyeEvent::Health(_) | EyeEvent::Gap(_)) => {
                unreachable!("session fence returned a scope incompatible with the event")
            }
        }
    }

    fn poll_health(&mut self, now_ms: u64) {
        self.logical_now_ms = self.logical_now_ms.max(now_ms);
        let poll = self
            .health_tracker
            .poll(self.logical_now_ms, self.counters.snapshot());
        self.absorb_health_poll(poll);
        self.sync_assessment_health();
        self.assessor.poll(self.logical_now_ms);
    }

    fn absorb_health_poll(&mut self, poll: HealthPoll) {
        self.health_counters = poll.counters;
        let Some(pending) = poll.pending_gap else {
            return;
        };

        let accepted = match pending.drop_window_token() {
            Some(token) => self.counters.acknowledge_drop_window(token),
            None => true,
        };
        if !accepted {
            // A producer extended the drop window after our snapshot. Leave the
            // old tracker token pending; the next poll will replace it with the
            // exact expanded window instead of double-counting old drops.
            return;
        }

        let sequence = self.next_sequence();
        self.record_pending_gap(pending, sequence);
        let acknowledged = self.health_tracker.acknowledge_gap(pending.token());
        debug_assert!(
            acknowledged,
            "accepted pending Gap must still own its token"
        );
    }

    fn record_pending_gap(&mut self, pending: PendingGap, sequence: u64) {
        self.record_gap_fields(
            GapSource::ProducerCounters,
            pending.from_ts,
            pending.to_ts,
            pending.dropped_events,
            sequence,
        );
    }

    fn record_gap(&mut self, source: GapSource, gap: GapEvent, sequence: u64) {
        self.record_gap_fields(source, gap.from_ts, gap.to_ts, gap.dropped_events, sequence);
    }

    fn record_gap_fields(
        &mut self,
        source: GapSource,
        from_ts: u64,
        to_ts: u64,
        dropped_events: u64,
        sequence: u64,
    ) {
        let (from_ts, to_ts) = (from_ts.min(to_ts), from_ts.max(to_ts));
        self.gaps.total_gaps = self.gaps.total_gaps.saturating_add(1);
        match source {
            GapSource::ProducerCounters => {
                self.gaps.producer_counter_gaps = self.gaps.producer_counter_gaps.saturating_add(1);
                self.gaps.producer_dropped_events = self
                    .gaps
                    .producer_dropped_events
                    .saturating_add(dropped_events);
            }
            GapSource::ReportedEvent => {
                self.gaps.reported_event_gaps = self.gaps.reported_event_gaps.saturating_add(1);
                self.gaps.reported_dropped_events = self
                    .gaps
                    .reported_dropped_events
                    .saturating_add(dropped_events);
            }
        }
        self.gaps.last = Some(ObservedGap {
            sequence,
            source,
            from_ts,
            to_ts,
            dropped_events,
        });
        self.last_gap_sequence = Some(sequence);
        self.last_external_gap_at_ms = Some(self.logical_now_ms.max(to_ts));
        self.mark_assessment_unreliable(SensorInvalidationReason::Gap);
    }

    fn accept_reported_health(&mut self, state: EyeHealthState, counters: EyeHealthCounters) {
        self.last_reported_health_counters = Some(counters);
        match (self.reported_terminal_state, state) {
            (Some(EyeHealthState::Stopped), _) => {}
            (_, EyeHealthState::Stopped) => {
                self.reported_terminal_state = Some(EyeHealthState::Stopped);
                self.last_reported_health_state = Some(EyeHealthState::Stopped);
            }
            (Some(EyeHealthState::Blind), _) => {}
            (_, EyeHealthState::Blind) => {
                self.reported_terminal_state = Some(EyeHealthState::Blind);
                self.last_reported_health_state = Some(EyeHealthState::Blind);
            }
            (None, EyeHealthState::Ready | EyeHealthState::Degraded) => {
                self.last_reported_health_state = Some(state);
            }
            // Only Blind and Stopped are stored as terminal states.
            (Some(EyeHealthState::Ready | EyeHealthState::Degraded), _) => {
                unreachable!("non-terminal health cannot be stored as terminal")
            }
        }
    }

    fn effective_health_state(&self) -> EyeHealthState {
        let mut state = self.health_tracker.state();
        if let Some(reported) = self.last_reported_health_state {
            state = less_reliable_health(state, reported);
        }
        if self.last_external_gap_at_ms.is_some_and(|gap_at| {
            self.logical_now_ms.saturating_sub(gap_at) < HEALTH_CLEAN_WINDOW_MS
        }) {
            state = less_reliable_health(state, EyeHealthState::Degraded);
        }
        state
    }

    fn close_after_receiver_failure(&mut self, now_ms: u64) {
        // Close first: even if a sender races with teardown, its late event can
        // no longer cross the generation fence.
        self.fence.close();
        self.logical_now_ms = self.logical_now_ms.max(now_ms);
        let poll = self
            .health_tracker
            .mark_unexpected_failure(self.logical_now_ms, self.counters.snapshot());
        self.absorb_health_poll(poll);
        self.receiver_failed = true;
        if self.reported_terminal_state != Some(EyeHealthState::Stopped) {
            self.reported_terminal_state = Some(EyeHealthState::Blind);
            self.last_reported_health_state = Some(EyeHealthState::Blind);
        }
        self.mark_assessment_unreliable(SensorInvalidationReason::ReceiverFailure);
    }

    fn observe_attributed_flow(
        &mut self,
        category: &str,
        lane_generation: LaneGeneration,
        flow: &FlowEvent,
    ) -> Option<String> {
        if !self.assessor.sensor_reliable() || flow.lane_generation != Some(lane_generation) {
            return None;
        }
        let registry = self.registry.as_ref()?;
        let Attribution::Matched { target, owner } = registry.attribute_active(&flow.domain) else {
            return None;
        };
        if owner.category != category {
            return None;
        }
        let _ = self
            .assessor
            .observe_flow(category, &target, flow, self.logical_now_ms);
        if !self.assessor.sensor_reliable() {
            self.last_assessment_unreliable_at_ms = Some(self.logical_now_ms);
        }
        Some(target)
    }

    fn record_confirmation_flow(
        &mut self,
        sequence: u64,
        category: &str,
        lane_generation: LaneGeneration,
        target: String,
        flow: &FlowEvent,
    ) {
        if flow.transport != super::contracts::Transport::Tls {
            return;
        }
        if self.confirmation_flows.len() == MAX_CONFIRMATION_FLOWS {
            self.confirmation_flows.pop_front();
        }
        self.confirmation_flows.push_back(ConfirmationFlow {
            sequence,
            category: category.to_owned(),
            lane_generation,
            flow_id: flow.flow_id,
            target,
            diagnosis: flow.diagnosis,
            monotonic_ts: flow.monotonic_ts,
        });
    }

    fn sync_assessment_health(&mut self) {
        if self.effective_health_state() == EyeHealthState::Ready {
            if !self.assessor.sensor_reliable()
                && self
                    .last_assessment_unreliable_at_ms
                    .is_some_and(|unreliable_at| {
                        self.logical_now_ms.saturating_sub(unreliable_at) >= HEALTH_CLEAN_WINDOW_MS
                    })
            {
                self.assessor.mark_sensor_ready();
                self.last_assessment_unreliable_at_ms = None;
            }
        } else {
            self.mark_assessment_unreliable(SensorInvalidationReason::Health);
        }
    }

    fn mark_assessment_unreliable(&mut self, reason: SensorInvalidationReason) {
        if self.assessor.sensor_reliable() {
            self.last_assessment_unreliable_at_ms = Some(self.logical_now_ms);
            self.assessor.invalidate(reason);
        } else if matches!(
            reason,
            SensorInvalidationReason::Gap
                | SensorInvalidationReason::CounterRegression
                | SensorInvalidationReason::ReceiverFailure
                | SensorInvalidationReason::EvidenceOverflow
        ) {
            self.last_assessment_unreliable_at_ms = Some(self.logical_now_ms);
        }
        self.confirmation_flows.clear();
    }

    fn next_sequence(&mut self) -> u64 {
        self.observation_sequence = self.observation_sequence.wrapping_add(1);
        if self.observation_sequence == 0 {
            self.observation_sequence = 1;
        }
        self.observation_sequence
    }
}

impl Drop for ObserveOnlyManager {
    fn drop(&mut self) {
        self.fence.close();
    }
}

fn less_reliable_health(left: EyeHealthState, right: EyeHealthState) -> EyeHealthState {
    if health_rank(left) >= health_rank(right) {
        left
    } else {
        right
    }
}

const fn health_rank(state: EyeHealthState) -> u8 {
    match state {
        EyeHealthState::Ready => 0,
        EyeHealthState::Degraded => 1,
        EyeHealthState::Blind => 2,
        EyeHealthState::Stopped => 3,
    }
}

const fn passive_evidence(evidence: super::assessment::EvidenceSummary) -> PassiveEvidenceSummary {
    PassiveEvidenceSummary {
        reset_after_client_hello_flows: evidence.reset_flows as u32,
        reset_targets: evidence.reset_targets as u32,
        confirmed_tls_blackhole_flows: evidence.blackhole_flows as u32,
        blackhole_targets: evidence.blackhole_targets as u32,
    }
}

fn gate_report_valid_until(now_ms: u64) -> u64 {
    let ttl_ms = GATE_REPORT_TTL
        .as_secs()
        .saturating_mul(1_000)
        .saturating_add(u64::from(GATE_REPORT_TTL.subsec_millis()));
    now_ms.saturating_add(ttl_ms)
}

const fn assessment_classification(classification: GateClassification) -> AssessmentClassification {
    match classification {
        GateClassification::Stable => AssessmentClassification::AwaitingEvidence,
        GateClassification::Offline => AssessmentClassification::Offline,
        GateClassification::DnsFailure => AssessmentClassification::DnsFailure,
        GateClassification::UpstreamDegraded => AssessmentClassification::UpstreamDegraded,
        GateClassification::TargetUnavailable => AssessmentClassification::TargetUnavailable,
        GateClassification::ServiceSlow => AssessmentClassification::ServiceSlow,
        GateClassification::DpiSuspected => AssessmentClassification::DpiSuspected,
        GateClassification::DpiBlocked => AssessmentClassification::DpiBlocked,
        GateClassification::SensorUnreliable => AssessmentClassification::SensorUnreliable,
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use crate::eyes::Diagnosis;

    use super::*;
    use crate::legacy_reliability::contracts::{EventEnvelope, FlowEvent, HealthEvent, Transport};
    use crate::legacy_reliability::environment_gate::{GateClassification, GateReport};
    use crate::legacy_reliability::ingress::{self, ControlIngressResult, FlowIngressResult};
    use crate::legacy_reliability::policy::PresumedIntent;
    use crate::legacy_reliability::target_registry::{LegacyConfigRecord, TargetRegistry};

    const SESSION: u64 = 10;
    const SENSOR: u64 = 20;
    const REGISTRY: u64 = 30;
    const LANE: u64 = 40;

    fn envelope(session: u64, sensor: u64, registry: u64) -> EventEnvelope {
        EventEnvelope::new(session.into(), sensor.into(), registry.into())
    }

    fn current_envelope() -> EventEnvelope {
        envelope(SESSION, SENSOR, REGISTRY)
    }

    fn flow(
        event_envelope: EventEnvelope,
        category: Option<&str>,
        lane_generation: Option<u64>,
        ts: u64,
    ) -> FlowEvent {
        FlowEvent::new(
            event_envelope,
            category.map(str::to_owned),
            lane_generation.map(Into::into),
            ts,
            "example.com",
            IpAddr::V4(Ipv4Addr::new(203, 0, 113, 10)),
            Transport::Tls,
            Diagnosis::Working,
            "server_hello",
            ts,
        )
        .unwrap()
    }

    fn flow_for(
        registry_version: RegistryVersion,
        domain: &str,
        flow_id: u64,
        diagnosis: Diagnosis,
        ts: u64,
    ) -> FlowEvent {
        FlowEvent::new(
            EventEnvelope::new(SESSION.into(), SENSOR.into(), registry_version),
            Some("discord".to_owned()),
            Some(LANE.into()),
            flow_id,
            domain,
            IpAddr::V4(Ipv4Addr::new(203, 0, 113, 10)),
            Transport::Tls,
            diagnosis,
            "typed",
            ts,
        )
        .unwrap()
    }

    fn production_fixture() -> (ObserveOnlyManager, Arc<TargetRegistry>) {
        let active = LegacyConfigRecord::new(
            "discord",
            "discord_1.conf",
            "--wf-tcp=443 --hostlist=lists/active.txt",
        )
        .with_hostlist("lists/active.txt", "one.example\ntwo.example\n");
        let candidate = LegacyConfigRecord::new(
            "discord",
            "discord_2.conf",
            "--wf-tcp=443 --hostlist=lists/candidate.txt",
        )
        .with_hostlist("lists/candidate.txt", "one.example\ntwo.example\n");
        let registry = Arc::new(
            TargetRegistry::from_records_with_active_selections(
                [active, candidate],
                [("discord", "discord_1.conf")],
            )
            .unwrap(),
        );
        let (_ingress, receiver) = ingress::channel();
        let counters = Arc::new(AtomicHealthCounters::default());
        let manager = ObserveOnlyManager::new_with_registry(
            LegacySessionContext::new(
                SESSION.into(),
                vec!["discord".to_owned()],
                NetworkFingerprint::Stable {
                    key: "test-network".to_owned(),
                },
            ),
            SENSOR.into(),
            Arc::clone(&registry),
            BTreeMap::from([("discord".to_owned(), LANE.into())]),
            counters,
            receiver,
        )
        .unwrap();
        (manager, registry)
    }

    fn fixture() -> (ObserveOnlyManager, super::super::ingress::LegacyIngress) {
        let (ingress, receiver) = ingress::channel();
        let manager = ObserveOnlyManager::new(
            LegacySessionContext::new(
                SESSION.into(),
                vec!["discord".to_owned()],
                NetworkFingerprint::Stable {
                    key: "gateway:test".to_owned(),
                },
            ),
            SENSOR.into(),
            REGISTRY.into(),
            BTreeMap::from([("discord".to_owned(), LANE.into())]),
            ingress.counters(),
            receiver,
        )
        .unwrap();
        (manager, ingress)
    }

    #[test]
    fn stale_session_lane_and_registry_have_zero_accepted_effects() {
        let (mut manager, _ingress) = fixture();

        let stale_session = EyeEvent::Health(HealthEvent {
            envelope: envelope(SESSION - 1, SENSOR, REGISTRY),
            state: EyeHealthState::Blind,
            counters: EyeHealthCounters {
                packet_count: 99,
                parse_errors: 99,
                queue_drops: 99,
                last_event_ts: Some(99),
            },
        });
        let stale_registry = EyeEvent::Gap(GapEvent {
            envelope: envelope(SESSION, SENSOR, REGISTRY - 1),
            from_ts: 10,
            to_ts: 20,
            dropped_events: 50,
        });
        let stale_lane = EyeEvent::Flow(flow(
            current_envelope(),
            Some("discord"),
            Some(LANE - 1),
            30,
        ));

        assert_eq!(
            manager.process_event(1, stale_session),
            EventDisposition::Rejected {
                reason: RejectionReason::Session
            }
        );
        assert_eq!(
            manager.process_event(2, stale_registry),
            EventDisposition::Rejected {
                reason: RejectionReason::RegistryVersion
            }
        );
        assert_eq!(
            manager.process_event(3, stale_lane),
            EventDisposition::Rejected {
                reason: RejectionReason::LaneGeneration
            }
        );

        let snapshot = manager.snapshot();
        assert_eq!(snapshot.accepted, AcceptedEventCounters::default());
        assert_eq!(snapshot.gaps, GapStatus::default());
        assert_eq!(snapshot.health.state, EyeHealthState::Ready);
        assert_eq!(snapshot.health.last_reported_state, None);
        assert_eq!(snapshot.rejected.total_events, 3);
        assert_eq!(snapshot.rejected.session, 1);
        assert_eq!(snapshot.rejected.registry_version, 1);
        assert_eq!(snapshot.rejected.lane_generation, 1);
    }

    #[test]
    fn control_gap_is_observed_before_later_flow_evidence() {
        let (mut manager, ingress) = fixture();
        assert_eq!(
            ingress.try_flow(flow(current_envelope(), Some("discord"), Some(LANE), 20,)),
            FlowIngressResult::Accepted
        );
        assert_eq!(
            ingress.try_gap(GapEvent {
                envelope: current_envelope(),
                from_ts: 10,
                to_ts: 15,
                dropped_events: 2,
            }),
            ControlIngressResult::Accepted
        );

        assert!(matches!(
            manager.try_process_next(20),
            Some(EventDisposition::Accepted {
                event: AcceptedEvent::Gap { .. }
            })
        ));
        assert!(matches!(
            manager.try_process_next(20),
            Some(EventDisposition::Accepted {
                event: AcceptedEvent::AttributedFlow { .. }
            })
        ));

        let snapshot = manager.snapshot();
        assert_eq!(snapshot.gaps.reported_event_gaps, 1);
        assert_eq!(snapshot.accepted.attributed_flows, 1);
        assert!(
            snapshot.last_gap_sequence.unwrap() < snapshot.last_accepted_flow_sequence.unwrap()
        );
    }

    #[test]
    fn producer_counter_gap_is_committed_before_flow() {
        let (mut manager, ingress) = fixture();
        ingress.counters().record_queue_drops(3, 50);
        assert_eq!(
            ingress.try_flow(flow(current_envelope(), Some("discord"), Some(LANE), 60,)),
            FlowIngressResult::Accepted
        );

        assert!(matches!(
            manager.try_process_next(60),
            Some(EventDisposition::Accepted {
                event: AcceptedEvent::AttributedFlow { .. }
            })
        ));
        let snapshot = manager.snapshot();
        assert_eq!(snapshot.gaps.producer_counter_gaps, 1);
        assert_eq!(snapshot.gaps.producer_dropped_events, 3);
        assert!(
            snapshot.last_gap_sequence.unwrap() < snapshot.last_accepted_flow_sequence.unwrap()
        );
        assert_eq!(snapshot.health.state, EyeHealthState::Degraded);
    }

    #[test]
    fn confirmation_journal_is_sequence_fenced_and_cleared_by_gap() {
        let (mut manager, registry) = production_fixture();
        assert!(manager.snapshot().confirmation_flows.is_empty());

        let disposition = manager.process_event(
            10,
            EyeEvent::Flow(flow_for(
                registry.version(),
                "one.example",
                77,
                Diagnosis::Working,
                10,
            )),
        );
        let sequence = match disposition {
            EventDisposition::Accepted {
                event: AcceptedEvent::AttributedFlow { sequence, .. },
            } => sequence,
            other => panic!("unexpected flow disposition: {other:?}"),
        };
        let snapshot = manager.snapshot();
        assert_eq!(snapshot.confirmation_flows.len(), 1);
        assert_eq!(snapshot.confirmation_flows[0].sequence, sequence);
        assert_eq!(snapshot.confirmation_flows[0].flow_id, 77);
        assert_eq!(snapshot.confirmation_flows[0].target, "one.example");

        manager.process_event(
            11,
            EyeEvent::Gap(GapEvent {
                envelope: EventEnvelope::new(SESSION.into(), SENSOR.into(), registry.version()),
                from_ts: 10,
                to_ts: 11,
                dropped_events: 1,
            }),
        );
        assert!(manager.snapshot().confirmation_flows.is_empty());
    }

    #[test]
    fn registry_correlated_reset_quorum_produces_unexecuted_switch_intent() {
        let (mut manager, registry) = production_fixture();
        for (flow_id, domain) in [
            (1, "api.one.example"),
            (2, "cdn.one.example"),
            (3, "two.example"),
        ] {
            let event = EyeEvent::Flow(flow_for(
                registry.version(),
                domain,
                flow_id,
                Diagnosis::TcpReset,
                flow_id,
            ));
            assert!(matches!(
                manager.process_event(flow_id, event),
                EventDisposition::Accepted { .. }
            ));
        }

        let pending = manager.snapshot();
        assert_eq!(
            pending.lanes[0].phase,
            super::super::assessment::LanePhase::GatePending
        );
        assert_eq!(pending.lanes[0].evidence.reset_flows, 3);
        assert_eq!(pending.lanes[0].evidence.reset_targets, 2);

        let local = LocalNetworkSnapshot {
            online: true,
            interface_up: true,
            default_route_available: true,
            gateway_reachable: true,
            network_fingerprint: NetworkFingerprint::Stable {
                key: "test-network".to_owned(),
            },
        };
        let prepared = manager.take_environment_gate_request(local).unwrap();
        assert_eq!(
            prepared.request.category_targets,
            ["cdn.one.example", "two.example"]
        );
        let report = GateReport {
            fence: prepared.request.fence.clone(),
            category: "discord".to_owned(),
            classification: GateClassification::DpiSuspected,
            controls: Vec::new(),
            category_targets: Vec::new(),
            baseline_latency_ms: Some(100),
            slow_threshold_ms: Some(1_600),
            generated_at_monotonic_ms: 4,
            valid_until_monotonic_ms: 10_004,
        };
        assert!(manager.apply_environment_gate_report(&prepared, &report, 4));

        let assessed = manager.snapshot();
        assert_eq!(
            assessed.lanes[0].classification,
            AssessmentClassification::DpiSuspected
        );
        assert_eq!(
            assessed.presumed_intent,
            PresumedIntent::SwitchLane {
                category: "discord".to_owned(),
                candidate_config: "discord_2.conf".to_owned(),
                reason: AssessmentClassification::DpiSuspected,
            }
        );
        // Phase 2 observes only: neither registry selection nor generation is
        // changed by assessment or Brain output.
        assert_eq!(registry.active_config("discord"), Some("discord_1.conf"));
        assert_eq!(
            assessed.session.lane_generations["discord"],
            LaneGeneration::new(LANE)
        );
    }

    #[test]
    fn production_timeline_holds_dpi_blocked_until_ten_clean_working_seconds() {
        let (mut manager, registry) = production_fixture();
        for (flow_id, domain) in [(1, "one.example"), (2, "two.example")] {
            manager.process_event(
                flow_id,
                EyeEvent::Flow(flow_for(
                    registry.version(),
                    domain,
                    flow_id,
                    Diagnosis::TlsBlackhole,
                    flow_id,
                )),
            );
        }
        let prepared = manager
            .take_environment_gate_request(LocalNetworkSnapshot {
                online: true,
                interface_up: true,
                default_route_available: true,
                gateway_reachable: true,
                network_fingerprint: NetworkFingerprint::Stable {
                    key: "test-network".to_owned(),
                },
            })
            .unwrap();
        let report = GateReport {
            fence: prepared.request.fence.clone(),
            category: "discord".into(),
            classification: GateClassification::DpiBlocked,
            controls: Vec::new(),
            category_targets: Vec::new(),
            baseline_latency_ms: Some(100),
            slow_threshold_ms: Some(1_600),
            generated_at_monotonic_ms: 3,
            valid_until_monotonic_ms: 10_003,
        };
        assert!(manager.apply_environment_gate_report(&prepared, &report, 3));

        let blocked = manager.snapshot();
        assert_eq!(
            blocked.lanes[0].classification,
            AssessmentClassification::DpiBlocked
        );
        assert_eq!(blocked.lanes[0].cooldown_until_ms, None);
        assert_eq!(
            blocked.presumed_intent,
            PresumedIntent::SwitchLane {
                category: "discord".into(),
                candidate_config: "discord_2.conf".into(),
                reason: AssessmentClassification::DpiBlocked,
            }
        );

        manager.process_event(
            4,
            EyeEvent::Flow(flow_for(
                registry.version(),
                "one.example",
                3,
                Diagnosis::Working,
                4,
            )),
        );
        manager.process_event(
            5,
            EyeEvent::Flow(flow_for(
                registry.version(),
                "two.example",
                4,
                Diagnosis::Working,
                5,
            )),
        );
        assert_eq!(
            manager.snapshot().lanes[0].classification,
            AssessmentClassification::DpiBlocked
        );

        // A single adverse flow just before the clean deadline invalidates the
        // first Working quorum. Silence after it cannot recover the lane.
        manager.process_event(
            10_004,
            EyeEvent::Flow(flow_for(
                registry.version(),
                "one.example",
                5,
                Diagnosis::TcpReset,
                10_004,
            )),
        );
        manager.poll(20_004);
        let interrupted = manager.snapshot();
        assert_eq!(
            interrupted.lanes[0].classification,
            AssessmentClassification::DpiBlocked
        );
        assert_eq!(interrupted.lanes[0].evidence.working_flows, 0);
        assert!(manager
            .take_environment_gate_request(LocalNetworkSnapshot {
                online: true,
                interface_up: true,
                default_route_available: true,
                gateway_reachable: true,
                network_fingerprint: NetworkFingerprint::Stable {
                    key: "test-network".to_owned(),
                },
            })
            .is_none());

        manager.process_event(
            20_005,
            EyeEvent::Flow(flow_for(
                registry.version(),
                "one.example",
                6,
                Diagnosis::Working,
                20_005,
            )),
        );
        manager.process_event(
            20_006,
            EyeEvent::Flow(flow_for(
                registry.version(),
                "two.example",
                7,
                Diagnosis::Working,
                20_006,
            )),
        );
        manager.poll(30_005);
        assert_eq!(
            manager.snapshot().lanes[0].classification,
            AssessmentClassification::DpiBlocked
        );
        manager.poll(30_006);
        let recovered = manager.snapshot();
        assert_eq!(
            recovered.lanes[0].classification,
            AssessmentClassification::Working
        );
        assert_eq!(
            recovered.presumed_intent,
            PresumedIntent::Wait {
                reason: AssessmentClassification::Working,
            }
        );
    }

    #[test]
    fn gap_invalidates_inflight_gate_and_all_correlated_evidence() {
        let (mut manager, registry) = production_fixture();
        for (flow_id, domain) in [(1, "one.example"), (2, "two.example"), (3, "two.example")] {
            manager.process_event(
                flow_id,
                EyeEvent::Flow(flow_for(
                    registry.version(),
                    domain,
                    flow_id,
                    Diagnosis::TcpReset,
                    flow_id,
                )),
            );
        }
        let prepared = manager
            .take_environment_gate_request(LocalNetworkSnapshot {
                online: true,
                interface_up: true,
                default_route_available: true,
                gateway_reachable: true,
                network_fingerprint: NetworkFingerprint::Stable {
                    key: "test-network".to_owned(),
                },
            })
            .unwrap();
        manager.process_event(
            5,
            EyeEvent::Gap(GapEvent {
                envelope: EventEnvelope::new(SESSION.into(), SENSOR.into(), registry.version()),
                from_ts: 2,
                to_ts: 5,
                dropped_events: 1,
            }),
        );
        let report = GateReport {
            fence: prepared.request.fence.clone(),
            category: "discord".into(),
            classification: GateClassification::DpiSuspected,
            controls: Vec::new(),
            category_targets: Vec::new(),
            baseline_latency_ms: Some(100),
            slow_threshold_ms: Some(1_600),
            generated_at_monotonic_ms: 6,
            valid_until_monotonic_ms: 10_006,
        };
        assert!(!manager.apply_environment_gate_report(&prepared, &report, 6));
        let snapshot = manager.snapshot();
        assert_eq!(
            snapshot.lanes[0].classification,
            AssessmentClassification::SensorUnreliable
        );
        assert_eq!(
            snapshot.lanes[0].evidence,
            super::super::assessment::EvidenceSummary::default()
        );
    }

    #[test]
    fn queued_gate_dispatch_uses_current_clock_not_incident_time() {
        let (mut manager, registry) = production_fixture();
        for (flow_id, domain) in [
            (1, "api.one.example"),
            (2, "cdn.one.example"),
            (3, "two.example"),
        ] {
            manager.process_event(
                flow_id,
                EyeEvent::Flow(flow_for(
                    registry.version(),
                    domain,
                    flow_id,
                    Diagnosis::TcpReset,
                    flow_id,
                )),
            );
        }
        manager.poll(6_000);

        let prepared = manager
            .take_environment_gate_request(LocalNetworkSnapshot {
                online: true,
                interface_up: true,
                default_route_available: true,
                gateway_reachable: true,
                network_fingerprint: NetworkFingerprint::Stable {
                    key: "test-network".to_owned(),
                },
            })
            .unwrap();
        assert_eq!(prepared.pending.requested_at_ms, 3);
        assert_eq!(prepared.request.requested_at_monotonic_ms, 6_000);
    }

    #[test]
    fn expired_gate_report_retries_with_fresh_identity_and_can_be_applied() {
        let (mut manager, registry) = production_fixture();
        for (flow_id, domain) in [
            (1, "api.one.example"),
            (2, "cdn.one.example"),
            (3, "two.example"),
        ] {
            manager.process_event(
                flow_id,
                EyeEvent::Flow(flow_for(
                    registry.version(),
                    domain,
                    flow_id,
                    Diagnosis::TcpReset,
                    flow_id,
                )),
            );
        }
        let local = LocalNetworkSnapshot {
            online: true,
            interface_up: true,
            default_route_available: true,
            gateway_reachable: true,
            network_fingerprint: NetworkFingerprint::Stable {
                key: "test-network".to_owned(),
            },
        };
        let prepared = manager
            .take_environment_gate_request(local.clone())
            .unwrap();
        let stale = GateReport {
            fence: prepared.request.fence.clone(),
            category: prepared.pending.category.clone(),
            classification: GateClassification::DpiSuspected,
            controls: Vec::new(),
            category_targets: Vec::new(),
            baseline_latency_ms: Some(100),
            slow_threshold_ms: Some(1_600),
            generated_at_monotonic_ms: 4,
            valid_until_monotonic_ms: 5,
        };
        assert!(!manager.apply_environment_gate_report(&prepared, &stale, 20));

        let retry = manager.take_environment_gate_request(local).unwrap();
        assert_ne!(retry.pending.gate_id, prepared.pending.gate_id);
        assert_eq!(
            retry.pending.requested_at_ms,
            prepared.pending.requested_at_ms
        );
        assert_eq!(retry.request.requested_at_monotonic_ms, 20);
        assert_eq!(
            retry.pending.evidence_epoch,
            prepared.pending.evidence_epoch
        );
        assert_eq!(
            retry.pending.lane_generation,
            prepared.pending.lane_generation
        );

        let fresh = GateReport {
            fence: retry.request.fence.clone(),
            category: retry.pending.category.clone(),
            classification: GateClassification::DpiSuspected,
            controls: Vec::new(),
            category_targets: Vec::new(),
            baseline_latency_ms: Some(100),
            slow_threshold_ms: Some(1_600),
            generated_at_monotonic_ms: 20,
            valid_until_monotonic_ms: 30,
        };
        assert!(manager.apply_environment_gate_report(&retry, &fresh, 21));
        assert_eq!(
            manager.snapshot().lanes[0].classification,
            AssessmentClassification::DpiSuspected
        );
    }

    #[test]
    fn degraded_and_blind_sensor_never_become_a_healthy_assessment() {
        let (mut manager, _ingress) = fixture();
        let degraded = EyeEvent::Health(HealthEvent {
            envelope: current_envelope(),
            state: EyeHealthState::Degraded,
            counters: EyeHealthCounters::default(),
        });
        manager.process_event(10, degraded);
        manager.process_event(
            11,
            EyeEvent::Flow(flow(current_envelope(), Some("discord"), Some(LANE), 11)),
        );
        assert_eq!(manager.snapshot().health.state, EyeHealthState::Degraded);

        let blind = EyeEvent::Health(HealthEvent {
            envelope: current_envelope(),
            state: EyeHealthState::Blind,
            counters: EyeHealthCounters::default(),
        });
        manager.process_event(20, blind);
        manager.process_event(
            50_000,
            EyeEvent::Health(HealthEvent {
                envelope: current_envelope(),
                state: EyeHealthState::Ready,
                counters: EyeHealthCounters::default(),
            }),
        );
        assert_eq!(manager.poll(60_000).health.state, EyeHealthState::Blind);
        assert_eq!(manager.snapshot().accepted.attributed_flows, 1);
    }

    #[test]
    fn shutdown_closes_fence_before_late_event() {
        let (mut manager, _ingress) = fixture();
        let stopped = manager.shutdown(10);
        assert!(stopped.session.closed);
        assert_eq!(stopped.health.state, EyeHealthState::Stopped);

        let late = EyeEvent::Flow(flow(current_envelope(), Some("discord"), Some(LANE), 20));
        assert_eq!(
            manager.process_event(20, late),
            EventDisposition::Rejected {
                reason: RejectionReason::Closed
            }
        );
        let snapshot = manager.snapshot();
        assert_eq!(snapshot.accepted, AcceptedEventCounters::default());
        assert_eq!(snapshot.rejected.closed, 1);
        assert_eq!(snapshot.health.state, EyeHealthState::Stopped);
    }

    #[tokio::test]
    async fn receiver_failure_closes_fence_and_becomes_blind() {
        let (mut manager, ingress) = fixture();
        drop(ingress);

        assert_eq!(manager.recv_next(100).await, ReceiveOutcome::ReceiverClosed);
        let snapshot = manager.snapshot();
        assert!(snapshot.session.closed);
        assert!(snapshot.health.receiver_failed);
        assert_eq!(snapshot.health.state, EyeHealthState::Blind);
    }

    #[test]
    fn snapshot_is_serializable_without_payload_data() {
        let (manager, _ingress) = fixture();
        let value = serde_json::to_value(manager.snapshot()).unwrap();

        assert_eq!(value["session"]["session_id"], SESSION);
        assert_eq!(value["session"]["sensor_generation"], SENSOR);
        assert_eq!(value["session"]["target_registry_version"], REGISTRY);
        assert_eq!(value["accepted"]["attributed_flows"], 0);
        assert_eq!(value["health"]["state"], "ready");
    }
}
