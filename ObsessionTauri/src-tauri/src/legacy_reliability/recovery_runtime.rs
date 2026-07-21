//! Process-owned integration state around the pure recovery coordinator.
//!
//! Manager evidence epochs restart with each Eyes generation, so they cannot
//! be used directly as globally unique incident identifiers. This wrapper
//! assigns a stable process-local id to one exact observed incident while
//! leaving all recovery policy inside [`RecoveryCoordinator`].

use std::collections::BTreeMap;

use super::assessment::AssessmentClassification;
use super::contracts::{
    ExecutorResult, IntentFence, LaneGeneration, ProcessOwner, SensorGeneration, SessionId,
};
use super::recovery::{
    ApprovalError, AssistedApproval, ConsiderError, IncidentId, RecoveryAction, RecoveryConfig,
    RecoveryCoordinator, RecoveryDecision, RecoveryMode, RecoveryRequest, RecoveryStatus,
    TransitionError,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncidentObservation {
    pub session_id: SessionId,
    pub sensor_generation: SensorGeneration,
    pub category: String,
    pub lane_generation: LaneGeneration,
    pub evidence_epoch: u64,
    pub classification: AssessmentClassification,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct IncidentLane {
    session_id: SessionId,
    category: String,
}

#[derive(Debug)]
pub struct LegacyRecoveryRuntime {
    coordinator: RecoveryCoordinator,
    next_incident_id: u64,
    current_session: Option<SessionId>,
    observed: BTreeMap<IncidentLane, (IncidentObservation, IncidentId)>,
}

impl LegacyRecoveryRuntime {
    pub fn new(mode: RecoveryMode) -> Self {
        Self {
            coordinator: RecoveryCoordinator::new(mode),
            next_incident_id: 1,
            current_session: None,
            observed: BTreeMap::new(),
        }
    }

    pub fn set_mode(&mut self, mode: RecoveryMode) {
        self.coordinator.set_mode(mode);
    }

    pub fn status(&self) -> RecoveryStatus {
        self.coordinator.status()
    }

    pub fn cancel_pending(&mut self) {
        self.coordinator.cancel_pending();
    }

    pub fn observe_session(&mut self, session_id: SessionId) {
        if self.current_session == Some(session_id) {
            return;
        }
        self.current_session = Some(session_id);
        self.observed.clear();
        self.coordinator.cancel_pending();
        self.coordinator.clear_last_completion_if_idle();
        self.coordinator.retain_incidents_for_session(session_id);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn consider(
        &mut self,
        observation: IncidentObservation,
        fence: IntentFence,
        previous: RecoveryConfig,
        previous_owner: ProcessOwner,
        candidates: Vec<RecoveryConfig>,
        now_ms: u64,
    ) -> Result<RecoveryDecision, ConsiderError> {
        let incident_id = self.incident_id(observation);
        self.coordinator.consider(
            RecoveryRequest::new(incident_id, fence, previous, previous_owner, candidates),
            now_ms,
        )
    }

    pub fn approve(
        &mut self,
        approval: AssistedApproval,
        current_fence: &IntentFence,
        now_ms: u64,
    ) -> Result<RecoveryAction, ApprovalError> {
        self.coordinator.approve(approval, current_fence, now_ms)
    }

    pub fn apply_result(
        &mut self,
        result: ExecutorResult,
    ) -> Result<RecoveryAction, TransitionError> {
        self.coordinator.apply_result(result)
    }

    pub fn force_manual_failure(&mut self, now_ms: u64) -> Option<RecoveryAction> {
        self.coordinator.force_manual_failure(now_ms)
    }

    fn incident_id(&mut self, observation: IncidentObservation) -> IncidentId {
        // A new Legacy session makes all previous incident keys unreachable;
        // pruning here keeps this process-local journal bounded by categories.
        self.observed
            .retain(|lane, _| lane.session_id == observation.session_id);
        let lane = IncidentLane {
            session_id: observation.session_id,
            category: observation.category.clone(),
        };
        if let Some((known, id)) = self.observed.get(&lane) {
            if known == &observation {
                return *id;
            }
        }

        let id = IncidentId::new(self.next_incident_id);
        self.next_incident_id = next_nonzero(self.next_incident_id);
        self.observed.insert(lane, (observation, id));
        id
    }
}

const fn next_nonzero(current: u64) -> u64 {
    let next = current.wrapping_add(1);
    if next == 0 {
        1
    } else {
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::legacy_reliability::contracts::{
        ConfigFingerprint, NetworkFingerprint, ProcessStartIdentity, RegistryVersion,
    };

    fn observation(sensor: u64, evidence_epoch: u64) -> IncidentObservation {
        IncidentObservation {
            session_id: SessionId::new(7),
            sensor_generation: SensorGeneration::new(sensor),
            category: "discord".into(),
            lane_generation: LaneGeneration::new(3),
            evidence_epoch,
            classification: AssessmentClassification::DpiSuspected,
        }
    }

    fn fence() -> IntentFence {
        IntentFence {
            session_id: SessionId::new(7),
            category: "discord".into(),
            lane_generation: LaneGeneration::new(3),
            sensor_generation: SensorGeneration::new(1),
            registry_version: RegistryVersion::new(9),
            network_fingerprint: NetworkFingerprint::Stable { key: "lan".into() },
        }
    }

    fn owner() -> ProcessOwner {
        ProcessOwner {
            pid: 42,
            process_start_identity: ProcessStartIdentity::new(5),
            config_fingerprint: ConfigFingerprint::new("old-hash"),
            lane_generation: LaneGeneration::new(3),
        }
    }

    fn consider(
        runtime: &mut LegacyRecoveryRuntime,
        observation: IncidentObservation,
    ) -> RecoveryDecision {
        runtime
            .consider(
                observation,
                fence(),
                RecoveryConfig::new("old.conf", "old-hash"),
                owner(),
                vec![RecoveryConfig::new("new.conf", "new-hash")],
                0,
            )
            .unwrap()
    }

    #[test]
    fn repeated_snapshot_keeps_one_incident_but_new_generation_gets_new_id() {
        let mut runtime = LegacyRecoveryRuntime::new(RecoveryMode::ObserveOnly);
        let first = consider(&mut runtime, observation(1, 2));
        let repeated = consider(&mut runtime, observation(1, 2));
        let restarted = consider(&mut runtime, observation(2, 2));

        let id = |decision: RecoveryDecision| match decision {
            RecoveryDecision::ObserveOnly { suggestion } => suggestion.incident_id,
            RecoveryDecision::Assisted { .. } => panic!("observe-only fixture"),
        };
        let first = id(first);
        let repeated = id(repeated);
        let restarted = id(restarted);
        assert_eq!(first, repeated);
        assert_ne!(repeated, restarted);
    }
}
