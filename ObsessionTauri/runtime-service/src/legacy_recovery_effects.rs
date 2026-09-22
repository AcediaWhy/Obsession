//! Fail-closed Windows effects for the staged Legacy recovery transaction.
//!
//! Exact coordinator action, protected plans, machine network, observer fence
//! and Job-owned process identity are rechecked immediately around the
//! tentative observer replacement. Later stages can only consume the retained
//! exact state.

#![cfg(windows)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Instant;

use obsession_runtime_protocol::{DpiCategory, DpiRuntimeSnapshot, DpiSelection};
use obsession_runtime_reliability::legacy_reliability::contracts::{
    ConfirmationFailure, ExecutorOutcome, ExecutorStage, IntentEnvelope, IntentFence,
    LaneGeneration, ProcessOwner, ProcessStartIdentity,
};
use obsession_runtime_reliability::legacy_reliability::environment_gate::{
    EndpointProbeBackend, EndpointProbeResult, LocalNetworkSnapshot, ReqwestProbeBackend,
    ENDPOINT_TIMEOUT,
};
use obsession_runtime_reliability::legacy_reliability::recovery::{
    RecoveryAction, RecoveryConfig, RecoveryOrigin,
};

use crate::dpi_executor::{
    DpiExecutor, ExecutorError, ProcessError, ProcessIdentity, RuntimeProcessLauncher,
};
use crate::legacy_confirmation::{
    LegacyConfirmationDecision, LegacyConfirmationWindow, LegacyHttpsProbeObservation,
    LegacyHttpsProbeResult, LEGACY_CONFIRMATION_DEADLINE_MS,
};
use crate::legacy_recovery_preflight::{prepared_matches_preflight_action, PreparedLegacyRecovery};
use crate::legacy_recovery_transaction::{
    LegacyRecoveryEffectError, LegacyRecoveryEffects, LegacyRecoveryStageEffect,
};
use crate::legacy_reliability::{
    confirmation_candidate_targets, LegacyObserverRuntime, LegacyObserverStartPlan,
};
use crate::protected_layout::VerifiedDpiPlan;

pub(crate) trait LegacyRecoveryObserverPort {
    fn runtime_generation(&self) -> Option<u64>;
    fn current_fence(&self, category: &str) -> Option<IntentFence>;
    fn confirmation_snapshot(
        &mut self,
    ) -> Option<obsession_runtime_reliability::legacy_reliability::manager::ObserveOnlySnapshot>;
    fn confirmation_targets(
        &self,
        category: &str,
        candidate: &RecoveryConfig,
    ) -> Option<Vec<String>>;
    fn stop(&mut self, generation: u64) -> Result<(), ()>;
    fn start_recovery(
        &mut self,
        plan: LegacyObserverStartPlan,
        selections: &[DpiSelection],
    ) -> Result<(), ()>;
    fn rebind_generation(&mut self, expected: u64, replacement: u64) -> Result<(), ()>;
}

impl LegacyRecoveryObserverPort for LegacyObserverRuntime {
    fn runtime_generation(&self) -> Option<u64> {
        self.recovery_scan().map(|scan| scan.generation)
    }

    fn current_fence(&self, category: &str) -> Option<IntentFence> {
        LegacyObserverRuntime::current_fence(self, category)
    }

    fn confirmation_snapshot(
        &mut self,
    ) -> Option<obsession_runtime_reliability::legacy_reliability::manager::ObserveOnlySnapshot>
    {
        LegacyObserverRuntime::snapshot(self)
    }

    fn confirmation_targets(
        &self,
        category: &str,
        candidate: &RecoveryConfig,
    ) -> Option<Vec<String>> {
        confirmation_candidate_targets(self, category, candidate)
    }

    fn stop(&mut self, generation: u64) -> Result<(), ()> {
        LegacyObserverRuntime::stop(self, generation).map_err(|_| ())
    }

    fn start_recovery(
        &mut self,
        plan: LegacyObserverStartPlan,
        selections: &[DpiSelection],
    ) -> Result<(), ()> {
        LegacyObserverRuntime::start_recovery(self, plan, selections).map_err(|_| ())
    }

    fn rebind_generation(&mut self, expected: u64, replacement: u64) -> Result<(), ()> {
        LegacyObserverRuntime::rebind_generation(self, expected, replacement).map_err(|_| ())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LegacyRuntimeStartError {
    ProtectedResourceChanged,
    ReadinessFailed,
    RuntimeFailure,
}

pub(crate) trait LegacyRecoveryProcessView {
    fn runtime_snapshot(&self) -> Option<DpiRuntimeSnapshot>;
    fn process_identities(&self) -> Vec<ProcessIdentity>;
    fn stop_runtime(&mut self, generation: u64) -> Result<(), ()>;
    fn start_runtime(&mut self, plan: &VerifiedDpiPlan) -> Result<u64, LegacyRuntimeStartError>;
}

impl<L: RuntimeProcessLauncher> LegacyRecoveryProcessView for DpiExecutor<L> {
    fn runtime_snapshot(&self) -> Option<DpiRuntimeSnapshot> {
        self.snapshot()
    }

    fn process_identities(&self) -> Vec<ProcessIdentity> {
        self.active_processes().to_vec()
    }

    fn stop_runtime(&mut self, generation: u64) -> Result<(), ()> {
        self.stop(generation).map_err(|_| ())
    }

    fn start_runtime(&mut self, plan: &VerifiedDpiPlan) -> Result<u64, LegacyRuntimeStartError> {
        self.start(plan)
            .map(|started| started.generation)
            .map_err(|error| match error {
                ExecutorError::Materialization(_) => {
                    LegacyRuntimeStartError::ProtectedResourceChanged
                }
                ExecutorError::Process(ProcessError::ExitedBeforeReady { .. }) => {
                    LegacyRuntimeStartError::ReadinessFailed
                }
                ExecutorError::Busy
                | ExecutorError::Conflict
                | ExecutorError::InvalidOwnership
                | ExecutorError::Process(_) => LegacyRuntimeStartError::RuntimeFailure,
            })
    }
}

pub(crate) trait LegacyRecoveryNetworkPort {
    fn snapshot(&mut self) -> LocalNetworkSnapshot;
}

/// Bounded asynchronous HTTPS I/O boundary. The effects adapter only consumes
/// typed observations; socket ownership and timeouts stay in the production
/// implementation added at the service wiring layer.
pub(crate) trait LegacyRecoveryProbePort {
    fn arm(&mut self, targets: &[String], deadline_at_monotonic_ms: u64);
    fn take_observations(&mut self, now_ms: u64) -> Vec<LegacyHttpsProbeObservation>;
    fn cancel(&mut self);
}

pub(crate) struct MachineNetwork;

impl LegacyRecoveryNetworkPort for MachineNetwork {
    fn snapshot(&mut self) -> LocalNetworkSnapshot {
        crate::network_identity::snapshot()
    }
}

/// Production probe runner. Each arm creates at most two bounded workers. A
/// cancellation token prevents observations from an older attempt crossing
/// into a newer transaction even if a socket finishes during teardown.
pub(crate) struct MachineLegacyRecoveryProbes {
    backend: Arc<ReqwestProbeBackend>,
    receiver: Option<mpsc::Receiver<LegacyHttpsProbeObservation>>,
    cancelled: Option<Arc<AtomicBool>>,
}

impl MachineLegacyRecoveryProbes {
    pub(crate) fn new() -> Result<Self, ()> {
        Ok(Self {
            backend: Arc::new(ReqwestProbeBackend::new().map_err(|_| ())?),
            receiver: None,
            cancelled: None,
        })
    }
}

impl LegacyRecoveryProbePort for MachineLegacyRecoveryProbes {
    fn arm(&mut self, targets: &[String], deadline_at_monotonic_ms: u64) {
        self.cancel();
        let armed_at_monotonic_ms =
            deadline_at_monotonic_ms.saturating_sub(LEGACY_CONFIRMATION_DEADLINE_MS);
        let mut probe_targets = targets.iter().take(2).cloned().collect::<Vec<_>>();
        if probe_targets.len() == 1 {
            probe_targets.push(probe_targets[0].clone());
        }
        let (sender, receiver) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let armed_at = Instant::now();
        for (index, target) in probe_targets.into_iter().enumerate() {
            let backend = Arc::clone(&self.backend);
            let sender = sender.clone();
            let cancelled = Arc::clone(&cancelled);
            let armed_at = armed_at;
            let _ = std::thread::Builder::new()
                .name(format!("obsession-confirm-{}", index + 1))
                .spawn(move || {
                    let started_at_monotonic_ms =
                        armed_at_monotonic_ms.saturating_add(elapsed_ms(armed_at));
                    let endpoint =
                        format!("https://{target}/?obsession_recovery_probe={}", index + 1);
                    let result = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .ok()
                        .map(|runtime| runtime.block_on(backend.probe(endpoint, ENDPOINT_TIMEOUT)));
                    if cancelled.load(Ordering::Acquire) {
                        return;
                    }
                    let finished_at_monotonic_ms =
                        armed_at_monotonic_ms.saturating_add(elapsed_ms(armed_at));
                    let result = match result {
                        Some(outcome)
                            if outcome.result == EndpointProbeResult::Succeeded
                                && outcome.http_status.is_some() =>
                        {
                            LegacyHttpsProbeResult::HttpResponse {
                                status: outcome.http_status.unwrap_or(0),
                            }
                        }
                        Some(_) | None => LegacyHttpsProbeResult::TargetFailure,
                    };
                    let _ = sender.send(LegacyHttpsProbeObservation {
                        probe_id: index as u64 + 1,
                        domain: target,
                        started_at_monotonic_ms,
                        finished_at_monotonic_ms,
                        result,
                    });
                });
        }
        drop(sender);
        self.receiver = Some(receiver);
        self.cancelled = Some(cancelled);
    }

    fn take_observations(&mut self, _now_ms: u64) -> Vec<LegacyHttpsProbeObservation> {
        self.receiver
            .as_ref()
            .map(|receiver| receiver.try_iter().collect())
            .unwrap_or_default()
    }

    fn cancel(&mut self) {
        if let Some(cancelled) = self.cancelled.take() {
            cancelled.store(true, Ordering::Release);
        }
        self.receiver = None;
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

#[derive(Clone)]
struct ArmedLegacyRecoveryEffect {
    original_envelope: IntentEnvelope,
    current_envelope: IntentEnvelope,
    previous: RecoveryConfig,
    previous_owner: ProcessOwner,
    candidate: RecoveryConfig,
    origin: RecoveryOrigin,
    runtime: DpiRuntimeSnapshot,
    processes: Vec<ProcessIdentity>,
    candidate_runtime: Option<DpiRuntimeSnapshot>,
    candidate_processes: Vec<ProcessIdentity>,
    candidate_owner: Option<ProcessOwner>,
    confirmation: Option<LegacyConfirmationWindow>,
    confirmation_succeeded: bool,
}

#[derive(Default)]
pub(crate) struct ProtectedLegacyRecoveryEffectState {
    active: Option<ArmedLegacyRecoveryEffect>,
}

impl ProtectedLegacyRecoveryEffectState {
    pub(crate) fn is_idle(&self) -> bool {
        self.active.is_none()
    }
}

pub(crate) struct ProtectedLegacyRecoveryEffects<'a, O, E, N, P> {
    prepared: &'a PreparedLegacyRecovery,
    observer: &'a mut O,
    executor: &'a mut E,
    network: &'a mut N,
    probes: &'a mut P,
    active: Option<ArmedLegacyRecoveryEffect>,
}

impl<'a, O, E, N, P> ProtectedLegacyRecoveryEffects<'a, O, E, N, P>
where
    O: LegacyRecoveryObserverPort,
    E: LegacyRecoveryProcessView,
    N: LegacyRecoveryNetworkPort,
    P: LegacyRecoveryProbePort,
{
    pub(crate) fn new(
        prepared: &'a PreparedLegacyRecovery,
        observer: &'a mut O,
        executor: &'a mut E,
        network: &'a mut N,
        probes: &'a mut P,
    ) -> Self {
        Self {
            prepared,
            observer,
            executor,
            network,
            probes,
            active: None,
        }
    }

    pub(crate) fn from_state(
        prepared: &'a PreparedLegacyRecovery,
        observer: &'a mut O,
        executor: &'a mut E,
        network: &'a mut N,
        probes: &'a mut P,
        state: ProtectedLegacyRecoveryEffectState,
    ) -> Self {
        Self {
            prepared,
            observer,
            executor,
            network,
            probes,
            active: state.active,
        }
    }

    pub(crate) fn into_state(self) -> ProtectedLegacyRecoveryEffectState {
        ProtectedLegacyRecoveryEffectState {
            active: self.active,
        }
    }

    /// Contains an unexpected transaction-driver failure without trusting the
    /// coordinator to produce another action. If a candidate attempt was
    /// armed, restore the previous protected plan under a fresh lane epoch;
    /// either way release all effect-owned probes and rollback state so the
    /// caller can enter a terminal blocked state.
    pub(crate) fn emergency_rollback(&mut self) -> bool {
        self.probes.cancel();
        let Some(active) = self.active.as_ref().cloned() else {
            return true;
        };
        let envelope = active.current_envelope.clone();
        let previous_lane_generation = LaneGeneration::new(next_nonzero_generation(
            envelope.expected_lane_generation.get(),
        ));
        let restored = self.perform_rollback(&active, &envelope, previous_lane_generation);
        self.active = None;
        restored.is_some()
    }

    fn execute_preflight(
        &mut self,
        envelope: &IntentEnvelope,
        previous: &RecoveryConfig,
        previous_owner: &ProcessOwner,
        candidate: &RecoveryConfig,
        origin: RecoveryOrigin,
    ) -> LegacyRecoveryStageEffect {
        if self.active.is_some()
            || !prepared_matches_preflight_action(
                self.prepared,
                envelope,
                previous,
                previous_owner,
                candidate,
            )
            || self.prepared.exclusive_target_count() == 0
            || self.prepared.previous_plan().reverify().is_err()
            || self.prepared.candidate_plan().reverify().is_err()
            || !network_matches(envelope, &self.network.snapshot())
        {
            return completed(ExecutorOutcome::PreflightRejected);
        }

        let Some(runtime) = self.executor.runtime_snapshot() else {
            return previous_missing(previous);
        };
        let processes = self.executor.process_identities();
        if runtime.generation != self.prepared.observer_plan().runtime_generation()
            || runtime.selections != self.prepared.previous_selections()
            || self.observer.runtime_generation() != Some(runtime.generation)
            || self.observer.current_fence(&envelope.category).as_ref()
                != Some(&fence_from_envelope(envelope))
        {
            return completed(ExecutorOutcome::PreflightRejected);
        }
        if exact_owner(
            &runtime,
            &processes,
            &envelope.category,
            previous,
            envelope.expected_lane_generation,
        )
        .as_ref()
            != Some(previous_owner)
        {
            return previous_missing(previous);
        }

        if self.observer.stop(runtime.generation).is_err() {
            return rollback_failed(previous);
        }
        if self
            .observer
            .start_recovery(
                self.prepared.observer_plan().clone(),
                self.prepared.candidate_selections(),
            )
            .is_err()
        {
            return if self.restore_previous_observer(envelope, &runtime) {
                completed(ExecutorOutcome::PreflightRejected)
            } else {
                rollback_failed(previous)
            };
        }

        let refreshed_fence = self.observer.current_fence(&envelope.category);
        let post_is_exact = refreshed_fence.as_ref().is_some_and(|refreshed| {
            refreshed.session_id == envelope.session_id
                && refreshed.category == envelope.category
                && refreshed.lane_generation
                    == self
                        .prepared
                        .observer_plan()
                        .lane_generation(&envelope.category)
                        .unwrap_or(envelope.expected_lane_generation)
                && refreshed.sensor_generation == self.prepared.observer_plan().sensor_generation()
                && refreshed.sensor_generation != envelope.expected_sensor_generation
                && refreshed.registry_version != envelope.expected_registry_version
                && refreshed.network_fingerprint == envelope.expected_network_fingerprint
        }) && network_matches(envelope, &self.network.snapshot())
            && self.prepared.previous_plan().reverify().is_ok()
            && self.prepared.candidate_plan().reverify().is_ok()
            && self.executor.runtime_snapshot().as_ref() == Some(&runtime)
            && self.executor.process_identities() == processes
            && exact_owner(
                &runtime,
                &processes,
                &envelope.category,
                previous,
                envelope.expected_lane_generation,
            )
            .as_ref()
                == Some(previous_owner);

        let Some(refreshed_fence) = refreshed_fence.filter(|_| post_is_exact) else {
            return if self.restore_previous_observer(envelope, &runtime) {
                completed(ExecutorOutcome::ExecutionAborted {
                    stage: ExecutorStage::Preflight,
                    reason: ConfirmationFailure::Sensor,
                })
            } else {
                rollback_failed(previous)
            };
        };

        self.active = Some(ArmedLegacyRecoveryEffect {
            original_envelope: envelope.clone(),
            current_envelope: IntentEnvelope::from_fence(envelope.attempt_id, &refreshed_fence),
            previous: previous.clone(),
            previous_owner: previous_owner.clone(),
            candidate: candidate.clone(),
            origin,
            runtime,
            processes,
            candidate_runtime: None,
            candidate_processes: Vec::new(),
            candidate_owner: None,
            confirmation: None,
            confirmation_succeeded: false,
        });
        completed(ExecutorOutcome::PreflightPassed { refreshed_fence })
    }

    fn execute_stop(
        &mut self,
        envelope: &IntentEnvelope,
        previous: &RecoveryConfig,
        previous_owner: &ProcessOwner,
        origin: RecoveryOrigin,
    ) -> Result<LegacyRecoveryStageEffect, LegacyRecoveryEffectError> {
        let active = self
            .active
            .as_ref()
            .cloned()
            .ok_or(LegacyRecoveryEffectError::InvalidAction)?;
        if active.current_envelope != *envelope
            || active.previous != *previous
            || active.previous_owner != *previous_owner
            || active.origin != origin
        {
            return Err(LegacyRecoveryEffectError::InvalidAction);
        }
        let exact = network_matches(envelope, &self.network.snapshot())
            && self.prepared.previous_plan().reverify().is_ok()
            && self.prepared.candidate_plan().reverify().is_ok()
            && self.executor.runtime_snapshot().as_ref() == Some(&active.runtime)
            && self.executor.process_identities() == active.processes
            && self.observer.runtime_generation() == Some(active.runtime.generation)
            && self.observer.current_fence(&envelope.category).as_ref()
                == Some(&fence_from_envelope(envelope))
            && exact_owner(
                &active.runtime,
                &active.processes,
                &envelope.category,
                previous,
                previous_owner.lane_generation,
            )
            .as_ref()
                == Some(previous_owner);
        if !exact {
            self.active = None;
            return Ok(
                if self.restore_previous_observer(&active.original_envelope, &active.runtime) {
                    completed(ExecutorOutcome::ExecutionAborted {
                        stage: ExecutorStage::Stop,
                        reason: ConfirmationFailure::Sensor,
                    })
                } else {
                    rollback_failed(previous)
                },
            );
        }

        let stop_succeeded = self
            .executor
            .stop_runtime(active.runtime.generation)
            .is_ok();
        let runtime_after_stop = self.executor.runtime_snapshot();
        let processes_after_stop = self.executor.process_identities();
        if !stop_succeeded || runtime_after_stop.is_some() || !processes_after_stop.is_empty() {
            let previous_is_intact = runtime_after_stop.as_ref() == Some(&active.runtime)
                && processes_after_stop == active.processes
                && exact_owner(
                    &active.runtime,
                    &processes_after_stop,
                    &envelope.category,
                    previous,
                    previous_owner.lane_generation,
                )
                .as_ref()
                    == Some(previous_owner);
            self.active = None;
            return Ok(
                if previous_is_intact
                    && self.restore_previous_observer(&active.original_envelope, &active.runtime)
                {
                    completed(ExecutorOutcome::ExecutionAborted {
                        stage: ExecutorStage::Stop,
                        reason: ConfirmationFailure::Sensor,
                    })
                } else {
                    rollback_failed(previous)
                },
            );
        }
        Ok(completed(ExecutorOutcome::Stopped {
            previous: previous_owner.clone(),
        }))
    }

    fn execute_start(
        &mut self,
        envelope: &IntentEnvelope,
        candidate: &RecoveryConfig,
        candidate_lane_generation: obsession_runtime_reliability::legacy_reliability::contracts::LaneGeneration,
        now_ms: u64,
    ) -> Result<LegacyRecoveryStageEffect, LegacyRecoveryEffectError> {
        let active = self
            .active
            .as_ref()
            .cloned()
            .ok_or(LegacyRecoveryEffectError::InvalidAction)?;
        if active.current_envelope != *envelope
            || active.candidate != *candidate
            || candidate_lane_generation != envelope.expected_lane_generation
        {
            return Err(LegacyRecoveryEffectError::InvalidAction);
        }
        if !network_matches(envelope, &self.network.snapshot())
            || self.prepared.candidate_plan().reverify().is_err()
            || self.executor.runtime_snapshot().is_some()
            || !self.executor.process_identities().is_empty()
            || self.observer.runtime_generation() != Some(active.runtime.generation)
            || self.observer.current_fence(&envelope.category).as_ref()
                != Some(&fence_from_envelope(envelope))
        {
            return Ok(completed(ExecutorOutcome::ExecutionAborted {
                stage: ExecutorStage::Start,
                reason: ConfirmationFailure::Sensor,
            }));
        }

        let generation = match self.executor.start_runtime(self.prepared.candidate_plan()) {
            Ok(generation) if generation != 0 => generation,
            Ok(_) => {
                return Ok(start_failure_outcome(
                    candidate,
                    LegacyRuntimeStartError::RuntimeFailure,
                ))
            }
            Err(error) => return Ok(start_failure_outcome(candidate, error)),
        };
        if self
            .observer
            .rebind_generation(active.runtime.generation, generation)
            .is_err()
        {
            let _ = self.executor.stop_runtime(generation);
            return Ok(completed(ExecutorOutcome::ExecutionAborted {
                stage: ExecutorStage::Start,
                reason: ConfirmationFailure::Sensor,
            }));
        }

        let runtime = self.executor.runtime_snapshot();
        let processes = self.executor.process_identities();
        let owner = runtime.as_ref().and_then(|runtime| {
            exact_owner(
                runtime,
                &processes,
                &envelope.category,
                candidate,
                candidate_lane_generation,
            )
        });
        let post_is_exact = runtime.as_ref().is_some_and(|runtime| {
            runtime.generation == generation
                && runtime.selections == self.prepared.candidate_selections()
        }) && owner
            .as_ref()
            .is_some_and(|owner| owner.owns(candidate.fingerprint(), candidate_lane_generation))
            && self.observer.runtime_generation() == Some(generation)
            && self.observer.current_fence(&envelope.category).as_ref()
                == Some(&fence_from_envelope(envelope))
            && network_matches(envelope, &self.network.snapshot())
            && self.prepared.candidate_plan().reverify().is_ok();
        let (Some(runtime), Some(owner)) = (runtime.filter(|_| post_is_exact), owner) else {
            let _ = self.executor.stop_runtime(generation);
            return Ok(completed(ExecutorOutcome::ExecutionAborted {
                stage: ExecutorStage::Start,
                reason: ConfirmationFailure::Sensor,
            }));
        };
        let confirmation = self
            .observer
            .confirmation_snapshot()
            .zip(
                self.observer
                    .confirmation_targets(&envelope.category, candidate),
            )
            .and_then(|(snapshot, targets)| {
                LegacyConfirmationWindow::new(envelope.clone(), &snapshot, targets, now_ms).ok()
            });
        let Some(active) = self.active.as_mut() else {
            let _ = self.executor.stop_runtime(generation);
            return Err(LegacyRecoveryEffectError::InvalidAction);
        };
        active.candidate_runtime = Some(runtime);
        active.candidate_processes = processes;
        active.candidate_owner = Some(owner.clone());
        let Some(confirmation) = confirmation else {
            return Ok(completed(ExecutorOutcome::ExecutionAborted {
                stage: ExecutorStage::Start,
                reason: ConfirmationFailure::Sensor,
            }));
        };
        self.probes.arm(
            confirmation.targets(),
            confirmation.arm().deadline_at_monotonic_ms(),
        );
        active.confirmation = Some(confirmation);
        Ok(completed(ExecutorOutcome::Ready { candidate: owner }))
    }

    fn execute_confirmation(
        &mut self,
        envelope: &IntentEnvelope,
        candidate: &RecoveryConfig,
        owner: &ProcessOwner,
        now_ms: u64,
    ) -> Result<LegacyRecoveryStageEffect, LegacyRecoveryEffectError> {
        let active = self
            .active
            .as_ref()
            .ok_or(LegacyRecoveryEffectError::InvalidAction)?;
        if active.current_envelope != *envelope
            || active.candidate != *candidate
            || active.candidate_owner.as_ref() != Some(owner)
        {
            return Err(LegacyRecoveryEffectError::InvalidAction);
        }
        let exact = active.candidate_runtime.as_ref().is_some_and(|runtime| {
            self.executor.runtime_snapshot().as_ref() == Some(runtime)
                && runtime.selections == self.prepared.candidate_selections()
        }) && self.executor.process_identities() == active.candidate_processes
            && self.observer.runtime_generation()
                == active
                    .candidate_runtime
                    .as_ref()
                    .map(|runtime| runtime.generation)
            && self.observer.current_fence(&envelope.category).as_ref()
                == Some(&fence_from_envelope(envelope))
            && network_matches(envelope, &self.network.snapshot())
            && self.prepared.candidate_plan().reverify().is_ok();
        if !exact {
            self.probes.cancel();
            return Ok(completed(ExecutorOutcome::ExecutionAborted {
                stage: ExecutorStage::Confirmation,
                reason: ConfirmationFailure::Sensor,
            }));
        }
        let Some(snapshot) = self.observer.confirmation_snapshot() else {
            self.probes.cancel();
            return Ok(completed(ExecutorOutcome::ExecutionAborted {
                stage: ExecutorStage::Confirmation,
                reason: ConfirmationFailure::Sensor,
            }));
        };
        let observations = self.probes.take_observations(now_ms);
        let active = self
            .active
            .as_mut()
            .ok_or(LegacyRecoveryEffectError::InvalidAction)?;
        let confirmation = active
            .confirmation
            .as_mut()
            .ok_or(LegacyRecoveryEffectError::InvalidAction)?;
        for observation in observations {
            confirmation.observe_probe(observation, now_ms);
        }
        let decision = confirmation.observe_snapshot(&snapshot, now_ms);
        match decision {
            LegacyConfirmationDecision::Pending => Ok(LegacyRecoveryStageEffect::Pending),
            LegacyConfirmationDecision::Succeeded => {
                active.confirmation_succeeded = true;
                self.probes.cancel();
                Ok(completed(ExecutorOutcome::ConfirmationSucceeded {
                    candidate: owner.clone(),
                }))
            }
            LegacyConfirmationDecision::Failed(reason) => {
                self.probes.cancel();
                Ok(completed(ExecutorOutcome::ConfirmationFailed {
                    candidate: owner.clone(),
                    reason,
                }))
            }
        }
    }

    fn execute_commit(
        &mut self,
        envelope: &IntentEnvelope,
        candidate: &RecoveryConfig,
        owner: &ProcessOwner,
    ) -> Result<LegacyRecoveryStageEffect, LegacyRecoveryEffectError> {
        let active = self
            .active
            .as_ref()
            .cloned()
            .ok_or(LegacyRecoveryEffectError::InvalidAction)?;
        if active.current_envelope != *envelope
            || active.candidate != *candidate
            || active.candidate_owner.as_ref() != Some(owner)
            || !active.confirmation_succeeded
        {
            return Err(LegacyRecoveryEffectError::InvalidAction);
        }
        let exact = active.candidate_runtime.as_ref().is_some_and(|runtime| {
            self.executor.runtime_snapshot().as_ref() == Some(runtime)
                && runtime.selections == self.prepared.candidate_selections()
        }) && self.executor.process_identities() == active.candidate_processes
            && self.observer.runtime_generation()
                == active
                    .candidate_runtime
                    .as_ref()
                    .map(|runtime| runtime.generation)
            && self.observer.current_fence(&envelope.category).as_ref()
                == Some(&fence_from_envelope(envelope))
            && network_matches(envelope, &self.network.snapshot())
            && self.prepared.candidate_plan().reverify().is_ok()
            && active.candidate_runtime.as_ref().is_some_and(|runtime| {
                exact_owner(
                    runtime,
                    &active.candidate_processes,
                    &envelope.category,
                    candidate,
                    owner.lane_generation,
                )
                .as_ref()
                    == Some(owner)
            });
        self.probes.cancel();
        if !exact {
            return Ok(completed(ExecutorOutcome::ExecutionAborted {
                stage: ExecutorStage::Commit,
                reason: ConfirmationFailure::Sensor,
            }));
        }
        self.active = None;
        Ok(completed(ExecutorOutcome::CandidateCommitted {
            candidate: owner.clone(),
        }))
    }

    fn execute_rollback(
        &mut self,
        envelope: &IntentEnvelope,
        previous: &RecoveryConfig,
        previous_lane_generation: obsession_runtime_reliability::legacy_reliability::contracts::LaneGeneration,
        retry_pending: bool,
    ) -> Result<LegacyRecoveryStageEffect, LegacyRecoveryEffectError> {
        self.probes.cancel();
        let active = self
            .active
            .as_ref()
            .cloned()
            .ok_or(LegacyRecoveryEffectError::InvalidAction)?;
        if active.current_envelope != *envelope
            || active.previous != *previous
            || previous_lane_generation.get() == 0
        {
            return Err(LegacyRecoveryEffectError::InvalidAction);
        }
        let restored = self.perform_rollback(&active, envelope, previous_lane_generation);
        self.active = None;
        let Some((previous_owner, refreshed_fence)) = restored else {
            return Ok(rollback_failed(previous));
        };
        Ok(completed(if retry_pending {
            ExecutorOutcome::RolledBackForRetry {
                previous: previous_owner,
                refreshed_fence,
            }
        } else {
            ExecutorOutcome::RolledBack {
                previous: previous_owner,
            }
        }))
    }

    fn perform_rollback(
        &mut self,
        active: &ArmedLegacyRecoveryEffect,
        envelope: &IntentEnvelope,
        previous_lane_generation: obsession_runtime_reliability::legacy_reliability::contracts::LaneGeneration,
    ) -> Option<(ProcessOwner, IntentFence)> {
        if let Some(runtime) = self.executor.runtime_snapshot() {
            let processes = self.executor.process_identities();
            let exact_original = runtime == active.runtime && processes == active.processes;
            let exact_candidate = active.candidate_runtime.as_ref() == Some(&runtime)
                && processes == active.candidate_processes
                && active.candidate_owner.as_ref().is_some_and(|owner| {
                    exact_owner(
                        &runtime,
                        &processes,
                        &envelope.category,
                        &active.candidate,
                        owner.lane_generation,
                    )
                    .as_ref()
                        == Some(owner)
                });
            if (!exact_original && !exact_candidate)
                || self.executor.stop_runtime(runtime.generation).is_err()
                || self.executor.runtime_snapshot().is_some()
                || !self.executor.process_identities().is_empty()
            {
                return None;
            }
        } else if !self.executor.process_identities().is_empty() {
            return None;
        }

        if let Some(generation) = self.observer.runtime_generation() {
            if self.observer.stop(generation).is_err() {
                return None;
            }
        }
        if self.prepared.previous_plan().reverify().is_err() {
            return None;
        }
        let plan = self.prepared.observer_plan().restore_previous(
            &envelope.category,
            previous_lane_generation,
            self.prepared.previous_selections(),
        )?;
        let placeholder_generation = plan.runtime_generation();
        if self
            .observer
            .start_recovery(plan, self.prepared.previous_selections())
            .is_err()
        {
            return None;
        }
        let generation = match self.executor.start_runtime(self.prepared.previous_plan()) {
            Ok(generation) if generation != 0 => generation,
            _ => {
                let _ = self.observer.stop(placeholder_generation);
                return None;
            }
        };
        if self
            .observer
            .rebind_generation(placeholder_generation, generation)
            .is_err()
        {
            let _ = self.executor.stop_runtime(generation);
            return None;
        }
        let runtime = self.executor.runtime_snapshot()?;
        let processes = self.executor.process_identities();
        let owner = exact_owner(
            &runtime,
            &processes,
            &envelope.category,
            &active.previous,
            previous_lane_generation,
        )?;
        let refreshed_fence = self.observer.current_fence(&envelope.category)?;
        let exact = runtime.generation == generation
            && runtime.selections == self.prepared.previous_selections()
            && self.observer.runtime_generation() == Some(generation)
            && refreshed_fence.session_id == envelope.session_id
            && refreshed_fence.category == envelope.category
            && refreshed_fence.lane_generation == previous_lane_generation
            && refreshed_fence.sensor_generation != envelope.expected_sensor_generation
            && refreshed_fence.registry_version
                == active.original_envelope.expected_registry_version
            && refreshed_fence.network_fingerprint == envelope.expected_network_fingerprint
            && self.prepared.previous_plan().reverify().is_ok();
        if !exact {
            let _ = self.executor.stop_runtime(generation);
            return None;
        }
        Some((owner, refreshed_fence))
    }

    fn restore_previous_observer(
        &mut self,
        envelope: &IntentEnvelope,
        runtime: &DpiRuntimeSnapshot,
    ) -> bool {
        if let Some(generation) = self.observer.runtime_generation() {
            if self.observer.stop(generation).is_err() {
                return false;
            }
        }
        if self.prepared.previous_plan().reverify().is_err() {
            return false;
        }
        let Some(plan) = self.prepared.observer_plan().restore_previous(
            &envelope.category,
            envelope.expected_lane_generation,
            self.prepared.previous_selections(),
        ) else {
            return false;
        };
        if self
            .observer
            .start_recovery(plan, self.prepared.previous_selections())
            .is_err()
        {
            return false;
        }
        self.observer.runtime_generation() == Some(runtime.generation)
            && self
                .observer
                .current_fence(&envelope.category)
                .is_some_and(|restored| {
                    restored.session_id == envelope.session_id
                        && restored.category == envelope.category
                        && restored.lane_generation == envelope.expected_lane_generation
                        && restored.sensor_generation != envelope.expected_sensor_generation
                        && restored.registry_version == envelope.expected_registry_version
                        && restored.network_fingerprint == envelope.expected_network_fingerprint
                })
    }
}

impl<O, E, N, P> LegacyRecoveryEffects for ProtectedLegacyRecoveryEffects<'_, O, E, N, P>
where
    O: LegacyRecoveryObserverPort,
    E: LegacyRecoveryProcessView,
    N: LegacyRecoveryNetworkPort,
    P: LegacyRecoveryProbePort,
{
    fn execute(
        &mut self,
        action: &RecoveryAction,
        now_ms: u64,
    ) -> Result<LegacyRecoveryStageEffect, LegacyRecoveryEffectError> {
        match action {
            RecoveryAction::Preflight {
                envelope,
                previous,
                previous_owner,
                candidate,
                origin,
            } => Ok(self.execute_preflight(envelope, previous, previous_owner, candidate, *origin)),
            RecoveryAction::StopPrevious {
                envelope,
                previous,
                previous_owner,
                origin,
            } => self.execute_stop(envelope, previous, previous_owner, *origin),
            RecoveryAction::StartCandidate {
                envelope,
                candidate,
                candidate_lane_generation,
            } => self.execute_start(envelope, candidate, *candidate_lane_generation, now_ms),
            RecoveryAction::ConfirmCandidate {
                envelope,
                candidate,
                owner,
            } => self.execute_confirmation(envelope, candidate, owner, now_ms),
            RecoveryAction::CommitCandidate {
                envelope,
                candidate,
                owner,
            } => self.execute_commit(envelope, candidate, owner),
            RecoveryAction::RollbackPrevious {
                envelope,
                previous,
                previous_lane_generation,
                retry_pending,
            } => self.execute_rollback(
                envelope,
                previous,
                *previous_lane_generation,
                *retry_pending,
            ),
            _ => Err(LegacyRecoveryEffectError::InvalidAction),
        }
    }
}

fn completed(outcome: ExecutorOutcome) -> LegacyRecoveryStageEffect {
    LegacyRecoveryStageEffect::Completed(outcome)
}

const fn next_nonzero_generation(current: u64) -> u64 {
    let next = current.wrapping_add(1);
    if next == 0 {
        1
    } else {
        next
    }
}

fn start_failure_outcome(
    candidate: &RecoveryConfig,
    error: LegacyRuntimeStartError,
) -> LegacyRecoveryStageEffect {
    completed(match error {
        LegacyRuntimeStartError::ProtectedResourceChanged => ExecutorOutcome::ExecutionAborted {
            stage: ExecutorStage::Start,
            reason: ConfirmationFailure::Sensor,
        },
        LegacyRuntimeStartError::ReadinessFailed => ExecutorOutcome::StartFailed {
            candidate_fingerprint: candidate.fingerprint().clone(),
        },
        LegacyRuntimeStartError::RuntimeFailure => ExecutorOutcome::ExecutionAborted {
            stage: ExecutorStage::Start,
            reason: ConfirmationFailure::Environment,
        },
    })
}

fn previous_missing(previous: &RecoveryConfig) -> LegacyRecoveryStageEffect {
    completed(ExecutorOutcome::PreviousProcessMissing {
        previous_fingerprint: previous.fingerprint().clone(),
    })
}

fn rollback_failed(previous: &RecoveryConfig) -> LegacyRecoveryStageEffect {
    completed(ExecutorOutcome::RollbackFailed {
        previous_fingerprint: previous.fingerprint().clone(),
    })
}

fn network_matches(envelope: &IntentEnvelope, network: &LocalNetworkSnapshot) -> bool {
    network.online
        && network.interface_up
        && network.default_route_available
        && network.gateway_reachable
        && network.network_fingerprint.is_stable()
        && network.network_fingerprint == envelope.expected_network_fingerprint
}

fn fence_from_envelope(envelope: &IntentEnvelope) -> IntentFence {
    IntentFence {
        session_id: envelope.session_id,
        category: envelope.category.clone(),
        lane_generation: envelope.expected_lane_generation,
        sensor_generation: envelope.expected_sensor_generation,
        registry_version: envelope.expected_registry_version,
        network_fingerprint: envelope.expected_network_fingerprint.clone(),
    }
}

fn exact_owner(
    runtime: &DpiRuntimeSnapshot,
    processes: &[ProcessIdentity],
    category: &str,
    config: &RecoveryConfig,
    lane_generation: obsession_runtime_reliability::legacy_reliability::contracts::LaneGeneration,
) -> Option<ProcessOwner> {
    if runtime.engine != obsession_runtime_protocol::DpiEngine::Legacy || processes.len() != 1 {
        return None;
    }
    let category = recovery_category(category)?;
    runtime.selections.iter().find(|selection| {
        selection.category == category && selection.strategy_id == config.config_id()
    })?;
    // All category profiles are owned by the same generation-fenced Job process.
    let process = processes.first()?;
    if process.pid == 0 || process.creation_time_100ns == 0 {
        return None;
    }
    Some(ProcessOwner {
        pid: process.pid,
        process_start_identity: ProcessStartIdentity::new(process.creation_time_100ns),
        config_fingerprint: config.fingerprint().clone(),
        lane_generation,
    })
}

fn recovery_category(category: &str) -> Option<DpiCategory> {
    match category {
        "discord" => Some(DpiCategory::Discord),
        "youtube_twitch" => Some(DpiCategory::YoutubeTwitch),
        "gaming" => Some(DpiCategory::Gaming),
        "atrisk" | "at_risk" => Some(DpiCategory::AtRisk),
        "universal" => Some(DpiCategory::Universal),
        _ => None,
    }
}

#[cfg(test)]
mod combined_owner_tests {
    use super::*;
    #[test]
    fn every_selected_category_uses_the_single_exact_process() {
        let runtime = DpiRuntimeSnapshot {
            generation: 3,
            engine: obsession_runtime_protocol::DpiEngine::Legacy,
            selections: vec![
                DpiSelection {
                    category: DpiCategory::Discord,
                    strategy_id: "discord_14.conf".into(),
                },
                DpiSelection {
                    category: DpiCategory::YoutubeTwitch,
                    strategy_id: "youtube_twitch_11.conf".into(),
                },
            ],
        };
        let processes = [ProcessIdentity {
            pid: 42,
            creation_time_100ns: 99,
            executable_sha256: "a".repeat(64),
        }];
        for (category, config_id) in [
            ("discord", "discord_14.conf"),
            ("youtube_twitch", "youtube_twitch_11.conf"),
        ] {
            let config = RecoveryConfig::new(config_id, format!("fingerprint-{category}"));
            let owner = exact_owner(
                &runtime,
                &processes,
                category,
                &config,
                LaneGeneration::new(7),
            )
            .unwrap();
            assert_eq!(owner.pid, 42);
            assert_eq!(owner.process_start_identity, ProcessStartIdentity::new(99));
            assert_eq!(&owner.config_fingerprint, config.fingerprint());
            assert!(
                exact_owner(&runtime, &[], category, &config, LaneGeneration::new(7)).is_none()
            );
            assert!(exact_owner(
                &runtime,
                &[processes[0].clone(), processes[0].clone()],
                category,
                &config,
                LaneGeneration::new(7)
            )
            .is_none());
        }
        assert!(exact_owner(
            &runtime,
            &processes,
            "discord",
            &RecoveryConfig::new("wrong.conf", "bad"),
            LaneGeneration::new(7)
        )
        .is_none());
    }
}
