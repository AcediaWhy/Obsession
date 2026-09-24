//! Production bridge between the typed wire dispatcher and the protected DPI
//! executor.
//!
//! Construction is the capability boundary: callers cannot obtain a backend
//! that advertises DPI until the immutable manifest and the service-owned state
//! root have both completed their fail-closed startup preflight.

#![cfg(windows)]

use std::collections::BTreeSet;
use std::sync::Mutex;

use obsession_runtime_protocol::{
    Capabilities, DpiCategory, DpiEngine, DpiReplaceRequest, DpiRuntimeSnapshot, DpiStartRequest,
    DpiStopRequest, Feature, FirewallOpenProxyLanRequest, HostsCheckRequest, HostsHealthSnapshot,
    HostsMutationRequest, LegacyRecoveryApprovalRequest, LegacyRecoveryControlsRequest,
    LegacyRecoveryMode, OperationAccepted, RuntimeSnapshot, RuntimeStarted,
};
use obsession_runtime_reliability::legacy_reliability::contracts::{
    IntentFence, ProcessOwner, ProcessStartIdentity,
};
use obsession_runtime_reliability::legacy_reliability::recovery::{
    ApprovalError, AssistedApproval, RecoveryAction,
};

use crate::dpi_executor::{
    DpiExecutor, ExecutorError, ProcessIdentity, RuntimeProcessLauncher, WindowsJobLauncher,
};
use crate::dpi_materializer::{MaterializationError, ProtectedDataLayout};
use crate::hosts_controller::HostsController;
use crate::legacy_access_activity::LegacyAccessTracker;
use crate::legacy_recovery_effects::{
    MachineLegacyRecoveryProbes, MachineNetwork, ProtectedLegacyRecoveryEffectState,
    ProtectedLegacyRecoveryEffects,
};
use crate::legacy_recovery_preflight::{
    preflight_key, prepare_legacy_recovery, PreparedLegacyRecovery,
};
use crate::legacy_recovery_transaction::{
    LegacyRecoveryTransaction, LegacyRecoveryTransactionProgress,
};
use crate::legacy_reliability::{
    LegacyObserverRuntime, LegacyObserverRuntimeError, LegacyRecoveryInput, LegacyRecoveryRuntime,
    LegacyRecoveryScan,
};
use crate::protected_layout::{LayoutError, ProtectedLayout, VerifiedRuntimeCatalog};
use crate::proxy_lan_firewall::ProxyLanFirewall;
use crate::{BackendError, RuntimeBackend};

pub struct ProtectedDpiBackend<L: RuntimeProcessLauncher> {
    catalog: VerifiedRuntimeCatalog,
    observer: Option<LegacyObserverRuntime>,
    recovery: LegacyRecoveryRuntime,
    recovery_preflight: Option<PreparedLegacyRecovery>,
    recovery_transaction: Option<LegacyRecoveryTransaction>,
    recovery_pending_preflight: Option<RecoveryAction>,
    recovery_effect_state: ProtectedLegacyRecoveryEffectState,
    recovery_network: MachineNetwork,
    recovery_probes: Option<MachineLegacyRecoveryProbes>,
    executor: DpiExecutor<L>,
    active_request: Option<DpiStartRequest>,
    access_tracker: Mutex<LegacyAccessTracker>,
    hosts: Option<HostsController>,
    proxy_lan_firewall: Option<ProxyLanFirewall>,
}

impl<L: RuntimeProcessLauncher> ProtectedDpiBackend<L> {
    pub fn from_preflight(
        install_layout: &ProtectedLayout,
        state_layout: ProtectedDataLayout,
        launcher: L,
    ) -> Result<Self, BackendInitializationError> {
        let catalog = install_layout.load_verified_catalog()?;
        Self::from_verified_catalog(catalog, state_layout, launcher)
    }

    fn from_verified_catalog(
        catalog: VerifiedRuntimeCatalog,
        state_layout: ProtectedDataLayout,
        launcher: L,
    ) -> Result<Self, BackendInitializationError> {
        if catalog.engine(DpiEngine::Legacy).is_none() {
            return Err(BackendInitializationError::LegacyEngineUnavailable);
        }
        let observer = LegacyObserverRuntime::from_catalog(&catalog).ok();
        let hosts = HostsController::discover(&state_layout).ok();
        let proxy_lan_firewall = ProxyLanFirewall::discover(&state_layout).ok();
        Ok(Self {
            catalog,
            observer,
            recovery: LegacyRecoveryRuntime::new(),
            recovery_preflight: None,
            recovery_transaction: None,
            recovery_pending_preflight: None,
            recovery_effect_state: ProtectedLegacyRecoveryEffectState::default(),
            recovery_network: MachineNetwork,
            recovery_probes: MachineLegacyRecoveryProbes::new().ok(),
            executor: DpiExecutor::new(state_layout, launcher),
            active_request: None,
            access_tracker: Mutex::new(LegacyAccessTracker::new()),
            hosts,
            proxy_lan_firewall,
        })
    }

    /// Replaces one exact Legacy generation with a service-selected candidate
    /// and restores the previous verified plan if either candidate launch or
    /// observer startup fails. This is an internal recovery primitive; it is
    /// intentionally not exposed as a caller-controlled wire operation.
    #[allow(dead_code)]
    pub(crate) fn replace_legacy_generation(
        &mut self,
        expected_generation: u64,
        selections: Vec<obsession_runtime_protocol::DpiSelection>,
    ) -> Result<RuntimeStarted, BackendError> {
        let previous = self.executor.snapshot().ok_or(BackendError::Conflict)?;
        if previous.engine != DpiEngine::Legacy || previous.generation != expected_generation {
            return Err(BackendError::Conflict);
        }
        if selections.is_empty()
            || selections.len() != previous.selections.len()
            || selections
                .iter()
                .map(|selection| selection.category)
                .collect::<BTreeSet<_>>()
                != previous
                    .selections
                    .iter()
                    .map(|selection| selection.category)
                    .collect::<BTreeSet<_>>()
        {
            return Err(BackendError::InvalidRequest);
        }

        let observer_active = self
            .observer
            .as_ref()
            .is_some_and(|observer| observer.snapshot().is_some());
        let previous_request = DpiStartRequest {
            engine: DpiEngine::Legacy,
            selections: previous.selections.clone(),
            options: obsession_runtime_protocol::DpiRuntimeOptions {
                zapret2_level: 0,
                legacy_reliability: observer_active,
                zapret2_overrides: Vec::new(),
            },
        };
        let candidate_request = DpiStartRequest {
            engine: DpiEngine::Legacy,
            selections: selections.clone(),
            options: obsession_runtime_protocol::DpiRuntimeOptions {
                zapret2_level: 0,
                legacy_reliability: observer_active,
                zapret2_overrides: Vec::new(),
            },
        };
        let previous_plan = self
            .catalog
            .resolve_dpi_plan(&previous_request)
            .map_err(|_| BackendError::ProtectedResourceInvalid)?;
        let candidate_plan = self
            .catalog
            .resolve_dpi_plan(&candidate_request)
            .map_err(|_| BackendError::ProtectedResourceInvalid)?;

        if observer_active {
            self.observer
                .as_mut()
                .ok_or(BackendError::ServiceUnavailable)?
                .stop(expected_generation)
                .map_err(map_observer_error)?;
        }
        if let Err(error) = self.executor.stop(expected_generation) {
            let _ = self.restore_legacy_plan(&previous_plan, &previous.selections, observer_active);
            return Err(map_executor_error(error));
        }

        let candidate_started = match self.executor.start(&candidate_plan) {
            Ok(started) => started,
            Err(error) => {
                let _ =
                    self.restore_legacy_plan(&previous_plan, &previous.selections, observer_active);
                return Err(map_executor_error(error));
            }
        };
        if observer_active {
            let observer = self
                .observer
                .as_mut()
                .ok_or(BackendError::ServiceUnavailable)?;
            if observer
                .start(candidate_started.generation, &selections)
                .is_err()
            {
                let _ = self.executor.stop(candidate_started.generation);
                let _ =
                    self.restore_legacy_plan(&previous_plan, &previous.selections, observer_active);
                return Err(BackendError::RuntimeFailed);
            }
        }
        self.active_request = Some(candidate_request);
        Ok(candidate_started)
    }

    #[allow(dead_code)]
    fn restore_legacy_plan(
        &mut self,
        plan: &crate::protected_layout::VerifiedDpiPlan,
        selections: &[obsession_runtime_protocol::DpiSelection],
        observer_active: bool,
    ) -> Result<RuntimeStarted, BackendError> {
        let started = self.executor.start(plan).map_err(map_executor_error)?;
        if observer_active {
            self.observer
                .as_mut()
                .ok_or(BackendError::ServiceUnavailable)?
                .start(started.generation, selections)
                .map_err(map_observer_error)?;
        }
        Ok(started)
    }

    fn prepare_recovery_action(
        &self,
        live_scan: &LegacyRecoveryScan,
        action: &RecoveryAction,
    ) -> Result<PreparedLegacyRecovery, BackendError> {
        let RecoveryAction::Preflight {
            envelope,
            previous_owner,
            candidate,
            ..
        } = action
        else {
            return Err(BackendError::Internal);
        };
        let observer = self.observer.as_ref().ok_or(BackendError::Internal)?;
        if !preflight_action_matches_live_fence(
            action,
            observer.current_fence(&envelope.category).as_ref(),
        ) {
            return Err(BackendError::Internal);
        }
        let action_scan = self
            .recovery
            .preflight_scan_for_action(live_scan, action)
            .ok_or(BackendError::Internal)?;
        let runtime = self.executor.snapshot().ok_or(BackendError::Internal)?;
        if exact_recovery_owner(
            Some(&runtime),
            self.executor.active_processes(),
            &action_scan,
        )
        .as_ref()
            != Some(previous_owner)
        {
            return Err(BackendError::Internal);
        }
        let candidate_context = observer
            .recovery_candidate_context(&action_scan, candidate)
            .ok_or(BackendError::Internal)?;
        prepare_legacy_recovery(
            &self.catalog,
            &runtime,
            &action_scan,
            previous_owner,
            candidate,
            &candidate_context,
            &crate::network_identity::snapshot(),
        )
        .map_err(|_| BackendError::Internal)
    }

    fn queue_recovery_action(
        &mut self,
        live_scan: &LegacyRecoveryScan,
        action: RecoveryAction,
    ) -> Result<(), BackendError> {
        if self.recovery_transaction.is_some()
            || !self.recovery_effect_state.is_idle()
            || self.recovery_probes.is_none()
        {
            return Err(BackendError::Internal);
        }
        let prepared = self.prepare_recovery_action(live_scan, &action)?;
        let transaction =
            LegacyRecoveryTransaction::new(action).map_err(|_| BackendError::Internal)?;
        self.recovery_preflight = Some(prepared);
        self.recovery_transaction = Some(transaction);
        Ok(())
    }

    fn drive_recovery_transaction(&mut self) -> Result<(), BackendError> {
        let Some(mut transaction) = self.recovery_transaction.take() else {
            return Ok(());
        };
        if self.recovery_preflight.is_none()
            || self.observer.is_none()
            || self.recovery_probes.is_none()
        {
            self.recovery_transaction = Some(transaction);
            return Err(BackendError::Internal);
        }
        let now_ms = self.recovery.monotonic_now_ms();
        let state = std::mem::take(&mut self.recovery_effect_state);
        let (progress, state) = {
            let prepared = self.recovery_preflight.as_ref().expect("checked preflight");
            let observer = self.observer.as_mut().expect("checked observer");
            let probes = self.recovery_probes.as_mut().expect("checked probes");
            let mut effects = ProtectedLegacyRecoveryEffects::from_state(
                prepared,
                observer,
                &mut self.executor,
                &mut self.recovery_network,
                probes,
                state,
            );
            let progress = transaction.advance(&mut self.recovery, &mut effects, now_ms);
            if progress.is_err() {
                let _ = effects.emergency_rollback();
            }
            (progress, effects.into_state())
        };
        self.recovery_effect_state = state;
        let progress = match progress {
            Ok(progress) => progress,
            Err(_) => {
                // Preserve the exact failed transaction but remove its
                // capability to execute. Future polls hit the missing
                // preflight guard and cannot repeat a partially applied
                // action; recovery remains terminally blocked until explicit
                // service-owned controls are added.
                self.recovery_transaction = Some(transaction);
                self.recovery_preflight = None;
                self.recovery_pending_preflight = None;
                let _ = self.recovery.force_manual_failure(now_ms);
                debug_assert!(self.recovery_effect_state.is_idle());
                return Err(BackendError::Internal);
            }
        };
        match progress {
            LegacyRecoveryTransactionProgress::Pending => {
                self.recovery_transaction = Some(transaction);
            }
            LegacyRecoveryTransactionProgress::PreflightRequired(action) => {
                if !self.recovery_effect_state.is_idle() {
                    self.recovery_transaction = Some(transaction);
                    return Err(BackendError::Internal);
                }
                self.recovery_preflight = None;
                self.recovery_pending_preflight = Some(action);
            }
            LegacyRecoveryTransactionProgress::Complete(_) => {
                if !self.recovery_effect_state.is_idle() {
                    self.recovery_transaction = Some(transaction);
                    return Err(BackendError::Internal);
                }
                self.recovery_preflight = None;
            }
        }
        Ok(())
    }
}

impl ProtectedDpiBackend<WindowsJobLauncher> {
    pub fn discover() -> Result<Self, BackendInitializationError> {
        let install_layout = ProtectedLayout::discover()?;
        let state_layout = ProtectedDataLayout::discover()?;
        let launcher = WindowsJobLauncher::new(state_layout.clone()).map_err(|source| {
            MaterializationError::Io {
                operation: "recover TCP timestamps",
                path: state_layout.root().to_path_buf(),
                source,
            }
        })?;
        Self::from_preflight(&install_layout, state_layout, launcher)
    }
}

impl<L: RuntimeProcessLauncher> RuntimeBackend for ProtectedDpiBackend<L> {
    fn capabilities(&self) -> Result<Capabilities, BackendError> {
        let mut features = vec![Feature::Dpi];
        if self.catalog.engine(DpiEngine::Zapret2).is_some() {
            features.push(Feature::DpiZapret2);
            features.push(Feature::DpiZapret2Adaptive);
        }
        if self.observer.is_some() {
            features.push(Feature::EyesEvents);
            features.push(Feature::LegacyReliabilityControls);
            if self.recovery_probes.is_some() {
                features.push(Feature::LegacyReliability);
            }
        }
        if self.hosts.is_some() {
            features.push(Feature::Hosts);
            features.push(Feature::HostsHealthV2);
        }
        if self.proxy_lan_firewall.is_some() {
            features.push(Feature::ProxyLanFirewall);
        }
        Ok(Capabilities {
            service_version: env!("CARGO_PKG_VERSION").into(),
            features,
        })
    }

    fn runtime_snapshot(&self) -> Result<RuntimeSnapshot, BackendError> {
        let access_snapshot = self
            .access_tracker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .public_snapshot();
        let mut legacy_reliability = self
            .observer
            .as_ref()
            .map(LegacyObserverRuntime::public_snapshot)
            .transpose()
            .map_err(|_| BackendError::Internal)?
            .flatten();
        if let Some(snapshot) = legacy_reliability.as_mut() {
            snapshot.recovery = self.recovery.public_snapshot(true);
        }
        let hosts = self.hosts.as_ref().and_then(HostsController::snapshot);
        let hosts_provider = hosts.as_ref().map(|snapshot| snapshot.provider);
        Ok(RuntimeSnapshot {
            dpi: self.executor.snapshot(),
            legacy_reliability,
            legacy_access_activity: Some(access_snapshot),
            hosts,
            hosts_provider,
            proxy_lan: self
                .proxy_lan_firewall
                .as_ref()
                .and_then(ProxyLanFirewall::snapshot),
        })
    }

    fn set_legacy_recovery_controls(
        &mut self,
        request: LegacyRecoveryControlsRequest,
    ) -> Result<OperationAccepted, BackendError> {
        if self.observer.is_none() {
            return Err(BackendError::ServiceUnavailable);
        }
        if request.mode != LegacyRecoveryMode::ObserveOnly && self.recovery_probes.is_none() {
            return Err(BackendError::ServiceUnavailable);
        }
        let operation_id = self
            .recovery
            .set_controls(request)
            .map_err(|()| BackendError::InvalidRequest)?;
        Ok(OperationAccepted { operation_id })
    }

    fn approve_legacy_recovery(
        &mut self,
        request: LegacyRecoveryApprovalRequest,
    ) -> Result<OperationAccepted, BackendError> {
        if self.recovery_transaction.is_some()
            || self.recovery_pending_preflight.is_some()
            || !self.recovery_effect_state.is_idle()
        {
            return Err(BackendError::Busy);
        }
        let proposal = self
            .recovery
            .public_snapshot(self.observer.is_some())
            .proposal
            .ok_or(BackendError::Conflict)?;
        if proposal.proposal_id != request.proposal_id || proposal.attempt_id != request.attempt_id
        {
            return Err(BackendError::Conflict);
        }
        let observer = self
            .observer
            .as_ref()
            .ok_or(BackendError::ServiceUnavailable)?;
        let live_scan = observer.recovery_scan().ok_or(BackendError::Conflict)?;
        let current_fence = observer
            .current_fence(recovery_category_name(proposal.category))
            .ok_or(BackendError::Conflict)?;
        let approval = AssistedApproval::new(request.proposal_id, request.attempt_id)
            .ok_or(BackendError::InvalidRequest)?;
        let action = self
            .recovery
            .approve(approval, &current_fence)
            .map_err(map_approval_error)?;
        if let Err(error) = self.queue_recovery_action(&live_scan, action) {
            let now_ms = self.recovery.monotonic_now_ms();
            let _ = self.recovery.force_manual_failure(now_ms);
            return Err(error);
        }
        Ok(OperationAccepted {
            operation_id: request.attempt_id,
        })
    }

    fn dpi_start(&mut self, request: DpiStartRequest) -> Result<RuntimeStarted, BackendError> {
        if self.catalog.engine(request.engine).is_none() {
            return Err(BackendError::ServiceUnavailable);
        }
        if request.engine == DpiEngine::Zapret2 && request.options.legacy_reliability {
            return Err(BackendError::InvalidRequest);
        }
        let plan = self
            .catalog
            .resolve_dpi_plan(&request)
            .map_err(|_| BackendError::ProtectedResourceInvalid)?;
        if let Some(observer) = self.observer.as_mut() {
            observer.prepare_start().map_err(map_observer_error)?;
        } else if plan.legacy_reliability() {
            return Err(BackendError::ServiceUnavailable);
        }
        let started = self.executor.start(&plan).map_err(map_executor_error)?;
        if plan.legacy_reliability() {
            let observer = self
                .observer
                .as_mut()
                .ok_or(BackendError::ServiceUnavailable)?;
            if let Err(error) = observer.start(started.generation, &request.selections) {
                let _ = self.executor.stop(started.generation);
                return Err(map_observer_error(error));
            }
        }
        self.active_request = Some(request);
        Ok(started)
    }

    fn dpi_replace(&mut self, request: DpiReplaceRequest) -> Result<RuntimeStarted, BackendError> {
        let current = self.executor.snapshot().ok_or(BackendError::Conflict)?;
        let previous_request = self.active_request.clone().ok_or(BackendError::Conflict)?;
        if current.generation != request.expected_generation
            || current.engine != DpiEngine::Zapret2
            || previous_request.engine != DpiEngine::Zapret2
            || request.runtime.engine != DpiEngine::Zapret2
            || previous_request.selections != current.selections
            || request.runtime.selections != current.selections
            || request.runtime.options.legacy_reliability
        {
            return Err(BackendError::Conflict);
        }

        let previous_plan = self
            .catalog
            .resolve_dpi_plan(&previous_request)
            .map_err(|_| BackendError::ProtectedResourceInvalid)?;
        let replacement_plan = self
            .catalog
            .resolve_dpi_plan(&request.runtime)
            .map_err(|_| BackendError::ProtectedResourceInvalid)?;
        self.executor
            .stop(request.expected_generation)
            .map_err(map_executor_error)?;

        match self.executor.start(&replacement_plan) {
            Ok(started) => {
                self.active_request = Some(request.runtime);
                Ok(started)
            }
            Err(error) => {
                self.active_request = None;
                if self.executor.start(&previous_plan).is_ok() {
                    self.active_request = Some(previous_request);
                }
                Err(map_executor_error(error))
            }
        }
    }

    fn dpi_stop(&mut self, request: DpiStopRequest) -> Result<(), BackendError> {
        if !self
            .executor
            .snapshot()
            .is_some_and(|runtime| runtime.generation == request.generation)
        {
            return Err(BackendError::Conflict);
        }
        let observer_result = self
            .observer
            .as_mut()
            .map(|observer| observer.stop(request.generation));
        self.executor
            .stop(request.generation)
            .map_err(map_executor_error)?;
        self.active_request = None;
        if let Some(result) = observer_result {
            result.map_err(map_observer_error)?;
        }
        Ok(())
    }

    fn hosts_install(
        &mut self,
        request: HostsMutationRequest,
    ) -> Result<OperationAccepted, BackendError> {
        self.hosts
            .as_mut()
            .ok_or(BackendError::ServiceUnavailable)?
            .install(request)
    }

    fn hosts_check(
        &mut self,
        request: HostsCheckRequest,
    ) -> Result<HostsHealthSnapshot, BackendError> {
        self.hosts
            .as_mut()
            .ok_or(BackendError::ServiceUnavailable)?
            .check(request.max_age_seconds)
    }

    fn hosts_uninstall(&mut self) -> Result<OperationAccepted, BackendError> {
        self.hosts
            .as_mut()
            .ok_or(BackendError::ServiceUnavailable)?
            .uninstall()
    }

    fn hosts_refresh_gemini(&mut self, preference: obsession_runtime_protocol::GeminiRoutePreference) -> Result<OperationAccepted, BackendError> {
        self.hosts.as_mut().ok_or(BackendError::ServiceUnavailable)?.refresh_gemini(preference)
    }

    fn hosts_restore(
        &mut self,
        request: HostsMutationRequest,
    ) -> Result<OperationAccepted, BackendError> {
        self.hosts
            .as_mut()
            .ok_or(BackendError::ServiceUnavailable)?
            .restore(request)
    }

    fn firewall_open_proxy_lan(
        &mut self,
        request: FirewallOpenProxyLanRequest,
    ) -> Result<OperationAccepted, BackendError> {
        self.proxy_lan_firewall
            .as_mut()
            .ok_or(BackendError::ServiceUnavailable)?
            .open(request)
    }

    fn firewall_close_proxy_lan(&mut self) -> Result<(), BackendError> {
        self.proxy_lan_firewall
            .as_mut()
            .ok_or(BackendError::ServiceUnavailable)?
            .close()
    }

    fn subscribe_events(&mut self) -> Result<OperationAccepted, BackendError> {
        Err(BackendError::ServiceUnavailable)
    }

    fn poll_background(&mut self) -> Result<(), BackendError> {
        let access_poll = self
            .access_tracker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .poll();
        if let (Some(observer), Some(access_poll)) = (self.observer.as_ref(), access_poll.as_ref())
        {
            observer.apply_access_poll(access_poll);
        }
        if let Some(firewall) = self.proxy_lan_firewall.as_mut() {
            // Expired leases remain pending and are retried on every poll, but
            // a transient Firewall COM failure must not starve DPI recovery.
            let _ = firewall.poll_expired();
        }
        if self.recovery_transaction.is_some() {
            return self.drive_recovery_transaction();
        }
        let Some(scan) = self
            .observer
            .as_ref()
            .and_then(LegacyObserverRuntime::recovery_scan)
        else {
            if self.recovery_pending_preflight.is_none() {
                self.recovery.reset();
                self.recovery_preflight = None;
            }
            return Ok(());
        };
        if let Some(action) = self.recovery_pending_preflight.take() {
            if let Err(error) = self.queue_recovery_action(&scan, action.clone()) {
                self.recovery_pending_preflight = Some(action);
                return Err(error);
            }
            return Ok(());
        }
        let runtime = self.executor.snapshot();
        let previous_owner =
            exact_recovery_owner(runtime.as_ref(), self.executor.active_processes(), &scan);
        let action = match self.recovery.poll(scan.clone(), previous_owner.clone()) {
            Ok(action) => action,
            Err(_) => {
                self.recovery_preflight = None;
                return Err(BackendError::Internal);
            }
        };
        if let Some(action) = action {
            if let Err(error) = self.queue_recovery_action(&scan, action) {
                let now_ms = self.recovery.monotonic_now_ms();
                let _ = self.recovery.force_manual_failure(now_ms);
                return Err(error);
            }
            return Ok(());
        }
        let Some((runtime, previous_owner, candidate)) = runtime
            .as_ref()
            .zip(previous_owner)
            .zip(
                scan.input
                    .as_ref()
                    .and_then(|input| input.candidates.first()),
            )
            .map(|((runtime, owner), candidate)| (runtime, owner, candidate))
        else {
            self.recovery_preflight = None;
            return Ok(());
        };
        let Some(candidate_context) = self
            .observer
            .as_ref()
            .and_then(|observer| observer.recovery_candidate_context(&scan, candidate))
        else {
            self.recovery_preflight = None;
            return Err(BackendError::Internal);
        };
        let Some(key) = preflight_key(&scan, &previous_owner, candidate, &candidate_context) else {
            self.recovery_preflight = None;
            return Err(BackendError::Internal);
        };
        if self
            .recovery_preflight
            .as_ref()
            .is_some_and(|prepared| prepared.key() == &key)
        {
            return Ok(());
        }
        let network = crate::network_identity::snapshot();
        self.recovery_preflight = Some(
            prepare_legacy_recovery(
                &self.catalog,
                runtime,
                &scan,
                &previous_owner,
                candidate,
                &candidate_context,
                &network,
            )
            .map_err(|_| BackendError::Internal)?,
        );
        Ok(())
    }
}

fn exact_recovery_owner(
    runtime: Option<&DpiRuntimeSnapshot>,
    processes: &[ProcessIdentity],
    scan: &LegacyRecoveryScan,
) -> Option<ProcessOwner> {
    let input: &LegacyRecoveryInput = scan.input.as_ref()?;
    let runtime = runtime?;
    if runtime.engine != DpiEngine::Legacy
        || runtime.generation != scan.generation
        || processes.len() != 1
    {
        return None;
    }
    let category = recovery_category(&input.fence.category)?;
    runtime.selections.iter().find(|selection| {
        selection.category == category && selection.strategy_id == input.previous.config_id()
    })?;
    let process = processes.first()?;
    if process.pid == 0 || process.creation_time_100ns == 0 {
        return None;
    }
    Some(ProcessOwner {
        pid: process.pid,
        process_start_identity: ProcessStartIdentity::new(process.creation_time_100ns),
        config_fingerprint: input.previous.fingerprint().clone(),
        lane_generation: input.fence.lane_generation,
    })
}

fn preflight_action_matches_live_fence(
    action: &RecoveryAction,
    live_fence: Option<&IntentFence>,
) -> bool {
    let RecoveryAction::Preflight { envelope, .. } = action else {
        return false;
    };
    live_fence
        == Some(&IntentFence {
            session_id: envelope.session_id,
            category: envelope.category.clone(),
            lane_generation: envelope.expected_lane_generation,
            sensor_generation: envelope.expected_sensor_generation,
            registry_version: envelope.expected_registry_version,
            network_fingerprint: envelope.expected_network_fingerprint.clone(),
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

const fn recovery_category_name(category: DpiCategory) -> &'static str {
    match category {
        DpiCategory::Discord => "discord",
        DpiCategory::YoutubeTwitch => "youtube_twitch",
        DpiCategory::Gaming => "gaming",
        DpiCategory::AtRisk => "atrisk",
        DpiCategory::Universal => "universal",
    }
}

const fn map_approval_error(error: ApprovalError) -> BackendError {
    match error {
        ApprovalError::Busy => BackendError::Busy,
        ApprovalError::ObserveOnly => BackendError::ServiceUnavailable,
        ApprovalError::UnknownProposal
        | ApprovalError::ProposalMismatch
        | ApprovalError::Expired
        | ApprovalError::Duplicate
        | ApprovalError::Cancelled
        | ApprovalError::FenceChanged
        | ApprovalError::ClockMovedBack => BackendError::Conflict,
    }
}

fn map_executor_error(error: ExecutorError) -> BackendError {
    match error {
        ExecutorError::Busy => BackendError::Busy,
        ExecutorError::Conflict => BackendError::Conflict,
        ExecutorError::Materialization(_) => BackendError::ProtectedResourceInvalid,
        ExecutorError::InvalidOwnership | ExecutorError::Process(_) => BackendError::RuntimeFailed,
    }
}

fn map_observer_error(error: LegacyObserverRuntimeError) -> BackendError {
    match error {
        LegacyObserverRuntimeError::Busy => BackendError::Busy,
        LegacyObserverRuntimeError::Conflict => BackendError::Conflict,
        LegacyObserverRuntimeError::StartFailed | LegacyObserverRuntimeError::StopUnresolved => {
            BackendError::RuntimeFailed
        }
    }
}

#[derive(Debug)]
pub enum BackendInitializationError {
    Layout(LayoutError),
    Materialization(MaterializationError),
    LegacyEngineUnavailable,
}

impl std::fmt::Display for BackendInitializationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Layout(error) => write!(formatter, "protected install preflight failed: {error}"),
            Self::Materialization(error) => {
                write!(formatter, "protected state preflight failed: {error}")
            }
            Self::LegacyEngineUnavailable => {
                formatter.write_str("verified catalog has no supported Legacy DPI engine")
            }
        }
    }
}

impl std::error::Error for BackendInitializationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Layout(source) => Some(source),
            Self::Materialization(source) => Some(source),
            Self::LegacyEngineUnavailable => None,
        }
    }
}

impl From<LayoutError> for BackendInitializationError {
    fn from(value: LayoutError) -> Self {
        Self::Layout(value)
    }
}

impl From<MaterializationError> for BackendInitializationError {
    fn from(value: MaterializationError) -> Self {
        Self::Materialization(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dpi_executor::{
        ProcessError, ProcessIdentity, RuntimeProcessGroup, RuntimeProcessLauncher,
    };
    use crate::dpi_materializer::{MaterializedLaunch, RUNTIME_STATE_RELATIVE};
    use crate::protected_layout::RESOURCE_MANIFEST;
    use obsession_runtime_protocol::{
        DpiCategory, DpiRuntimeOptions, DpiSelection, HostsProvider, Zapret2AdaptiveFunction,
        Zapret2AdaptiveOverride, Zapret2AdaptivePayload, Zapret2AdaptiveRange, Zapret2AdaptiveStep,
        Zapret2AdaptiveTransport, Zapret2AdaptiveValue, ZAPRET2_ADAPTIVE_SCHEMA_VERSION,
    };
    use obsession_runtime_reliability::legacy_reliability::assessment::AssessmentClassification;
    use obsession_runtime_reliability::legacy_reliability::contracts::{
        AttemptId, ConfigFingerprint, IntentEnvelope, IntentFence, LaneGeneration,
        NetworkFingerprint, RegistryVersion, SensorGeneration, SessionId,
    };
    use obsession_runtime_reliability::legacy_reliability::recovery::{
        RecoveryConfig, RecoveryOrigin,
    };
    use obsession_runtime_reliability::legacy_reliability::recovery_runtime::IncidentObservation;
    use sha2::{Digest, Sha256};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!("obsession-backend-test-{nonce}"));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn resource(root: &Path, relative: &str, bytes: &[u8]) -> serde_json::Value {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
        let sha256: String = Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        serde_json::json!({
            "path": relative,
            "size": bytes.len(),
            "sha256": sha256,
        })
    }

    struct Fixture {
        _root: TestRoot,
        install_layout: ProtectedLayout,
        state_layout: ProtectedDataLayout,
        executable: PathBuf,
    }

    fn sha256(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn setup_fixture_with_zapret2(include_zapret2: bool) -> Fixture {
        let test = TestRoot::new();
        let program_files = test.0.join("Program Files");
        let install_root = program_files.join("Obsession");
        let program_data = test.0.join("ProgramData");
        let state_root = program_data.join(RUNTIME_STATE_RELATIVE);
        fs::create_dir_all(&program_files).unwrap();
        fs::create_dir_all(&state_root).unwrap();

        let executable_relative = "runtime/legacy/winws.exe";
        let windivert_relative = "runtime/legacy/WinDivert.dll";
        let config_relative = "runtime/configs/discord.conf";
        let hostlist_relative = "lists/discord.txt";
        let files = vec![
            resource(&install_root, executable_relative, b"protected-engine"),
            resource(&install_root, windivert_relative, b"protected-windivert"),
            resource(
                &install_root,
                config_relative,
                b"--wf-tcp=80,443 --hostlist=lists\\discord.txt --new\n",
            ),
            resource(&install_root, hostlist_relative, b"discord.com\n"),
        ];
        let mut engines = vec![serde_json::json!({
            "engine": "legacy",
            "executable": executable_relative,
            "files": files,
            "strategies": [{
                "id": "discord_1.conf",
                "category": "discord",
                "artifact": config_relative,
                "dependencies": [windivert_relative, hostlist_relative]
            }]
        })];
        if include_zapret2 {
            let zapret2_executable = "runtime/zapret2/winws2.exe";
            let pack_manifest = "strategy-packs/builtin/manifest.json";
            let lua_lib = "strategy-packs/builtin/lua/zapret-lib.lua";
            let lua_antidpi = "strategy-packs/builtin/lua/zapret-antidpi.lua";
            let lua_lib_bytes = b"-- protected lua runtime\n";
            let lua_antidpi_bytes = b"-- protected adaptive functions\n";
            let pack = serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "pack_id": "builtin",
                "pack_version": "1.0.0",
                "engine": { "winws2_min": ">=1.0.2", "lua_api": 6 },
                "categories": ["discord"],
                "protocols": ["tcp", "tls"],
                "files": [
                    { "path": "lua/zapret-lib.lua", "sha256": sha256(lua_lib_bytes) },
                    { "path": "lua/zapret-antidpi.lua", "sha256": sha256(lua_antidpi_bytes) }
                ],
                "strategies": [{
                    "id": "discord_tls_text",
                    "category": "discord",
                    "aggressiveness": 1,
                    "lua": "lua/zapret-antidpi.lua",
                    "desync": ["multidisorder_legacy:pos=1,midsld"],
                    "transports": ["tcp", "tls"],
                    "filter_l7": ["tls"],
                    "payload": ["tls_client_hello"],
                    "out_range": "-d10"
                }]
            }))
            .unwrap();
            let zapret2_files = vec![
                resource(&install_root, zapret2_executable, b"protected-zapret2"),
                resource(&install_root, lua_lib, lua_lib_bytes),
                resource(&install_root, lua_antidpi, lua_antidpi_bytes),
                resource(&install_root, hostlist_relative, b"discord.com\n"),
                resource(&install_root, pack_manifest, &pack),
            ];
            engines.push(serde_json::json!({
                "engine": "zapret2",
                "executable": zapret2_executable,
                "files": zapret2_files,
                "strategies": [{
                    "id": "builtin-discord",
                    "category": "discord",
                    "artifact": pack_manifest,
                    "dependencies": [lua_lib, lua_antidpi, hostlist_relative]
                }]
            }));
        }

        let manifest = install_root.join(RESOURCE_MANIFEST);
        fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        fs::write(
            &manifest,
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "engines": engines
            }))
            .unwrap(),
        )
        .unwrap();

        Fixture {
            install_layout: ProtectedLayout::inspect(&program_files, &install_root).unwrap(),
            state_layout: ProtectedDataLayout::inspect(&program_data, &state_root).unwrap(),
            executable: install_root.join(executable_relative),
            _root: test,
        }
    }

    fn setup_fixture() -> Fixture {
        setup_fixture_with_zapret2(false)
    }

    #[derive(Default)]
    struct FakeState {
        launches: usize,
        stops: usize,
        fail_launch: bool,
        corrupt_identity: bool,
    }

    #[derive(Clone)]
    struct FakeLauncher(Arc<Mutex<FakeState>>);

    struct FakeGroup {
        state: Arc<Mutex<FakeState>>,
        identities: Vec<ProcessIdentity>,
    }

    impl RuntimeProcessLauncher for FakeLauncher {
        type Group = FakeGroup;

        fn launch_group(
            &self,
            launches: &[MaterializedLaunch],
            _readiness_timeout: Duration,
        ) -> Result<Self::Group, ProcessError> {
            let mut state = self.0.lock().unwrap();
            state.launches += 1;
            if state.fail_launch {
                return Err(ProcessError::InvalidLaunchPath);
            }
            let identities = launches
                .iter()
                .enumerate()
                .map(|(index, launch)| ProcessIdentity {
                    pid: 100 + index as u32,
                    creation_time_100ns: 500 + index as u64,
                    executable_sha256: if state.corrupt_identity {
                        "0".repeat(64)
                    } else {
                        launch.executable_sha256().to_owned()
                    },
                })
                .collect();
            drop(state);
            Ok(FakeGroup {
                state: self.0.clone(),
                identities,
            })
        }
    }

    impl RuntimeProcessGroup for FakeGroup {
        fn identities(&self) -> &[ProcessIdentity] {
            &self.identities
        }

        fn stop(&mut self, _timeout: Duration) -> Result<(), ProcessError> {
            self.state.lock().unwrap().stops += 1;
            Ok(())
        }
    }

    fn request(engine: DpiEngine) -> DpiStartRequest {
        DpiStartRequest {
            engine,
            selections: vec![DpiSelection {
                category: DpiCategory::Discord,
                strategy_id: "discord_1.conf".into(),
            }],
            options: DpiRuntimeOptions {
                zapret2_level: 0,
                legacy_reliability: false,
                zapret2_overrides: Vec::new(),
            },
        }
    }

    fn backend(
        fixture: &Fixture,
        state: Arc<Mutex<FakeState>>,
    ) -> ProtectedDpiBackend<FakeLauncher> {
        ProtectedDpiBackend::from_preflight(
            &fixture.install_layout,
            fixture.state_layout.clone(),
            FakeLauncher(state),
        )
        .unwrap()
    }

    #[test]
    fn capability_exists_only_after_full_constructor_preflight() {
        let fixture = setup_fixture();
        fs::write(&fixture.executable, b"tampered-before-startup").unwrap();
        let state = Arc::new(Mutex::new(FakeState::default()));
        assert!(matches!(
            ProtectedDpiBackend::from_preflight(
                &fixture.install_layout,
                fixture.state_layout.clone(),
                FakeLauncher(state)
            ),
            Err(BackendInitializationError::Layout(_))
        ));
    }

    #[test]
    fn mutation_capability_is_hidden_without_confirmation_probes() {
        let fixture = setup_fixture();
        let state = Arc::new(Mutex::new(FakeState::default()));
        let mut backend = backend(&fixture, state);
        backend.recovery_probes = None;

        let features = backend.capabilities().unwrap().features;
        assert!(features.contains(&Feature::EyesEvents));
        assert!(features.contains(&Feature::LegacyReliabilityControls));
        assert!(!features.contains(&Feature::LegacyReliability));
        assert_eq!(
            backend.set_legacy_recovery_controls(LegacyRecoveryControlsRequest {
                mode: LegacyRecoveryMode::Automatic,
                automatic_paused: false,
                frozen_categories: Vec::new(),
            }),
            Err(BackendError::ServiceUnavailable)
        );
        assert!(backend
            .set_legacy_recovery_controls(LegacyRecoveryControlsRequest {
                mode: LegacyRecoveryMode::ObserveOnly,
                automatic_paused: true,
                frozen_categories: Vec::new(),
            })
            .is_ok());
    }

    #[test]
    fn legacy_start_snapshot_and_exact_stop_are_exposed() {
        let fixture = setup_fixture();
        let state = Arc::new(Mutex::new(FakeState::default()));
        let mut backend = backend(&fixture, state.clone());
        assert_eq!(
            backend.capabilities().unwrap().features,
            vec![
                Feature::Dpi,
                Feature::EyesEvents,
                Feature::LegacyReliabilityControls,
                Feature::LegacyReliability,
            ]
        );
        assert_eq!(backend.runtime_snapshot().unwrap().dpi, None);

        let started = backend.dpi_start(request(DpiEngine::Legacy)).unwrap();
        assert_eq!(
            backend.runtime_snapshot().unwrap().dpi.unwrap().generation,
            started.generation
        );
        assert_eq!(
            backend.dpi_stop(DpiStopRequest {
                generation: started.generation + 1,
            }),
            Err(BackendError::Conflict)
        );
        backend
            .dpi_stop(DpiStopRequest {
                generation: started.generation,
            })
            .unwrap();
        assert_eq!(backend.runtime_snapshot().unwrap().dpi, None);
        let state = state.lock().unwrap();
        assert_eq!(state.launches, 1);
        assert_eq!(state.stops, 1);
    }

    #[test]
    fn legacy_controls_have_a_service_owned_generation_and_mutation_capability() {
        let fixture = setup_fixture();
        let state = Arc::new(Mutex::new(FakeState::default()));
        let mut backend = backend(&fixture, state);
        assert!(backend
            .capabilities()
            .unwrap()
            .features
            .contains(&Feature::LegacyReliability));

        let request = LegacyRecoveryControlsRequest {
            mode: LegacyRecoveryMode::Automatic,
            automatic_paused: false,
            frozen_categories: vec![DpiCategory::Discord],
        };
        let accepted = backend
            .set_legacy_recovery_controls(request.clone())
            .unwrap();
        assert_eq!(accepted.operation_id, 2);
        assert_eq!(
            backend
                .set_legacy_recovery_controls(request)
                .unwrap()
                .operation_id,
            accepted.operation_id
        );
        let controls = backend.recovery.public_snapshot(true).controls;
        assert_eq!(controls.control_generation, accepted.operation_id);
        assert_eq!(controls.mode, LegacyRecoveryMode::Automatic);
        assert!(!controls.automatic_paused);
        assert_eq!(controls.frozen_categories, [DpiCategory::Discord]);

        assert_eq!(
            backend.set_legacy_recovery_controls(LegacyRecoveryControlsRequest {
                mode: LegacyRecoveryMode::Assisted,
                automatic_paused: false,
                frozen_categories: Vec::new(),
            }),
            Err(BackendError::InvalidRequest)
        );
    }

    #[test]
    fn legacy_replace_requires_exact_generation_and_restores_selection_shape() {
        let fixture = setup_fixture();
        let state = Arc::new(Mutex::new(FakeState::default()));
        let mut backend = backend(&fixture, state.clone());
        let started = backend.dpi_start(request(DpiEngine::Legacy)).unwrap();

        assert_eq!(
            backend.replace_legacy_generation(
                started.generation + 1,
                vec![DpiSelection {
                    category: DpiCategory::Discord,
                    strategy_id: "discord_1.conf".into(),
                }],
            ),
            Err(BackendError::Conflict)
        );
        let replaced = backend
            .replace_legacy_generation(
                started.generation,
                vec![DpiSelection {
                    category: DpiCategory::Discord,
                    strategy_id: "discord_1.conf".into(),
                }],
            )
            .unwrap();
        assert_ne!(replaced.generation, started.generation);
        assert_eq!(
            backend.runtime_snapshot().unwrap().dpi.unwrap().generation,
            replaced.generation
        );
        assert_eq!(state.lock().unwrap().launches, 2);
        assert_eq!(state.lock().unwrap().stops, 1);
    }

    #[test]
    fn zapret2_adaptive_replace_is_capability_gated_and_generation_fenced() {
        let fixture = setup_fixture_with_zapret2(true);
        let state = Arc::new(Mutex::new(FakeState::default()));
        let mut backend = backend(&fixture, state.clone());
        assert!(backend
            .capabilities()
            .unwrap()
            .features
            .contains(&Feature::DpiZapret2Adaptive));

        let initial_request = DpiStartRequest {
            engine: DpiEngine::Zapret2,
            selections: vec![DpiSelection {
                category: DpiCategory::Discord,
                strategy_id: "builtin-discord".into(),
            }],
            options: DpiRuntimeOptions {
                zapret2_level: 1,
                legacy_reliability: false,
                zapret2_overrides: Vec::new(),
            },
        };
        let started = backend.dpi_start(initial_request.clone()).unwrap();
        let adaptive_override = Zapret2AdaptiveOverride {
            schema_version: ZAPRET2_ADAPTIVE_SCHEMA_VERSION,
            category: DpiCategory::Discord,
            transport: Zapret2AdaptiveTransport::Tls,
            steps: vec![Zapret2AdaptiveStep {
                function: Zapret2AdaptiveFunction::MultiDisorderLegacy,
                args: std::collections::BTreeMap::from([(
                    "pos".into(),
                    Zapret2AdaptiveValue::Text("1,midsld".into()),
                )]),
            }],
            payload: Zapret2AdaptivePayload::TlsClientHello,
            out_range: Some(Zapret2AdaptiveRange::FirstTenDataPackets),
        };
        let replacement_request = DpiStartRequest {
            options: DpiRuntimeOptions {
                zapret2_overrides: vec![adaptive_override.clone()],
                ..initial_request.options.clone()
            },
            ..initial_request
        };

        assert_eq!(
            backend.dpi_replace(DpiReplaceRequest {
                expected_generation: started.generation + 1,
                runtime: replacement_request.clone(),
            }),
            Err(BackendError::Conflict)
        );
        let replaced = backend
            .dpi_replace(DpiReplaceRequest {
                expected_generation: started.generation,
                runtime: replacement_request,
            })
            .unwrap();

        assert_ne!(replaced.generation, started.generation);
        assert_eq!(
            backend.runtime_snapshot().unwrap().dpi.unwrap().generation,
            replaced.generation
        );
        assert_eq!(
            backend
                .active_request
                .as_ref()
                .unwrap()
                .options
                .zapret2_overrides,
            vec![adaptive_override]
        );
        let state = state.lock().unwrap();
        assert_eq!(state.launches, 2);
        assert_eq!(state.stops, 1);
    }

    #[test]
    fn recovery_owner_is_joined_by_exact_generation_category_and_selection() {
        let fingerprint = ConfigFingerprint::new("config-fingerprint");
        let scan = LegacyRecoveryScan {
            generation: 7,
            revision: 3,
            session_id: SessionId::new(7),
            input: Some(LegacyRecoveryInput {
                observation: IncidentObservation {
                    session_id: SessionId::new(7),
                    sensor_generation: SensorGeneration::new(11),
                    category: "discord".into(),
                    lane_generation: LaneGeneration::new(13),
                    evidence_epoch: 17,
                    classification: AssessmentClassification::DpiSuspected,
                },
                fence: IntentFence {
                    session_id: SessionId::new(7),
                    category: "discord".into(),
                    lane_generation: LaneGeneration::new(13),
                    sensor_generation: SensorGeneration::new(11),
                    registry_version: RegistryVersion::new(19),
                    network_fingerprint: NetworkFingerprint::Stable {
                        key: "network".into(),
                    },
                },
                previous: RecoveryConfig::new("discord_1.conf", fingerprint.clone()),
                candidates: vec![RecoveryConfig::new("discord_2.conf", "candidate")],
            }),
        };
        let runtime = DpiRuntimeSnapshot {
            generation: 7,
            engine: DpiEngine::Legacy,
            selections: vec![DpiSelection {
                category: DpiCategory::Discord,
                strategy_id: "discord_1.conf".into(),
            }],
        };
        let processes = [ProcessIdentity {
            pid: 23,
            creation_time_100ns: 29,
            executable_sha256: "a".repeat(64),
        }];

        let owner = exact_recovery_owner(Some(&runtime), &processes, &scan).unwrap();
        assert_eq!(owner.pid, 23);
        assert_eq!(owner.process_start_identity, ProcessStartIdentity::new(29));
        assert_eq!(owner.config_fingerprint, fingerprint);
        assert_eq!(owner.lane_generation, LaneGeneration::new(13));

        let mut combined = runtime.clone();
        combined.selections.insert(
            0,
            DpiSelection {
                category: DpiCategory::YoutubeTwitch,
                strategy_id: "youtube_twitch_11.conf".into(),
            },
        );
        assert_eq!(
            exact_recovery_owner(Some(&combined), &processes, &scan),
            Some(owner)
        );
        assert!(exact_recovery_owner(Some(&combined), &[], &scan).is_none());
        assert!(exact_recovery_owner(
            Some(&combined),
            &[processes[0].clone(), processes[0].clone()],
            &scan
        )
        .is_none());
        combined.selections[1].strategy_id = "discord_other.conf".into();
        assert!(exact_recovery_owner(Some(&combined), &processes, &scan).is_none());

        let mut stale = runtime;
        stale.generation = 8;
        assert!(exact_recovery_owner(Some(&stale), &processes, &scan).is_none());
    }

    #[test]
    fn stale_live_observer_fence_cannot_reach_retry_preflight() {
        let fence = IntentFence {
            session_id: SessionId::new(7),
            category: "discord".into(),
            lane_generation: LaneGeneration::new(11),
            sensor_generation: SensorGeneration::new(13),
            registry_version: RegistryVersion::new(17),
            network_fingerprint: NetworkFingerprint::Stable {
                key: "network".into(),
            },
        };
        let previous = RecoveryConfig::new("discord_1.conf", "previous");
        let action = RecoveryAction::Preflight {
            envelope: IntentEnvelope::from_fence(AttemptId::new(19), &fence),
            previous: previous.clone(),
            previous_owner: ProcessOwner {
                pid: 23,
                process_start_identity: ProcessStartIdentity::new(29),
                config_fingerprint: previous.fingerprint().clone(),
                lane_generation: fence.lane_generation,
            },
            candidate: RecoveryConfig::new("discord_2.conf", "candidate"),
            origin: RecoveryOrigin::Automatic {
                control_generation: 31,
            },
        };

        assert!(preflight_action_matches_live_fence(&action, Some(&fence)));
        let stale = IntentFence {
            sensor_generation: SensorGeneration::new(37),
            ..fence
        };
        assert!(!preflight_action_matches_live_fence(&action, Some(&stale)));
        assert!(!preflight_action_matches_live_fence(&action, None));
    }

    #[test]
    fn zapret2_hosts_and_firewall_stay_closed() {
        let fixture = setup_fixture();
        let state = Arc::new(Mutex::new(FakeState::default()));
        let mut backend = backend(&fixture, state.clone());
        assert_eq!(
            backend.dpi_start(request(DpiEngine::Zapret2)),
            Err(BackendError::ServiceUnavailable)
        );
        assert_eq!(
            backend.hosts_install(HostsMutationRequest {
                provider: HostsProvider::Malw,
            }),
            Err(BackendError::ServiceUnavailable)
        );
        assert_eq!(
            backend.firewall_close_proxy_lan(),
            Err(BackendError::ServiceUnavailable)
        );
        assert_eq!(state.lock().unwrap().launches, 0);
    }

    #[test]
    fn runtime_and_integrity_failures_map_to_stable_wire_errors() {
        let fixture = setup_fixture();
        let process_state = Arc::new(Mutex::new(FakeState {
            fail_launch: true,
            ..FakeState::default()
        }));
        let mut process_backend = backend(&fixture, process_state);
        assert_eq!(
            process_backend.dpi_start(request(DpiEngine::Legacy)),
            Err(BackendError::RuntimeFailed)
        );

        let fixture = setup_fixture();
        let integrity_state = Arc::new(Mutex::new(FakeState::default()));
        let mut integrity_backend = backend(&fixture, integrity_state.clone());
        fs::write(&fixture.executable, b"tampered-after-startup").unwrap();
        assert_eq!(
            integrity_backend.dpi_start(request(DpiEngine::Legacy)),
            Err(BackendError::ProtectedResourceInvalid)
        );
        assert_eq!(integrity_state.lock().unwrap().launches, 0);
    }

    #[test]
    fn invalid_process_identity_is_stopped_and_reported_as_runtime_failure() {
        let fixture = setup_fixture();
        let state = Arc::new(Mutex::new(FakeState {
            corrupt_identity: true,
            ..FakeState::default()
        }));
        let mut backend = backend(&fixture, state.clone());
        assert_eq!(
            backend.dpi_start(request(DpiEngine::Legacy)),
            Err(BackendError::RuntimeFailed)
        );
        let state = state.lock().unwrap();
        assert_eq!(state.launches, 1);
        assert_eq!(state.stops, 1);
    }
}
