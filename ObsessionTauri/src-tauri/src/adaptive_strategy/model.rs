//! Чистая state machine адаптивного recovery.
//!
//! Модель не знает про Tokio, Tauri, WinDivert, процессы и файловую систему.
//! Runtime исполняет [`RecoveryAction`] и возвращает результат событием с
//! тройкой `session_id + attempt_id + candidate_id`; stale события игнорируются.

use std::collections::HashSet;

use serde::Serialize;

use super::dsl::{AdaptiveCategory, StrategyCandidate, StrategyTransport};
use super::evidence::FailureStage;
use super::{generator, validator};

pub const MAX_SESSION_CANDIDATES: usize = 12;

#[derive(Clone, Debug)]
pub struct RecoveryCfg {
    pub verification_ms: u64,
}

impl Default for RecoveryCfg {
    fn default() -> Self {
        Self {
            verification_ms: 60_000,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryPhase {
    Idle,
    Suggested,
    DiscoveringQuic,
    Calibrating,
    Searching,
    CandidateProbe,
    TemporaryVerification,
    Applying,
    RollingBack,
    Applied,
    Exhausted,
    ProbeUnreliable,
    QuicTargetsUnavailable,
    BaseUnhealthy,
    InternalError,
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchSessionMode {
    Comparison,
    Recovery,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreparationFailure {
    ProbeUnreliable,
    QuicTargetsUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosisReason {
    RepeatedReset,
    TlsBlackhole,
    #[allow(dead_code)] // подключится к UDP/QUIC observation fanout
    QuicBlackhole,
    ProbeFailure,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RollbackReason {
    CandidateStartFailed,
    CandidateProbeFailed,
    CandidateCrashed,
    UserRejected,
    VerificationTimeout,
    UserCancelled,
    PersistenceFailed,
    Shutdown,
}

impl RollbackReason {
    fn should_continue(self) -> bool {
        matches!(
            self,
            Self::CandidateStartFailed
                | Self::CandidateProbeFailed
                | Self::CandidateCrashed
                | Self::UserRejected
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateProbeResult {
    pub transport: StrategyTransport,
    pub dns_ok: bool,
    pub tcp_ok: bool,
    pub tls_or_quic_ok: bool,
    pub https_ok: bool,
    pub eyes_working_ok: bool,
    pub eyes_working_count: u32,
    pub eyes_working_hosts: u8,
    pub reset_count: u32,
    pub blackhole_count: u32,
    pub successful_rounds: u8,
    pub required_successes: u8,
    pub total_rounds: u8,
    pub failure_stage: FailureStage,
}

impl Default for CandidateProbeResult {
    fn default() -> Self {
        Self {
            transport: StrategyTransport::Tls,
            dns_ok: false,
            tcp_ok: false,
            tls_or_quic_ok: false,
            https_ok: false,
            eyes_working_ok: false,
            eyes_working_count: 0,
            eyes_working_hosts: 0,
            reset_count: 0,
            blackhole_count: 0,
            successful_rounds: 0,
            required_successes: 2,
            total_rounds: 3,
            failure_stage: FailureStage::None,
        }
    }
}

impl CandidateProbeResult {
    pub fn is_success(&self) -> bool {
        self.successful_rounds >= self.required_successes
            && self.dns_ok
            && self.tcp_ok
            && self.tls_or_quic_ok
            && (self.https_ok || self.eyes_working_ok)
            && self.reset_count < 2
            && self.blackhole_count < 2
            && self.failure_stage == FailureStage::None
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum RecoveryEvent {
    DiagnosisConfirmed {
        category: AdaptiveCategory,
        reason: DiagnosisReason,
    },
    #[allow(dead_code)] // UI dismiss добавляется вместе с DPI recovery panel
    DismissSuggestion,
    #[allow(dead_code)]
    CalibrationFailed,
    PreparationStarted {
        category: AdaptiveCategory,
        transport: StrategyTransport,
    },
    PreparationDiscoveryFinished {
        session_id: u64,
    },
    PreparationProgress {
        session_id: u64,
        current_round: u8,
        total_rounds: u8,
    },
    PreparationReady {
        session_id: u64,
        candidates: Vec<StrategyCandidate>,
        mode: SearchSessionMode,
    },
    PreparationFailed {
        session_id: u64,
        reason: PreparationFailure,
    },
    #[allow(dead_code)]
    UserStart {
        category: AdaptiveCategory,
        candidates: Vec<StrategyCandidate>,
    },
    CandidateStarted {
        session_id: u64,
        attempt_id: u64,
        candidate_id: String,
        ok: bool,
    },
    ProbeFinished {
        session_id: u64,
        attempt_id: u64,
        candidate_id: String,
        result: CandidateProbeResult,
        now: u64,
    },
    CandidateCrashed {
        session_id: u64,
        attempt_id: u64,
        candidate_id: String,
    },
    UserConfirm {
        session_id: u64,
        attempt_id: u64,
        candidate_id: String,
    },
    UserReject {
        session_id: u64,
        attempt_id: u64,
        candidate_id: String,
    },
    PersistFinished {
        session_id: u64,
        attempt_id: u64,
        candidate_id: String,
        ok: bool,
    },
    RollbackFinished {
        session_id: u64,
        attempt_id: u64,
        restored: bool,
        base_healthy: bool,
        base_probe_reliable: bool,
    },
    UserCancel,
    Tick(u64),
    Shutdown,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RecoveryAction {
    NotifySuggestion {
        category: AdaptiveCategory,
        reason: DiagnosisReason,
    },
    StartCandidate {
        session_id: u64,
        attempt_id: u64,
        index: usize,
        total: usize,
        candidate: StrategyCandidate,
    },
    RunProbes {
        session_id: u64,
        attempt_id: u64,
        candidate: StrategyCandidate,
    },
    BeginVerification {
        session_id: u64,
        attempt_id: u64,
        candidate_id: String,
        deadline: u64,
    },
    Rollback {
        session_id: u64,
        attempt_id: u64,
        reason: RollbackReason,
    },
    PersistConfirmed {
        session_id: u64,
        attempt_id: u64,
        candidate: StrategyCandidate,
    },
    EmitStatus(RecoveryStatus),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryStatus {
    pub phase: RecoveryPhase,
    pub category: Option<AdaptiveCategory>,
    pub diagnosis: Option<DiagnosisReason>,
    pub session_id: Option<u64>,
    pub attempt_id: Option<u64>,
    pub candidate_id: Option<String>,
    pub candidate_index: Option<usize>,
    pub candidate_total: Option<usize>,
    pub verification_deadline_ms: Option<u64>,
    pub rollback_reason: Option<RollbackReason>,
    pub transport: Option<StrategyTransport>,
    pub session_mode: Option<SearchSessionMode>,
    pub current_round: Option<u8>,
    pub total_rounds: Option<u8>,
    pub failure_stage: Option<FailureStage>,
}

#[derive(Clone, Debug)]
struct Suggestion {
    category: AdaptiveCategory,
    reason: DiagnosisReason,
}

#[derive(Clone, Debug)]
struct CurrentAttempt {
    attempt_id: u64,
    index: usize,
    candidate: StrategyCandidate,
}

#[derive(Clone, Debug)]
struct Preparation {
    id: u64,
    category: AdaptiveCategory,
    diagnosis: DiagnosisReason,
    transport: StrategyTransport,
    current_round: u8,
    total_rounds: u8,
}

#[derive(Clone, Debug)]
struct Session {
    id: u64,
    category: AdaptiveCategory,
    diagnosis: DiagnosisReason,
    mode: SearchSessionMode,
    candidates: Vec<StrategyCandidate>,
    next_index: usize,
    next_attempt_id: u64,
    current: Option<CurrentAttempt>,
    verification_deadline: Option<u64>,
    rollback_reason: Option<RollbackReason>,
    last_failure: FailureStage,
}

pub struct RecoveryModel {
    cfg: RecoveryCfg,
    phase: RecoveryPhase,
    suggestion: Option<Suggestion>,
    preparation: Option<Preparation>,
    session: Option<Session>,
    next_session_id: u64,
}

impl RecoveryModel {
    pub fn new(cfg: RecoveryCfg) -> Self {
        Self {
            cfg,
            phase: RecoveryPhase::Idle,
            suggestion: None,
            preparation: None,
            session: None,
            next_session_id: 0,
        }
    }

    pub fn status(&self) -> RecoveryStatus {
        let preparation = self.preparation.as_ref();
        let session = self.session.as_ref();
        let current = session.and_then(|session| session.current.as_ref());
        RecoveryStatus {
            phase: self.phase,
            category: session
                .map(|session| session.category)
                .or_else(|| preparation.map(|item| item.category))
                .or_else(|| self.suggestion.as_ref().map(|item| item.category)),
            diagnosis: session
                .map(|session| session.diagnosis)
                .or_else(|| preparation.map(|item| item.diagnosis))
                .or_else(|| self.suggestion.as_ref().map(|item| item.reason)),
            session_id: session
                .map(|session| session.id)
                .or_else(|| preparation.map(|item| item.id)),
            attempt_id: current.map(|attempt| attempt.attempt_id),
            candidate_id: current.map(|attempt| attempt.candidate.candidate_id()),
            candidate_index: current.map(|attempt| attempt.index + 1),
            candidate_total: session.map(|session| session.candidates.len()),
            verification_deadline_ms: session.and_then(|session| session.verification_deadline),
            rollback_reason: session.and_then(|session| session.rollback_reason),
            transport: current
                .map(|attempt| attempt.candidate.transport)
                .or_else(|| preparation.map(|item| item.transport)),
            session_mode: session.map(|session| session.mode),
            current_round: preparation.map(|item| item.current_round),
            total_rounds: preparation.map(|item| item.total_rounds),
            failure_stage: session.map(|session| session.last_failure),
        }
    }
    pub fn step(&mut self, event: RecoveryEvent) -> Vec<RecoveryAction> {
        match event {
            RecoveryEvent::DiagnosisConfirmed { category, reason } => {
                if !matches!(
                    self.phase,
                    RecoveryPhase::Idle
                        | RecoveryPhase::Suggested
                        | RecoveryPhase::Applied
                        | RecoveryPhase::Exhausted
                        | RecoveryPhase::ProbeUnreliable
                        | RecoveryPhase::QuicTargetsUnavailable
                        | RecoveryPhase::BaseUnhealthy
                        | RecoveryPhase::InternalError
                        | RecoveryPhase::Cancelled
                ) {
                    return Vec::new();
                }
                self.preparation = None;
                self.session = None;
                self.suggestion = Some(Suggestion { category, reason });
                self.phase = RecoveryPhase::Suggested;
                vec![
                    RecoveryAction::NotifySuggestion { category, reason },
                    self.emit_status(),
                ]
            }
            RecoveryEvent::DismissSuggestion if self.phase == RecoveryPhase::Suggested => {
                self.suggestion = None;
                self.phase = RecoveryPhase::Idle;
                vec![self.emit_status()]
            }
            RecoveryEvent::PreparationStarted {
                category,
                transport,
            } if self.phase == RecoveryPhase::Suggested
                && self
                    .suggestion
                    .as_ref()
                    .is_some_and(|suggestion| suggestion.category == category) =>
            {
                let diagnosis = self.suggestion.as_ref().unwrap().reason;
                self.next_session_id = next_nonzero(self.next_session_id);
                self.preparation = Some(Preparation {
                    id: self.next_session_id,
                    category,
                    diagnosis,
                    transport,
                    current_round: 0,
                    total_rounds: 0,
                });
                self.phase = if transport == StrategyTransport::Quic {
                    RecoveryPhase::DiscoveringQuic
                } else {
                    RecoveryPhase::Calibrating
                };
                vec![self.emit_status()]
            }
            RecoveryEvent::PreparationDiscoveryFinished { session_id }
                if self.phase == RecoveryPhase::DiscoveringQuic
                    && self
                        .preparation
                        .as_ref()
                        .is_some_and(|item| item.id == session_id) =>
            {
                self.phase = RecoveryPhase::Calibrating;
                vec![self.emit_status()]
            }
            RecoveryEvent::PreparationProgress {
                session_id,
                current_round,
                total_rounds,
            } if matches!(
                self.phase,
                RecoveryPhase::DiscoveringQuic | RecoveryPhase::Calibrating
            ) && self
                .preparation
                .as_ref()
                .is_some_and(|item| item.id == session_id) =>
            {
                if let Some(preparation) = self.preparation.as_mut() {
                    preparation.current_round = current_round;
                    preparation.total_rounds = total_rounds;
                }
                vec![self.emit_status()]
            }
            RecoveryEvent::PreparationReady {
                session_id,
                candidates,
                mode,
            } if matches!(
                self.phase,
                RecoveryPhase::DiscoveringQuic | RecoveryPhase::Calibrating
            ) && self
                .preparation
                .as_ref()
                .is_some_and(|item| item.id == session_id) =>
            {
                let preparation = self.preparation.take().unwrap();
                self.start_session(preparation.category, candidates, mode, Some(preparation))
            }
            RecoveryEvent::PreparationFailed { session_id, reason }
                if matches!(
                    self.phase,
                    RecoveryPhase::DiscoveringQuic | RecoveryPhase::Calibrating
                ) && self
                    .preparation
                    .as_ref()
                    .is_some_and(|item| item.id == session_id) =>
            {
                self.preparation = None;
                self.suggestion = None;
                self.phase = match reason {
                    PreparationFailure::ProbeUnreliable => RecoveryPhase::ProbeUnreliable,
                    PreparationFailure::QuicTargetsUnavailable => {
                        RecoveryPhase::QuicTargetsUnavailable
                    }
                };
                vec![self.emit_status()]
            }
            RecoveryEvent::CalibrationFailed if self.phase == RecoveryPhase::Suggested => {
                self.suggestion = None;
                self.phase = RecoveryPhase::ProbeUnreliable;
                vec![self.emit_status()]
            }
            RecoveryEvent::UserStart {
                category,
                candidates,
            } if self.phase == RecoveryPhase::Suggested
                && self
                    .suggestion
                    .as_ref()
                    .is_some_and(|suggestion| suggestion.category == category) =>
            {
                self.start_session(category, candidates, SearchSessionMode::Comparison, None)
            }
            RecoveryEvent::CandidateStarted {
                session_id,
                attempt_id,
                candidate_id,
                ok,
            } if self.phase == RecoveryPhase::Searching
                && self.matches_current(session_id, attempt_id, &candidate_id) =>
            {
                if ok {
                    self.phase = RecoveryPhase::CandidateProbe;
                    let candidate = self.current_candidate().unwrap().clone();
                    vec![
                        RecoveryAction::RunProbes {
                            session_id,
                            attempt_id,
                            candidate,
                        },
                        self.emit_status(),
                    ]
                } else {
                    if let Some(session) = self.session.as_mut() {
                        session.last_failure = FailureStage::Spawn;
                    }
                    self.begin_rollback(RollbackReason::CandidateStartFailed)
                }
            }
            RecoveryEvent::ProbeFinished {
                session_id,
                attempt_id,
                candidate_id,
                result,
                now,
            } if self.phase == RecoveryPhase::CandidateProbe
                && self.matches_current(session_id, attempt_id, &candidate_id) =>
            {
                if let Some(session) = self.session.as_mut() {
                    session.last_failure = result.failure_stage;
                }
                if result.is_success() {
                    if self
                        .session
                        .as_ref()
                        .is_some_and(|session| session.mode == SearchSessionMode::Recovery)
                    {
                        self.phase = RecoveryPhase::Applying;
                        if let Some(session) = self.session.as_mut() {
                            session.verification_deadline = None;
                        }
                        let candidate = self.current_candidate().unwrap().clone();
                        return vec![
                            RecoveryAction::PersistConfirmed {
                                session_id,
                                attempt_id,
                                candidate,
                            },
                            self.emit_status(),
                        ];
                    }
                    let deadline = now.saturating_add(self.cfg.verification_ms);
                    if let Some(session) = self.session.as_mut() {
                        session.verification_deadline = Some(deadline);
                    }
                    self.phase = RecoveryPhase::TemporaryVerification;
                    vec![
                        RecoveryAction::BeginVerification {
                            session_id,
                            attempt_id,
                            candidate_id,
                            deadline,
                        },
                        self.emit_status(),
                    ]
                } else {
                    self.begin_rollback(RollbackReason::CandidateProbeFailed)
                }
            }
            RecoveryEvent::CandidateCrashed {
                session_id,
                attempt_id,
                candidate_id,
            } if self.matches_current(session_id, attempt_id, &candidate_id)
                && matches!(
                    self.phase,
                    RecoveryPhase::Searching
                        | RecoveryPhase::CandidateProbe
                        | RecoveryPhase::TemporaryVerification
                ) =>
            {
                if let Some(session) = self.session.as_mut() {
                    session.last_failure = FailureStage::Stability;
                }
                self.begin_rollback(RollbackReason::CandidateCrashed)
            }
            RecoveryEvent::UserConfirm {
                session_id,
                attempt_id,
                candidate_id,
            } if self.phase == RecoveryPhase::TemporaryVerification
                && self.matches_current(session_id, attempt_id, &candidate_id) =>
            {
                self.phase = RecoveryPhase::Applying;
                let candidate = self.current_candidate().unwrap().clone();
                vec![
                    RecoveryAction::PersistConfirmed {
                        session_id,
                        attempt_id,
                        candidate,
                    },
                    self.emit_status(),
                ]
            }
            RecoveryEvent::UserReject {
                session_id,
                attempt_id,
                candidate_id,
            } if self.phase == RecoveryPhase::TemporaryVerification
                && self.matches_current(session_id, attempt_id, &candidate_id) =>
            {
                self.begin_rollback(RollbackReason::UserRejected)
            }
            RecoveryEvent::PersistFinished {
                session_id,
                attempt_id,
                candidate_id,
                ok,
            } if self.phase == RecoveryPhase::Applying
                && self.matches_current(session_id, attempt_id, &candidate_id) =>
            {
                if ok {
                    self.phase = RecoveryPhase::Applied;
                    if let Some(session) = self.session.as_mut() {
                        session.verification_deadline = None;
                    }
                    vec![self.emit_status()]
                } else {
                    self.begin_rollback(RollbackReason::PersistenceFailed)
                }
            }
            RecoveryEvent::RollbackFinished {
                session_id,
                attempt_id,
                restored,
                base_healthy,
                base_probe_reliable,
            } if self.phase == RecoveryPhase::RollingBack
                && self.matches_attempt(session_id, attempt_id) =>
            {
                self.on_rollback_finished(restored, base_healthy, base_probe_reliable)
            }
            RecoveryEvent::UserCancel if self.phase == RecoveryPhase::RollingBack => {
                if let Some(session) = self.session.as_mut() {
                    session.rollback_reason = Some(RollbackReason::UserCancelled);
                }
                vec![self.emit_status()]
            }
            RecoveryEvent::Shutdown if self.phase == RecoveryPhase::RollingBack => {
                if let Some(session) = self.session.as_mut() {
                    session.rollback_reason = Some(RollbackReason::Shutdown);
                }
                vec![self.emit_status()]
            }
            RecoveryEvent::UserCancel
                if matches!(
                    self.phase,
                    RecoveryPhase::DiscoveringQuic | RecoveryPhase::Calibrating
                ) =>
            {
                self.preparation = None;
                self.suggestion = None;
                self.phase = RecoveryPhase::Cancelled;
                vec![self.emit_status()]
            }
            RecoveryEvent::UserCancel
                if matches!(
                    self.phase,
                    RecoveryPhase::Searching
                        | RecoveryPhase::CandidateProbe
                        | RecoveryPhase::TemporaryVerification
                        | RecoveryPhase::Applying
                ) =>
            {
                self.begin_rollback(RollbackReason::UserCancelled)
            }
            RecoveryEvent::Tick(now) if self.phase == RecoveryPhase::TemporaryVerification => {
                if self
                    .session
                    .as_ref()
                    .and_then(|session| session.verification_deadline)
                    .is_some_and(|deadline| now >= deadline)
                {
                    self.begin_rollback(RollbackReason::VerificationTimeout)
                } else {
                    Vec::new()
                }
            }
            RecoveryEvent::Shutdown
                if matches!(
                    self.phase,
                    RecoveryPhase::DiscoveringQuic | RecoveryPhase::Calibrating
                ) =>
            {
                self.preparation = None;
                self.suggestion = None;
                self.phase = RecoveryPhase::Idle;
                vec![self.emit_status()]
            }
            RecoveryEvent::Shutdown
                if matches!(
                    self.phase,
                    RecoveryPhase::Searching
                        | RecoveryPhase::CandidateProbe
                        | RecoveryPhase::TemporaryVerification
                        | RecoveryPhase::Applying
                ) =>
            {
                self.begin_rollback(RollbackReason::Shutdown)
            }
            RecoveryEvent::Shutdown => {
                self.phase = RecoveryPhase::Idle;
                self.suggestion = None;
                self.session = None;
                vec![self.emit_status()]
            }
            _ => Vec::new(),
        }
    }

    fn start_session(
        &mut self,
        category: AdaptiveCategory,
        candidates: Vec<StrategyCandidate>,
        mode: SearchSessionMode,
        prepared: Option<Preparation>,
    ) -> Vec<RecoveryAction> {
        let (session_id, diagnosis) = if let Some(preparation) = prepared {
            (preparation.id, preparation.diagnosis)
        } else {
            let diagnosis = self.suggestion.as_ref().unwrap().reason;
            self.next_session_id = next_nonzero(self.next_session_id);
            (self.next_session_id, diagnosis)
        };
        let mut seen = HashSet::new();
        let candidates = candidates
            .into_iter()
            .filter(|candidate| {
                candidate.category == category
                    && validator::validate(candidate).is_valid()
                    && seen.insert(candidate.candidate_id())
            })
            .take(MAX_SESSION_CANDIDATES)
            .collect::<Vec<_>>();

        self.suggestion = None;
        self.preparation = None;
        self.session = Some(Session {
            id: session_id,
            category,
            diagnosis,
            mode,
            candidates,
            next_index: 0,
            next_attempt_id: 0,
            current: None,
            verification_deadline: None,
            rollback_reason: None,
            last_failure: FailureStage::None,
        });
        self.start_next_candidate()
    }
    fn start_next_candidate(&mut self) -> Vec<RecoveryAction> {
        let Some(session) = self.session.as_mut() else {
            return Vec::new();
        };
        if session.next_index >= session.candidates.len() {
            self.phase = RecoveryPhase::Exhausted;
            session.current = None;
            return vec![self.emit_status()];
        }
        let index = session.next_index;
        if session.last_failure != FailureStage::None {
            if let Some(best) = (index..session.candidates.len()).min_by_key(|candidate_index| {
                generator::candidate_priority(
                    &session.candidates[*candidate_index],
                    session.last_failure,
                )
            }) {
                session.candidates.swap(index, best);
            }
        }
        session.next_index += 1;
        session.next_attempt_id = next_nonzero(session.next_attempt_id);
        let attempt_id = session.next_attempt_id;
        let candidate = session.candidates[index].clone();
        session.current = Some(CurrentAttempt {
            attempt_id,
            index,
            candidate: candidate.clone(),
        });
        session.verification_deadline = None;
        session.rollback_reason = None;
        self.phase = RecoveryPhase::Searching;
        vec![
            RecoveryAction::StartCandidate {
                session_id: session.id,
                attempt_id,
                index: index + 1,
                total: session.candidates.len(),
                candidate,
            },
            self.emit_status(),
        ]
    }

    fn begin_rollback(&mut self, reason: RollbackReason) -> Vec<RecoveryAction> {
        let Some(session) = self.session.as_mut() else {
            return Vec::new();
        };
        let Some(current) = session.current.as_ref() else {
            return Vec::new();
        };
        session.rollback_reason = Some(reason);
        session.verification_deadline = None;
        self.phase = RecoveryPhase::RollingBack;
        vec![
            RecoveryAction::Rollback {
                session_id: session.id,
                attempt_id: current.attempt_id,
                reason,
            },
            self.emit_status(),
        ]
    }

    fn on_rollback_finished(
        &mut self,
        restored: bool,
        base_healthy: bool,
        base_probe_reliable: bool,
    ) -> Vec<RecoveryAction> {
        let reason = self
            .session
            .as_ref()
            .and_then(|session| session.rollback_reason)
            .unwrap_or(RollbackReason::CandidateProbeFailed);
        if !restored {
            self.phase = RecoveryPhase::InternalError;
            return vec![self.emit_status()];
        }
        if !base_probe_reliable {
            self.phase = RecoveryPhase::ProbeUnreliable;
            return vec![self.emit_status()];
        }
        if !base_healthy {
            self.phase = RecoveryPhase::BaseUnhealthy;
            return vec![self.emit_status()];
        }
        if reason.should_continue() {
            if let Some(session) = self.session.as_mut() {
                session.current = None;
                session.rollback_reason = None;
            }
            return self.start_next_candidate();
        }

        self.phase = match reason {
            RollbackReason::UserCancelled | RollbackReason::Shutdown => RecoveryPhase::Cancelled,
            RollbackReason::VerificationTimeout => RecoveryPhase::Idle,
            RollbackReason::PersistenceFailed => RecoveryPhase::Exhausted,
            _ => RecoveryPhase::Exhausted,
        };
        vec![self.emit_status()]
    }

    fn matches_current(&self, session_id: u64, attempt_id: u64, candidate_id: &str) -> bool {
        self.session.as_ref().is_some_and(|session| {
            session.id == session_id
                && session.current.as_ref().is_some_and(|current| {
                    current.attempt_id == attempt_id
                        && current.candidate.candidate_id() == candidate_id
                })
        })
    }

    fn matches_attempt(&self, session_id: u64, attempt_id: u64) -> bool {
        self.session.as_ref().is_some_and(|session| {
            session.id == session_id
                && session
                    .current
                    .as_ref()
                    .is_some_and(|current| current.attempt_id == attempt_id)
        })
    }

    fn current_candidate(&self) -> Option<&StrategyCandidate> {
        self.session
            .as_ref()?
            .current
            .as_ref()
            .map(|attempt| &attempt.candidate)
    }

    fn emit_status(&self) -> RecoveryAction {
        RecoveryAction::EmitStatus(self.status())
    }
}

fn next_nonzero(value: u64) -> u64 {
    let next = value.wrapping_add(1);
    if next == 0 {
        1
    } else {
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adaptive_strategy::dsl::{
        AllowedPayload, AllowedRange, StrategyFunction, StrategyStep, StrategyTransport,
        StrategyValue,
    };

    fn candidate(pos: &str) -> StrategyCandidate {
        StrategyCandidate::new(
            AdaptiveCategory::YoutubeTwitch,
            StrategyTransport::Tls,
            vec![StrategyStep::new(StrategyFunction::MultiDisorderLegacy)
                .with_arg("pos", StrategyValue::Text(pos.into()))],
            vec![AllowedPayload::TlsClientHello],
            Some(AllowedRange::FirstTenDataPackets),
        )
    }

    fn successful_probe() -> CandidateProbeResult {
        CandidateProbeResult {
            transport: StrategyTransport::Tls,
            dns_ok: true,
            tcp_ok: true,
            tls_or_quic_ok: true,
            https_ok: true,
            eyes_working_ok: false,
            eyes_working_count: 0,
            eyes_working_hosts: 0,
            reset_count: 0,
            blackhole_count: 0,
            successful_rounds: 2,
            required_successes: 2,
            total_rounds: 3,
            failure_stage: FailureStage::None,
        }
    }

    fn started_model(candidates: Vec<StrategyCandidate>) -> (RecoveryModel, u64, u64, String) {
        let mut model = RecoveryModel::new(RecoveryCfg::default());
        model.step(RecoveryEvent::DiagnosisConfirmed {
            category: AdaptiveCategory::YoutubeTwitch,
            reason: DiagnosisReason::TlsBlackhole,
        });
        let actions = model.step(RecoveryEvent::UserStart {
            category: AdaptiveCategory::YoutubeTwitch,
            candidates,
        });
        let RecoveryAction::StartCandidate {
            session_id,
            attempt_id,
            candidate,
            ..
        } = &actions[0]
        else {
            panic!("expected StartCandidate")
        };
        (model, *session_id, *attempt_id, candidate.candidate_id())
    }

    #[test]
    fn diagnosis_only_suggests_and_never_starts_search() {
        let mut model = RecoveryModel::new(RecoveryCfg::default());
        let actions = model.step(RecoveryEvent::DiagnosisConfirmed {
            category: AdaptiveCategory::YoutubeTwitch,
            reason: DiagnosisReason::TlsBlackhole,
        });
        assert_eq!(model.status().phase, RecoveryPhase::Suggested);
        assert!(matches!(
            actions[0],
            RecoveryAction::NotifySuggestion { .. }
        ));
        assert!(!actions
            .iter()
            .any(|action| matches!(action, RecoveryAction::StartCandidate { .. })));
    }

    #[test]
    fn successful_candidate_requires_manual_confirmation_before_apply() {
        let (mut model, session, attempt, id) = started_model(vec![candidate("1,midsld")]);
        let actions = model.step(RecoveryEvent::CandidateStarted {
            session_id: session,
            attempt_id: attempt,
            candidate_id: id.clone(),
            ok: true,
        });
        assert!(matches!(actions[0], RecoveryAction::RunProbes { .. }));
        let actions = model.step(RecoveryEvent::ProbeFinished {
            session_id: session,
            attempt_id: attempt,
            candidate_id: id.clone(),
            result: successful_probe(),
            now: 1_000,
        });
        assert_eq!(model.status().phase, RecoveryPhase::TemporaryVerification);
        assert_eq!(model.status().verification_deadline_ms, Some(61_000));
        assert!(matches!(
            actions[0],
            RecoveryAction::BeginVerification { .. }
        ));
        assert!(!actions
            .iter()
            .any(|action| matches!(action, RecoveryAction::PersistConfirmed { .. })));

        let actions = model.step(RecoveryEvent::UserConfirm {
            session_id: session,
            attempt_id: attempt,
            candidate_id: id.clone(),
        });
        assert_eq!(model.status().phase, RecoveryPhase::Applying);
        assert!(matches!(
            actions[0],
            RecoveryAction::PersistConfirmed { .. }
        ));
        model.step(RecoveryEvent::PersistFinished {
            session_id: session,
            attempt_id: attempt,
            candidate_id: id,
            ok: true,
        });
        assert_eq!(model.status().phase, RecoveryPhase::Applied);
    }

    #[test]
    fn successful_recovery_candidate_is_persisted_after_automated_stability_probe() {
        let value = candidate("1,midsld");
        let mut model = RecoveryModel::new(RecoveryCfg::default());
        model.step(RecoveryEvent::DiagnosisConfirmed {
            category: AdaptiveCategory::YoutubeTwitch,
            reason: DiagnosisReason::TlsBlackhole,
        });
        model.step(RecoveryEvent::PreparationStarted {
            category: AdaptiveCategory::YoutubeTwitch,
            transport: StrategyTransport::Tls,
        });
        let actions = model.step(RecoveryEvent::PreparationReady {
            session_id: 1,
            candidates: vec![value],
            mode: SearchSessionMode::Recovery,
        });
        let RecoveryAction::StartCandidate {
            session_id,
            attempt_id,
            candidate,
            ..
        } = &actions[0]
        else {
            panic!("expected StartCandidate")
        };
        let (session_id, attempt_id, candidate_id) =
            (*session_id, *attempt_id, candidate.candidate_id());
        model.step(RecoveryEvent::CandidateStarted {
            session_id,
            attempt_id,
            candidate_id: candidate_id.clone(),
            ok: true,
        });

        let actions = model.step(RecoveryEvent::ProbeFinished {
            session_id,
            attempt_id,
            candidate_id: candidate_id.clone(),
            result: successful_probe(),
            now: 1_000,
        });
        assert_eq!(model.status().phase, RecoveryPhase::Applying);
        assert_eq!(model.status().verification_deadline_ms, None);
        assert!(matches!(
            actions[0],
            RecoveryAction::PersistConfirmed { .. }
        ));
        assert!(!actions
            .iter()
            .any(|action| matches!(action, RecoveryAction::BeginVerification { .. })));

        model.step(RecoveryEvent::PersistFinished {
            session_id,
            attempt_id,
            candidate_id,
            ok: true,
        });
        assert_eq!(model.status().phase, RecoveryPhase::Applied);
    }

    #[test]
    fn rejection_rolls_back_then_advances_to_next_candidate() {
        let (mut model, session, attempt, id) =
            started_model(vec![candidate("1,midsld"), candidate("1,sniext")]);
        model.step(RecoveryEvent::CandidateStarted {
            session_id: session,
            attempt_id: attempt,
            candidate_id: id.clone(),
            ok: true,
        });
        model.step(RecoveryEvent::ProbeFinished {
            session_id: session,
            attempt_id: attempt,
            candidate_id: id.clone(),
            result: successful_probe(),
            now: 0,
        });
        let actions = model.step(RecoveryEvent::UserReject {
            session_id: session,
            attempt_id: attempt,
            candidate_id: id,
        });
        assert!(matches!(
            actions[0],
            RecoveryAction::Rollback {
                reason: RollbackReason::UserRejected,
                ..
            }
        ));
        let actions = model.step(RecoveryEvent::RollbackFinished {
            session_id: session,
            attempt_id: attempt,
            restored: true,
            base_healthy: true,
            base_probe_reliable: true,
        });
        let RecoveryAction::StartCandidate {
            attempt_id: next_attempt,
            index,
            ..
        } = actions[0]
        else {
            panic!("expected next candidate")
        };
        assert_eq!(index, 2);
        assert_ne!(next_attempt, attempt);
    }

    #[test]
    fn verification_timeout_rolls_back_and_does_not_continue() {
        let (mut model, session, attempt, id) = started_model(vec![candidate("1,midsld")]);
        model.step(RecoveryEvent::CandidateStarted {
            session_id: session,
            attempt_id: attempt,
            candidate_id: id.clone(),
            ok: true,
        });
        model.step(RecoveryEvent::ProbeFinished {
            session_id: session,
            attempt_id: attempt,
            candidate_id: id,
            result: successful_probe(),
            now: 5,
        });
        assert!(model.step(RecoveryEvent::Tick(60_004)).is_empty());
        let actions = model.step(RecoveryEvent::Tick(60_005));
        assert!(matches!(
            actions[0],
            RecoveryAction::Rollback {
                reason: RollbackReason::VerificationTimeout,
                ..
            }
        ));
        model.step(RecoveryEvent::RollbackFinished {
            session_id: session,
            attempt_id: attempt,
            restored: true,
            base_healthy: true,
            base_probe_reliable: true,
        });
        assert_eq!(model.status().phase, RecoveryPhase::Idle);
    }

    #[test]
    fn stale_candidate_events_are_ignored() {
        let (mut model, session, attempt, id) = started_model(vec![candidate("1,midsld")]);
        assert!(model
            .step(RecoveryEvent::CandidateStarted {
                session_id: session + 1,
                attempt_id: attempt,
                candidate_id: id.clone(),
                ok: true,
            })
            .is_empty());
        assert!(model
            .step(RecoveryEvent::CandidateStarted {
                session_id: session,
                attempt_id: attempt + 1,
                candidate_id: id,
                ok: true,
            })
            .is_empty());
        assert_eq!(model.status().phase, RecoveryPhase::Searching);
    }

    #[test]
    fn cancel_and_shutdown_require_rollback() {
        let (mut model, session, attempt, _) = started_model(vec![candidate("1,midsld")]);
        let actions = model.step(RecoveryEvent::UserCancel);
        assert!(matches!(
            actions[0],
            RecoveryAction::Rollback {
                reason: RollbackReason::UserCancelled,
                ..
            }
        ));
        model.step(RecoveryEvent::RollbackFinished {
            session_id: session,
            attempt_id: attempt,
            restored: true,
            base_healthy: true,
            base_probe_reliable: true,
        });
        assert_eq!(model.status().phase, RecoveryPhase::Cancelled);

        let (mut model, _, _, _) = started_model(vec![candidate("1,midsld")]);
        assert!(matches!(
            model.step(RecoveryEvent::Shutdown)[0],
            RecoveryAction::Rollback {
                reason: RollbackReason::Shutdown,
                ..
            }
        ));
    }

    #[test]
    fn cancel_and_shutdown_during_rollback_do_not_schedule_second_restore() {
        let (mut model, session, attempt, id) = started_model(vec![candidate("1,midsld")]);
        model.step(RecoveryEvent::CandidateStarted {
            session_id: session,
            attempt_id: attempt,
            candidate_id: id.clone(),
            ok: true,
        });
        let actions = model.step(RecoveryEvent::ProbeFinished {
            session_id: session,
            attempt_id: attempt,
            candidate_id: id,
            result: CandidateProbeResult::default(),
            now: 0,
        });
        assert!(matches!(
            actions[0],
            RecoveryAction::Rollback {
                reason: RollbackReason::CandidateProbeFailed,
                ..
            }
        ));

        let cancel_actions = model.step(RecoveryEvent::UserCancel);
        assert!(cancel_actions
            .iter()
            .all(|action| !matches!(action, RecoveryAction::Rollback { .. })));
        assert_eq!(
            model.status().rollback_reason,
            Some(RollbackReason::UserCancelled)
        );

        let shutdown_actions = model.step(RecoveryEvent::Shutdown);
        assert!(shutdown_actions
            .iter()
            .all(|action| !matches!(action, RecoveryAction::Rollback { .. })));
        assert_eq!(
            model.status().rollback_reason,
            Some(RollbackReason::Shutdown)
        );
    }

    #[test]
    fn invalid_duplicates_and_candidates_over_budget_are_filtered() {
        let mut candidates = Vec::new();
        for repeats in 1..=12 {
            candidates.push(StrategyCandidate::new(
                AdaptiveCategory::YoutubeTwitch,
                StrategyTransport::Tls,
                vec![StrategyStep::new(StrategyFunction::Fake)
                    .with_arg("blob", StrategyValue::Text("fake_default_tls".into()))
                    .with_arg("repeats", StrategyValue::Integer(repeats))],
                vec![AllowedPayload::TlsClientHello],
                None,
            ));
        }
        candidates.push(candidate("1,midsld"));
        candidates.push(candidate("1,midsld"));
        let mut invalid = candidate("1,sniext");
        invalid.steps.clear();
        candidates.push(invalid);

        let (model, _, _, _) = started_model(candidates);
        assert_eq!(model.status().candidate_total, Some(12));
    }

    #[test]
    fn failed_probe_rolls_back_and_exhausts_after_last_candidate() {
        let (mut model, session, attempt, id) = started_model(vec![candidate("1,midsld")]);
        model.step(RecoveryEvent::CandidateStarted {
            session_id: session,
            attempt_id: attempt,
            candidate_id: id.clone(),
            ok: true,
        });
        let actions = model.step(RecoveryEvent::ProbeFinished {
            session_id: session,
            attempt_id: attempt,
            candidate_id: id,
            result: CandidateProbeResult::default(),
            now: 0,
        });
        assert!(matches!(
            actions[0],
            RecoveryAction::Rollback {
                reason: RollbackReason::CandidateProbeFailed,
                ..
            }
        ));
        model.step(RecoveryEvent::RollbackFinished {
            session_id: session,
            attempt_id: attempt,
            restored: true,
            base_healthy: true,
            base_probe_reliable: true,
        });
        assert_eq!(model.status().phase, RecoveryPhase::Exhausted);
    }

    #[test]
    fn candidate_start_failure_rolls_back_then_advances() {
        let (mut model, session, attempt, id) =
            started_model(vec![candidate("1,midsld"), candidate("1,sniext")]);
        let actions = model.step(RecoveryEvent::CandidateStarted {
            session_id: session,
            attempt_id: attempt,
            candidate_id: id,
            ok: false,
        });
        assert!(matches!(
            actions[0],
            RecoveryAction::Rollback {
                reason: RollbackReason::CandidateStartFailed,
                ..
            }
        ));
        let actions = model.step(RecoveryEvent::RollbackFinished {
            session_id: session,
            attempt_id: attempt,
            restored: true,
            base_healthy: true,
            base_probe_reliable: true,
        });
        assert!(matches!(
            actions[0],
            RecoveryAction::StartCandidate { index: 2, .. }
        ));
    }

    #[test]
    fn persistence_or_rollback_failure_never_applies_candidate() {
        let (mut model, session, attempt, id) = started_model(vec![candidate("1,midsld")]);
        model.step(RecoveryEvent::CandidateStarted {
            session_id: session,
            attempt_id: attempt,
            candidate_id: id.clone(),
            ok: true,
        });
        model.step(RecoveryEvent::ProbeFinished {
            session_id: session,
            attempt_id: attempt,
            candidate_id: id.clone(),
            result: successful_probe(),
            now: 0,
        });
        model.step(RecoveryEvent::UserConfirm {
            session_id: session,
            attempt_id: attempt,
            candidate_id: id.clone(),
        });
        let actions = model.step(RecoveryEvent::PersistFinished {
            session_id: session,
            attempt_id: attempt,
            candidate_id: id,
            ok: false,
        });
        assert!(matches!(
            actions[0],
            RecoveryAction::Rollback {
                reason: RollbackReason::PersistenceFailed,
                ..
            }
        ));
        model.step(RecoveryEvent::RollbackFinished {
            session_id: session,
            attempt_id: attempt,
            restored: false,
            base_healthy: false,
            base_probe_reliable: true,
        });
        assert_eq!(model.status().phase, RecoveryPhase::InternalError);
    }

    #[test]
    fn preparation_is_visible_and_cancelable_before_candidate_spawn() {
        let mut model = RecoveryModel::new(RecoveryCfg::default());
        model.step(RecoveryEvent::DiagnosisConfirmed {
            category: AdaptiveCategory::Gaming,
            reason: DiagnosisReason::ProbeFailure,
        });
        let actions = model.step(RecoveryEvent::PreparationStarted {
            category: AdaptiveCategory::Gaming,
            transport: StrategyTransport::Quic,
        });
        assert!(matches!(actions[0], RecoveryAction::EmitStatus(_)));
        let status = model.status();
        assert_eq!(status.phase, RecoveryPhase::DiscoveringQuic);
        assert_eq!(status.session_id, Some(1));
        assert_eq!(status.transport, Some(StrategyTransport::Quic));

        model.step(RecoveryEvent::PreparationProgress {
            session_id: 1,
            current_round: 1,
            total_rounds: 3,
        });
        assert_eq!(model.status().current_round, Some(1));
        assert_eq!(model.status().total_rounds, Some(3));

        let actions = model.step(RecoveryEvent::UserCancel);
        assert!(actions
            .iter()
            .all(|action| !matches!(action, RecoveryAction::Rollback { .. })));
        assert_eq!(model.status().phase, RecoveryPhase::Cancelled);
    }

    #[test]
    fn recovery_preparation_starts_candidate_and_exposes_mode() {
        let mut model = RecoveryModel::new(RecoveryCfg::default());
        model.step(RecoveryEvent::DiagnosisConfirmed {
            category: AdaptiveCategory::Gaming,
            reason: DiagnosisReason::ProbeFailure,
        });
        model.step(RecoveryEvent::PreparationStarted {
            category: AdaptiveCategory::Gaming,
            transport: StrategyTransport::Quic,
        });
        model.step(RecoveryEvent::PreparationDiscoveryFinished { session_id: 1 });
        let value = generator::builtin_baseline_candidates(AdaptiveCategory::Gaming)
            .into_iter()
            .find(|candidate| candidate.transport == StrategyTransport::Quic)
            .expect("gaming QUIC baseline");
        let actions = model.step(RecoveryEvent::PreparationReady {
            session_id: 1,
            candidates: vec![value],
            mode: SearchSessionMode::Recovery,
        });
        assert!(matches!(
            actions.first(),
            Some(RecoveryAction::StartCandidate { session_id: 1, .. })
        ));
        assert_eq!(
            model.status().session_mode,
            Some(SearchSessionMode::Recovery)
        );
    }

    #[test]
    fn unavailable_quic_targets_are_distinct_from_exhaustion() {
        let mut model = RecoveryModel::new(RecoveryCfg::default());
        model.step(RecoveryEvent::DiagnosisConfirmed {
            category: AdaptiveCategory::Gaming,
            reason: DiagnosisReason::ProbeFailure,
        });
        model.step(RecoveryEvent::PreparationStarted {
            category: AdaptiveCategory::Gaming,
            transport: StrategyTransport::Quic,
        });
        model.step(RecoveryEvent::PreparationFailed {
            session_id: 1,
            reason: PreparationFailure::QuicTargetsUnavailable,
        });
        assert_eq!(model.status().phase, RecoveryPhase::QuicTargetsUnavailable);
    }

    #[test]
    fn calibration_failure_is_terminal_without_starting_candidate() {
        let mut model = RecoveryModel::new(RecoveryCfg::default());
        model.step(RecoveryEvent::DiagnosisConfirmed {
            category: AdaptiveCategory::YoutubeTwitch,
            reason: DiagnosisReason::ProbeFailure,
        });
        let actions = model.step(RecoveryEvent::CalibrationFailed);
        assert_eq!(model.status().phase, RecoveryPhase::ProbeUnreliable);
        assert!(matches!(
            actions.as_slice(),
            [RecoveryAction::EmitStatus(_)]
        ));
    }

    #[test]
    fn failed_base_recheck_is_distinct_from_rollback_failure() {
        let (mut model, session, attempt, id) =
            started_model(vec![candidate("1,midsld"), candidate("1,sniext")]);
        model.step(RecoveryEvent::CandidateStarted {
            session_id: session,
            attempt_id: attempt,
            candidate_id: id.clone(),
            ok: true,
        });
        let mut failed = successful_probe();
        failed.successful_rounds = 0;
        failed.failure_stage = FailureStage::Tls;
        model.step(RecoveryEvent::ProbeFinished {
            session_id: session,
            attempt_id: attempt,
            candidate_id: id,
            result: failed,
            now: 1,
        });
        model.step(RecoveryEvent::RollbackFinished {
            session_id: session,
            attempt_id: attempt,
            restored: true,
            base_healthy: false,
            base_probe_reliable: true,
        });
        assert_eq!(model.status().phase, RecoveryPhase::BaseUnhealthy);
    }
    #[test]
    fn repeated_dns_base_recheck_failure_is_probe_unreliable() {
        let (mut model, session, attempt, id) =
            started_model(vec![candidate("1,midsld"), candidate("1,sniext")]);
        model.step(RecoveryEvent::CandidateStarted {
            session_id: session,
            attempt_id: attempt,
            candidate_id: id.clone(),
            ok: true,
        });
        let mut failed = successful_probe();
        failed.successful_rounds = 0;
        failed.dns_ok = false;
        failed.failure_stage = FailureStage::Dns;
        model.step(RecoveryEvent::ProbeFinished {
            session_id: session,
            attempt_id: attempt,
            candidate_id: id,
            result: failed,
            now: 1,
        });
        model.step(RecoveryEvent::RollbackFinished {
            session_id: session,
            attempt_id: attempt,
            restored: true,
            base_healthy: false,
            base_probe_reliable: false,
        });
        assert_eq!(model.status().phase, RecoveryPhase::ProbeUnreliable);
    }

    #[test]
    fn session_and_attempt_ids_never_wrap_to_zero() {
        assert_eq!(next_nonzero(u64::MAX), 1);
        let mut model = RecoveryModel::new(RecoveryCfg::default());
        model.next_session_id = u64::MAX;
        model.step(RecoveryEvent::DiagnosisConfirmed {
            category: AdaptiveCategory::YoutubeTwitch,
            reason: DiagnosisReason::ProbeFailure,
        });
        model.step(RecoveryEvent::UserStart {
            category: AdaptiveCategory::YoutubeTwitch,
            candidates: vec![candidate("1,midsld")],
        });
        assert_eq!(model.status().session_id, Some(1));
        assert_eq!(model.status().attempt_id, Some(1));
    }
}
