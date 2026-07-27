//! Pure scoped Legacy recovery coordinator (Assisted and Automatic).
//!
//! This module describes what the Phase 3/4 executor is allowed to do. It never
//! starts a process, writes selection/cache state, or reads a wall clock. All
//! time values are supplied by the caller from one monotonic clock.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use super::contracts::{
    AttemptId, ConfigFingerprint, ConfirmationFailure, ExecutorOutcome, ExecutorResult,
    ExecutorStage, IntentEnvelope, IntentFence, LaneGeneration, NetworkFingerprint, ProcessOwner,
    SessionId,
};

pub const ASSISTED_PROPOSAL_TTL_MS: u64 = 30_000;
pub const AUTOMATIC_PACING_MS: u64 = 30_000;
pub const NEGATIVE_COOLDOWN_MS: u64 = 300_000;
pub const MAX_NEGATIVE_COOLDOWNS: usize = 128;
pub const MAX_AUTOMATIC_LADDER_CANDIDATES: usize = 16;

const MAX_RETIRED_PROPOSALS: usize = 128;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryMode {
    #[default]
    ObserveOnly,
    Assisted,
    Automatic,
}

/// How an attempt was armed. Automatic actions carry the exact operator
/// control generation observed when Manager produced the request so the
/// executor can reject a queued action after pause/freeze/mode changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum RecoveryOrigin {
    Assisted,
    Automatic { control_generation: u64 },
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
/// the caller; this coordinator applies safety exclusions and preserves the
/// eligible order for an Automatic recovery ladder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryRequest {
    incident_id: IncidentId,
    fence: IntentFence,
    previous: RecoveryConfig,
    previous_owner: ProcessOwner,
    candidates: Vec<RecoveryConfig>,
    automatic_control_generation: u64,
}

impl RecoveryRequest {
    pub fn new(
        incident_id: IncidentId,
        fence: IntentFence,
        previous: RecoveryConfig,
        previous_owner: ProcessOwner,
        candidates: Vec<RecoveryConfig>,
    ) -> Self {
        Self::new_with_control_generation(
            incident_id,
            fence,
            previous,
            previous_owner,
            candidates,
            0,
        )
    }

    pub fn new_with_control_generation(
        incident_id: IncidentId,
        fence: IntentFence,
        previous: RecoveryConfig,
        previous_owner: ProcessOwner,
        candidates: Vec<RecoveryConfig>,
        automatic_control_generation: u64,
    ) -> Self {
        Self {
            incident_id,
            fence,
            previous,
            previous_owner,
            candidates,
            automatic_control_generation,
        }
    }

    pub const fn incident_id(&self) -> IncidentId {
        self.incident_id
    }

    pub const fn fence(&self) -> &IntentFence {
        &self.fence
    }

    pub const fn automatic_control_generation(&self) -> u64 {
        self.automatic_control_generation
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
    Automatic { action: RecoveryAction },
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
    pub origin: RecoveryOrigin,
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
    pub origin: RecoveryOrigin,
    pub phase: RecoveryPhase,
    pub disposition: RecoveryDisposition,
    pub finished_at_monotonic_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryStatus {
    pub mode: RecoveryMode,
    pub automatic_paused: bool,
    pub manual_frozen_categories: Vec<String>,
    pub halted_categories: Vec<String>,
    pub automatic_pacing_until_monotonic_ms: Option<u64>,
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
        origin: RecoveryOrigin,
    },
    StopPrevious {
        envelope: IntentEnvelope,
        previous: RecoveryConfig,
        previous_owner: ProcessOwner,
        origin: RecoveryOrigin,
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
        retry_pending: bool,
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
    AutomaticPaused,
    CategoryFrozen,
    CategoryHalted,
    AutomaticPacing { until_monotonic_ms: u64 },
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
    remaining_candidates: VecDeque<RecoveryConfig>,
    candidate_lane_generation: LaneGeneration,
    current_lane_generation: LaneGeneration,
    expected_rollback_generation: Option<LaneGeneration>,
    candidate_owner: Option<ProcessOwner>,
    confirmation_succeeded: bool,
    retry_after_rollback: bool,
    origin: RecoveryOrigin,
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
            origin: self.origin,
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
            origin: self.origin,
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
    automatic_paused: bool,
    manual_frozen_categories: BTreeSet<String>,
    halted_categories: BTreeSet<String>,
    automatic_pacing_until_monotonic_ms: Option<u64>,
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
            automatic_paused: false,
            manual_frozen_categories: BTreeSet::new(),
            halted_categories: BTreeSet::new(),
            automatic_pacing_until_monotonic_ms: None,
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

    /// Any transition away from Assisted revokes its unapproved proposal. An
    /// active attempt remains executor-owned and must reach a safe terminal
    /// state; automatic control generation fencing decides whether its queued
    /// preflight is still authorized.
    pub fn set_mode(&mut self, mode: RecoveryMode) {
        self.mode = mode;
        if mode != RecoveryMode::Assisted {
            self.cancel_pending();
        }
    }

    pub fn set_automatic_paused(&mut self, paused: bool) {
        self.automatic_paused = paused;
    }

    pub fn freeze_category(&mut self, category: impl Into<String>) -> bool {
        let category = category.into();
        let inserted = self.manual_frozen_categories.insert(category.clone());
        if self
            .proposal
            .as_ref()
            .is_some_and(|proposal| proposal.envelope.category == category)
        {
            self.cancel_pending();
        }
        inserted
    }

    pub fn set_manual_frozen_categories<I, C>(&mut self, categories: I) -> bool
    where
        I: IntoIterator<Item = C>,
        C: Into<String>,
    {
        let next = categories
            .into_iter()
            .map(Into::into)
            .collect::<BTreeSet<_>>();
        if self.manual_frozen_categories == next {
            return false;
        }
        self.manual_frozen_categories = next;
        if self.proposal.as_ref().is_some_and(|proposal| {
            self.manual_frozen_categories
                .contains(&proposal.envelope.category)
        }) {
            self.cancel_pending();
        }
        true
    }

    pub fn unfreeze_category(&mut self, category: &str) -> bool {
        self.manual_frozen_categories.remove(category)
    }

    pub fn clear_halt(&mut self, category: &str) -> bool {
        self.halted_categories.remove(category)
    }

    /// A real Legacy session boundary clears automatic terminal failures from
    /// the retired session. Manual freezes are operator controls and survive.
    pub fn clear_automatic_halts(&mut self) {
        self.halted_categories.clear();
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
        self.finish_automatic_attempt(&active, self.last_monotonic_ms, true);
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
            automatic_paused: self.automatic_paused,
            manual_frozen_categories: self.manual_frozen_categories.iter().cloned().collect(),
            halted_categories: self.halted_categories.iter().cloned().collect(),
            automatic_pacing_until_monotonic_ms: self.automatic_pacing_until_monotonic_ms,
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

        let mut seen_fingerprints = BTreeSet::new();
        let mut eligible_candidates = request
            .candidates
            .iter()
            .filter(|candidate| {
                candidate.fingerprint != request.previous.fingerprint
                    && seen_fingerprints.insert(candidate.fingerprint.clone())
                    && !self.negative_cooldowns.contains_key(&CooldownKey {
                        stable_network: stable_network.clone(),
                        category: request.fence.category.clone(),
                        candidate_fingerprint: candidate.fingerprint.clone(),
                    })
            })
            .cloned()
            .collect::<VecDeque<_>>();
        if self.mode == RecoveryMode::Automatic {
            eligible_candidates.truncate(MAX_AUTOMATIC_LADDER_CANDIDATES);
        }
        let candidate = eligible_candidates
            .pop_front()
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

        let category = request.fence.category.clone();
        if self.manual_frozen_categories.contains(&category) {
            return Err(ConsiderError::CategoryFrozen);
        }
        if self.halted_categories.contains(&category) {
            return Err(ConsiderError::CategoryHalted);
        }
        if self.mode == RecoveryMode::Automatic {
            if self.automatic_paused {
                return Err(ConsiderError::AutomaticPaused);
            }
            if let Some(until_monotonic_ms) = self.automatic_pacing_until_monotonic_ms {
                return Err(ConsiderError::AutomaticPacing { until_monotonic_ms });
            }
        }

        let attempt_id = AttemptId::new(self.take_attempt_id());
        let envelope = IntentEnvelope::from_fence(attempt_id, &request.fence);
        if self.mode == RecoveryMode::Automatic {
            let action = self.begin_attempt(
                request.incident_id,
                envelope,
                request.previous,
                request.previous_owner,
                candidate,
                eligible_candidates,
                RecoveryOrigin::Automatic {
                    control_generation: request.automatic_control_generation,
                },
                now_ms,
            );
            return Ok(RecoveryDecision::Automatic { action });
        }

        let proposal_id = ProposalId(self.take_proposal_id());
        let proposal = AssistedProposal {
            proposal_id,
            incident_id: request.incident_id,
            envelope,
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

        let pending = match self.proposal.take() {
            Some(p) => p,
            // Инвариант: proposal проверен as_ref выше. При нарушении (будущий
            // рефакторинг) возвращаем Expired, а не панику — recovery-координатор
            // не должен убивать процесс из-за гонки состояния.
            None => return Err(ApprovalError::Expired),
        };
        self.retire_proposal(pending.proposal_id, RetiredProposalReason::Approved);
        Ok(self.begin_attempt(
            pending.incident_id,
            pending.envelope,
            pending.previous,
            pending.previous_owner,
            pending.candidate,
            VecDeque::new(),
            RecoveryOrigin::Assisted,
            now_ms,
        ))
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

        let mut active = match self.active.take() {
            Some(a) => a,
            // as_ref выше гарантировал Some; при нарушении — ошибка перехода,
            // а не паника (координатор на горячем пути восстановления).
            None => return Err(TransitionError::NoActiveAttempt),
        };
        let transition = self.transition(
            &mut active,
            result.outcome,
            result.completed_at_monotonic_ms,
        );
        match transition {
            Ok(Transition::Continue(action)) => {
                if matches!(active.origin, RecoveryOrigin::Automatic { .. })
                    && matches!(&action, RecoveryAction::StartCandidate { .. })
                {
                    self.latest_handled_incident.insert(
                        IncidentLane {
                            session_id: active.envelope.session_id,
                            category: active.envelope.category.clone(),
                        },
                        active.incident_id,
                    );
                }
                self.active = Some(active);
                Ok(action)
            }
            Ok(Transition::Complete(completion, manual)) => {
                self.finish_automatic_attempt(&active, completion.finished_at_monotonic_ms, manual);
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

    #[allow(clippy::too_many_arguments)]
    fn begin_attempt(
        &mut self,
        incident_id: IncidentId,
        envelope: IntentEnvelope,
        previous: RecoveryConfig,
        previous_owner: ProcessOwner,
        candidate: RecoveryConfig,
        remaining_candidates: VecDeque<RecoveryConfig>,
        origin: RecoveryOrigin,
        now_ms: u64,
    ) -> RecoveryAction {
        let candidate_lane_generation = next_generation(envelope.expected_lane_generation);
        let active = ActiveAttempt {
            incident_id,
            current_lane_generation: envelope.expected_lane_generation,
            candidate_lane_generation,
            expected_rollback_generation: None,
            candidate_owner: None,
            confirmation_succeeded: false,
            retry_after_rollback: false,
            origin,
            phase: RecoveryPhase::Preflight,
            phase_started_at_monotonic_ms: now_ms,
            envelope,
            previous,
            previous_owner,
            candidate,
            remaining_candidates,
        };
        let action = RecoveryAction::Preflight {
            envelope: active.envelope.clone(),
            previous: active.previous.clone(),
            previous_owner: active.previous_owner.clone(),
            candidate: active.candidate.clone(),
            origin,
        };
        self.active = Some(active);
        action
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
                    origin: active.origin,
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
                mark_automatic_retry(active);
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
                    mark_automatic_retry(active);
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
                if matches!(
                    reason,
                    ConfirmationFailure::Strategy
                        | ConfirmationFailure::Target
                        | ConfirmationFailure::MissingWorkingEvidence
                ) {
                    mark_automatic_retry(active);
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
                    mark_automatic_retry(active);
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
                mark_automatic_retry(active);
                Ok(Transition::Continue(begin_rollback(active, now_ms)))
            }
            (RecoveryPhase::RollingBack, ExecutorOutcome::RolledBack { previous }) => {
                if active.retry_after_rollback {
                    return Err(TransitionError::UnexpectedOutcome);
                }
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
                ExecutorOutcome::RolledBackForRetry {
                    previous,
                    refreshed_fence,
                },
            ) => {
                if !active.retry_after_rollback
                    || !matches!(active.origin, RecoveryOrigin::Automatic { .. })
                {
                    return Err(TransitionError::UnexpectedOutcome);
                }
                let expected = active
                    .expected_rollback_generation
                    .ok_or(TransitionError::UnexpectedOutcome)?;
                ensure_owner(&previous, active.previous.fingerprint(), expected)?;
                rebase_after_retry_rollback(active, &refreshed_fence, expected)?;
                let next_candidate = active
                    .remaining_candidates
                    .pop_front()
                    .ok_or(TransitionError::UnexpectedOutcome)?;
                active.previous_owner = previous;
                active.candidate = next_candidate;
                active.candidate_lane_generation = next_generation(expected);
                active.expected_rollback_generation = None;
                active.candidate_owner = None;
                active.confirmation_succeeded = false;
                active.retry_after_rollback = false;
                active.enter(RecoveryPhase::Preflight, now_ms);
                Ok(Transition::Continue(RecoveryAction::Preflight {
                    envelope: active.envelope.clone(),
                    previous: active.previous.clone(),
                    previous_owner: active.previous_owner.clone(),
                    candidate: active.candidate.clone(),
                    origin: active.origin,
                }))
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

    fn finish_automatic_attempt(
        &mut self,
        active: &ActiveAttempt,
        now_ms: u64,
        manual_failure: bool,
    ) {
        if !matches!(active.origin, RecoveryOrigin::Automatic { .. }) {
            return;
        }
        let until = now_ms.saturating_add(AUTOMATIC_PACING_MS);
        self.automatic_pacing_until_monotonic_ms = Some(
            self.automatic_pacing_until_monotonic_ms
                .map_or(until, |current| current.max(until)),
        );
        if manual_failure {
            self.halted_categories
                .insert(active.envelope.category.clone());
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
            .automatic_pacing_until_monotonic_ms
            .is_some_and(|until_ms| now_ms >= until_ms)
        {
            self.automatic_pacing_until_monotonic_ms = None;
        }
        if self
            .proposal
            .as_ref()
            .is_some_and(|proposal| now_ms >= proposal.expires_at_monotonic_ms)
        {
            // take() после is_some_and: None быть не может, но при нарушении
            // просто ничего не делаем — tick не должен паниковать.
            if let Some(proposal) = self.proposal.take() {
                self.retire_proposal(proposal.proposal_id, RetiredProposalReason::Expired);
            }
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
        retry_pending: active.retry_after_rollback,
    }
}

fn mark_automatic_retry(active: &mut ActiveAttempt) {
    active.retry_after_rollback = matches!(active.origin, RecoveryOrigin::Automatic { .. })
        && !active.remaining_candidates.is_empty();
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

fn rebase_after_retry_rollback(
    active: &mut ActiveAttempt,
    refreshed: &IntentFence,
    expected_lane_generation: LaneGeneration,
) -> Result<(), TransitionError> {
    let previous = &active.envelope;
    if refreshed.session_id != previous.session_id
        || refreshed.category != previous.category
        || refreshed.network_fingerprint != previous.expected_network_fingerprint
        || refreshed.lane_generation != expected_lane_generation
        || refreshed.sensor_generation == previous.expected_sensor_generation
        || refreshed.registry_version == previous.expected_registry_version
    {
        return Err(TransitionError::RefreshedFenceMismatch);
    }

    active.current_lane_generation = expected_lane_generation;
    active.envelope = IntentEnvelope::from_fence(previous.attempt_id, refreshed);
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

    #[test]
    fn automatic_origin_uses_the_strict_camel_case_wire_shape() {
        let value = serde_json::to_value(RecoveryOrigin::Automatic {
            control_generation: 73,
        })
        .unwrap();
        assert_eq!(value["kind"], "automatic");
        assert_eq!(value["controlGeneration"], 73);
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

    fn request_with_control(
        incident: u64,
        candidates: &[&str],
        control_generation: u64,
    ) -> RecoveryRequest {
        RecoveryRequest::new_with_control_generation(
            IncidentId::new(incident),
            fence(),
            config("active"),
            owner("active", 7, 1),
            candidates.iter().map(|id| config(id)).collect(),
            control_generation,
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
    fn automatic_arms_one_backend_owned_preflight_without_a_proposal() {
        let clock = FakeClock::default();
        let mut coordinator = RecoveryCoordinator::new(RecoveryMode::Automatic);
        let decision = coordinator
            .consider(request_with_control(1, &["candidate-a"], 41), clock.now())
            .unwrap();
        let RecoveryDecision::Automatic { action } = decision else {
            panic!("automatic mode must return an executable preflight");
        };
        let (envelope, origin) = match action {
            RecoveryAction::Preflight {
                envelope, origin, ..
            } => (envelope, origin),
            other => panic!("unexpected automatic action: {other:?}"),
        };
        assert_eq!(
            origin,
            RecoveryOrigin::Automatic {
                control_generation: 41
            }
        );
        assert!(coordinator.status().proposal.is_none());
        assert_eq!(coordinator.status().active_attempt.unwrap().origin, origin);
        assert_eq!(
            coordinator.consider(request_with_control(2, &["candidate-b"], 41), clock.now()),
            Err(ConsiderError::Busy)
        );

        let stop = coordinator
            .apply_result(result(&envelope, preflight_outcome(&envelope), &clock))
            .unwrap();
        assert!(matches!(
            stop,
            RecoveryAction::StopPrevious {
                origin: RecoveryOrigin::Automatic {
                    control_generation: 41
                },
                ..
            }
        ));
    }

    #[test]
    fn automatic_controls_block_without_consuming_the_incident_or_attempt_id() {
        let clock = FakeClock::default();
        let mut coordinator = RecoveryCoordinator::new(RecoveryMode::Automatic);
        coordinator.set_automatic_paused(true);
        assert_eq!(
            coordinator.consider(request_with_control(7, &["candidate-a"], 10), clock.now()),
            Err(ConsiderError::AutomaticPaused)
        );
        coordinator.set_automatic_paused(false);
        coordinator.freeze_category("video");
        assert_eq!(
            coordinator.consider(request_with_control(7, &["candidate-a"], 11), clock.now()),
            Err(ConsiderError::CategoryFrozen)
        );
        coordinator.unfreeze_category("video");

        let decision = coordinator
            .consider(request_with_control(7, &["candidate-a"], 12), clock.now())
            .unwrap();
        let RecoveryDecision::Automatic { action } = decision else {
            panic!("unblocked incident must remain actionable");
        };
        assert!(matches!(
            action,
            RecoveryAction::Preflight {
                envelope: IntentEnvelope {
                    attempt_id: AttemptId(1),
                    ..
                },
                origin: RecoveryOrigin::Automatic {
                    control_generation: 12
                },
                ..
            }
        ));
    }

    #[test]
    fn observe_only_remains_diagnostic_while_automatic_controls_are_blocked() {
        let clock = FakeClock::default();
        let mut coordinator = RecoveryCoordinator::new(RecoveryMode::Automatic);
        let _ = coordinator
            .consider(request_with_control(1, &["candidate-a"], 1), clock.now())
            .unwrap();
        let _ = coordinator.force_manual_failure(clock.now()).unwrap();
        coordinator.set_mode(RecoveryMode::ObserveOnly);
        coordinator.set_automatic_paused(true);
        coordinator.freeze_category("video");

        assert!(matches!(
            coordinator.consider(request(2, &["candidate-b"]), clock.now()),
            Ok(RecoveryDecision::ObserveOnly { .. })
        ));
    }

    #[test]
    fn assisted_proposal_is_revoked_by_mode_transition_or_matching_freeze() {
        let clock = FakeClock::default();
        for mode in [RecoveryMode::ObserveOnly, RecoveryMode::Automatic] {
            let mut coordinator = RecoveryCoordinator::new(RecoveryMode::Assisted);
            let proposal = propose(&mut coordinator, &clock, 1, &["candidate-a"]);
            coordinator.set_mode(mode);
            assert!(coordinator.proposal().is_none());
            assert_eq!(
                coordinator.approve(proposal.approval(), &fence(), clock.now()),
                Err(ApprovalError::Cancelled)
            );
        }

        let mut coordinator = RecoveryCoordinator::new(RecoveryMode::Assisted);
        let proposal = propose(&mut coordinator, &clock, 2, &["candidate-b"]);
        coordinator.freeze_category("chat");
        assert!(coordinator.proposal().is_some());
        coordinator.freeze_category("video");
        assert!(coordinator.proposal().is_none());
        assert_eq!(
            coordinator.approve(proposal.approval(), &fence(), clock.now()),
            Err(ApprovalError::Cancelled)
        );
    }

    #[test]
    fn frozen_category_snapshot_is_exact_sorted_and_revokes_a_matching_proposal() {
        let clock = FakeClock::default();
        let mut coordinator = RecoveryCoordinator::new(RecoveryMode::Assisted);
        let _ = propose(&mut coordinator, &clock, 1, &["candidate-a"]);

        assert!(coordinator.set_manual_frozen_categories([
            "video".to_owned(),
            "chat".to_owned(),
            "video".to_owned(),
        ]));
        assert!(coordinator.proposal().is_none());
        assert_eq!(
            coordinator.status().manual_frozen_categories,
            ["chat", "video"]
        );
        assert!(!coordinator.set_manual_frozen_categories(["chat".to_owned(), "video".to_owned(),]));
        assert!(coordinator.set_manual_frozen_categories(["chat".to_owned()]));
        assert!(!coordinator
            .status()
            .manual_frozen_categories
            .contains(&"video".into()));
    }

    #[test]
    fn automatic_pacing_starts_at_terminal_completion_and_expires_exactly() {
        let mut clock = FakeClock::default();
        let mut coordinator = RecoveryCoordinator::new(RecoveryMode::Automatic);
        let decision = coordinator
            .consider(request_with_control(1, &["candidate-a"], 1), clock.now())
            .unwrap();
        let RecoveryDecision::Automatic { action } = decision else {
            panic!("automatic fixture");
        };
        let RecoveryAction::Preflight { envelope, .. } = action else {
            panic!("preflight fixture");
        };
        clock.advance(5);
        let terminal = coordinator
            .apply_result(result(
                &envelope,
                ExecutorOutcome::PreflightRejected,
                &clock,
            ))
            .unwrap();
        assert!(matches!(terminal, RecoveryAction::Complete { .. }));
        let until = clock.now() + AUTOMATIC_PACING_MS;
        assert_eq!(
            coordinator.status().automatic_pacing_until_monotonic_ms,
            Some(until)
        );
        assert_eq!(
            coordinator.consider(
                request_with_control(2, &["candidate-b"], 1),
                clock.now() - 1
            ),
            Err(ConsiderError::ClockMovedBack)
        );

        clock.advance(AUTOMATIC_PACING_MS - 1);
        assert_eq!(
            coordinator.consider(request_with_control(2, &["candidate-b"], 1), clock.now()),
            Err(ConsiderError::AutomaticPacing {
                until_monotonic_ms: until
            })
        );
        clock.advance(1);
        assert!(matches!(
            coordinator.consider(request_with_control(2, &["candidate-b"], 1), clock.now()),
            Ok(RecoveryDecision::Automatic { .. })
        ));
    }

    #[test]
    fn automatic_preflight_rejection_does_not_consume_the_incident() {
        let mut clock = FakeClock::default();
        let mut coordinator = RecoveryCoordinator::new(RecoveryMode::Automatic);
        let decision = coordinator
            .consider(request_with_control(7, &["candidate-a"], 1), clock.now())
            .unwrap();
        let RecoveryDecision::Automatic { action } = decision else {
            panic!("automatic fixture");
        };
        let RecoveryAction::Preflight { envelope, .. } = action else {
            panic!("preflight fixture");
        };
        let _ = coordinator
            .apply_result(result(
                &envelope,
                ExecutorOutcome::PreflightRejected,
                &clock,
            ))
            .unwrap();

        clock.advance(AUTOMATIC_PACING_MS);
        assert!(matches!(
            coordinator.consider(request_with_control(7, &["candidate-a"], 1), clock.now()),
            Ok(RecoveryDecision::Automatic { .. })
        ));
    }

    #[test]
    fn automatic_stop_timeout_does_not_consume_the_incident() {
        let mut clock = FakeClock::default();
        let mut coordinator = RecoveryCoordinator::new(RecoveryMode::Automatic);
        let decision = coordinator
            .consider(request_with_control(8, &["candidate-a"], 1), clock.now())
            .unwrap();
        let RecoveryDecision::Automatic { action } = decision else {
            panic!("automatic fixture");
        };
        let RecoveryAction::Preflight { envelope, .. } = action else {
            panic!("preflight fixture");
        };
        let stop_envelope = pass_preflight(&mut coordinator, &envelope, &clock);
        let _ = coordinator
            .apply_result(result(
                &stop_envelope,
                ExecutorOutcome::StopTimedOut {
                    previous: owner("active", 7, 1),
                },
                &clock,
            ))
            .unwrap();

        clock.advance(AUTOMATIC_PACING_MS);
        assert!(matches!(
            coordinator.consider(request_with_control(8, &["candidate-a"], 1), clock.now()),
            Ok(RecoveryDecision::Automatic { .. })
        ));
    }

    #[test]
    fn automatic_consumes_the_incident_after_the_previous_process_stops() {
        let mut clock = FakeClock::default();
        let mut coordinator = RecoveryCoordinator::new(RecoveryMode::Automatic);
        let decision = coordinator
            .consider(request_with_control(9, &["candidate-a"], 1), clock.now())
            .unwrap();
        let RecoveryDecision::Automatic { action } = decision else {
            panic!("automatic fixture");
        };
        let RecoveryAction::Preflight { envelope, .. } = action else {
            panic!("preflight fixture");
        };
        let stop_envelope = pass_preflight(&mut coordinator, &envelope, &clock);
        let start = coordinator
            .apply_result(result(
                &stop_envelope,
                ExecutorOutcome::Stopped {
                    previous: owner("active", 7, 1),
                },
                &clock,
            ))
            .unwrap();
        assert!(matches!(start, RecoveryAction::StartCandidate { .. }));

        let _ = coordinator.force_manual_failure(clock.now()).unwrap();
        assert!(coordinator.clear_halt("video"));
        clock.advance(AUTOMATIC_PACING_MS);
        assert_eq!(
            coordinator.consider(request_with_control(9, &["candidate-a"], 1), clock.now()),
            Err(ConsiderError::IncidentAlreadyHandled)
        );
    }

    #[test]
    fn automatic_manual_failure_halts_only_its_lane_until_explicit_clear() {
        let mut clock = FakeClock::default();
        let mut coordinator = RecoveryCoordinator::new(RecoveryMode::Automatic);
        let _ = coordinator
            .consider(request_with_control(1, &["candidate-a"], 7), clock.now())
            .unwrap();
        clock.advance(2);
        let terminal = coordinator.force_manual_failure(clock.now()).unwrap();
        assert!(matches!(
            terminal,
            RecoveryAction::ManualIntervention { .. }
        ));
        assert_eq!(coordinator.status().halted_categories, ["video"]);
        assert_eq!(
            coordinator.consider(request_with_control(2, &["candidate-b"], 7), clock.now()),
            Err(ConsiderError::CategoryHalted)
        );

        assert!(coordinator.clear_halt("video"));
        assert!(!coordinator.clear_halt("video"));
        assert_eq!(
            coordinator.consider(request_with_control(2, &["candidate-b"], 7), clock.now()),
            Err(ConsiderError::AutomaticPacing {
                until_monotonic_ms: clock.now() + AUTOMATIC_PACING_MS
            })
        );
    }

    #[test]
    fn assisted_attempt_does_not_arm_automatic_pacing_or_halt() {
        let clock = FakeClock::default();
        let mut coordinator = RecoveryCoordinator::new(RecoveryMode::Assisted);
        let proposal = propose(&mut coordinator, &clock, 1, &["candidate-a"]);
        let envelope = approve(&mut coordinator, &clock, &proposal);
        let terminal = coordinator
            .apply_result(result(
                &envelope,
                ExecutorOutcome::PreflightRejected,
                &clock,
            ))
            .unwrap();
        assert!(matches!(terminal, RecoveryAction::Complete { .. }));
        assert_eq!(
            coordinator.status().automatic_pacing_until_monotonic_ms,
            None
        );
        assert!(coordinator.status().halted_categories.is_empty());
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
    fn automatic_candidate_failure_marks_rollback_for_immediate_retry_without_pacing() {
        let clock = FakeClock::default();
        let mut coordinator = RecoveryCoordinator::new(RecoveryMode::Automatic);
        let envelope = match coordinator
            .consider(request(1, &["candidate-a", "candidate-b"]), clock.now())
            .unwrap()
        {
            RecoveryDecision::Automatic {
                action: RecoveryAction::Preflight { envelope, .. },
            } => envelope,
            other => panic!("unexpected decision: {other:?}"),
        };
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
        let serialized = serde_json::to_value(&rollback).unwrap();

        assert_eq!(serialized["kind"], "rollback_previous");
        assert_eq!(serialized["retryPending"], true);
        assert_eq!(
            coordinator
                .status()
                .automatic_pacing_until_monotonic_ms,
            None
        );
    }

    #[test]
    fn automatic_retry_rebases_same_attempt_and_preflights_next_candidate() {
        let clock = FakeClock::default();
        let mut coordinator = RecoveryCoordinator::new(RecoveryMode::Automatic);
        let initial_envelope = match coordinator
            .consider(request(1, &["candidate-a", "candidate-b"]), clock.now())
            .unwrap()
        {
            RecoveryDecision::Automatic {
                action: RecoveryAction::Preflight { envelope, .. },
            } => envelope,
            other => panic!("unexpected decision: {other:?}"),
        };
        let envelope = pass_preflight(&mut coordinator, &initial_envelope, &clock);
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
        let (rollback_envelope, rollback_generation) = match rollback {
            RecoveryAction::RollbackPrevious {
                envelope,
                previous_lane_generation,
                retry_pending: true,
                ..
            } => (envelope, previous_lane_generation),
            other => panic!("unexpected action: {other:?}"),
        };
        let refreshed_fence = IntentFence {
            session_id: rollback_envelope.session_id,
            category: rollback_envelope.category.clone(),
            network_fingerprint: rollback_envelope.expected_network_fingerprint.clone(),
            lane_generation: rollback_generation,
            sensor_generation: SensorGeneration::new(90),
            registry_version: RegistryVersion::new(91),
        };
        let previous_owner = ProcessOwner {
            pid: 9,
            process_start_identity: ProcessStartIdentity::new(9),
            config_fingerprint: ConfigFingerprint::new("sha256:active"),
            lane_generation: rollback_generation,
        };
        let mut serialized = serde_json::to_value(ExecutorOutcome::RolledBack {
            previous: previous_owner.clone(),
        })
        .unwrap();
        serialized["kind"] = serde_json::json!("rolled_back_for_retry");
        serialized["refreshedFence"] = serde_json::to_value(&refreshed_fence).unwrap();
        let retry_outcome: ExecutorOutcome = serde_json::from_value(serialized)
            .expect("executor outcome must represent a fenced retry rollback");

        let next = coordinator
            .apply_result(result(&rollback_envelope, retry_outcome, &clock))
            .unwrap();
        match next {
            RecoveryAction::Preflight {
                envelope,
                previous,
                previous_owner: actual_owner,
                candidate,
                origin: RecoveryOrigin::Automatic { .. },
            } => {
                assert_eq!(envelope.attempt_id, initial_envelope.attempt_id);
                assert!(envelope.matches_fence(&refreshed_fence));
                assert_eq!(previous.config_id(), "active");
                assert_eq!(actual_owner, previous_owner);
                assert_eq!(candidate.config_id(), "candidate-b");
            }
            other => panic!("unexpected action: {other:?}"),
        }
        assert_eq!(
            coordinator
                .status()
                .automatic_pacing_until_monotonic_ms,
            None
        );
    }

    #[test]
    fn automatic_environment_failure_never_walks_the_candidate_queue() {
        let clock = FakeClock::default();
        let mut coordinator = RecoveryCoordinator::new(RecoveryMode::Automatic);
        let envelope = match coordinator
            .consider(request(1, &["candidate-a", "candidate-b"]), clock.now())
            .unwrap()
        {
            RecoveryDecision::Automatic {
                action: RecoveryAction::Preflight { envelope, .. },
            } => envelope,
            other => panic!("unexpected decision: {other:?}"),
        };
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
        let candidate = ProcessOwner {
            pid: 8,
            process_start_identity: ProcessStartIdentity::new(8),
            config_fingerprint: ConfigFingerprint::new("sha256:candidate-a"),
            lane_generation: envelope.expected_lane_generation,
        };
        let envelope = match coordinator
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
        assert!(matches!(
            rollback,
            RecoveryAction::RollbackPrevious {
                retry_pending: false,
                ..
            }
        ));
        assert_eq!(coordinator.status().negative_cooldown_count, 0);
    }

    #[test]
    fn automatic_ambiguous_target_failure_rechecks_environment_on_next_candidate() {
        let clock = FakeClock::default();
        let mut coordinator = RecoveryCoordinator::new(RecoveryMode::Automatic);
        let envelope = match coordinator
            .consider(request(1, &["candidate-a", "candidate-b"]), clock.now())
            .unwrap()
        {
            RecoveryDecision::Automatic {
                action: RecoveryAction::Preflight { envelope, .. },
            } => envelope,
            other => panic!("unexpected decision: {other:?}"),
        };
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
        let candidate = ProcessOwner {
            pid: 8,
            process_start_identity: ProcessStartIdentity::new(8),
            config_fingerprint: ConfigFingerprint::new("sha256:candidate-a"),
            lane_generation: envelope.expected_lane_generation,
        };
        let envelope = match coordinator
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
                    reason: ConfirmationFailure::Target,
                },
                &clock,
            ))
            .unwrap();
        assert!(matches!(
            rollback,
            RecoveryAction::RollbackPrevious {
                retry_pending: true,
                ..
            }
        ));
        assert_eq!(
            coordinator.status().negative_cooldown_count,
            0,
            "ambiguous target failure is rechecked, not blamed on the candidate"
        );
    }

    #[test]
    fn automatic_exhaustion_cools_every_failed_candidate_and_paces_once() {
        let clock = FakeClock::default();
        let mut coordinator = RecoveryCoordinator::new(RecoveryMode::Automatic);
        let initial_envelope = match coordinator
            .consider(request(1, &["candidate-a", "candidate-b"]), clock.now())
            .unwrap()
        {
            RecoveryDecision::Automatic {
                action: RecoveryAction::Preflight { envelope, .. },
            } => envelope,
            other => panic!("unexpected decision: {other:?}"),
        };
        let first_envelope = pass_preflight(&mut coordinator, &initial_envelope, &clock);
        coordinator
            .apply_result(result(
                &first_envelope,
                ExecutorOutcome::Stopped {
                    previous: owner("active", 7, 1),
                },
                &clock,
            ))
            .unwrap();
        let first_rollback = coordinator
            .apply_result(result(
                &first_envelope,
                ExecutorOutcome::StartFailed {
                    candidate_fingerprint: ConfigFingerprint::new("sha256:candidate-a"),
                },
                &clock,
            ))
            .unwrap();
        let (first_rollback_envelope, first_rollback_generation) = match first_rollback {
            RecoveryAction::RollbackPrevious {
                envelope,
                previous_lane_generation,
                retry_pending: true,
                ..
            } => (envelope, previous_lane_generation),
            other => panic!("unexpected action: {other:?}"),
        };
        let first_previous_owner = ProcessOwner {
            pid: 9,
            process_start_identity: ProcessStartIdentity::new(9),
            config_fingerprint: ConfigFingerprint::new("sha256:active"),
            lane_generation: first_rollback_generation,
        };
        let refreshed_fence = IntentFence {
            session_id: first_rollback_envelope.session_id,
            category: first_rollback_envelope.category.clone(),
            network_fingerprint: first_rollback_envelope
                .expected_network_fingerprint
                .clone(),
            lane_generation: first_rollback_generation,
            sensor_generation: SensorGeneration::new(90),
            registry_version: RegistryVersion::new(91),
        };
        let second_preflight = coordinator
            .apply_result(result(
                &first_rollback_envelope,
                ExecutorOutcome::RolledBackForRetry {
                    previous: first_previous_owner.clone(),
                    refreshed_fence,
                },
                &clock,
            ))
            .unwrap();
        let second_envelope = match second_preflight {
            RecoveryAction::Preflight {
                envelope,
                candidate,
                ..
            } => {
                assert_eq!(candidate.config_id(), "candidate-b");
                envelope
            }
            other => panic!("unexpected action: {other:?}"),
        };
        let second_envelope = pass_preflight(&mut coordinator, &second_envelope, &clock);
        coordinator
            .apply_result(result(
                &second_envelope,
                ExecutorOutcome::Stopped {
                    previous: first_previous_owner,
                },
                &clock,
            ))
            .unwrap();
        let final_rollback = coordinator
            .apply_result(result(
                &second_envelope,
                ExecutorOutcome::StartFailed {
                    candidate_fingerprint: ConfigFingerprint::new("sha256:candidate-b"),
                },
                &clock,
            ))
            .unwrap();
        let (final_envelope, final_generation) = match final_rollback {
            RecoveryAction::RollbackPrevious {
                envelope,
                previous_lane_generation,
                retry_pending: false,
                ..
            } => (envelope, previous_lane_generation),
            other => panic!("unexpected action: {other:?}"),
        };
        let terminal = coordinator
            .apply_result(result(
                &final_envelope,
                ExecutorOutcome::RolledBack {
                    previous: ProcessOwner {
                        pid: 10,
                        process_start_identity: ProcessStartIdentity::new(10),
                        config_fingerprint: ConfigFingerprint::new("sha256:active"),
                        lane_generation: final_generation,
                    },
                },
                &clock,
            ))
            .unwrap();

        assert!(matches!(
            terminal,
            RecoveryAction::Complete {
                completion: RecoveryCompletion {
                    disposition: RecoveryDisposition::RolledBack,
                    ..
                }
            }
        ));
        let status = coordinator.status();
        assert_eq!(status.negative_cooldown_count, 2);
        assert_eq!(
            status.automatic_pacing_until_monotonic_ms,
            Some(clock.now() + AUTOMATIC_PACING_MS)
        );
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
