//! Bounded driver for coordinator-owned Legacy recovery actions.
//!
//! Windows effects are injected one stage at a time. Synchronous stages may
//! advance in one service tick, while confirmation returns `Pending` and is
//! resumed by a later background poll. Every result preserves the exact
//! coordinator envelope.

#![cfg(windows)]

use obsession_runtime_reliability::legacy_reliability::contracts::{
    ExecutorOutcome, ExecutorResult, IntentEnvelope,
};
use obsession_runtime_reliability::legacy_reliability::recovery::{
    RecoveryAction, TransitionError, MAX_AUTOMATIC_LADDER_CANDIDATES,
};

use crate::legacy_reliability::LegacyRecoveryRuntime;

const MAX_TRANSACTION_TRANSITIONS: usize = MAX_AUTOMATIC_LADDER_CANDIDATES * 6 + 2;

pub(crate) trait LegacyRecoveryCoordinatorPort {
    fn apply_executor_result(
        &mut self,
        result: ExecutorResult,
    ) -> Result<RecoveryAction, TransitionError>;
}

impl LegacyRecoveryCoordinatorPort for LegacyRecoveryRuntime {
    fn apply_executor_result(
        &mut self,
        result: ExecutorResult,
    ) -> Result<RecoveryAction, TransitionError> {
        LegacyRecoveryRuntime::apply_executor_result(self, result)
    }
}

pub(crate) trait LegacyRecoveryEffects {
    fn execute(
        &mut self,
        action: &RecoveryAction,
        now_ms: u64,
    ) -> Result<LegacyRecoveryStageEffect, LegacyRecoveryEffectError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LegacyRecoveryStageEffect {
    Pending,
    Completed(ExecutorOutcome),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LegacyRecoveryEffectError {
    InvalidAction,
    FenceChanged,
    OwnershipChanged,
    ProtectedResourceChanged,
    RuntimeFailure,
}

pub(crate) struct LegacyRecoveryTransaction {
    next: RecoveryAction,
    transitions: usize,
    preflight_handoff_required: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LegacyRecoveryTransactionProgress {
    Pending,
    PreflightRequired(RecoveryAction),
    Complete(RecoveryAction),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LegacyRecoveryTransactionError {
    Effect(LegacyRecoveryEffectError),
    Coordinator(TransitionError),
    MissingEnvelope,
    TransitionLimit,
}

impl LegacyRecoveryTransaction {
    pub(crate) fn new(initial: RecoveryAction) -> Result<Self, LegacyRecoveryTransactionError> {
        if action_envelope(&initial).is_none() {
            return Err(LegacyRecoveryTransactionError::MissingEnvelope);
        }
        Ok(Self {
            next: initial,
            transitions: 0,
            preflight_handoff_required: false,
        })
    }

    pub(crate) fn advance<C, E>(
        &mut self,
        coordinator: &mut C,
        effects: &mut E,
        now_ms: u64,
    ) -> Result<LegacyRecoveryTransactionProgress, LegacyRecoveryTransactionError>
    where
        C: LegacyRecoveryCoordinatorPort,
        E: LegacyRecoveryEffects,
    {
        if self.preflight_handoff_required {
            return Ok(LegacyRecoveryTransactionProgress::PreflightRequired(
                self.next.clone(),
            ));
        }
        loop {
            if matches!(
                self.next,
                RecoveryAction::Complete { .. } | RecoveryAction::ManualIntervention { .. }
            ) {
                return Ok(LegacyRecoveryTransactionProgress::Complete(
                    self.next.clone(),
                ));
            }
            if self.transitions >= MAX_TRANSACTION_TRANSITIONS {
                return Err(LegacyRecoveryTransactionError::TransitionLimit);
            }
            let envelope = action_envelope(&self.next)
                .cloned()
                .ok_or(LegacyRecoveryTransactionError::MissingEnvelope)?;
            let outcome = match effects
                .execute(&self.next, now_ms)
                .map_err(LegacyRecoveryTransactionError::Effect)?
            {
                LegacyRecoveryStageEffect::Pending => {
                    return Ok(LegacyRecoveryTransactionProgress::Pending)
                }
                LegacyRecoveryStageEffect::Completed(outcome) => outcome,
            };
            self.transitions += 1;
            self.next = coordinator
                .apply_executor_result(ExecutorResult {
                    envelope,
                    outcome,
                    completed_at_monotonic_ms: now_ms,
                })
                .map_err(LegacyRecoveryTransactionError::Coordinator)?;
            if matches!(self.next, RecoveryAction::Preflight { .. }) {
                // A retry candidate needs a freshly resolved immutable
                // PreparedLegacyRecovery. Never let the effects instance for
                // the previous candidate consume this action in the same tick.
                self.preflight_handoff_required = true;
                return Ok(LegacyRecoveryTransactionProgress::PreflightRequired(
                    self.next.clone(),
                ));
            }
        }
    }
}

fn action_envelope(action: &RecoveryAction) -> Option<&IntentEnvelope> {
    match action {
        RecoveryAction::Preflight { envelope, .. }
        | RecoveryAction::StopPrevious { envelope, .. }
        | RecoveryAction::StartCandidate { envelope, .. }
        | RecoveryAction::ConfirmCandidate { envelope, .. }
        | RecoveryAction::CommitCandidate { envelope, .. }
        | RecoveryAction::RollbackPrevious { envelope, .. } => Some(envelope),
        RecoveryAction::Complete { .. } | RecoveryAction::ManualIntervention { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use obsession_runtime_reliability::legacy_reliability::assessment::AssessmentClassification;
    use obsession_runtime_reliability::legacy_reliability::contracts::{
        AttemptId, ConfigFingerprint, IntentFence, LaneGeneration, NetworkFingerprint,
        ProcessOwner, ProcessStartIdentity, RegistryVersion, SensorGeneration, SessionId,
    };
    use obsession_runtime_reliability::legacy_reliability::recovery::{
        RecoveryConfig, RecoveryDecision, RecoveryMode,
    };
    use obsession_runtime_reliability::legacy_reliability::recovery_runtime::{
        IncidentObservation, LegacyRecoveryRuntime as CoordinatorRuntime,
    };

    impl LegacyRecoveryCoordinatorPort for CoordinatorRuntime {
        fn apply_executor_result(
            &mut self,
            result: ExecutorResult,
        ) -> Result<RecoveryAction, TransitionError> {
            CoordinatorRuntime::apply_result(self, result)
        }
    }

    struct FakeEffects {
        previous: ProcessOwner,
        candidate: ProcessOwner,
        refreshed_fence: obsession_runtime_reliability::legacy_reliability::contracts::IntentFence,
        confirmation_pending: bool,
        stages: Vec<&'static str>,
    }

    impl LegacyRecoveryEffects for FakeEffects {
        fn execute(
            &mut self,
            action: &RecoveryAction,
            _now_ms: u64,
        ) -> Result<LegacyRecoveryStageEffect, LegacyRecoveryEffectError> {
            let outcome = match action {
                RecoveryAction::Preflight { .. } => {
                    self.stages.push("preflight");
                    ExecutorOutcome::PreflightPassed {
                        refreshed_fence: self.refreshed_fence.clone(),
                    }
                }
                RecoveryAction::StopPrevious { .. } => {
                    self.stages.push("stop");
                    ExecutorOutcome::Stopped {
                        previous: self.previous.clone(),
                    }
                }
                RecoveryAction::StartCandidate { .. } => {
                    self.stages.push("start");
                    ExecutorOutcome::Ready {
                        candidate: self.candidate.clone(),
                    }
                }
                RecoveryAction::ConfirmCandidate { .. } if self.confirmation_pending => {
                    self.confirmation_pending = false;
                    self.stages.push("confirm_pending");
                    return Ok(LegacyRecoveryStageEffect::Pending);
                }
                RecoveryAction::ConfirmCandidate { .. } => {
                    self.stages.push("confirm");
                    ExecutorOutcome::ConfirmationSucceeded {
                        candidate: self.candidate.clone(),
                    }
                }
                RecoveryAction::CommitCandidate { .. } => {
                    self.stages.push("commit");
                    ExecutorOutcome::CandidateCommitted {
                        candidate: self.candidate.clone(),
                    }
                }
                RecoveryAction::RollbackPrevious { .. }
                | RecoveryAction::Complete { .. }
                | RecoveryAction::ManualIntervention { .. } => {
                    return Err(LegacyRecoveryEffectError::InvalidAction)
                }
            };
            Ok(LegacyRecoveryStageEffect::Completed(outcome))
        }
    }

    #[derive(Default)]
    struct AutomaticRetryEffects {
        stages: Vec<String>,
    }

    impl LegacyRecoveryEffects for AutomaticRetryEffects {
        fn execute(
            &mut self,
            action: &RecoveryAction,
            _now_ms: u64,
        ) -> Result<LegacyRecoveryStageEffect, LegacyRecoveryEffectError> {
            let outcome = match action {
                RecoveryAction::Preflight {
                    envelope,
                    candidate,
                    ..
                } => {
                    self.stages
                        .push(format!("preflight:{}", candidate.config_id()));
                    ExecutorOutcome::PreflightPassed {
                        refreshed_fence: refreshed_fence(
                            envelope,
                            LaneGeneration::new(
                                envelope.expected_lane_generation.get().saturating_add(1),
                            ),
                        ),
                    }
                }
                RecoveryAction::StopPrevious { previous_owner, .. } => {
                    self.stages.push("stop".into());
                    ExecutorOutcome::Stopped {
                        previous: previous_owner.clone(),
                    }
                }
                RecoveryAction::StartCandidate { candidate, .. } => {
                    self.stages.push(format!("start:{}", candidate.config_id()));
                    ExecutorOutcome::StartFailed {
                        candidate_fingerprint: candidate.fingerprint().clone(),
                    }
                }
                RecoveryAction::RollbackPrevious {
                    envelope,
                    previous,
                    previous_lane_generation,
                    retry_pending: true,
                } => {
                    self.stages.push("rollback".into());
                    ExecutorOutcome::RolledBackForRetry {
                        previous: ProcessOwner {
                            pid: 51,
                            process_start_identity: ProcessStartIdentity::new(151),
                            config_fingerprint: previous.fingerprint().clone(),
                            lane_generation: *previous_lane_generation,
                        },
                        refreshed_fence: refreshed_fence(envelope, *previous_lane_generation),
                    }
                }
                _ => return Err(LegacyRecoveryEffectError::InvalidAction),
            };
            Ok(LegacyRecoveryStageEffect::Completed(outcome))
        }
    }

    fn refreshed_fence(envelope: &IntentEnvelope, lane_generation: LaneGeneration) -> IntentFence {
        IntentFence {
            session_id: envelope.session_id,
            category: envelope.category.clone(),
            lane_generation,
            sensor_generation: SensorGeneration::new(
                envelope.expected_sensor_generation.get().saturating_add(1),
            ),
            registry_version: RegistryVersion::new(
                envelope.expected_registry_version.get().saturating_add(1),
            ),
            network_fingerprint: envelope.expected_network_fingerprint.clone(),
        }
    }

    fn owner(config: &str, generation: u64, pid: u32) -> ProcessOwner {
        ProcessOwner {
            pid,
            process_start_identity: ProcessStartIdentity::new(u64::from(pid) + 100),
            config_fingerprint: ConfigFingerprint::new(config),
            lane_generation: LaneGeneration::new(generation),
        }
    }

    #[test]
    fn transaction_drives_sync_stages_and_resumes_bounded_confirmation() {
        let network_fingerprint = NetworkFingerprint::Stable {
            key: "network".into(),
        };
        let fence = obsession_runtime_reliability::legacy_reliability::contracts::IntentFence {
            session_id: SessionId::new(7),
            category: "discord".into(),
            lane_generation: LaneGeneration::new(3),
            sensor_generation: SensorGeneration::new(5),
            registry_version: RegistryVersion::new(11),
            network_fingerprint: network_fingerprint.clone(),
        };
        let previous = RecoveryConfig::new("old", "old");
        let candidate = RecoveryConfig::new("new", "new");
        let previous_owner = owner("old", 3, 41);
        let mut coordinator = CoordinatorRuntime::new(RecoveryMode::Automatic);
        let decision = coordinator
            .consider(
                IncidentObservation {
                    session_id: fence.session_id,
                    sensor_generation: fence.sensor_generation,
                    category: fence.category.clone(),
                    lane_generation: fence.lane_generation,
                    evidence_epoch: 13,
                    classification: AssessmentClassification::DpiSuspected,
                },
                fence.clone(),
                previous,
                previous_owner.clone(),
                vec![candidate],
                100,
            )
            .unwrap();
        let RecoveryDecision::Automatic { action } = decision else {
            panic!("automatic coordinator must arm preflight");
        };
        let refreshed_fence =
            obsession_runtime_reliability::legacy_reliability::contracts::IntentFence {
                session_id: fence.session_id,
                category: fence.category,
                lane_generation: LaneGeneration::new(4),
                sensor_generation: SensorGeneration::new(6),
                registry_version: RegistryVersion::new(12),
                network_fingerprint,
            };
        let candidate_owner = owner("new", 4, 43);
        let mut effects = FakeEffects {
            previous: previous_owner,
            candidate: candidate_owner,
            refreshed_fence,
            confirmation_pending: true,
            stages: Vec::new(),
        };
        let mut transaction = LegacyRecoveryTransaction::new(action).unwrap();

        assert_eq!(
            transaction.advance(&mut coordinator, &mut effects, 110),
            Ok(LegacyRecoveryTransactionProgress::Pending)
        );
        assert_eq!(
            effects.stages,
            ["preflight", "stop", "start", "confirm_pending"]
        );
        let terminal = transaction
            .advance(&mut coordinator, &mut effects, 120)
            .unwrap();
        assert!(matches!(
            terminal,
            LegacyRecoveryTransactionProgress::Complete(RecoveryAction::Complete { .. })
        ));
        assert_eq!(
            effects.stages,
            [
                "preflight",
                "stop",
                "start",
                "confirm_pending",
                "confirm",
                "commit"
            ]
        );
    }

    #[test]
    fn automatic_retry_requires_a_new_preflight_transaction_between_candidates() {
        let network_fingerprint = NetworkFingerprint::Stable {
            key: "network".into(),
        };
        let fence = IntentFence {
            session_id: SessionId::new(7),
            category: "discord".into(),
            lane_generation: LaneGeneration::new(3),
            sensor_generation: SensorGeneration::new(5),
            registry_version: RegistryVersion::new(11),
            network_fingerprint,
        };
        let previous = RecoveryConfig::new("old", "old");
        let candidate_a = RecoveryConfig::new("candidate-a", "candidate-a");
        let candidate_b = RecoveryConfig::new("candidate-b", "candidate-b");
        let previous_owner = owner("old", 3, 41);
        let mut coordinator = CoordinatorRuntime::new(RecoveryMode::Automatic);
        let decision = coordinator
            .consider(
                IncidentObservation {
                    session_id: fence.session_id,
                    sensor_generation: fence.sensor_generation,
                    category: fence.category.clone(),
                    lane_generation: fence.lane_generation,
                    evidence_epoch: 13,
                    classification: AssessmentClassification::DpiSuspected,
                },
                fence,
                previous,
                previous_owner,
                vec![candidate_a, candidate_b.clone()],
                100,
            )
            .unwrap();
        let RecoveryDecision::Automatic { action } = decision else {
            panic!("automatic coordinator must arm the first preflight");
        };
        let mut transaction = LegacyRecoveryTransaction::new(action).unwrap();
        let mut effects = AutomaticRetryEffects::default();

        let progress = transaction
            .advance(&mut coordinator, &mut effects, 110)
            .unwrap();
        let LegacyRecoveryTransactionProgress::PreflightRequired(next_action) = progress else {
            panic!("the next candidate must cross an immutable preflight boundary");
        };
        let RecoveryAction::Preflight {
            candidate,
            envelope,
            ..
        } = &next_action
        else {
            panic!("preflight handoff must retain the exact next action");
        };
        assert_eq!(candidate, &candidate_b);
        assert_eq!(envelope.attempt_id, AttemptId::new(1));
        assert_eq!(
            effects.stages,
            [
                "preflight:candidate-a",
                "stop",
                "start:candidate-a",
                "rollback"
            ]
        );

        let repeated = transaction
            .advance(&mut coordinator, &mut effects, 120)
            .unwrap();
        assert_eq!(
            repeated,
            LegacyRecoveryTransactionProgress::PreflightRequired(next_action.clone())
        );
        assert_eq!(effects.stages.len(), 4);
        assert!(LegacyRecoveryTransaction::new(next_action).is_ok());
    }
}
