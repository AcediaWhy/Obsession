//! Public, read-only projection of the active Legacy observe-only runtime.

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::state::{AppState, RevisionClock};
use crate::util::VersionedSection;

use super::assessment::{
    AssessmentClassification, AssessmentConfidence, EvidenceSummary, LanePhase,
};
use super::contracts::{EyeHealthState, SensorGeneration, SessionId};
use super::manager::ObserveOnlySnapshot;
use super::policy::PresumedIntent;

pub const STATUS_EVENT: &str = "legacy-reliability://status";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyReliabilityMode {
    ObserveOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyReliabilityPhase {
    Inactive,
    Starting,
    Observing,
    Degraded,
    Blind,
}

/// Stable wire payload consumed by the Legacy reliability UI.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyReliabilityStatus {
    pub mode: LegacyReliabilityMode,
    pub phase: LegacyReliabilityPhase,
    pub active_categories: Vec<String>,
    pub session_id: Option<u64>,
    pub sensor_generation: Option<u64>,
    pub lanes: Vec<LegacyLaneStatus>,
    pub presumed_intent: PresumedIntent,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyLaneStatus {
    pub category: String,
    pub active_config: Option<String>,
    pub lane_generation: u64,
    pub phase: LanePhase,
    pub classification: AssessmentClassification,
    pub confidence: AssessmentConfidence,
    pub evidence: EvidenceSummary,
    pub working_confirmed_recently: bool,
    pub cooldown_until_ms: Option<u64>,
}

impl Default for LegacyReliabilityStatus {
    fn default() -> Self {
        Self::inactive()
    }
}

impl LegacyReliabilityStatus {
    pub const fn inactive() -> Self {
        Self {
            mode: LegacyReliabilityMode::ObserveOnly,
            phase: LegacyReliabilityPhase::Inactive,
            active_categories: Vec::new(),
            session_id: None,
            sensor_generation: None,
            lanes: Vec::new(),
            presumed_intent: PresumedIntent::Wait {
                reason: AssessmentClassification::AwaitingEvidence,
            },
        }
    }

    pub fn starting(
        active_categories: Vec<String>,
        session_id: SessionId,
        sensor_generation: SensorGeneration,
    ) -> Self {
        Self::for_session(
            LegacyReliabilityPhase::Starting,
            active_categories,
            session_id,
            sensor_generation,
        )
    }

    pub fn blind(
        active_categories: Vec<String>,
        session_id: SessionId,
        sensor_generation: SensorGeneration,
    ) -> Self {
        let mut status = Self::for_session(
            LegacyReliabilityPhase::Blind,
            active_categories,
            session_id,
            sensor_generation,
        );
        status.normalize_blind();
        status
    }

    pub fn from_snapshot(snapshot: &ObserveOnlySnapshot) -> Self {
        let mut status = Self::for_session(
            phase_from_health(snapshot.health.state),
            snapshot.session.active_categories.clone(),
            snapshot.session.session_id,
            snapshot.session.sensor_generation,
        );
        status.lanes = snapshot
            .lanes
            .iter()
            .map(|lane| LegacyLaneStatus {
                category: lane.category.clone(),
                active_config: snapshot.active_configs.get(&lane.category).cloned(),
                lane_generation: lane.lane_generation.get(),
                phase: lane.phase,
                classification: lane.classification,
                confidence: lane.confidence,
                evidence: public_evidence(lane.evidence),
                working_confirmed_recently: lane.working_confirmed_recently,
                cooldown_until_ms: lane.cooldown_until_ms,
            })
            .collect();
        status
            .lanes
            .sort_by(|left, right| left.category.cmp(&right.category));
        status.presumed_intent = snapshot.presumed_intent.clone();
        if status.phase == LegacyReliabilityPhase::Blind {
            status.normalize_blind();
        }
        status
    }

    fn for_session(
        phase: LegacyReliabilityPhase,
        mut active_categories: Vec<String>,
        session_id: SessionId,
        sensor_generation: SensorGeneration,
    ) -> Self {
        active_categories.sort_unstable();
        active_categories.dedup();
        Self {
            mode: LegacyReliabilityMode::ObserveOnly,
            phase,
            active_categories,
            session_id: Some(session_id.get()),
            sensor_generation: Some(sensor_generation.get()),
            lanes: Vec::new(),
            presumed_intent: PresumedIntent::Wait {
                reason: AssessmentClassification::AwaitingEvidence,
            },
        }
    }

    fn owner(&self) -> Option<StatusOwner> {
        Some(StatusOwner {
            session_id: self.session_id?,
            sensor_generation: self.sensor_generation?,
        })
    }

    fn normalize_blind(&mut self) {
        self.phase = LegacyReliabilityPhase::Blind;
        for lane in &mut self.lanes {
            lane.phase = LanePhase::SensorUnreliable;
            lane.classification = AssessmentClassification::SensorUnreliable;
            lane.confidence = AssessmentConfidence::None;
            lane.evidence = EvidenceSummary::default();
            lane.working_confirmed_recently = false;
            lane.cooldown_until_ms = None;
        }
        self.presumed_intent = PresumedIntent::Wait {
            reason: AssessmentClassification::SensorUnreliable,
        };
    }
}

const fn public_evidence(evidence: EvidenceSummary) -> EvidenceSummary {
    EvidenceSummary {
        working_flows: if evidence.working_flows > 2 {
            2
        } else {
            evidence.working_flows
        },
        working_targets: if evidence.working_targets > 2 {
            2
        } else {
            evidence.working_targets
        },
        reset_flows: if evidence.reset_flows > 3 {
            3
        } else {
            evidence.reset_flows
        },
        reset_targets: if evidence.reset_targets > 2 {
            2
        } else {
            evidence.reset_targets
        },
        blackhole_flows: if evidence.blackhole_flows > 2 {
            2
        } else {
            evidence.blackhole_flows
        },
        blackhole_targets: if evidence.blackhole_targets > 2 {
            2
        } else {
            evidence.blackhole_targets
        },
    }
}

pub const fn phase_from_health(state: EyeHealthState) -> LegacyReliabilityPhase {
    match state {
        EyeHealthState::Ready => LegacyReliabilityPhase::Observing,
        EyeHealthState::Degraded => LegacyReliabilityPhase::Degraded,
        EyeHealthState::Blind | EyeHealthState::Stopped => LegacyReliabilityPhase::Blind,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StatusOwner {
    session_id: u64,
    sensor_generation: u64,
}

impl StatusOwner {
    const fn new(session_id: SessionId, sensor_generation: SensorGeneration) -> Self {
        Self {
            session_id: session_id.get(),
            sensor_generation: sensor_generation.get(),
        }
    }
}

/// Publishes a lifecycle transition that intentionally replaces any prior
/// Legacy session (currently `inactive` and `starting`).
pub fn publish(app: &AppHandle, next: LegacyReliabilityStatus) -> bool {
    publish_inner(app, None, next)
}

/// Publishes a manager/startup update only while its exact session and sensor
/// still own the public status. Late watch callbacks are rejected here.
pub fn publish_if_owned(
    app: &AppHandle,
    session_id: SessionId,
    sensor_generation: SensorGeneration,
    next: LegacyReliabilityStatus,
) -> bool {
    publish_inner(
        app,
        Some(StatusOwner::new(session_id, sensor_generation)),
        next,
    )
}

pub fn publish_snapshot_if_owned(app: &AppHandle, snapshot: &ObserveOnlySnapshot) -> bool {
    publish_if_owned(
        app,
        snapshot.session.session_id,
        snapshot.session.sensor_generation,
        LegacyReliabilityStatus::from_snapshot(snapshot),
    )
}

pub(crate) fn current_owner(app: &AppHandle) -> Option<StatusOwner> {
    app.state::<AppState>()
        .legacy_reliability_status
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .owner()
}

pub(crate) fn publish_blind_if_owner(app: &AppHandle, owner: StatusOwner) -> bool {
    let state = app.state::<AppState>();
    let next = {
        let guard = state
            .legacy_reliability_status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if guard.owner() != Some(owner) {
            return false;
        }
        let mut next = guard.clone();
        next.normalize_blind();
        next
    };
    publish_inner(app, Some(owner), next)
}

/// Marks the currently owned session terminally unavailable without allowing a
/// stale process monitor to target a later session. The exact owner captured
/// here is rechecked by `publish_inner` before the transition is committed.
pub fn publish_current_blind(app: &AppHandle) -> bool {
    current_owner(app).is_some_and(|owner| publish_blind_if_owner(app, owner))
}

fn publish_inner(
    app: &AppHandle,
    expected_owner: Option<StatusOwner>,
    next: LegacyReliabilityStatus,
) -> bool {
    let state = app.state::<AppState>();
    let Some(section) = transition(
        &state.legacy_reliability_status,
        &state.legacy_reliability_revision,
        expected_owner,
        next,
    ) else {
        return false;
    };
    let _ = app.emit(STATUS_EVENT, section);
    true
}

fn transition(
    current: &Mutex<LegacyReliabilityStatus>,
    revision: &RevisionClock,
    expected_owner: Option<StatusOwner>,
    next: LegacyReliabilityStatus,
) -> Option<VersionedSection<LegacyReliabilityStatus>> {
    let mut guard = current
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if expected_owner.is_some_and(|owner| guard.owner() != Some(owner))
        || (expected_owner.is_some()
            && guard.phase == LegacyReliabilityPhase::Blind
            && next.phase != LegacyReliabilityPhase::Blind)
        || *guard == next
    {
        return None;
    }
    *guard = next.clone();
    let revision = revision.bump();
    Some(VersionedSection::new(revision, next))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;
    use crate::dpi_engine::EngineKind;
    use crate::legacy_reliability::assessment::{
        AssessmentConfidence, EvidenceSummary, LaneAssessment, LanePhase,
    };
    use crate::legacy_reliability::contracts::{
        EyeHealthCounters, LaneGeneration, NetworkFingerprint, RegistryVersion,
    };
    use crate::legacy_reliability::manager::{
        AcceptedEventCounters, GapStatus, ObserveOnlyHealthStatus, ObserveOnlySessionStatus,
        RejectedEventCounters,
    };
    use crate::legacy_reliability::policy::PresumedIntent;

    fn snapshot(state: EyeHealthState) -> ObserveOnlySnapshot {
        ObserveOnlySnapshot {
            session: ObserveOnlySessionStatus {
                session_id: SessionId::new(11),
                engine: EngineKind::Legacy,
                active_categories: vec!["youtube_twitch".into(), "discord".into()],
                network_fingerprint_at_start: NetworkFingerprint::Unknown,
                sensor_generation: SensorGeneration::new(17),
                target_registry_version: RegistryVersion::new(19),
                lane_generations: BTreeMap::from([("discord".into(), LaneGeneration::new(23))]),
                closed: false,
            },
            accepted: AcceptedEventCounters::default(),
            rejected: RejectedEventCounters::default(),
            gaps: GapStatus::default(),
            health: ObserveOnlyHealthStatus {
                state,
                counters: EyeHealthCounters::default(),
                last_reported_state: None,
                last_reported_counters: None,
                receiver_failed: false,
            },
            logical_now_ms: 0,
            last_gap_sequence: None,
            last_accepted_flow_sequence: None,
            lanes: Vec::new(),
            presumed_intent: PresumedIntent::default(),
            active_configs: BTreeMap::new(),
        }
    }

    #[test]
    fn payload_serializes_with_exact_camel_case_shape() {
        let value = serde_json::to_value(LegacyReliabilityStatus::starting(
            vec!["discord".into()],
            SessionId::new(11),
            SensorGeneration::new(17),
        ))
        .expect("status serializes");
        assert_eq!(
            value,
            json!({
                "mode": "observe_only",
                "phase": "starting",
                "activeCategories": ["discord"],
                "sessionId": 11,
                "sensorGeneration": 17,
                "lanes": [],
                "presumedIntent": {
                    "kind": "wait",
                    "reason": "awaiting_evidence"
                }
            })
        );

        let inactive = serde_json::to_value(LegacyReliabilityStatus::inactive())
            .expect("inactive status serializes");
        assert_eq!(inactive["sessionId"], json!(null));
        assert_eq!(inactive["sensorGeneration"], json!(null));
    }

    #[test]
    fn manager_health_maps_to_public_phase() {
        for (health, expected) in [
            (EyeHealthState::Ready, LegacyReliabilityPhase::Observing),
            (EyeHealthState::Degraded, LegacyReliabilityPhase::Degraded),
            (EyeHealthState::Blind, LegacyReliabilityPhase::Blind),
            (EyeHealthState::Stopped, LegacyReliabilityPhase::Blind),
        ] {
            let status = LegacyReliabilityStatus::from_snapshot(&snapshot(health));
            assert_eq!(status.phase, expected);
            assert_eq!(status.active_categories, ["discord", "youtube_twitch"]);
        }
    }

    #[test]
    fn lane_and_presumed_intent_use_stable_camel_case_wire_shape() {
        let mut source = snapshot(EyeHealthState::Ready);
        source
            .active_configs
            .insert("discord".into(), "discord_1.conf".into());
        source.lanes = vec![LaneAssessment {
            category: "discord".into(),
            lane_generation: LaneGeneration::new(23),
            phase: LanePhase::Suspect,
            classification: AssessmentClassification::DpiSuspected,
            confidence: AssessmentConfidence::Medium,
            evidence: EvidenceSummary {
                reset_flows: 9,
                reset_targets: 4,
                ..EvidenceSummary::default()
            },
            working_confirmed_recently: true,
            evidence_epoch: 2,
            assessed_at_ms: 100,
            cooldown_until_ms: None,
        }];
        source.presumed_intent = PresumedIntent::SwitchLane {
            category: "discord".into(),
            candidate_config: "discord_2.conf".into(),
            reason: AssessmentClassification::DpiSuspected,
        };

        let value = serde_json::to_value(LegacyReliabilityStatus::from_snapshot(&source)).unwrap();
        assert_eq!(value["lanes"][0]["activeConfig"], "discord_1.conf");
        assert_eq!(value["lanes"][0]["laneGeneration"], 23);
        assert_eq!(value["lanes"][0]["classification"], "dpi_suspected");
        assert_eq!(value["lanes"][0]["workingConfirmedRecently"], true);
        // Public counts stop at policy thresholds, preventing status storms.
        assert_eq!(value["lanes"][0]["evidence"]["resetFlows"], 3);
        assert_eq!(value["lanes"][0]["evidence"]["resetTargets"], 2);
        assert_eq!(value["presumedIntent"]["kind"], "switch_lane");
        assert_eq!(value["presumedIntent"]["candidateConfig"], "discord_2.conf");
    }

    #[test]
    fn blind_status_clears_stale_actions_and_marks_lanes_unreliable() {
        let mut source = snapshot(EyeHealthState::Ready);
        source.lanes = vec![LaneAssessment {
            category: "discord".into(),
            lane_generation: LaneGeneration::new(23),
            phase: LanePhase::Suspect,
            classification: AssessmentClassification::DpiSuspected,
            confidence: AssessmentConfidence::High,
            evidence: EvidenceSummary {
                reset_flows: 3,
                reset_targets: 2,
                ..EvidenceSummary::default()
            },
            working_confirmed_recently: true,
            evidence_epoch: 2,
            assessed_at_ms: 100,
            cooldown_until_ms: None,
        }];
        source.presumed_intent = PresumedIntent::SwitchLane {
            category: "discord".into(),
            candidate_config: "discord_2.conf".into(),
            reason: AssessmentClassification::DpiSuspected,
        };
        let mut status = LegacyReliabilityStatus::from_snapshot(&source);
        status.normalize_blind();

        assert_eq!(status.phase, LegacyReliabilityPhase::Blind);
        assert_eq!(status.lanes[0].phase, LanePhase::SensorUnreliable);
        assert_eq!(
            status.lanes[0].classification,
            AssessmentClassification::SensorUnreliable
        );
        assert_eq!(status.lanes[0].evidence, EvidenceSummary::default());
        assert!(!status.lanes[0].working_confirmed_recently);
        assert_eq!(
            status.presumed_intent,
            PresumedIntent::Wait {
                reason: AssessmentClassification::SensorUnreliable,
            }
        );

        assert_eq!(
            LegacyReliabilityStatus::blind(
                vec!["discord".into()],
                SessionId::new(11),
                SensorGeneration::new(17),
            )
            .presumed_intent,
            PresumedIntent::Wait {
                reason: AssessmentClassification::SensorUnreliable,
            }
        );
    }

    #[test]
    fn transitions_are_monotonic_deduplicated_and_generation_fenced() {
        let current = Mutex::new(LegacyReliabilityStatus::inactive());
        let revision = RevisionClock::default();
        let session = SessionId::new(3);
        let sensor = SensorGeneration::new(5);
        let starting = LegacyReliabilityStatus::starting(vec!["discord".into()], session, sensor);

        let first = transition(&current, &revision, None, starting.clone()).unwrap();
        assert_eq!(first.revision, 1);
        assert!(transition(&current, &revision, None, starting).is_none());
        assert_eq!(revision.current(), 1);

        let stale = LegacyReliabilityStatus::blind(
            vec!["discord".into()],
            SessionId::new(4),
            SensorGeneration::new(6),
        );
        assert!(transition(
            &current,
            &revision,
            Some(StatusOwner::new(
                SessionId::new(4),
                SensorGeneration::new(6)
            )),
            stale,
        )
        .is_none());
        assert_eq!(revision.current(), 1);

        let blind = LegacyReliabilityStatus::blind(vec!["discord".into()], session, sensor);
        let second = transition(
            &current,
            &revision,
            Some(StatusOwner::new(session, sensor)),
            blind,
        )
        .unwrap();
        assert_eq!(second.revision, 2);

        let stale_ready = LegacyReliabilityStatus::for_session(
            LegacyReliabilityPhase::Observing,
            vec!["discord".into()],
            session,
            sensor,
        );
        assert!(transition(
            &current,
            &revision,
            Some(StatusOwner::new(session, sensor)),
            stale_ready,
        )
        .is_none());
        assert_eq!(revision.current(), 2);

        let inactive = transition(
            &current,
            &revision,
            None,
            LegacyReliabilityStatus::inactive(),
        )
        .unwrap();
        assert_eq!(inactive.revision, 3);

        let late_snapshot =
            LegacyReliabilityStatus::from_snapshot(&snapshot(EyeHealthState::Ready));
        assert!(transition(
            &current,
            &revision,
            Some(StatusOwner::new(
                SessionId::new(11),
                SensorGeneration::new(17)
            )),
            late_snapshot,
        )
        .is_none());
        assert_eq!(revision.current(), 3);
    }
}
