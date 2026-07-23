//! Public projection and reconciliation boundary for Legacy reliability.

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::state::{AppState, RevisionClock};
use crate::util::VersionedSection;

use super::assessment::{
    AssessmentClassification, AssessmentConfidence, EvidenceSummary, LanePhase,
};
use super::contracts::{
    ConfigFingerprint, EyeHealthState, IntentFence, ProcessOwner, ProcessStartIdentity,
    SensorGeneration, SessionId,
};
use super::manager::ObserveOnlySnapshot;
use super::policy::PresumedIntent;
use super::recovery::{
    AssistedApproval, AssistedProposalView, RecoveryAction, RecoveryAttemptView,
    RecoveryCompletion, RecoveryConfig, RecoveryDecision, RecoveryMode, RecoveryStatus,
};
use super::recovery_runtime::IncidentObservation;

pub const STATUS_EVENT: &str = "legacy-reliability://status";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyReliabilityMode {
    ObserveOnly,
    Assisted,
    Automatic,
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
    pub proposal: Option<AssistedProposalView>,
    pub active_attempt: Option<RecoveryAttemptView>,
    pub last_completion: Option<RecoveryCompletion>,
    pub negative_cooldown_count: usize,
    pub automatic_paused: bool,
    pub automatic_pacing_remaining_ms: Option<u64>,
    pub frozen_categories: Vec<String>,
    pub halted_categories: Vec<String>,
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
    /// Legacy public name for session/generation-scoped last-known-good state.
    /// It is display-only and does not participate in recovery policy.
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
            proposal: None,
            active_attempt: None,
            last_completion: None,
            negative_cooldown_count: 0,
            automatic_paused: true,
            automatic_pacing_remaining_ms: None,
            frozen_categories: Vec::new(),
            halted_categories: Vec::new(),
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
            proposal: None,
            active_attempt: None,
            last_completion: None,
            negative_cooldown_count: 0,
            automatic_paused: true,
            automatic_pacing_remaining_ms: None,
            frozen_categories: Vec::new(),
            halted_categories: Vec::new(),
        }
    }

    fn apply_recovery(&mut self, recovery: RecoveryStatus, now_ms: u64) {
        self.mode = match recovery.mode {
            RecoveryMode::ObserveOnly => LegacyReliabilityMode::ObserveOnly,
            RecoveryMode::Assisted => LegacyReliabilityMode::Assisted,
            RecoveryMode::Automatic => LegacyReliabilityMode::Automatic,
        };
        self.automatic_paused = recovery.automatic_paused;
        self.automatic_pacing_remaining_ms = recovery
            .automatic_pacing_until_monotonic_ms
            .map(|until_ms| until_ms.saturating_sub(now_ms))
            .filter(|remaining_ms| *remaining_ms > 0);
        self.frozen_categories = recovery.manual_frozen_categories;
        self.halted_categories = recovery.halted_categories;
        self.proposal = recovery.proposal;
        self.active_attempt = recovery.active_attempt;
        self.last_completion = recovery.last_completion;
        self.negative_cooldown_count = recovery.negative_cooldown_count;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StatusFence {
    /// Lifecycle code intentionally replaces whichever session is currently
    /// public (`inactive` and `starting` use this path).
    ReplaceAny,
    /// A Manager callback may update only the exact session + Eyes generation
    /// that still owns the public projection.
    ExactOwner(StatusOwner),
    /// Overlay-only updates must derive their base payload from the status held
    /// under the same lock; they are never allowed to replace lifecycle state.
    PreserveCurrent,
}

/// Publishes a lifecycle transition that intentionally replaces any prior
/// Legacy session (currently `inactive` and `starting`).
pub fn publish(app: &AppHandle, next: LegacyReliabilityStatus) -> bool {
    publish_inner(
        app,
        StatusFence::ReplaceAny,
        RecoveryUpdate::Lifecycle,
        move |_| next,
    )
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
        StatusFence::ExactOwner(StatusOwner::new(session_id, sensor_generation)),
        RecoveryUpdate::Lifecycle,
        move |_| next,
    )
}

pub fn publish_snapshot_if_owned(app: &AppHandle, snapshot: &ObserveOnlySnapshot) -> bool {
    let state = app.state::<AppState>();
    let owner = StatusOwner::new(
        snapshot.session.session_id,
        snapshot.session.sensor_generation,
    );
    let prepared = prepare_recovery(&state, snapshot);
    let update = RecoveryUpdate::Snapshot {
        session_id: snapshot.session.session_id,
        prepared: prepared.map(Box::new),
        now_ms: state.legacy_monotonic_ms(),
    };
    let next = LegacyReliabilityStatus::from_snapshot(snapshot);
    publish_inner(app, StatusFence::ExactOwner(owner), update, move |_| next)
}

struct PreparedRecovery {
    observation: IncidentObservation,
    fence: IntentFence,
    previous: RecoveryConfig,
    previous_owner: ProcessOwner,
    candidates: Vec<RecoveryConfig>,
}

enum RecoveryUpdate {
    Lifecycle,
    OverlayOnly,
    Snapshot {
        session_id: SessionId,
        prepared: Option<Box<PreparedRecovery>>,
        now_ms: u64,
    },
}

fn prepare_recovery(state: &AppState, snapshot: &ObserveOnlySnapshot) -> Option<PreparedRecovery> {
    if snapshot.session.closed || snapshot.health.state != EyeHealthState::Ready {
        return None;
    }
    let PresumedIntent::SwitchLane {
        category,
        candidate_config,
        ..
    } = &snapshot.presumed_intent
    else {
        return None;
    };
    let lane = snapshot
        .lanes
        .iter()
        .find(|lane| lane.category == *category)?;
    let active_config = snapshot.active_configs.get(category)?;
    let registry = state
        .legacy_manager
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_ref()?
        .registry();
    if registry.version() != snapshot.session.target_registry_version {
        return None;
    }

    let runtime = state
        .dpi
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .snapshot_legacy_category(category)
        .ok()?;
    if runtime.owner.config_file != *active_config
        || runtime.owner.lane_generation != lane.lane_generation
    {
        return None;
    }
    let active_fingerprint = registry
        .config_fingerprint(category, active_config)
        .ok()?
        .as_hex()
        .to_owned();
    if runtime.owner.config_fingerprint != active_fingerprint {
        return None;
    }

    let mut candidate_ids = Vec::new();
    for candidate in std::iter::once(candidate_config).chain(
        snapshot
            .candidate_configs
            .get(category)
            .into_iter()
            .flatten(),
    ) {
        if candidate != active_config && !candidate_ids.contains(candidate) {
            candidate_ids.push(candidate.clone());
        }
    }
    let mut candidates = candidate_ids
        .into_iter()
        .filter_map(|candidate| {
            let fingerprint = registry
                .config_fingerprint(category, &candidate)
                .ok()?
                .as_hex()
                .to_owned();
            Some(RecoveryConfig::new(candidate, fingerprint))
        })
        .collect::<Vec<_>>();
    let stable_network = snapshot.session.network_fingerprint_at_start.stable_key()?;
    let now_unix = unix_now_secs();
    {
        let cache = state
            .legacy_trust_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        candidates.retain(|candidate| {
            cache
                .cooldown_until(
                    stable_network,
                    category,
                    candidate.fingerprint().as_str(),
                    now_unix,
                )
                .is_none()
        });
        let trusted = cache.fresh_trusted_candidates(stable_network, category, now_unix);
        rank_candidates_by_trusted_fingerprint(&mut candidates, &trusted);
    }
    if candidates.is_empty() {
        return None;
    }

    Some(PreparedRecovery {
        observation: IncidentObservation {
            session_id: snapshot.session.session_id,
            sensor_generation: snapshot.session.sensor_generation,
            category: category.clone(),
            lane_generation: lane.lane_generation,
            evidence_epoch: lane.evidence_epoch,
            classification: lane.classification,
        },
        fence: IntentFence {
            session_id: snapshot.session.session_id,
            category: category.clone(),
            lane_generation: lane.lane_generation,
            sensor_generation: snapshot.session.sensor_generation,
            registry_version: snapshot.session.target_registry_version,
            network_fingerprint: snapshot.session.network_fingerprint_at_start.clone(),
        },
        previous: RecoveryConfig::new(active_config.clone(), active_fingerprint.clone()),
        previous_owner: ProcessOwner {
            pid: runtime.owner.pid,
            process_start_identity: ProcessStartIdentity::new(runtime.owner.process_identity.get()),
            config_fingerprint: ConfigFingerprint::new(active_fingerprint),
            lane_generation: runtime.owner.lane_generation,
        },
        candidates,
    })
}

fn rank_candidates_by_trusted_fingerprint(
    candidates: &mut [RecoveryConfig],
    trusted: &[super::cache::TrustedCandidate],
) {
    candidates.sort_by_key(|candidate| {
        trusted
            .iter()
            .position(|entry| entry.config_fingerprint == candidate.fingerprint().as_str())
            .unwrap_or(usize::MAX)
    });
}

fn unix_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn recovery_mode_from_settings(settings: &crate::settings::Settings) -> RecoveryMode {
    match settings.legacy_reliability_mode.as_str() {
        "assisted" => RecoveryMode::Assisted,
        "automatic" => RecoveryMode::Automatic,
        _ => RecoveryMode::ObserveOnly,
    }
}

fn apply_recovery_settings(
    recovery: &mut super::recovery_runtime::LegacyRecoveryRuntime,
    settings: &crate::settings::Settings,
) {
    recovery.set_mode(recovery_mode_from_settings(settings));
    recovery.set_automatic_paused(settings.legacy_automatic_paused);
    recovery.set_manual_frozen_categories(
        settings
            .legacy_reliability_frozen_categories
            .iter()
            .cloned(),
    );
}

pub fn refresh_recovery_overlay(app: &AppHandle) -> bool {
    publish_inner(
        app,
        StatusFence::PreserveCurrent,
        RecoveryUpdate::OverlayOnly,
        LegacyReliabilityStatus::clone,
    )
}

/// Consumes only the opaque UI token and reconstructs every safety-sensitive
/// field from the current backend Manager snapshot.
pub fn approve_pending(
    app: &AppHandle,
    approval: AssistedApproval,
) -> Result<RecoveryAction, String> {
    let state = app.state::<AppState>();
    if state
        .shutting_down
        .load(std::sync::atomic::Ordering::SeqCst)
    {
        return Err("Приложение завершает работу".into());
    }
    let proposal = state
        .legacy_recovery
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .status()
        .proposal
        .ok_or_else(|| "Предложение уже недоступно".to_owned())?;
    let snapshot = state
        .legacy_manager
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_ref()
        .map(super::runtime::LegacyReliabilityHandle::snapshot)
        .ok_or_else(|| "Legacy Manager не запущен".to_owned())?;
    if snapshot.session.closed || snapshot.health.state != EyeHealthState::Ready {
        return Err("Наблюдение сейчас ненадёжно; предложение отменено".into());
    }
    let current_fence = super::executor::intent_fence_from_snapshot(&snapshot, &proposal.category)
        .ok_or_else(|| "Категория предложения больше не активна".to_owned())?;
    let now_ms = state.legacy_monotonic_ms();
    // Settings -> recovery is the canonical mode/approval order. A concurrent
    // downgrade therefore either revokes this proposal first or linearizes
    // after approval (an already active attempt still finishes safely).
    let settings = state
        .settings
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let action = {
        let mut recovery = state
            .legacy_recovery
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        apply_recovery_settings(&mut recovery, &settings);
        recovery
            .approve(approval, &current_fence, now_ms)
            .map_err(|error| format!("Предложение отклонено: {error:?}"))?
    };
    drop(settings);
    refresh_recovery_overlay(app);
    Ok(action)
}

pub(crate) fn current_owner(app: &AppHandle) -> Option<StatusOwner> {
    app.state::<AppState>()
        .legacy_reliability_status
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .owner()
}

pub(crate) fn publish_blind_if_owner(app: &AppHandle, owner: StatusOwner) -> bool {
    publish_inner(
        app,
        StatusFence::ExactOwner(owner),
        RecoveryUpdate::Lifecycle,
        |current| {
            let mut next = current.clone();
            next.normalize_blind();
            next
        },
    )
}

/// Marks the currently owned session terminally unavailable without allowing a
/// stale process monitor to target a later session. The exact owner captured
/// here is rechecked by `publish_inner` before the transition is committed.
pub fn publish_current_blind(app: &AppHandle) -> bool {
    current_owner(app).is_some_and(|owner| publish_blind_if_owner(app, owner))
}

fn publish_inner<BuildNext>(
    app: &AppHandle,
    fence: StatusFence,
    recovery_update: RecoveryUpdate,
    build_next: BuildNext,
) -> bool
where
    BuildNext: FnOnce(&LegacyReliabilityStatus) -> LegacyReliabilityStatus,
{
    let state = app.state::<AppState>();
    // A tentative observer registry points at the candidate before the old
    // process is stopped. Public "active config" must instead reflect the
    // exact process-backed DpiState at every recovery phase.
    let live_configs =
        {
            let dpi = state
                .dpi
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match dpi.active_launch.as_ref() {
                Some(crate::state::DpiLaunchSpec::Legacy { selections }) => selections
                    .iter()
                    .cloned()
                    .collect::<std::collections::BTreeMap<_, _>>(),
                _ => std::collections::BTreeMap::new(),
            }
        };
    // Keep the settings snapshot locked through status -> recovery commit so
    // an older Assisted read cannot overwrite a concurrent ObserveOnly
    // downgrade after that downgrade has already refreshed the coordinator.
    let settings = state
        .settings
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut automatic_action = None;

    let Some(section) = transition_with(
        &state.legacy_reliability_status,
        &state.legacy_reliability_revision,
        fence,
        build_next,
        |next| {
            for lane in &mut next.lanes {
                lane.active_config = live_configs.get(&lane.category).cloned();
            }

            // `transition_with` holds the status guard and has already checked
            // the owner/revision fence. This status -> recovery lock order is
            // process-wide; recovery callers release their guard before asking
            // for an overlay refresh.
            let recovery_status = {
                let mut recovery = state
                    .legacy_recovery
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                match recovery_update {
                    RecoveryUpdate::Lifecycle => {
                        apply_recovery_settings(&mut recovery, &settings);
                        if matches!(
                            next.phase,
                            LegacyReliabilityPhase::Inactive
                                | LegacyReliabilityPhase::Degraded
                                | LegacyReliabilityPhase::Blind
                        ) {
                            recovery.cancel_pending();
                        }
                    }
                    RecoveryUpdate::OverlayOnly => {
                        apply_recovery_settings(&mut recovery, &settings)
                    }
                    RecoveryUpdate::Snapshot {
                        session_id,
                        prepared,
                        now_ms,
                    } => {
                        recovery.observe_session(session_id);
                        apply_recovery_settings(&mut recovery, &settings);
                        if let Some(prepared) = prepared {
                            let prepared = *prepared;
                            if let Ok(RecoveryDecision::Automatic { action }) = recovery
                                .consider_with_control_generation(
                                    prepared.observation,
                                    prepared.fence,
                                    prepared.previous,
                                    prepared.previous_owner,
                                    prepared.candidates,
                                    state.legacy_automation_revision.current(),
                                    now_ms,
                                )
                            {
                                automatic_action = Some(action);
                            }
                        } else {
                            // A proposal is useful only while the exact adverse
                            // assessment still owns the lane. Active attempts
                            // are intentionally unaffected.
                            recovery.cancel_pending();
                        }
                    }
                }
                recovery.status()
            };
            next.apply_recovery(recovery_status, state.legacy_monotonic_ms());
        },
    ) else {
        return false;
    };
    drop(settings);
    let _ = app.emit(STATUS_EVENT, section);
    if let Some(action) = automatic_action {
        spawn_automatic_recovery(app, action);
    }
    true
}

fn spawn_automatic_recovery(app: &AppHandle, initial: RecoveryAction) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        match super::executor::run_scoped_recovery(&app, initial).await {
            Ok(RecoveryAction::Complete { .. } | RecoveryAction::ManualIntervention { .. }) => {}
            Ok(_) => crate::util::emit_log(
                &app,
                "error",
                "legacy-reliability",
                "Automatic recovery завершился без terminal результата",
            ),
            Err(error) => crate::util::emit_log(
                &app,
                "error",
                "legacy-reliability",
                &format!("Automatic recovery failed: {error}"),
            ),
        }
        refresh_recovery_overlay(&app);
    });
}

#[cfg(test)]
fn transition(
    current: &Mutex<LegacyReliabilityStatus>,
    revision: &RevisionClock,
    fence: StatusFence,
    next: LegacyReliabilityStatus,
) -> Option<VersionedSection<LegacyReliabilityStatus>> {
    transition_with(current, revision, fence, move |_| next, |_| {})
}

fn transition_with<BuildNext, PrepareNext>(
    current: &Mutex<LegacyReliabilityStatus>,
    revision: &RevisionClock,
    fence: StatusFence,
    build_next: BuildNext,
    prepare_next: PrepareNext,
) -> Option<VersionedSection<LegacyReliabilityStatus>>
where
    BuildNext: FnOnce(&LegacyReliabilityStatus) -> LegacyReliabilityStatus,
    PrepareNext: FnOnce(&mut LegacyReliabilityStatus),
{
    let mut guard = current
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut next = build_next(&guard);
    if !fence.allows(&guard, &next) {
        return None;
    }
    // Safety-sensitive side effects (proposal reconciliation/cancellation)
    // happen only after the exact fence succeeds and while the same status
    // guard prevents lifecycle replacement.
    prepare_next(&mut next);
    if *guard == next {
        return None;
    }
    *guard = next.clone();
    let revision = revision.bump();
    Some(VersionedSection::new(revision, next))
}

impl StatusFence {
    fn allows(self, current: &LegacyReliabilityStatus, next: &LegacyReliabilityStatus) -> bool {
        let exact_owner_is_allowed = |owner| {
            current.owner() == Some(owner)
                && next.owner() == Some(owner)
                && !(current.phase == LegacyReliabilityPhase::Blind
                    && next.phase != LegacyReliabilityPhase::Blind)
        };
        match self {
            Self::ReplaceAny => true,
            Self::ExactOwner(owner) => exact_owner_is_allowed(owner),
            Self::PreserveCurrent => current == next,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
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
            pending_ingress_control_events: 0,
            pending_ingress_flow_events: BTreeMap::new(),
            last_evicted_confirmation_flow_sequences: BTreeMap::new(),
            lanes: Vec::new(),
            presumed_intent: PresumedIntent::default(),
            active_configs: BTreeMap::new(),
            candidate_configs: BTreeMap::new(),
            gate_probe_hosts: BTreeMap::new(),
            confirmation_flows: Vec::new(),
        }
    }

    #[test]
    fn trusted_ranking_follows_exact_fingerprint_across_rename_and_keeps_duplicate_order() {
        let mut candidates = vec![
            RecoveryConfig::new("bundled-first.conf", "other-fingerprint"),
            RecoveryConfig::new("renamed-b.conf", "trusted-fingerprint"),
            RecoveryConfig::new("renamed-a.conf", "trusted-fingerprint"),
        ];
        let trusted = vec![super::super::cache::TrustedCandidate {
            config_id: "old-name.conf".into(),
            config_fingerprint: "trusted-fingerprint".into(),
            last_success_at: 100,
            success_count: 2,
        }];

        rank_candidates_by_trusted_fingerprint(&mut candidates, &trusted);

        assert_eq!(
            candidates
                .iter()
                .map(RecoveryConfig::config_id)
                .collect::<Vec<_>>(),
            vec!["renamed-b.conf", "renamed-a.conf", "bundled-first.conf"]
        );
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
                },
                "proposal": null,
                "activeAttempt": null,
                "lastCompletion": null,
                "negativeCooldownCount": 0,
                "automaticPaused": true,
                "automaticPacingRemainingMs": null,
                "frozenCategories": [],
                "haltedCategories": []
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

        let first = transition(
            &current,
            &revision,
            StatusFence::ReplaceAny,
            starting.clone(),
        )
        .unwrap();
        assert_eq!(first.revision, 1);
        assert!(transition(&current, &revision, StatusFence::ReplaceAny, starting,).is_none());
        assert_eq!(revision.current(), 1);

        let stale = LegacyReliabilityStatus::blind(
            vec!["discord".into()],
            SessionId::new(4),
            SensorGeneration::new(6),
        );
        assert!(transition(
            &current,
            &revision,
            StatusFence::ExactOwner(StatusOwner::new(
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
            StatusFence::ExactOwner(StatusOwner::new(session, sensor)),
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
            StatusFence::ExactOwner(StatusOwner::new(session, sensor)),
            stale_ready,
        )
        .is_none());
        assert_eq!(revision.current(), 2);

        let inactive = transition(
            &current,
            &revision,
            StatusFence::ReplaceAny,
            LegacyReliabilityStatus::inactive(),
        )
        .unwrap();
        assert_eq!(inactive.revision, 3);

        let late_snapshot =
            LegacyReliabilityStatus::from_snapshot(&snapshot(EyeHealthState::Ready));
        assert!(transition(
            &current,
            &revision,
            StatusFence::ExactOwner(StatusOwner::new(
                SessionId::new(11),
                SensorGeneration::new(17)
            )),
            late_snapshot,
        )
        .is_none());
        assert_eq!(revision.current(), 3);
    }

    #[test]
    fn rejected_owned_transition_does_not_run_recovery_mutator() {
        let session = SessionId::new(3);
        let sensor = SensorGeneration::new(5);
        let current = Mutex::new(LegacyReliabilityStatus::starting(
            vec!["discord".into()],
            session,
            sensor,
        ));
        let revision = RevisionClock::default();
        revision.bump();
        let mutated = Cell::new(false);

        let stale = LegacyReliabilityStatus::blind(
            vec!["discord".into()],
            SessionId::new(4),
            SensorGeneration::new(6),
        );
        assert!(transition_with(
            &current,
            &revision,
            StatusFence::ExactOwner(StatusOwner::new(
                SessionId::new(4),
                SensorGeneration::new(6),
            )),
            move |_| stale,
            |_| mutated.set(true),
        )
        .is_none());

        assert!(!mutated.get());
        assert_eq!(revision.current(), 1);
        assert_eq!(
            current.lock().unwrap().owner(),
            Some(StatusOwner::new(session, sensor))
        );
    }

    #[test]
    fn preserve_current_refresh_cannot_replace_ownerless_or_newer_lifecycle_state() {
        let session = SessionId::new(3);
        let sensor = SensorGeneration::new(5);
        let current = Mutex::new(LegacyReliabilityStatus::starting(
            vec!["discord".into()],
            session,
            sensor,
        ));
        let revision = RevisionClock::default();
        let mutated = Cell::new(false);

        assert!(transition_with(
            &current,
            &revision,
            StatusFence::PreserveCurrent,
            |_| LegacyReliabilityStatus::inactive(),
            |_| mutated.set(true),
        )
        .is_none());
        assert!(!mutated.get());

        let section = transition_with(
            &current,
            &revision,
            StatusFence::PreserveCurrent,
            LegacyReliabilityStatus::clone,
            |next| next.mode = LegacyReliabilityMode::Assisted,
        )
        .unwrap();
        assert_eq!(section.value.phase, LegacyReliabilityPhase::Starting);
        assert_eq!(
            section.value.owner(),
            Some(StatusOwner::new(session, sensor))
        );
        assert_eq!(section.value.mode, LegacyReliabilityMode::Assisted);
        assert_eq!(revision.current(), 1);
    }
}
