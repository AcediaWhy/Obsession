//! Cross-platform data contracts shared by Legacy Eyes and Reliability Manager.

use std::fmt;
use std::net::IpAddr;

use serde::{Deserialize, Serialize};

use crate::dpi_engine::EngineKind;
use crate::eyes::Diagnosis;

macro_rules! u64_newtype {
    ($name:ident) => {
        #[derive(
            Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub u64);

        impl $name {
            pub const fn new(value: u64) -> Self {
                Self(value)
            }

            pub const fn get(self) -> u64 {
                self.0
            }
        }

        impl From<u64> for $name {
            fn from(value: u64) -> Self {
                Self(value)
            }
        }

        impl From<$name> for u64 {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

u64_newtype!(SessionId);
u64_newtype!(LaneGeneration);
u64_newtype!(SensorGeneration);
u64_newtype!(RegistryVersion);
u64_newtype!(AttemptId);
u64_newtype!(ProcessStartIdentity);

/// Network identity captured at session start.
///
/// Only [`NetworkFingerprint::Stable`] is eligible for L1 reliability cache
/// reads and writes. Unstable and unknown identities remain diagnostic-only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum NetworkFingerprint {
    Stable { key: String },
    Unstable { reason: String },
    Unknown,
}

impl NetworkFingerprint {
    pub const fn is_stable(&self) -> bool {
        matches!(self, Self::Stable { .. })
    }

    pub fn stable_key(&self) -> Option<&str> {
        match self {
            Self::Stable { key } => Some(key),
            Self::Unstable { .. } | Self::Unknown => None,
        }
    }
}

/// Content-derived identity of one immutable Legacy configuration snapshot.
///
/// The value is deliberately opaque to the reliability model. Production code
/// computes it from the config and referenced bundled resources; recovery only
/// compares it for exact equality.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ConfigFingerprint(String);

impl ConfigFingerprint {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl From<String> for ConfigFingerprint {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for ConfigFingerprint {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl fmt::Display for ConfigFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Live values captured by the backend immediately before a recovery side
/// effect. UI approval never supplies this structure.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntentFence {
    pub session_id: SessionId,
    pub category: String,
    pub lane_generation: LaneGeneration,
    pub sensor_generation: SensorGeneration,
    pub registry_version: RegistryVersion,
    pub network_fingerprint: NetworkFingerprint,
}

/// Exact fence attached to every executor command and result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntentEnvelope {
    pub session_id: SessionId,
    pub attempt_id: AttemptId,
    pub category: String,
    pub expected_lane_generation: LaneGeneration,
    pub expected_sensor_generation: SensorGeneration,
    pub expected_registry_version: RegistryVersion,
    pub expected_network_fingerprint: NetworkFingerprint,
}

impl IntentEnvelope {
    pub fn from_fence(attempt_id: AttemptId, fence: &IntentFence) -> Self {
        Self {
            session_id: fence.session_id,
            attempt_id,
            category: fence.category.clone(),
            expected_lane_generation: fence.lane_generation,
            expected_sensor_generation: fence.sensor_generation,
            expected_registry_version: fence.registry_version,
            expected_network_fingerprint: fence.network_fingerprint.clone(),
        }
    }

    /// Exact comparison used immediately before stop, start, commit and cache
    /// writes. A generation increase is not interchangeable with equality.
    pub fn matches_fence(&self, current: &IntentFence) -> bool {
        self.session_id == current.session_id
            && self.category == current.category
            && self.expected_lane_generation == current.lane_generation
            && self.expected_sensor_generation == current.sensor_generation
            && self.expected_registry_version == current.registry_version
            && self.expected_network_fingerprint == current.network_fingerprint
    }
}

/// Unambiguous owner of a Windows process. A PID by itself is insufficient
/// because Windows can reuse it after process exit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessOwner {
    pub pid: u32,
    pub process_start_identity: ProcessStartIdentity,
    pub config_fingerprint: ConfigFingerprint,
    pub lane_generation: LaneGeneration,
}

impl ProcessOwner {
    pub fn owns(
        &self,
        config_fingerprint: &ConfigFingerprint,
        lane_generation: LaneGeneration,
    ) -> bool {
        &self.config_fingerprint == config_fingerprint && self.lane_generation == lane_generation
    }
}

/// Why candidate confirmation failed. Only failures that actually test the
/// candidate strategy are eligible for a negative cooldown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmationFailure {
    Strategy,
    Environment,
    Target,
    Sensor,
    MissingWorkingEvidence,
}

/// Executor phase at which a fresh fence invalidated a pending side effect.
/// This is distinct from a strategy/readiness failure: an environmental or
/// ownership change must abort or roll back without blaming the candidate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutorStage {
    Preflight,
    Stop,
    Start,
    Confirmation,
    Commit,
    Rollback,
}

impl ConfirmationFailure {
    pub const fn penalizes_candidate(self) -> bool {
        matches!(self, Self::Strategy)
    }
}

/// Typed completion emitted by the scoped executor or process supervisor.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ExecutorOutcome {
    /// Preflight installs the tentative Eyes/Manager plan before stopping the
    /// lane. The result is correlated with the old envelope, while subsequent
    /// actions use this exact refreshed sensor/registry/lane fence.
    PreflightPassed {
        refreshed_fence: IntentFence,
    },
    PreflightRejected,
    PreviousProcessMissing {
        previous_fingerprint: ConfigFingerprint,
    },
    ExecutionAborted {
        stage: ExecutorStage,
        reason: ConfirmationFailure,
    },
    Stopped {
        previous: ProcessOwner,
    },
    StopTimedOut {
        previous: ProcessOwner,
    },
    Ready {
        candidate: ProcessOwner,
    },
    StartFailed {
        candidate_fingerprint: ConfigFingerprint,
    },
    ConfirmationSucceeded {
        candidate: ProcessOwner,
    },
    ConfirmationFailed {
        candidate: ProcessOwner,
        reason: ConfirmationFailure,
    },
    CandidateCommitted {
        candidate: ProcessOwner,
    },
    CommitFailed {
        candidate: ProcessOwner,
    },
    RolledBack {
        previous: ProcessOwner,
    },
    RolledBackForRetry {
        previous: ProcessOwner,
        refreshed_fence: IntentFence,
    },
    RollbackFailed {
        previous_fingerprint: ConfigFingerprint,
    },
    Exited {
        process: ProcessOwner,
        intentional: bool,
    },
}

/// Every asynchronous executor result carries the original exact envelope.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutorResult {
    pub envelope: IntentEnvelope,
    pub outcome: ExecutorOutcome,
    pub completed_at_monotonic_ms: u64,
}

/// Immutable identity and starting conditions of one Legacy lifecycle.
///
/// Mutable lane, sensor and registry epochs deliberately live outside this
/// context, so changing one lane cannot invalidate the whole session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LegacySessionContext {
    session_id: SessionId,
    engine: EngineKind,
    active_categories: Vec<String>,
    network_fingerprint_at_start: NetworkFingerprint,
}

impl LegacySessionContext {
    pub fn new(
        session_id: SessionId,
        active_categories: Vec<String>,
        network_fingerprint_at_start: NetworkFingerprint,
    ) -> Self {
        Self {
            session_id,
            engine: EngineKind::Legacy,
            active_categories,
            network_fingerprint_at_start,
        }
    }

    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub const fn engine(&self) -> EngineKind {
        self.engine
    }

    pub fn active_categories(&self) -> &[String] {
        &self.active_categories
    }

    pub const fn network_fingerprint_at_start(&self) -> &NetworkFingerprint {
        &self.network_fingerprint_at_start
    }
}

/// Epochs required to reject events from a stale Eyes or registry snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub session_id: SessionId,
    pub sensor_generation: SensorGeneration,
    pub target_registry_version: RegistryVersion,
}

impl EventEnvelope {
    pub const fn new(
        session_id: SessionId,
        sensor_generation: SensorGeneration,
        target_registry_version: RegistryVersion,
    ) -> Self {
        Self {
            session_id,
            sensor_generation,
            target_registry_version,
        }
    }
}

/// Transport classification for Legacy v1 evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    /// Plain TCP is diagnostic-only (for example TCP/80).
    Tcp,
    /// TLS over TCP is eligible for Legacy v1 automatic evidence.
    Tls,
    /// Preserved for diagnostics; automatic UDP recovery is out of scope.
    UdpOutOfScope,
    /// Preserved for diagnostics; automatic QUIC recovery is out of scope.
    QuicOutOfScope,
}

impl Transport {
    pub const fn is_automatic_evidence(self) -> bool {
        matches!(self, Self::Tls)
    }
}

/// Owned evidence label produced by Eyes.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FlowEvidence(String);

impl FlowEvidence {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl From<String> for FlowEvidence {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for FlowEvidence {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl fmt::Display for FlowEvidence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// One classified network flow.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FlowEvent {
    pub envelope: EventEnvelope,
    pub category: Option<String>,
    pub lane_generation: Option<LaneGeneration>,
    pub flow_id: u64,
    pub domain: String,
    pub destination_ip: IpAddr,
    pub transport: Transport,
    #[serde(with = "diagnosis_serde")]
    pub diagnosis: Diagnosis,
    pub evidence: FlowEvidence,
    /// ClientHello arming time in the current sensor-generation clock.
    /// Missing timing remains valid diagnostic input but cannot confirm or
    /// reject a recovery candidate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub armed_at_sensor_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub armed_at_capture_timestamp: Option<i64>,
    pub monotonic_ts: u64,
}

impl FlowEvent {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        envelope: EventEnvelope,
        category: Option<String>,
        lane_generation: Option<LaneGeneration>,
        flow_id: u64,
        domain: impl Into<String>,
        destination_ip: IpAddr,
        transport: Transport,
        diagnosis: Diagnosis,
        evidence: impl Into<FlowEvidence>,
        monotonic_ts: u64,
    ) -> Result<Self, FlowEventError> {
        if category.is_some() != lane_generation.is_some() {
            return Err(FlowEventError::PartialAttribution);
        }

        Ok(Self {
            envelope,
            category,
            lane_generation,
            flow_id,
            domain: domain.into(),
            destination_ip,
            transport,
            diagnosis,
            evidence: evidence.into(),
            armed_at_sensor_ms: None,
            armed_at_capture_timestamp: None,
            monotonic_ts,
        })
    }

    pub fn with_armed_at_sensor_ms(mut self, armed_at_sensor_ms: u64) -> Self {
        self.armed_at_sensor_ms = Some(armed_at_sensor_ms);
        self
    }

    pub fn with_armed_at_capture_timestamp(mut self, capture_timestamp: i64) -> Self {
        self.armed_at_capture_timestamp = Some(capture_timestamp);
        self
    }

    pub const fn envelope(&self) -> EventEnvelope {
        self.envelope
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowEventError {
    PartialAttribution,
}

impl fmt::Display for FlowEventError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PartialAttribution => formatter.write_str(
                "flow category and lane generation must either both be present or both be absent",
            ),
        }
    }
}

impl std::error::Error for FlowEventError {}

#[derive(Deserialize)]
struct FlowEventWire {
    envelope: EventEnvelope,
    category: Option<String>,
    lane_generation: Option<LaneGeneration>,
    flow_id: u64,
    domain: String,
    destination_ip: IpAddr,
    transport: Transport,
    #[serde(with = "diagnosis_serde")]
    diagnosis: Diagnosis,
    evidence: FlowEvidence,
    #[serde(default)]
    armed_at_sensor_ms: Option<u64>,
    #[serde(default)]
    armed_at_capture_timestamp: Option<i64>,
    monotonic_ts: u64,
}

impl<'de> Deserialize<'de> for FlowEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let event = FlowEventWire::deserialize(deserializer)?;
        let armed_at_sensor_ms = event.armed_at_sensor_ms;
        let armed_at_capture_timestamp = event.armed_at_capture_timestamp;
        Self::new(
            event.envelope,
            event.category,
            event.lane_generation,
            event.flow_id,
            event.domain,
            event.destination_ip,
            event.transport,
            event.diagnosis,
            event.evidence,
            event.monotonic_ts,
        )
        .map(|mut flow| {
            flow.armed_at_sensor_ms = armed_at_sensor_ms;
            flow.armed_at_capture_timestamp = armed_at_capture_timestamp;
            flow
        })
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EyeHealthState {
    Ready,
    Degraded,
    Blind,
    Stopped,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EyeHealthCounters {
    pub packet_count: u64,
    pub parse_errors: u64,
    pub queue_drops: u64,
    pub last_event_ts: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthEvent {
    pub envelope: EventEnvelope,
    pub state: EyeHealthState,
    pub counters: EyeHealthCounters,
}

impl HealthEvent {
    pub const fn envelope(&self) -> EventEnvelope {
        self.envelope
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GapEvent {
    pub envelope: EventEnvelope,
    pub from_ts: u64,
    pub to_ts: u64,
    pub dropped_events: u64,
}

impl GapEvent {
    pub const fn envelope(&self) -> EventEnvelope {
        self.envelope
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "event", rename_all = "snake_case")]
pub enum EyeEvent {
    Flow(FlowEvent),
    Health(HealthEvent),
    Gap(GapEvent),
}

impl EyeEvent {
    pub const fn envelope(&self) -> EventEnvelope {
        match self {
            Self::Flow(event) => event.envelope,
            Self::Health(event) => event.envelope,
            Self::Gap(event) => event.envelope,
        }
    }
}

mod diagnosis_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use crate::eyes::Diagnosis;

    pub fn serialize<S>(diagnosis: &Diagnosis, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        diagnosis.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Diagnosis, D::Error>
    where
        D: Deserializer<'de>,
    {
        let tag = String::deserialize(deserializer)?;
        match tag.as_str() {
            "working" => Ok(Diagnosis::Working),
            "dns_failure" => Ok(Diagnosis::DnsFailure),
            "tcp_reset" => Ok(Diagnosis::TcpReset),
            "tcp_blackhole" => Ok(Diagnosis::TcpBlackhole),
            "tls_blackhole" => Ok(Diagnosis::TlsBlackhole),
            "quic_blocked" => Ok(Diagnosis::QuicBlocked),
            "udp_blocked" => Ok(Diagnosis::UdpBlocked),
            "http_block_page" => Ok(Diagnosis::HttpBlockPage),
            "throttled" => Ok(Diagnosis::Throttled),
            "ip_unreachable" => Ok(Diagnosis::IpUnreachable),
            "unknown" => Ok(Diagnosis::Unknown),
            other => Err(serde::de::Error::unknown_variant(
                other,
                &[
                    "working",
                    "dns_failure",
                    "tcp_reset",
                    "tcp_blackhole",
                    "tls_blackhole",
                    "quic_blocked",
                    "udp_blocked",
                    "http_block_page",
                    "throttled",
                    "ip_unreachable",
                    "unknown",
                ],
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::any::TypeId;
    use std::net::{IpAddr, Ipv4Addr};

    use serde_json::json;

    use super::*;

    fn envelope() -> EventEnvelope {
        EventEnvelope::new(SessionId(41), SensorGeneration(3), RegistryVersion(7))
    }

    fn flow_event() -> FlowEvent {
        FlowEvent::new(
            envelope(),
            Some("discord".to_owned()),
            Some(LaneGeneration(9)),
            12,
            "discord.com",
            IpAddr::V4(Ipv4Addr::new(203, 0, 113, 4)),
            Transport::Tls,
            Diagnosis::TcpReset,
            FlowEvidence::new("inbound_rst"),
            1234,
        )
        .unwrap()
    }

    #[test]
    fn stable_serde_tags_and_event_shape() {
        assert_eq!(
            serde_json::to_value(NetworkFingerprint::Stable {
                key: "gateway:aa:bb".to_owned(),
            })
            .unwrap(),
            json!({"state": "stable", "key": "gateway:aa:bb"})
        );
        assert_eq!(
            serde_json::to_value(Transport::UdpOutOfScope).unwrap(),
            json!("udp_out_of_scope")
        );
        assert_eq!(
            serde_json::to_value(EyeHealthState::Degraded).unwrap(),
            json!("degraded")
        );

        let event = EyeEvent::Flow(flow_event());
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(
            value,
            json!({
                "kind": "flow",
                "event": {
                    "envelope": {
                        "session_id": 41,
                        "sensor_generation": 3,
                        "target_registry_version": 7
                    },
                    "category": "discord",
                    "lane_generation": 9,
                    "flow_id": 12,
                    "domain": "discord.com",
                    "destination_ip": "203.0.113.4",
                    "transport": "tls",
                    "diagnosis": "tcp_reset",
                    "evidence": "inbound_rst",
                    "monotonic_ts": 1234
                }
            })
        );
        assert_eq!(serde_json::from_value::<EyeEvent>(value).unwrap(), event);

        let partial = json!({
            "kind": "flow",
            "event": {
                "envelope": {
                    "session_id": 41,
                    "sensor_generation": 3,
                    "target_registry_version": 7
                },
                "category": "discord",
                "lane_generation": null,
                "flow_id": 12,
                "domain": "discord.com",
                "destination_ip": "203.0.113.4",
                "transport": "tls",
                "diagnosis": "tcp_reset",
                "evidence": "inbound_rst",
                "monotonic_ts": 1234
            }
        });
        assert!(serde_json::from_value::<EyeEvent>(partial).is_err());
    }

    #[test]
    fn every_event_exposes_its_envelope() {
        let expected = envelope();
        let events = [
            EyeEvent::Flow(flow_event()),
            EyeEvent::Health(HealthEvent {
                envelope: expected,
                state: EyeHealthState::Ready,
                counters: EyeHealthCounters::default(),
            }),
            EyeEvent::Gap(GapEvent {
                envelope: expected,
                from_ts: 10,
                to_ts: 20,
                dropped_events: 2,
            }),
        ];

        assert!(events.iter().all(|event| event.envelope() == expected));
    }

    #[test]
    fn lane_sensor_and_registry_generations_are_separate_types() {
        assert_ne!(
            TypeId::of::<LaneGeneration>(),
            TypeId::of::<SensorGeneration>()
        );
        assert_ne!(
            TypeId::of::<SensorGeneration>(),
            TypeId::of::<RegistryVersion>()
        );
        assert_eq!(LaneGeneration::new(5).get(), 5);
        assert_eq!(SensorGeneration::new(5).get(), 5);
        assert_eq!(RegistryVersion::new(5).get(), 5);
        assert_eq!(AttemptId::new(5).get(), 5);
        assert_ne!(TypeId::of::<AttemptId>(), TypeId::of::<SessionId>());
    }

    #[test]
    fn recovery_envelope_requires_every_fence_to_match_exactly() {
        let fence = IntentFence {
            session_id: SessionId::new(41),
            category: "discord".into(),
            lane_generation: LaneGeneration::new(9),
            sensor_generation: SensorGeneration::new(3),
            registry_version: RegistryVersion::new(7),
            network_fingerprint: NetworkFingerprint::Stable {
                key: "network-a".into(),
            },
        };
        let intent = IntentEnvelope::from_fence(AttemptId::new(12), &fence);
        assert!(intent.matches_fence(&fence));

        let mut stale = fence.clone();
        stale.sensor_generation = SensorGeneration::new(4);
        assert!(!intent.matches_fence(&stale));
        stale = fence.clone();
        stale.network_fingerprint = NetworkFingerprint::Stable {
            key: "network-b".into(),
        };
        assert!(!intent.matches_fence(&stale));

        assert_eq!(
            serde_json::to_value(&intent).unwrap(),
            json!({
                "sessionId": 41,
                "attemptId": 12,
                "category": "discord",
                "expectedLaneGeneration": 9,
                "expectedSensorGeneration": 3,
                "expectedRegistryVersion": 7,
                "expectedNetworkFingerprint": {
                    "state": "stable",
                    "key": "network-a"
                }
            })
        );
    }

    #[test]
    fn process_owner_includes_start_identity_and_config_generation() {
        let owner = ProcessOwner {
            pid: 9001,
            process_start_identity: ProcessStartIdentity::new(77),
            config_fingerprint: ConfigFingerprint::new("candidate-sha256"),
            lane_generation: LaneGeneration::new(10),
        };
        assert!(owner.owns(
            &ConfigFingerprint::new("candidate-sha256"),
            LaneGeneration::new(10)
        ));
        assert!(!owner.owns(
            &ConfigFingerprint::new("candidate-sha256"),
            LaneGeneration::new(9)
        ));
        assert!(!owner.owns(
            &ConfigFingerprint::new("other-sha256"),
            LaneGeneration::new(10)
        ));
    }

    #[test]
    fn flow_attribution_is_all_or_nothing() {
        let make = |category, generation| {
            FlowEvent::new(
                envelope(),
                category,
                generation,
                1,
                "example.com",
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                Transport::Tls,
                Diagnosis::Working,
                "server_hello",
                1,
            )
        };

        assert!(make(None, None).is_ok());
        assert!(make(Some("discord".to_owned()), Some(LaneGeneration(1))).is_ok());
        assert_eq!(
            make(Some("discord".to_owned()), None),
            Err(FlowEventError::PartialAttribution)
        );
        assert_eq!(
            make(None, Some(LaneGeneration(1))),
            Err(FlowEventError::PartialAttribution)
        );
    }

    #[test]
    fn only_stable_networks_expose_a_cache_key() {
        let stable = NetworkFingerprint::Stable {
            key: "gateway:aa:bb".to_owned(),
        };
        let unstable = NetworkFingerprint::Unstable {
            reason: "gateway_changed".to_owned(),
        };

        assert!(stable.is_stable());
        assert_eq!(stable.stable_key(), Some("gateway:aa:bb"));
        assert!(!unstable.is_stable());
        assert_eq!(unstable.stable_key(), None);
        assert!(!NetworkFingerprint::Unknown.is_stable());
        assert_eq!(NetworkFingerprint::Unknown.stable_key(), None);
    }

    #[test]
    fn session_context_is_legacy_and_keeps_epochs_outside() {
        let context = LegacySessionContext::new(
            SessionId(99),
            vec!["discord".to_owned(), "youtube".to_owned()],
            NetworkFingerprint::Unknown,
        );

        assert_eq!(context.engine(), EngineKind::Legacy);
        assert_eq!(context.session_id(), SessionId(99));
        assert_eq!(context.active_categories(), ["discord", "youtube"]);
        let value = serde_json::to_value(context).unwrap();
        assert_eq!(value["engine"], json!("legacy"));
        assert!(value.get("lane_generation").is_none());
        assert!(value.get("sensor_generation").is_none());
        assert!(value.get("target_registry_version").is_none());
    }
}
