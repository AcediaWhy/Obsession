//! Side-effect-free observe-only runtime for Legacy reliability events.
//!
//! This module is deliberately limited to generation fencing and diagnostics.
//! It cannot invoke the Legacy Brain, mutate cache state, manage processes, or
//! emit UI events. Later phases may consume its snapshots, but Phase 1 only
//! records what the current Legacy session observed.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::Serialize;

use crate::dpi_engine::EngineKind;

use super::contracts::{
    EyeEvent, EyeHealthCounters, EyeHealthState, GapEvent, LaneGeneration, LegacySessionContext,
    NetworkFingerprint, RegistryVersion, SensorGeneration, SessionId,
};
use super::health::{
    AtomicHealthCounters, HealthPoll, HealthTracker, PendingGap, HEALTH_CLEAN_WINDOW_MS,
};
use super::ingress::{
    AcceptedScope, FenceBuildError, FenceRejection, LegacyIngressReceiver, SessionFence,
};

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
    receiver_failed: bool,
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
        let fence = SessionFence::new(
            context.clone(),
            sensor_generation,
            registry_version,
            lane_generations.clone(),
        )?;

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
            receiver_failed: false,
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
        self.snapshot()
    }

    pub fn snapshot(&self) -> ObserveOnlySnapshot {
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
        }
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
                let sequence = self.next_sequence();
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

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use crate::eyes::Diagnosis;

    use super::*;
    use crate::legacy_reliability::contracts::{EventEnvelope, FlowEvent, HealthEvent, Transport};
    use crate::legacy_reliability::ingress::{self, ControlIngressResult, FlowIngressResult};

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
