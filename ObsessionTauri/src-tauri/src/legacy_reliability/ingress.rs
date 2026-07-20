//! Bounded, nonblocking ingress and generation fencing for Legacy Eyes.
//!
//! Phase 1 deliberately stops at an observe-only boundary. Accepted events are
//! available to a future Reliability Manager, but this module has no process,
//! cache, UI, or legacy Brain side effects.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use tokio::sync::mpsc;

use super::contracts::{
    EventEnvelope, EyeEvent, FlowEvent, GapEvent, HealthEvent, LaneGeneration,
    LegacySessionContext, RegistryVersion, SensorGeneration, SessionId,
};
use super::health::AtomicHealthCounters;

pub const FLOW_QUEUE_CAPACITY: usize = 1_024;
pub const CONTROL_QUEUE_CAPACITY: usize = 64;

/// Capture-side sender. `try_*` methods never wait for the manager.
#[derive(Clone)]
pub struct LegacyIngress {
    flow_tx: mpsc::Sender<FlowEvent>,
    control_tx: mpsc::Sender<EyeEvent>,
    counters: Arc<AtomicHealthCounters>,
}

/// Manager-owned receivers. Control traffic cannot be displaced by a Flow
/// burst because it has a separate queue.
pub struct LegacyIngressReceiver {
    flow_rx: mpsc::Receiver<FlowEvent>,
    control_rx: mpsc::Receiver<EyeEvent>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowIngressResult {
    Accepted,
    DroppedFull,
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlIngressResult {
    Accepted,
    Full,
    Closed,
}

pub fn channel() -> (LegacyIngress, LegacyIngressReceiver) {
    channel_with_capacity(FLOW_QUEUE_CAPACITY, CONTROL_QUEUE_CAPACITY)
}

fn channel_with_capacity(
    flow_capacity: usize,
    control_capacity: usize,
) -> (LegacyIngress, LegacyIngressReceiver) {
    assert!(flow_capacity > 0, "flow queue capacity must be non-zero");
    assert!(
        control_capacity > 0,
        "control queue capacity must be non-zero"
    );
    let (flow_tx, flow_rx) = mpsc::channel(flow_capacity);
    let (control_tx, control_rx) = mpsc::channel(control_capacity);
    let counters = Arc::new(AtomicHealthCounters::default());
    (
        LegacyIngress {
            flow_tx,
            control_tx,
            counters,
        },
        LegacyIngressReceiver {
            flow_rx,
            control_rx,
        },
    )
}

impl LegacyIngress {
    pub fn counters(&self) -> Arc<AtomicHealthCounters> {
        Arc::clone(&self.counters)
    }

    pub fn try_flow(&self, event: FlowEvent) -> FlowIngressResult {
        let event_ts = event.monotonic_ts;
        match self.flow_tx.try_send(event) {
            Ok(()) => FlowIngressResult::Accepted,
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.counters.record_queue_drop(event_ts);
                FlowIngressResult::DroppedFull
            }
            Err(mpsc::error::TrySendError::Closed(_)) => FlowIngressResult::Closed,
        }
    }

    pub fn try_health(&self, event: HealthEvent) -> ControlIngressResult {
        self.try_control(EyeEvent::Health(event))
    }

    pub fn try_gap(&self, event: GapEvent) -> ControlIngressResult {
        self.try_control(EyeEvent::Gap(event))
    }

    fn try_control(&self, event: EyeEvent) -> ControlIngressResult {
        debug_assert!(!matches!(event, EyeEvent::Flow(_)));
        match self.control_tx.try_send(event) {
            Ok(()) => ControlIngressResult::Accepted,
            Err(mpsc::error::TrySendError::Full(_)) => ControlIngressResult::Full,
            Err(mpsc::error::TrySendError::Closed(_)) => ControlIngressResult::Closed,
        }
    }
}

impl LegacyIngressReceiver {
    /// Control-first receive for the observe-only manager loop.
    pub async fn recv(&mut self) -> Option<EyeEvent> {
        if let Ok(event) = self.control_rx.try_recv() {
            return Some(event);
        }

        tokio::select! {
            biased;
            control = self.control_rx.recv() => control,
            flow = self.flow_rx.recv() => flow.map(EyeEvent::Flow),
        }
    }

    pub fn try_recv_flow(&mut self) -> Option<FlowEvent> {
        self.flow_rx.try_recv().ok()
    }

    pub fn try_recv_control(&mut self) -> Option<EyeEvent> {
        self.control_rx.try_recv().ok()
    }
}

/// Immutable expected epochs plus an explicit close bit. This is the first
/// guard applied by the manager, before model, UI, cache, or process effects.
pub struct SessionFence {
    context: LegacySessionContext,
    sensor_generation: SensorGeneration,
    registry_version: RegistryVersion,
    lane_generations: BTreeMap<String, LaneGeneration>,
    closed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FenceBuildError {
    MissingLane { category: String },
    UnexpectedLane { category: String },
    DuplicateCategory { category: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FenceRejection {
    Closed,
    Session {
        expected: SessionId,
        actual: SessionId,
    },
    SensorGeneration {
        expected: SensorGeneration,
        actual: SensorGeneration,
    },
    RegistryVersion {
        expected: RegistryVersion,
        actual: RegistryVersion,
    },
    UnknownCategory {
        category: String,
    },
    LaneGeneration {
        category: String,
        expected: LaneGeneration,
        actual: LaneGeneration,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AcceptedScope {
    Sensor,
    DiagnosticFlow,
    AttributedFlow {
        category: String,
        lane_generation: LaneGeneration,
    },
}

impl SessionFence {
    pub fn new(
        context: LegacySessionContext,
        sensor_generation: SensorGeneration,
        registry_version: RegistryVersion,
        lane_generations: BTreeMap<String, LaneGeneration>,
    ) -> Result<Self, FenceBuildError> {
        let mut categories = BTreeSet::new();
        for category in context.active_categories() {
            if !categories.insert(category.clone()) {
                return Err(FenceBuildError::DuplicateCategory {
                    category: category.clone(),
                });
            }
            if !lane_generations.contains_key(category) {
                return Err(FenceBuildError::MissingLane {
                    category: category.clone(),
                });
            }
        }
        if let Some(category) = lane_generations
            .keys()
            .find(|category| !categories.contains(*category))
        {
            return Err(FenceBuildError::UnexpectedLane {
                category: category.clone(),
            });
        }

        Ok(Self {
            context,
            sensor_generation,
            registry_version,
            lane_generations,
            closed: false,
        })
    }

    pub fn close(&mut self) {
        self.closed = true;
    }

    pub const fn is_closed(&self) -> bool {
        self.closed
    }

    pub fn check(&self, event: &EyeEvent) -> Result<AcceptedScope, FenceRejection> {
        if self.closed {
            return Err(FenceRejection::Closed);
        }
        self.check_envelope(event.envelope())?;

        match event {
            EyeEvent::Health(_) | EyeEvent::Gap(_) => Ok(AcceptedScope::Sensor),
            EyeEvent::Flow(flow) => self.check_flow(flow),
        }
    }

    fn check_envelope(&self, envelope: EventEnvelope) -> Result<(), FenceRejection> {
        if envelope.session_id != self.context.session_id() {
            return Err(FenceRejection::Session {
                expected: self.context.session_id(),
                actual: envelope.session_id,
            });
        }
        if envelope.sensor_generation != self.sensor_generation {
            return Err(FenceRejection::SensorGeneration {
                expected: self.sensor_generation,
                actual: envelope.sensor_generation,
            });
        }
        if envelope.target_registry_version != self.registry_version {
            return Err(FenceRejection::RegistryVersion {
                expected: self.registry_version,
                actual: envelope.target_registry_version,
            });
        }
        Ok(())
    }

    fn check_flow(&self, flow: &FlowEvent) -> Result<AcceptedScope, FenceRejection> {
        let (Some(category), Some(actual)) = (&flow.category, flow.lane_generation) else {
            return Ok(AcceptedScope::DiagnosticFlow);
        };
        let Some(expected) = self.lane_generations.get(category).copied() else {
            return Err(FenceRejection::UnknownCategory {
                category: category.clone(),
            });
        };
        if actual != expected {
            return Err(FenceRejection::LaneGeneration {
                category: category.clone(),
                expected,
                actual,
            });
        }
        Ok(AcceptedScope::AttributedFlow {
            category: category.clone(),
            lane_generation: actual,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use crate::eyes::Diagnosis;

    use super::*;
    use crate::legacy_reliability::contracts::{
        EyeHealthCounters, EyeHealthState, FlowEvidence, NetworkFingerprint, Transport,
    };

    fn envelope(session: u64, sensor: u64, registry: u64) -> EventEnvelope {
        EventEnvelope::new(session.into(), sensor.into(), registry.into())
    }

    fn flow(
        event_envelope: EventEnvelope,
        category: Option<&str>,
        lane: Option<u64>,
        ts: u64,
    ) -> FlowEvent {
        FlowEvent::new(
            event_envelope,
            category.map(str::to_owned),
            lane.map(Into::into),
            ts,
            "example.com",
            IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1)),
            Transport::Tls,
            Diagnosis::Working,
            FlowEvidence::new("server_hello"),
            ts,
        )
        .unwrap()
    }

    fn fence() -> SessionFence {
        SessionFence::new(
            LegacySessionContext::new(
                10.into(),
                vec!["discord".to_string()],
                NetworkFingerprint::Unknown,
            ),
            20.into(),
            30.into(),
            BTreeMap::from([("discord".to_string(), 40.into())]),
        )
        .unwrap()
    }

    #[test]
    fn fence_accepts_current_attributed_and_diagnostic_events() {
        let fence = fence();
        let attributed = EyeEvent::Flow(flow(envelope(10, 20, 30), Some("discord"), Some(40), 1));
        assert_eq!(
            fence.check(&attributed),
            Ok(AcceptedScope::AttributedFlow {
                category: "discord".to_string(),
                lane_generation: 40.into(),
            })
        );

        let diagnostic = EyeEvent::Flow(flow(envelope(10, 20, 30), None, None, 2));
        assert_eq!(fence.check(&diagnostic), Ok(AcceptedScope::DiagnosticFlow));

        let health = EyeEvent::Health(HealthEvent {
            envelope: envelope(10, 20, 30),
            state: EyeHealthState::Ready,
            counters: EyeHealthCounters::default(),
        });
        assert_eq!(fence.check(&health), Ok(AcceptedScope::Sensor));
    }

    #[test]
    fn fence_rejects_every_stale_epoch() {
        let fence = fence();
        let stale_session = EyeEvent::Flow(flow(envelope(9, 20, 30), None, None, 1));
        assert!(matches!(
            fence.check(&stale_session),
            Err(FenceRejection::Session { .. })
        ));

        let stale_sensor = EyeEvent::Flow(flow(envelope(10, 19, 30), None, None, 2));
        assert!(matches!(
            fence.check(&stale_sensor),
            Err(FenceRejection::SensorGeneration { .. })
        ));

        let stale_registry = EyeEvent::Flow(flow(envelope(10, 20, 29), None, None, 3));
        assert!(matches!(
            fence.check(&stale_registry),
            Err(FenceRejection::RegistryVersion { .. })
        ));

        let stale_lane = EyeEvent::Flow(flow(envelope(10, 20, 30), Some("discord"), Some(39), 4));
        assert!(matches!(
            fence.check(&stale_lane),
            Err(FenceRejection::LaneGeneration { .. })
        ));
    }

    #[test]
    fn closed_fence_rejects_even_current_events() {
        let mut fence = fence();
        fence.close();
        let event = EyeEvent::Flow(flow(envelope(10, 20, 30), Some("discord"), Some(40), 1));
        assert_eq!(fence.check(&event), Err(FenceRejection::Closed));
    }

    #[test]
    fn fence_requires_exact_lane_set() {
        let context = LegacySessionContext::new(
            1.into(),
            vec!["discord".to_string()],
            NetworkFingerprint::Unknown,
        );
        assert_eq!(
            SessionFence::new(context.clone(), 1.into(), 1.into(), BTreeMap::new())
                .err()
                .unwrap(),
            FenceBuildError::MissingLane {
                category: "discord".to_string()
            }
        );
        assert_eq!(
            SessionFence::new(
                context,
                1.into(),
                1.into(),
                BTreeMap::from([
                    ("discord".to_string(), 1.into()),
                    ("other".to_string(), 1.into()),
                ]),
            )
            .err()
            .unwrap(),
            FenceBuildError::UnexpectedLane {
                category: "other".to_string()
            }
        );
    }

    #[tokio::test]
    async fn full_flow_queue_is_nonblocking_and_control_remains_available() {
        let (ingress, mut receiver) = channel_with_capacity(1, 1);
        assert_eq!(
            ingress.try_flow(flow(envelope(1, 1, 1), None, None, 10)),
            FlowIngressResult::Accepted
        );
        assert_eq!(
            ingress.try_flow(flow(envelope(1, 1, 1), None, None, 20)),
            FlowIngressResult::DroppedFull
        );
        assert_eq!(ingress.counters().snapshot().counters.queue_drops, 1);

        let health = HealthEvent {
            envelope: envelope(1, 1, 1),
            state: EyeHealthState::Degraded,
            counters: EyeHealthCounters::default(),
        };
        assert_eq!(
            ingress.try_health(health.clone()),
            ControlIngressResult::Accepted
        );
        assert_eq!(receiver.recv().await, Some(EyeEvent::Health(health)));
        assert!(matches!(receiver.try_recv_flow(), Some(event) if event.monotonic_ts == 10));
    }
}
