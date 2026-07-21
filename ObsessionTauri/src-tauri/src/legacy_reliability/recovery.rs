//! Pure Phase 3 recovery coordinator.
//!
//! This module describes what a scoped executor is allowed to do. It never
//! starts a process, writes selection/cache state, or reads a wall clock. All
//! time values are supplied by the caller from one monotonic clock.

use std::collections::{BTreeMap, VecDeque};

use serde::{Deserialize, Serialize};

use super::contracts::{
    AttemptId, ConfigFingerprint, ExecutorOutcome, ExecutorResult, ExecutorStage, IntentEnvelope,
    IntentFence, LaneGeneration, NetworkFingerprint, ProcessOwner, SessionId,
};

pub const ASSISTED_PROPOSAL_TTL_MS: u64 = 30_000;
pub const NEGATIVE_COOLDOWN_MS: u64 = 300_000;
pub const MAX_NEGATIVE_COOLDOWNS: usize = 128;

const MAX_RETIRED_PROPOSALS: usize = 128;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryMode {
    #[default]
    ObserveOnly,
    Assisted,
}

/// Monotonic incident sequence assigned per session and category by Manager.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct IncidentId(u64);

impl IncidentId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProposalId(u64);

impl ProposalId {
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryConfig {
    config_id: String,
    fingerprint: ConfigFingerprint,
}

impl RecoveryConfig {
    pub fn new(config_id: impl Into<String>, fingerprint: impl Into<ConfigFingerprint>) -> Self {
        Self {
            config_id: config_id.into(),
            fingerprint: fingerprint.into(),
        }
    }

    pub fn config_id(&self) -> &str {
        &self.config_id
    }

    pub const fn fingerprint(&self) -> &ConfigFingerprint {
        &self.fingerprint
    }
}

/// One independently fenced incident. Candidate order is already ranked by
/// the caller; this coordinator only applies safety exclusions and chooses at
/// most the first eligible candidate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryRequest {
    incident_id: IncidentId,
    fence: IntentFence,
    previous: RecoveryConfig,
    previous_owner: ProcessOwner,
    candidates: Vec<RecoveryConfig>,
}

impl RecoveryRequest {
    pub fn new(
        incident_id: IncidentId,
        fence: IntentFence,
        previous: RecoveryConfig,
        previous_owner: ProcessOwner,
        candidates: Vec<RecoveryConfig>,
    ) -> Self {
        Self {
            incident_id,
            fence,
            previous,
            previous_owner,
            candidates,
        }
    }

    pub const fn incident_id(&self) -> IncidentId {
        self.incident_id
    }

    pub const fn fence(&self) -> &IntentFence {
        &self.fence
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoverySuggestion {
    pub incident_id: IncidentId,
    pub category: String,
    pub previous_config_id: String,
    pub candidate_config_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum RecoveryDecision {
    ObserveOnly { suggestion: RecoverySuggestion },
    Assisted { proposal: AssistedProposalView },
}

/// Backend-owned proposal. Fence fields and candidate fingerprints are never
/// accepted back from the UI. The public projection exposes only an opaque
/// token and display-safe identifiers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssistedProposal {
    proposal_id: ProposalId,
    incident_id: IncidentId,
    envelope: IntentEnvelope,
    previous: RecoveryConfig,
    previous_owner: ProcessOwner,
    candidate: RecoveryConfig,
    issued_at_monotonic_ms: u64,
    expires_at_monotonic_ms: u64,
}

impl AssistedProposal {
    pub const fn proposal_id(&self) -> ProposalId {
        self.proposal_id
    }

    pub const fn attempt_id(&self) -> AttemptId {
        self.envelope.attempt_id
    }

    pub const fn expires_at_monotonic_ms(&self) -> u64 {
        self.expires_at_monotonic_ms
    }

    pub fn view(&self) -> AssistedProposalView {
        AssistedProposalView {
            proposal_id: self.proposal_id,
            attempt_id: self.envelope.attempt_id,
            incident_id: self.incident_id,
            category: self.envelope.category.clone(),
            previous_config_id: self.previous.config_id.clone(),
            candidate_config_id: self.candidate.config_id.clone(),
            expires_at_monotonic_ms: self.expires_at_monotonic_ms,
        }
    }

    pub const fn approval(&self) -> AssistedApproval {
        AssistedApproval {
            proposal_id: self.proposal_id,
            attempt_id: self.envelope.attempt_id,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistedProposalView {
    pub proposal_id: ProposalId,
    pub attempt_id: AttemptId,
    pub incident_id: IncidentId,
    pub category: String,
    pub previous_config_id: String,
    pub candidate_config_id: String,
    pub expires_at_monotonic_ms: u64,
}

/// The only UI-originating approval data. All safety-critical fields are read
/// from the matching backend-owned [`AssistedProposal`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistedApproval {
    proposal_id: ProposalId,
    attempt_id: AttemptId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryPhase {
    Preflight,
    Stopping,
    Starting,
    Confirming,
    RollingBack,
    Applied,
    ProcessFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryDisposition {
    CandidateApplied,
    PreviousPreserved,
    RolledBack,
    ProcessFailed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryAttemptView {
    pub attempt_id: AttemptId,
    pub incident_id: IncidentId,
    pub category: String,
    pub previous_config_id: String,
    pub candidate_config_id: String,
    pub phase: RecoveryPhase,
    pub phase_started_at_monotonic_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryCompletion {
    pub attempt_id: AttemptId,
    pub incident_id: IncidentId,
    pub category: String,
    pub previous_config_id: String,
    pub candidate_config_id: String,
    pub phase: RecoveryPhase,
    pub disposition: RecoveryDisposition,
    pub finished_at_monotonic_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryStatus {
    pub mode: RecoveryMode,
    pub proposal: Option<AssistedProposalView>,
    pub active_attempt: Option<RecoveryAttemptView>,
    pub last_completion: Option<RecoveryCompletion>,
    pub negative_cooldown_count: usize,
}

/// Commands are inert values. The runtime must fence them immediately before
/// executing the represented side effect and return an [`ExecutorResult`]
/// carrying the exact same envelope.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum RecoveryAction {
    Preflight {
        envelope: IntentEnvelope,
        previous: RecoveryConfig,
        previous_owner: ProcessOwner,
        candidate: RecoveryConfig,
    },
    StopPrevious {
        envelope: IntentEnvelope,
        previous: RecoveryConfig,
        previous_owner: ProcessOwner,
    },
    StartCandidate {
        envelope: IntentEnvelope,
        candidate: RecoveryConfig,
        candidate_lane_generation: LaneGeneration,
    },
    ConfirmCandidate {
        envelope: IntentEnvelope,
        candidate: RecoveryConfig,
        owner: ProcessOwner,
    },
    CommitCandidate {
        envelope: IntentEnvelope,
        candidate: RecoveryConfig,
        owner: ProcessOwner,
    },
    RollbackPrevious {
        envelope: IntentEnvelope,
        previous: RecoveryConfig,
        previous_lane_generation: LaneGeneration,
    },
    Complete {
        completion: RecoveryCompletion,
    },
    ManualIntervention {
        completion: RecoveryCompletion,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsiderError {
    Busy,
    IncidentAlreadyHandled,
    StableNetworkRequired,
    NoEligibleCandidate,
    PreviousOwnerMismatch,
    ClockMovedBack,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApprovalError {
    ObserveOnly,
    Busy,
    UnknownProposal,
    ProposalMismatch,
    Expired,
    Duplicate,
    Cancelled,
    FenceChanged,
    ClockMovedBack,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransitionError {
    NoActiveAttempt,
    DuplicateResult,
    StaleEnvelope,
    UnexpectedOutcome,
    ProcessOwnerMismatch,
    RefreshedFenceMismatch,
    ClockMovedBack,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct IncidentLane {
    session_id: SessionId,
    category: String,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct CooldownKey {
    stable_network: String,
    category: String,
    candidate_fingerprint: ConfigFingerprint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RetiredProposalReason {
    Approved,
    Expired,
    Cancelled,
}

#[derive(Clone, Debug)]
struct ActiveAttempt {
    incident_id: IncidentId,
    envelope: IntentEnvelope,
    previous: RecoveryConfig,
    previous_owner: ProcessOwner,
    candidate: RecoveryConfig,
    candidate_lane_generation: LaneGeneration,
    current_lane_generation: LaneGeneration,
    expected_rollback_generation: Option<LaneGeneration>,
    candidate_owner: Option<ProcessOwner>,
    confirmation_succeeded: bool,
    phase: RecoveryPhase,
    phase_started_at_monotonic_ms: u64,
}

impl ActiveAttempt {
    fn view(&self) -> RecoveryAttemptView {
        RecoveryAttemptView {
            attempt_id: self.envelope.attempt_id,
            incident_id: self.incident_id,
            category: self.envelope.category.clone(),
            previous_config_id: self.previous.config_id.clone(),
            candidate_config_id: self.candidate.config_id.clone(),
            phase: self.phase,
            phase_started_at_monotonic_ms: self.phase_started_at_monotonic_ms,
        }
    }

    fn complete(
        &self,
        phase: RecoveryPhase,
        disposition: RecoveryDisposition,
        finished_at_monotonic_ms: u64,
    ) -> RecoveryCompletion {
        RecoveryCompletion {
            attempt_id: self.envelope.attempt_id,
            incident_id: self.incident_id,
            category: self.envelope.category.clone(),
            previous_config_id: self.previous.config_id.clone(),
            candidate_config_id: self.candidate.config_id.clone(),
            phase,
            disposition,
            finished_at_monotonic_ms,
        }
    }

    fn enter(&mut self, phase: RecoveryPhase, now_ms: u64) {
        self.phase = phase;
        self.phase_started_at_monotonic_ms = now_ms;
    }
}

#[derive(Debug)]
pub struct RecoveryCoordinator {
    mode: RecoveryMode,
    next_attempt_id: u64,
    next_proposal_id: u64,
    last_monotonic_ms: u64,
    proposal: Option<AssistedProposal>,
    active: Option<ActiveAttempt>,
    last_completion: Option<RecoveryCompletion>,
    retired_proposals: VecDeque<(ProposalId, RetiredProposalReason)>,
    latest_handled_incident: BTreeMap<IncidentLane, IncidentId>,
    negative_cooldowns: BTreeMap<CooldownKey, u64>,
}

impl Default for RecoveryCoordinator {
    fn default() -> Self {
        Self::new(RecoveryMode::ObserveOnly)
    }
}

impl RecoveryCoordinator {
    pub fn new(mode: RecoveryMode) -> Self {
        Self {
            mode,
            next_attempt_id: 1,
            next_proposal_id: 1,
            last_monotonic_ms: 0,
            proposal: None,
            active: None,
            last_completion: None,
            retired_proposals: VecDeque::new(),
            latest_handled_incident: BTreeMap::new(),
            negative_cooldowns: BTreeMap::new(),
        }
    }

    pub const fn mode(&self) -> RecoveryMode {
        self.mode
    }

    /// Downgrading the mode revokes an unapproved proposal. An attempt that has
    /// already entered stop/start continues so it can reach candidate or exact
    /// rollback safely.
    pub fn set_mode(&mut self, mode: RecoveryMode) {
        self.mode = mode;
        if mode == RecoveryMode::ObserveOnly {
            self.cancel_pending();
        }
    }

    /// Revokes only an unapproved proposal. An active stop/start transaction
    /// remains owned by the executor and must reach a safe commit or rollback.
    pub fn cancel_pending(&mut self) {
        if let Some(proposal) = self.proposal.take() {
            self.retire_proposal(proposal.proposal_id, RetiredProposalReason::Cancelled);
        }
    }

    pub fn clear_last_completion_if_idle(&mut self) {
        if self.active.is_none() {
            self.last_completion = None;
        }
    }

    /// Last-resort terminalization for an executor/coordinator invariant
    /// failure. The caller must first attempt process cleanup; this method only
    /// guarantees that stale approvals cannot leave the coordinator Busy.
    pub fn force_manual_failure(&mut self, now_ms: u64) -> Option<RecoveryAction> {
        let active = self.active.take()?;
        self.last_monotonic_ms = self.last_monotonic_ms.max(now_ms);
        let completion = active.complete(
            RecoveryPhase::ProcessFailed,
            RecoveryDisposition::ProcessFailed,
            self.last_monotonic_ms,
        );
        self.last_completion = Some(completion.clone());
        Some(RecoveryAction::ManualIntervention { completion })
    }

    pub fn retain_incidents_for_session(&mut self, session_id: SessionId) {
        self.latest_handled_incident
            .retain(|lane, _| lane.session_id == session_id);
    }

    pub fn proposal(&self) -> Option<&AssistedProposal> {
        self.proposal.as_ref()
    }

    pub fn status(&self) -> RecoveryStatus {
        RecoveryStatus {
            mode: self.mode,
            proposal: self.proposal.as_ref().map(AssistedProposal::view),
            active_attempt: self.active.as_ref().map(ActiveAttempt::view),
            last_completion: self.last_completion.clone(),
            negative_cooldown_count: self.negative_cooldowns.len(),
        }
    }

    pub fn consider(
        &mut self,
        request: RecoveryRequest,
        now_ms: u64,
    ) -> Result<RecoveryDecision, ConsiderError> {
        // Reconciliation keeps calling `consider` while an approved executor
        // transaction is running. A rejected Busy observation must not advance
        // the coordinator clock ahead of the executor result that owns the
        // active attempt. Proposals are different: they have a backend-owned
        // TTL, so `tick` must get a chance to retire an expired one.
        if self.active.is_some() {
            return Err(ConsiderError::Busy);
        }
        self.tick(now_ms)
            .map_err(|()| ConsiderError::ClockMovedBack)?;
        if self.proposal.is_some() {
            return Err(ConsiderError::Busy);
        }

        let stable_network = request
            .fence
            .network_fingerprint
            .stable_key()
            .ok_or(ConsiderError::StableNetworkRequired)?
            .to_owned();
        if !request.previous_owner.owns(
            request.previous.fingerprint(),
            request.fence.lane_generation,
        ) {
            return Err(ConsiderError::PreviousOwnerMismatch);
        }
        let incident_lane = IncidentLane {
            session_id: request.fence.session_id,
            category: request.fence.category.clone(),
        };
        if self
            .latest_handled_incident
            .get(&incident_lane)
            .is_some_and(|latest| request.incident_id <= *latest)
        {
            return Err(ConsiderError::IncidentAlreadyHandled);
        }

        let candidate = request
            .candidates
            .iter()
            .find(|candidate| {
                candidate.fingerprint != request.previous.fingerprint
                    && !self.negative_cooldowns.contains_key(&CooldownKey {
                        stable_network: stable_network.clone(),
                        category: request.fence.category.clone(),
                        candidate_fingerprint: candidate.fingerprint.clone(),
                    })
            })
            .cloned()
            .ok_or(ConsiderError::NoEligibleCandidate)?;

        let suggestion = RecoverySuggestion {
            incident_id: request.incident_id,
            category: request.fence.category.clone(),
            previous_config_id: request.previous.config_id.clone(),
            candidate_config_id: candidate.config_id.clone(),
        };
        if self.mode == RecoveryMode::ObserveOnly {
            return Ok(RecoveryDecision::ObserveOnly { suggestion });
        }

        let attempt_id = AttemptId::new(self.take_attempt_id());
        let proposal_id = ProposalId(self.take_proposal_id());
        let proposal = AssistedProposal {
            proposal_id,
            incident_id: request.incident_id,
            envelope: IntentEnvelope::from_fence(attempt_id, &request.fence),
            previous: request.previous,
            previous_owner: request.previous_owner,
            candidate,
            issued_at_monotonic_ms: now_ms,
            expires_at_monotonic_ms: now_ms.saturating_add(ASSISTED_PROPOSAL_TTL_MS),
        };
        let view = proposal.view();
        self.latest_handled_incident
            .insert(incident_lane, request.incident_id);
        self.proposal = Some(proposal);
        Ok(RecoveryDecision::Assisted { proposal: view })
    }

    pub fn approve(
        &mut self,
        approval: AssistedApproval,
        current_fence: &IntentFence,
        now_ms: u64,
    ) -> Result<RecoveryAction, ApprovalError> {
        if let Some(reason) = self.retired_proposal_reason(approval.proposal_id) {
            return Err(match reason {
                RetiredProposalReason::Approved => ApprovalError::Duplicate,
                RetiredProposalReason::Expired => ApprovalError::Expired,
                RetiredProposalReason::Cancelled => ApprovalError::Cancelled,
            });
        }
        if self.mode != RecoveryMode::Assisted {
            return Err(ApprovalError::ObserveOnly);
        }
        if self.active.is_some() {
            return Err(ApprovalError::Busy);
        }
        let Some(pending) = self.proposal.as_ref() else {
            return Err(ApprovalError::UnknownProposal);
        };
        if pending.proposal_id != approval.proposal_id
            || pending.envelope.attempt_id != approval.attempt_id
        {
            return Err(ApprovalError::ProposalMismatch);
        }
        self.tick(now_ms)
            .map_err(|()| ApprovalError::ClockMovedBack)?;
        let Some(pending) = self.proposal.as_ref() else {
            return Err(ApprovalError::Expired);
        };
        if !pending.envelope.matches_fence(current_fence) {
            return Err(ApprovalError::FenceChanged);
        }

        let pending = self.proposal.take().expect("proposal checked above");
        self.retire_proposal(pending.proposal_id, RetiredProposalReason::Approved);
        let candidate_lane_generation = next_generation(pending.envelope.expected_lane_generation);
        let active = ActiveAttempt {
            incident_id: pending.incident_id,
            current_lane_generation: pending.envelope.expected_lane_generation,
            candidate_lane_generation,
            expected_rollback_generation: None,
            candidate_owner: None,
            confirmation_succeeded: false,
            phase: RecoveryPhase::Preflight,
            phase_started_at_monotonic_ms: now_ms,
            envelope: pending.envelope,
            previous: pending.previous,
            previous_owner: pending.previous_owner,
            candidate: pending.candidate,
        };
        let action = RecoveryAction::Preflight {
            envelope: active.envelope.clone(),
            previous: active.previous.clone(),
            previous_owner: active.previous_owner.clone(),
            candidate: active.candidate.clone(),
        };
        self.active = Some(active);
        Ok(action)
    }

    pub fn apply_result(
        &mut self,
        result: ExecutorResult,
    ) -> Result<RecoveryAction, TransitionError> {
        let Some(active) = self.active.as_ref() else {
            return if self
                .last_completion
                .as_ref()
                .is_some_and(|last| last.attempt_id == result.envelope.attempt_id)
            {
                Err(TransitionError::DuplicateResult)
            } else {
                Err(TransitionError::NoActiveAttempt)
            };
        };
        if active.envelope != result.envelope {
            return Err(TransitionError::StaleEnvelope);
        }
        self.tick(result.completed_at_monotonic_ms)
            .map_err(|()| TransitionError::ClockMovedBack)?;

        let mut active = self.active.take().expect("active attempt checked above");
        let transition = self.transition(
            &mut active,
            result.outcome,
            result.completed_at_monotonic_ms,
        );
        match transition {
            Ok(Transition::Continue(action)) => {
                self.active = Some(active);
                Ok(action)
            }
            Ok(Transition::Complete(completion, manual)) => {
                self.last_completion = Some(completion.clone());
                if manual {
                    Ok(RecoveryAction::ManualIntervention { completion })
                } else {
                    Ok(RecoveryAction::Complete { completion })
                }
            }
            Err(error) => {
                self.active = Some(active);
                Err(error)
            }
        }
    }

    pub fn cooldown_until(
        &mut self,
        network: &NetworkFingerprint,
        category: &str,
        candidate: &ConfigFingerprint,
        now_ms: u64,
    ) -> Result<Option<u64>, ConsiderError> {
        self.tick(now_ms)
            .map_err(|()| ConsiderError::ClockMovedBack)?;
        let Some(stable_network) = network.stable_key() else {
            return Ok(None);
        };
        Ok(self
            .negative_cooldowns
            .get(&CooldownKey {
                stable_network: stable_network.to_owned(),
                category: category.to_owned(),
                candidate_fingerprint: candidate.clone(),
            })
            .copied())
    }

    fn transition(
        &mut self,
        active: &mut ActiveAttempt,
        outcome: ExecutorOutcome,
        now_ms: u64,
    ) -> Result<Transition, TransitionError> {
        match (active.phase, outcome) {
            (RecoveryPhase::Preflight, ExecutorOutcome::PreflightPassed { refreshed_fence }) => {
                rebase_after_observer_restart(active, refreshed_fence)?;
                active.enter(RecoveryPhase::Stopping, now_ms);
                Ok(Transition::Continue(RecoveryAction::StopPrevious {
                    envelope: active.envelope.clone(),
                    previous: active.previous.clone(),
                    previous_owner: active.previous_owner.clone(),
                }))
            }
            (RecoveryPhase::Preflight, ExecutorOutcome::PreflightRejected) => {
                Ok(Transition::Complete(
                    active.complete(
                        RecoveryPhase::Applied,
                        RecoveryDisposition::PreviousPreserved,
                        now_ms,
                    ),
                    false,
                ))
            }
            (
                RecoveryPhase::Preflight | RecoveryPhase::Stopping,
                ExecutorOutcome::PreviousProcessMissing {
                    previous_fingerprint,
                },
            ) => {
                if previous_fingerprint != active.previous.fingerprint {
                    return Err(TransitionError::ProcessOwnerMismatch);
                }
                Ok(Transition::Complete(
                    active.complete(
                        RecoveryPhase::ProcessFailed,
                        RecoveryDisposition::ProcessFailed,
                        now_ms,
                    ),
                    true,
                ))
            }
            (
                RecoveryPhase::Preflight | RecoveryPhase::Stopping,
                ExecutorOutcome::RollbackFailed {
                    previous_fingerprint,
                },
            ) => {
                if previous_fingerprint != active.previous.fingerprint {
                    return Err(TransitionError::ProcessOwnerMismatch);
                }
                Ok(Transition::Complete(
                    active.complete(
                        RecoveryPhase::ProcessFailed,
                        RecoveryDisposition::ProcessFailed,
                        now_ms,
                    ),
                    true,
                ))
            }
            (
                RecoveryPhase::Preflight,
                ExecutorOutcome::ExecutionAborted {
                    stage: ExecutorStage::Preflight,
                    ..
                },
            )
            | (
                RecoveryPhase::Stopping,
                ExecutorOutcome::ExecutionAborted {
                    stage: ExecutorStage::Stop,
                    ..
                },
            ) => Ok(Transition::Complete(
                active.complete(
                    RecoveryPhase::Applied,
                    RecoveryDisposition::PreviousPreserved,
                    now_ms,
                ),
                false,
            )),
            (RecoveryPhase::Stopping, ExecutorOutcome::Stopped { previous }) => {
                ensure_exact_owner(&previous, &active.previous_owner)?;
                active.enter(RecoveryPhase::Starting, now_ms);
                Ok(Transition::Continue(RecoveryAction::StartCandidate {
                    envelope: active.envelope.clone(),
                    candidate: active.candidate.clone(),
                    candidate_lane_generation: active.candidate_lane_generation,
                }))
            }
            (RecoveryPhase::Stopping, ExecutorOutcome::StopTimedOut { previous }) => {
                ensure_exact_owner(&previous, &active.previous_owner)?;
                Ok(Transition::Complete(
                    active.complete(
                        RecoveryPhase::Applied,
                        RecoveryDisposition::PreviousPreserved,
                        now_ms,
                    ),
                    false,
                ))
            }
            (RecoveryPhase::Starting, ExecutorOutcome::Ready { candidate }) => {
                ensure_owner(
                    &candidate,
                    active.candidate.fingerprint(),
                    active.candidate_lane_generation,
                )?;
                active.current_lane_generation = active.candidate_lane_generation;
                active.envelope.expected_lane_generation = active.current_lane_generation;
                active.candidate_owner = Some(candidate.clone());
                active.enter(RecoveryPhase::Confirming, now_ms);
                Ok(Transition::Continue(RecoveryAction::ConfirmCandidate {
                    envelope: active.envelope.clone(),
                    candidate: active.candidate.clone(),
                    owner: candidate,
                }))
            }
            (
                RecoveryPhase::Starting,
                ExecutorOutcome::StartFailed {
                    candidate_fingerprint,
                },
            ) => {
                if candidate_fingerprint != active.candidate.fingerprint {
                    return Err(TransitionError::ProcessOwnerMismatch);
                }
                self.record_negative_cooldown(active, now_ms);
                Ok(Transition::Continue(begin_rollback(active, now_ms)))
            }
            (
                RecoveryPhase::Starting,
                ExecutorOutcome::ExecutionAborted {
                    stage: ExecutorStage::Start,
                    reason,
                },
            ) => {
                if reason.penalizes_candidate() {
                    self.record_negative_cooldown(active, now_ms);
                }
                Ok(Transition::Continue(begin_rollback(active, now_ms)))
            }
            (RecoveryPhase::Confirming, ExecutorOutcome::ConfirmationSucceeded { candidate })
                if !active.confirmation_succeeded =>
            {
                ensure_candidate_owner(active, &candidate)?;
                active.confirmation_succeeded = true;
                active.phase_started_at_monotonic_ms = now_ms;
                Ok(Transition::Continue(RecoveryAction::CommitCandidate {
                    envelope: active.envelope.clone(),
                    candidate: active.candidate.clone(),
                    owner: candidate,
                }))
            }
            (
                RecoveryPhase::Confirming,
                ExecutorOutcome::ConfirmationFailed { candidate, reason },
            ) if !active.confirmation_succeeded => {
                ensure_candidate_owner(active, &candidate)?;
                if reason.penalizes_candidate() {
                    self.record_negative_cooldown(active, now_ms);
                }
                Ok(Transition::Continue(begin_rollback(active, now_ms)))
            }
            (RecoveryPhase::Confirming, ExecutorOutcome::CandidateCommitted { candidate })
                if active.confirmation_succeeded =>
            {
                ensure_candidate_owner(active, &candidate)?;
                Ok(Transition::Complete(
                    active.complete(
                        RecoveryPhase::Applied,
                        RecoveryDisposition::CandidateApplied,
                        now_ms,
                    ),
                    false,
                ))
            }
            (RecoveryPhase::Confirming, ExecutorOutcome::CommitFailed { candidate })
                if active.confirmation_succeeded =>
            {
                ensure_candidate_owner(active, &candidate)?;
                Ok(Transition::Continue(begin_rollback(active, now_ms)))
            }
            (
                RecoveryPhase::Confirming,
                ExecutorOutcome::ExecutionAborted {
                    stage: ExecutorStage::Confirmation | ExecutorStage::Commit,
                    reason,
                },
            ) => {
                if reason.penalizes_candidate() {
                    self.record_negative_cooldown(active, now_ms);
                }
                Ok(Transition::Continue(begin_rollback(active, now_ms)))
            }
            (
                RecoveryPhase::Confirming,
                ExecutorOutcome::Exited {
                    process,
                    intentional: false,
                },
            ) => {
                ensure_candidate_owner(active, &process)?;
                self.record_negative_cooldown(active, now_ms);
                Ok(Transition::Continue(begin_rollback(active, now_ms)))
            }
            (RecoveryPhase::RollingBack, ExecutorOutcome::RolledBack { previous }) => {
                let expected = active
                    .expected_rollback_generation
                    .ok_or(TransitionError::UnexpectedOutcome)?;
                ensure_owner(&previous, active.previous.fingerprint(), expected)?;
                Ok(Transition::Complete(
                    active.complete(
                        RecoveryPhase::Applied,
                        RecoveryDisposition::RolledBack,
                        now_ms,
                    ),
                    false,
                ))
            }
            (
                RecoveryPhase::RollingBack,
                ExecutorOutcome::RollbackFailed {
                    previous_fingerprint,
                },
            ) => {
                if previous_fingerprint != active.previous.fingerprint {
                    return Err(TransitionError::ProcessOwnerMismatch);
                }
                Ok(Transition::Complete(
                    active.complete(
                        RecoveryPhase::ProcessFailed,
                        RecoveryDisposition::ProcessFailed,
                        now_ms,
                    ),
                    true,
                ))
            }
            _ => Err(TransitionError::UnexpectedOutcome),
        }
    }

    fn record_negative_cooldown(&mut self, active: &ActiveAttempt, now_ms: u64) {
        let Some(stable_network) = active.envelope.expected_network_fingerprint.stable_key() else {
            return;
        };
        self.negative_cooldowns.insert(
            CooldownKey {
                stable_network: stable_network.to_owned(),
                category: active.envelope.category.clone(),
                candidate_fingerprint: active.candidate.fingerprint.clone(),
            },
            now_ms.saturating_add(NEGATIVE_COOLDOWN_MS),
        );
        while self.negative_cooldowns.len() > MAX_NEGATIVE_COOLDOWNS {
            let Some(key) = self
                .negative_cooldowns
                .iter()
                .min_by_key(|(key, until_ms)| (**until_ms, (*key).clone()))
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            self.negative_cooldowns.remove(&key);
        }
    }

    fn tick(&mut self, now_ms: u64) -> Result<(), ()> {
        if now_ms < self.last_monotonic_ms {
            return Err(());
        }
        self.last_monotonic_ms = now_ms;
        self.negative_cooldowns
            .retain(|_, until_ms| now_ms < *until_ms);
        if self
            .proposal
            .as_ref()
            .is_some_and(|proposal| now_ms >= proposal.expires_at_monotonic_ms)
        {
            let proposal = self.proposal.take().expect("proposal checked above");
            self.retire_proposal(proposal.proposal_id, RetiredProposalReason::Expired);
        }
        Ok(())
    }

    fn take_attempt_id(&mut self) -> u64 {
        let current = self.next_attempt_id;
        self.next_attempt_id = next_nonzero(self.next_attempt_id);
        current
    }

    fn take_proposal_id(&mut self) -> u64 {
        let current = self.next_proposal_id;
        self.next_proposal_id = next_nonzero(self.next_proposal_id);
        current
    }

    fn retire_proposal(&mut self, id: ProposalId, reason: RetiredProposalReason) {
        if self.retired_proposals.len() == MAX_RETIRED_PROPOSALS {
            self.retired_proposals.pop_front();
        }
        self.retired_proposals.push_back((id, reason));
    }

    fn retired_proposal_reason(&self, id: ProposalId) -> Option<RetiredProposalReason> {
        self.retired_proposals
            .iter()
            .rev()
            .find_map(|(retired_id, reason)| (*retired_id == id).then_some(*reason))
    }
}

enum Transition {
    Continue(RecoveryAction),
    Complete(RecoveryCompletion, bool),
}

fn begin_rollback(active: &mut ActiveAttempt, now_ms: u64) -> RecoveryAction {
    active.envelope.expected_lane_generation = active.current_lane_generation;
    let previous_lane_generation = next_generation(active.current_lane_generation);
    active.expected_rollback_generation = Some(previous_lane_generation);
    active.enter(RecoveryPhase::RollingBack, now_ms);
    RecoveryAction::RollbackPrevious {
        envelope: active.envelope.clone(),
        previous: active.previous.clone(),
        previous_lane_generation,
    }
}

fn rebase_after_observer_restart(
    active: &mut ActiveAttempt,
    refreshed: IntentFence,
) -> Result<(), TransitionError> {
    let previous = &active.envelope;
    if refreshed.session_id != previous.session_id
        || refreshed.category != previous.category
        || refreshed.network_fingerprint != previous.expected_network_fingerprint
        || refreshed.lane_generation != active.candidate_lane_generation
        || refreshed.sensor_generation == previous.expected_sensor_generation
        || refreshed.registry_version == previous.expected_registry_version
    {
        return Err(TransitionError::RefreshedFenceMismatch);
    }

    active.current_lane_generation = active.candidate_lane_generation;
    active.envelope = IntentEnvelope::from_fence(previous.attempt_id, &refreshed);
    Ok(())
}

fn ensure_candidate_owner(
    active: &ActiveAttempt,
    owner: &ProcessOwner,
) -> Result<(), TransitionError> {
    ensure_owner(
        owner,
        active.candidate.fingerprint(),
        active.candidate_lane_generation,
    )?;
    if active.candidate_owner.as_ref() != Some(owner) {
        return Err(TransitionError::ProcessOwnerMismatch);
    }
    Ok(())
}

fn ensure_owner(
    owner: &ProcessOwner,
    fingerprint: &ConfigFingerprint,
    generation: LaneGeneration,
) -> Result<(), TransitionError> {
    if owner.owns(fingerprint, generation) {
        Ok(())
    } else {
        Err(TransitionError::ProcessOwnerMismatch)
    }
}

fn ensure_exact_owner(
    actual: &ProcessOwner,
    expected: &ProcessOwner,
) -> Result<(), TransitionError> {
    if actual == expected {
        Ok(())
    } else {
        Err(TransitionError::ProcessOwnerMismatch)
    }
}

const fn next_generation(current: LaneGeneration) -> LaneGeneration {
    LaneGeneration::new(next_nonzero(current.get()))
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
        ConfirmationFailure, ProcessStartIdentity, RegistryVersion, SensorGeneration,
    };

    #[derive(Default)]
    struct FakeClock {
        now_ms: u64,
    }

    impl FakeClock {
        fn now(&self) -> u64 {
            self.now_ms
        }

        fn advance(&mut self, delta_ms: u64) {
            self.now_ms = self.now_ms.saturating_add(delta_ms);
        }
    }

    fn fence() -> IntentFence {
        IntentFence {
            session_id: SessionId::new(11),
            category: "video".into(),
            lane_generation: LaneGeneration::new(7),
            sensor_generation: SensorGeneration::new(3),
            registry_version: RegistryVersion::new(5),
            network_fingerprint: NetworkFingerprint::Stable {
                key: "stable-network-a".into(),
            },
        }
    }

    fn config(id: &str) -> RecoveryConfig {
        RecoveryConfig::new(id, format!("sha256:{id}"))
    }

    fn request(incident: u64, candidates: &[&str]) -> RecoveryRequest {
        RecoveryRequest::new(
            IncidentId::new(incident),
            fence(),
            config("active"),
            owner("active", 7, 1),
            candidates.iter().map(|id| config(id)).collect(),
        )
    }

    fn owner(config: &str, generation: u64, identity: u64) -> ProcessOwner {
        ProcessOwner {
            pid: 4000 + identity as u32,
            process_start_identity: ProcessStartIdentity::new(identity),
            config_fingerprint: ConfigFingerprint::new(format!("sha256:{config}")),
            lane_generation: LaneGeneration::new(generation),
        }
    }

    fn propose(
        coordinator: &mut RecoveryCoordinator,
        clock: &FakeClock,
        incident: u64,
        candidates: &[&str],
    ) -> AssistedProposal {
        let decision = coordinator
            .consider(request(incident, candidates), clock.now())
            .unwrap();
        assert!(matches!(decision, RecoveryDecision::Assisted { .. }));
        coordinator.proposal().unwrap().clone()
    }

    fn approve(
        coordinator: &mut RecoveryCoordinator,
        clock: &FakeClock,
        proposal: &AssistedProposal,
    ) -> IntentEnvelope {
        let action = coordinator
            .approve(proposal.approval(), &fence(), clock.now())
            .unwrap();
        match action {
            RecoveryAction::Preflight { envelope, .. } => envelope,
            other => panic!("unexpected action: {other:?}"),
        }
    }

    fn result(
        envelope: &IntentEnvelope,
        outcome: ExecutorOutcome,
        clock: &FakeClock,
    ) -> ExecutorResult {
        ExecutorResult {
            envelope: envelope.clone(),
            outcome,
            completed_at_monotonic_ms: clock.now(),
        }
    }

    fn preflight_outcome(envelope: &IntentEnvelope) -> ExecutorOutcome {
        ExecutorOutcome::PreflightPassed {
            refreshed_fence: IntentFence {
                session_id: envelope.session_id,
                category: envelope.category.clone(),
                lane_generation: next_generation(envelope.expected_lane_generation),
                sensor_generation: SensorGeneration::new(next_nonzero(
                    envelope.expected_sensor_generation.get(),
                )),
                registry_version: RegistryVersion::new(next_nonzero(
                    envelope.expected_registry_version.get(),
                )),
                network_fingerprint: envelope.expected_network_fingerprint.clone(),
            },
        }
    }

    fn pass_preflight(
        coordinator: &mut RecoveryCoordinator,
        envelope: &IntentEnvelope,
        clock: &FakeClock,
    ) -> IntentEnvelope {
        match coordinator
            .apply_result(result(envelope, preflight_outcome(envelope), clock))
            .unwrap()
        {
            RecoveryAction::StopPrevious { envelope, .. } => envelope,
            other => panic!("unexpected action: {other:?}"),
        }
    }

    #[test]
    fn observe_only_returns_a_projection_without_arming_an_attempt() {
        let clock = FakeClock::default();
        let mut coordinator = RecoveryCoordinator::default();
        assert_eq!(
            coordinator.consider(request(1, &["active", "candidate-a"]), clock.now()),
            Ok(RecoveryDecision::ObserveOnly {
                suggestion: RecoverySuggestion {
                    incident_id: IncidentId::new(1),
                    category: "video".into(),
                    previous_config_id: "active".into(),
                    candidate_config_id: "candidate-a".into(),
                }
            })
        );
        assert!(coordinator.status().proposal.is_none());
        assert!(coordinator.status().active_attempt.is_none());

        coordinator.set_mode(RecoveryMode::Assisted);
        assert!(matches!(
            coordinator.consider(request(1, &["candidate-a"]), clock.now()),
            Ok(RecoveryDecision::Assisted { .. })
        ));
    }

    #[test]
    fn busy_reconciliation_does_not_advance_the_active_coordinator_clock() {
        let clock = FakeClock::default();
        let mut coordinator = RecoveryCoordinator::new(RecoveryMode::Assisted);
        let proposal = propose(&mut coordinator, &clock, 1, &["candidate-a"]);
        let envelope = approve(&mut coordinator, &clock, &proposal);

        assert_eq!(
            coordinator.consider(request(2, &["candidate-b"]), 10_000),
            Err(ConsiderError::Busy)
        );
        let envelope = pass_preflight(&mut coordinator, &envelope, &clock);
        assert_eq!(envelope.attempt_id, AttemptId::new(1));
    }

    #[test]
    fn reconciliation_retires_an_expired_proposal_before_considering_the_incident() {
        let mut clock = FakeClock::default();
        let mut coordinator = RecoveryCoordinator::new(RecoveryMode::Assisted);
        let first = propose(&mut coordinator, &clock, 1, &["candidate-a"]);

        clock.advance(ASSISTED_PROPOSAL_TTL_MS - 1);
        assert_eq!(
            coordinator.consider(request(2, &["candidate-b"]), clock.now()),
            Err(ConsiderError::Busy)
        );

        clock.advance(1);
        let second = coordinator
            .consider(request(2, &["candidate-b"]), clock.now())
            .unwrap();
        let RecoveryDecision::Assisted { proposal } = second else {
            panic!("expired proposal must be replaced by a fresh assisted proposal");
        };
        assert_ne!(proposal.proposal_id, first.proposal_id);
        assert_eq!(proposal.incident_id, IncidentId::new(2));
    }

    #[test]
    fn approval_is_exact_expires_at_thirty_seconds_and_is_single_use() {
        let mut clock = FakeClock::default();
        let mut coordinator = RecoveryCoordinator::new(RecoveryMode::Assisted);
        let proposal = propose(&mut coordinator, &clock, 1, &["candidate-a"]);

        let mut changed = fence();
        changed.sensor_generation = SensorGeneration::new(4);
        assert_eq!(
            coordinator.approve(proposal.approval(), &changed, clock.now()),
            Err(ApprovalError::FenceChanged)
        );
        let _ = approve(&mut coordinator, &clock, &proposal);
        assert_eq!(
            coordinator.approve(proposal.approval(), &fence(), clock.now()),
            Err(ApprovalError::Duplicate)
        );

        let mut expiring = RecoveryCoordinator::new(RecoveryMode::Assisted);
        let expiring_proposal = propose(&mut expiring, &clock, 2, &["candidate-b"]);
        clock.advance(ASSISTED_PROPOSAL_TTL_MS);
        assert_eq!(
            expiring.approve(expiring_proposal.approval(), &fence(), clock.now()),
            Err(ApprovalError::Expired)
        );
    }

    #[test]
    fn only_one_attempt_can_run_globally_and_success_uses_exact_owner() {
        let mut clock = FakeClock::default();
        let mut coordinator = RecoveryCoordinator::new(RecoveryMode::Assisted);
        let proposal = propose(&mut coordinator, &clock, 1, &["candidate-a"]);
        let mut envelope = approve(&mut coordinator, &clock, &proposal);

        assert_eq!(
            coordinator.consider(request(2, &["candidate-b"]), clock.now()),
            Err(ConsiderError::Busy)
        );
        clock.advance(1);
        envelope = pass_preflight(&mut coordinator, &envelope, &clock);
        clock.advance(1);
        assert_eq!(
            coordinator.apply_result(result(
                &envelope,
                ExecutorOutcome::Stopped {
                    previous: owner("active", 7, 99),
                },
                &clock,
            )),
            Err(TransitionError::ProcessOwnerMismatch)
        );
        let start = coordinator
            .apply_result(result(
                &envelope,
                ExecutorOutcome::Stopped {
                    previous: owner("active", 7, 1),
                },
                &clock,
            ))
            .unwrap();
        assert!(matches!(start, RecoveryAction::StartCandidate { .. }));

        clock.advance(1);
        let candidate = owner("candidate-a", 8, 2);
        let confirm = coordinator
            .apply_result(result(
                &envelope,
                ExecutorOutcome::Ready {
                    candidate: candidate.clone(),
                },
                &clock,
            ))
            .unwrap();
        envelope = match confirm {
            RecoveryAction::ConfirmCandidate { envelope, .. } => envelope,
            other => panic!("unexpected action: {other:?}"),
        };
        assert_eq!(envelope.expected_lane_generation, LaneGeneration::new(8));

        clock.advance(1);
        assert!(matches!(
            coordinator
                .apply_result(result(
                    &envelope,
                    ExecutorOutcome::ConfirmationSucceeded {
                        candidate: candidate.clone(),
                    },
                    &clock,
                ))
                .unwrap(),
            RecoveryAction::CommitCandidate { .. }
        ));
        clock.advance(1);
        let complete = coordinator
            .apply_result(result(
                &envelope,
                ExecutorOutcome::CandidateCommitted { candidate },
                &clock,
            ))
            .unwrap();
        assert!(matches!(
            complete,
            RecoveryAction::Complete {
                completion: RecoveryCompletion {
                    phase: RecoveryPhase::Applied,
                    disposition: RecoveryDisposition::CandidateApplied,
                    ..
                }
            }
        ));
        assert!(coordinator.status().active_attempt.is_none());
    }

    #[test]
    fn stale_result_cannot_advance_or_mutate_an_attempt() {
        let clock = FakeClock::default();
        let mut coordinator = RecoveryCoordinator::new(RecoveryMode::Assisted);
        let proposal = propose(&mut coordinator, &clock, 1, &["candidate-a"]);
        let envelope = approve(&mut coordinator, &clock, &proposal);
        let before = coordinator.status();
        let mut stale = envelope.clone();
        stale.expected_registry_version = RegistryVersion::new(99);
        assert_eq!(
            coordinator.apply_result(result(&stale, preflight_outcome(&stale), &clock)),
            Err(TransitionError::StaleEnvelope)
        );
        assert_eq!(coordinator.status(), before);
    }

    #[test]
    fn observer_refresh_requires_exact_candidate_lane_and_new_sensor_registry() {
        let clock = FakeClock::default();
        let mut coordinator = RecoveryCoordinator::new(RecoveryMode::Assisted);
        let proposal = propose(&mut coordinator, &clock, 1, &["candidate-a"]);
        let envelope = approve(&mut coordinator, &clock, &proposal);
        let mut unchanged = fence();
        unchanged.lane_generation = LaneGeneration::new(8);

        assert_eq!(
            coordinator.apply_result(result(
                &envelope,
                ExecutorOutcome::PreflightPassed {
                    refreshed_fence: unchanged,
                },
                &clock,
            )),
            Err(TransitionError::RefreshedFenceMismatch)
        );
        assert_eq!(
            coordinator.status().active_attempt.unwrap().phase,
            RecoveryPhase::Preflight
        );
    }

    #[test]
    fn missing_previous_process_is_manual_not_falsely_preserved() {
        let clock = FakeClock::default();
        let mut coordinator = RecoveryCoordinator::new(RecoveryMode::Assisted);
        let proposal = propose(&mut coordinator, &clock, 1, &["candidate-a"]);
        let envelope = approve(&mut coordinator, &clock, &proposal);
        let action = coordinator
            .apply_result(result(
                &envelope,
                ExecutorOutcome::PreviousProcessMissing {
                    previous_fingerprint: ConfigFingerprint::new("sha256:active"),
                },
                &clock,
            ))
            .unwrap();
        assert!(matches!(
            action,
            RecoveryAction::ManualIntervention {
                completion: RecoveryCompletion {
                    disposition: RecoveryDisposition::ProcessFailed,
                    ..
                }
            }
        ));
    }

    #[test]
    fn environment_abort_after_stop_rolls_back_without_candidate_penalty() {
        let clock = FakeClock::default();
        let mut coordinator = RecoveryCoordinator::new(RecoveryMode::Assisted);
        let proposal = propose(&mut coordinator, &clock, 1, &["candidate-a"]);
        let envelope = approve(&mut coordinator, &clock, &proposal);
        let envelope = pass_preflight(&mut coordinator, &envelope, &clock);
        coordinator
            .apply_result(result(
                &envelope,
                ExecutorOutcome::Stopped {
                    previous: owner("active", 7, 1),
                },
                &clock,
            ))
            .unwrap();

        let action = coordinator
            .apply_result(result(
                &envelope,
                ExecutorOutcome::ExecutionAborted {
                    stage: ExecutorStage::Start,
                    reason: ConfirmationFailure::Environment,
                },
                &clock,
            ))
            .unwrap();
        assert!(matches!(action, RecoveryAction::RollbackPrevious { .. }));
        assert_eq!(coordinator.status().negative_cooldown_count, 0);
    }

    #[test]
    fn strategy_failure_cools_only_the_selected_candidate_and_never_walks_incident() {
        let clock = FakeClock::default();
        let mut coordinator = RecoveryCoordinator::new(RecoveryMode::Assisted);
        let proposal = propose(&mut coordinator, &clock, 1, &["candidate-a", "candidate-b"]);
        let envelope = approve(&mut coordinator, &clock, &proposal);
        let envelope = pass_preflight(&mut coordinator, &envelope, &clock);
        coordinator
            .apply_result(result(
                &envelope,
                ExecutorOutcome::Stopped {
                    previous: owner("active", 7, 1),
                },
                &clock,
            ))
            .unwrap();
        let rollback = coordinator
            .apply_result(result(
                &envelope,
                ExecutorOutcome::StartFailed {
                    candidate_fingerprint: ConfigFingerprint::new("sha256:candidate-a"),
                },
                &clock,
            ))
            .unwrap();
        let rollback_envelope = match rollback {
            RecoveryAction::RollbackPrevious { envelope, .. } => envelope,
            other => panic!("unexpected action: {other:?}"),
        };
        assert_eq!(
            coordinator.cooldown_until(
                &fence().network_fingerprint,
                "video",
                &ConfigFingerprint::new("sha256:candidate-a"),
                clock.now(),
            ),
            Ok(Some(NEGATIVE_COOLDOWN_MS))
        );

        coordinator
            .apply_result(result(
                &rollback_envelope,
                ExecutorOutcome::RolledBack {
                    previous: owner("active", 9, 3),
                },
                &clock,
            ))
            .unwrap();
        assert_eq!(
            coordinator.consider(request(1, &["candidate-a", "candidate-b"]), clock.now()),
            Err(ConsiderError::IncidentAlreadyHandled)
        );

        let next = coordinator
            .consider(request(2, &["candidate-a", "candidate-b"]), clock.now())
            .unwrap();
        assert!(matches!(
            next,
            RecoveryDecision::Assisted {
                proposal: AssistedProposalView {
                    candidate_config_id,
                    ..
                }
            } if candidate_config_id == "candidate-b"
        ));
    }

    #[test]
    fn cooldown_expires_after_exactly_five_minutes() {
        let mut clock = FakeClock::default();
        let mut coordinator = RecoveryCoordinator::new(RecoveryMode::Assisted);
        let proposal = propose(&mut coordinator, &clock, 1, &["candidate-a"]);
        let envelope = approve(&mut coordinator, &clock, &proposal);
        let envelope = pass_preflight(&mut coordinator, &envelope, &clock);
        coordinator
            .apply_result(result(
                &envelope,
                ExecutorOutcome::Stopped {
                    previous: owner("active", 7, 1),
                },
                &clock,
            ))
            .unwrap();
        let rollback = coordinator
            .apply_result(result(
                &envelope,
                ExecutorOutcome::StartFailed {
                    candidate_fingerprint: ConfigFingerprint::new("sha256:candidate-a"),
                },
                &clock,
            ))
            .unwrap();
        let rollback_envelope = match rollback {
            RecoveryAction::RollbackPrevious { envelope, .. } => envelope,
            other => panic!("unexpected action: {other:?}"),
        };
        coordinator
            .apply_result(result(
                &rollback_envelope,
                ExecutorOutcome::RolledBack {
                    previous: owner("active", 9, 2),
                },
                &clock,
            ))
            .unwrap();

        clock.advance(NEGATIVE_COOLDOWN_MS - 1);
        assert_eq!(
            coordinator.consider(request(2, &["candidate-a"]), clock.now()),
            Err(ConsiderError::NoEligibleCandidate)
        );
        clock.advance(1);
        assert!(matches!(
            coordinator.consider(request(3, &["candidate-a"]), clock.now()),
            Ok(RecoveryDecision::Assisted { .. })
        ));
    }

    #[test]
    fn negative_cooldown_memory_is_bounded_and_evicts_earliest_expiry() {
        let mut clock = FakeClock::default();
        let mut coordinator = RecoveryCoordinator::new(RecoveryMode::Assisted);
        let total = MAX_NEGATIVE_COOLDOWNS + 9;

        for index in 0..total {
            let candidate_id = format!("candidate-{index}");
            let proposal = propose(
                &mut coordinator,
                &clock,
                index as u64 + 1,
                &[candidate_id.as_str()],
            );
            let envelope = approve(&mut coordinator, &clock, &proposal);
            let envelope = pass_preflight(&mut coordinator, &envelope, &clock);
            coordinator
                .apply_result(result(
                    &envelope,
                    ExecutorOutcome::Stopped {
                        previous: owner("active", 7, 1),
                    },
                    &clock,
                ))
                .unwrap();
            let rollback = coordinator
                .apply_result(result(
                    &envelope,
                    ExecutorOutcome::StartFailed {
                        candidate_fingerprint: ConfigFingerprint::new(format!(
                            "sha256:{candidate_id}"
                        )),
                    },
                    &clock,
                ))
                .unwrap();
            let rollback_envelope = match rollback {
                RecoveryAction::RollbackPrevious { envelope, .. } => envelope,
                other => panic!("unexpected action: {other:?}"),
            };
            coordinator
                .apply_result(result(
                    &rollback_envelope,
                    ExecutorOutcome::RolledBack {
                        previous: owner("active", 9, index as u64 * 3 + 2),
                    },
                    &clock,
                ))
                .unwrap();
            clock.advance(1);
        }

        assert_eq!(
            coordinator.status().negative_cooldown_count,
            MAX_NEGATIVE_COOLDOWNS
        );
        assert_eq!(
            coordinator
                .cooldown_until(
                    &fence().network_fingerprint,
                    "video",
                    &ConfigFingerprint::new("sha256:candidate-0"),
                    clock.now(),
                )
                .unwrap(),
            None
        );
        assert!(coordinator
            .cooldown_until(
                &fence().network_fingerprint,
                "video",
                &ConfigFingerprint::new(format!("sha256:candidate-{}", total - 1)),
                clock.now(),
            )
            .unwrap()
            .is_some());
    }

    #[test]
    fn environment_failure_rolls_back_without_negative_cooldown() {
        let clock = FakeClock::default();
        let mut coordinator = RecoveryCoordinator::new(RecoveryMode::Assisted);
        let proposal = propose(&mut coordinator, &clock, 1, &["candidate-a"]);
        let mut envelope = approve(&mut coordinator, &clock, &proposal);
        envelope = pass_preflight(&mut coordinator, &envelope, &clock);
        coordinator
            .apply_result(result(
                &envelope,
                ExecutorOutcome::Stopped {
                    previous: owner("active", 7, 1),
                },
                &clock,
            ))
            .unwrap();
        let candidate = owner("candidate-a", 8, 2);
        envelope = match coordinator
            .apply_result(result(
                &envelope,
                ExecutorOutcome::Ready {
                    candidate: candidate.clone(),
                },
                &clock,
            ))
            .unwrap()
        {
            RecoveryAction::ConfirmCandidate { envelope, .. } => envelope,
            other => panic!("unexpected action: {other:?}"),
        };
        let rollback = coordinator
            .apply_result(result(
                &envelope,
                ExecutorOutcome::ConfirmationFailed {
                    candidate,
                    reason: ConfirmationFailure::Environment,
                },
                &clock,
            ))
            .unwrap();
        assert!(matches!(rollback, RecoveryAction::RollbackPrevious { .. }));
        assert_eq!(coordinator.status().negative_cooldown_count, 0);
    }

    #[test]
    fn rollback_failure_is_terminal_process_failed() {
        let clock = FakeClock::default();
        let mut coordinator = RecoveryCoordinator::new(RecoveryMode::Assisted);
        let proposal = propose(&mut coordinator, &clock, 1, &["candidate-a"]);
        let envelope = approve(&mut coordinator, &clock, &proposal);
        let envelope = pass_preflight(&mut coordinator, &envelope, &clock);
        coordinator
            .apply_result(result(
                &envelope,
                ExecutorOutcome::Stopped {
                    previous: owner("active", 7, 1),
                },
                &clock,
            ))
            .unwrap();
        let rollback = coordinator
            .apply_result(result(
                &envelope,
                ExecutorOutcome::StartFailed {
                    candidate_fingerprint: ConfigFingerprint::new("sha256:candidate-a"),
                },
                &clock,
            ))
            .unwrap();
        let rollback_envelope = match rollback {
            RecoveryAction::RollbackPrevious { envelope, .. } => envelope,
            other => panic!("unexpected action: {other:?}"),
        };
        let action = coordinator
            .apply_result(result(
                &rollback_envelope,
                ExecutorOutcome::RollbackFailed {
                    previous_fingerprint: ConfigFingerprint::new("sha256:active"),
                },
                &clock,
            ))
            .unwrap();
        assert!(matches!(
            action,
            RecoveryAction::ManualIntervention {
                completion: RecoveryCompletion {
                    phase: RecoveryPhase::ProcessFailed,
                    ..
                }
            }
        ));
        assert!(coordinator.status().active_attempt.is_none());
    }
}
