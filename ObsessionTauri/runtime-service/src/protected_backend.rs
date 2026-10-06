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
use crate::dns_recovery::DnsRecovery;
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
    packet_observer: crate::packet_observer::PacketObserver,
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
    dns_recovery: Option<DnsRecovery>,
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
        let packet_observer = crate::packet_observer::PacketObserver::from_catalog(&catalog);
        let hosts = HostsController::discover(&state_layout).ok();
        let dns_recovery = DnsRecovery::discover(&state_layout).ok();
        let proxy_lan_firewall = ProxyLanFirewall::discover(&state_layout).ok();
        Ok(Self {
            catalog,
            observer,
            packet_observer,
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
            dns_recovery,
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
                zapret2_level: 0, zapret2_clienthello: false, zapret2_profiles: Vec::new(),
                legacy_reliability: observer_active,
                zapret2_overrides: Vec::new(),
                zapret2_diagnostics: false,
            },
        };
        let candidate_request = DpiStartRequest {
            engine: DpiEngine::Legacy,
            selections: selections.clone(),
            options: obsession_runtime_protocol::DpiRuntimeOptions {
                zapret2_level: 0, zapret2_clienthello: false, zapret2_profiles: Vec::new(),
                legacy_reliability: observer_active,
                zapret2_overrides: Vec::new(),
                zapret2_diagnostics: false,
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
        }
        if self.observer.is_some() {
            features.push(Feature::EyesEvents);
            features.push(Feature::LegacyReliabilityControls);
            if self.recovery_probes.is_some() {
                features.push(Feature::LegacyReliability);
            }
        }
        if self.packet_observer.available() {
            features.push(Feature::DpiZapret2PacketDiagnostics);
        }
        if self.hosts.is_some() {
            features.push(Feature::Hosts);
            features.push(Feature::HostsHealthV2);
        }
        if self.dns_recovery.is_some() {
            features.push(Feature::SystemDnsRestore);
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
        self.packet_observer.start(started.generation, plan.engine() == DpiEngine::Zapret2 && plan.zapret2_diagnostics());
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
        let stop_result = self.executor.stop(request.expected_generation);
        self.packet_observer.stop(request.expected_generation);
        stop_result.map_err(map_executor_error)?;

        match self.executor.start(&replacement_plan) {
            Ok(started) => {
                self.packet_observer.start(started.generation, replacement_plan.zapret2_diagnostics());
                self.active_request = Some(request.runtime);
                Ok(started)
            }
            Err(error) => {
                self.active_request = None;
                let failure = map_executor_error(error);
                if let Ok(restored) = self.executor.start(&previous_plan) {
                    self.packet_observer.start(restored.generation, previous_plan.zapret2_diagnostics());
                    self.active_request = Some(previous_request);
                    return Err(BackendError::ReplacementRolledBack {
                        code: failure.wire_code(),
                        detail: match failure {
                            BackendError::RuntimeProcessFailed(detail) | BackendError::ProtectedMaterializationFailed(detail) => Some(detail),
                            _ => None,
                        },
                        generation: restored.generation,
                    });
                }
                Err(failure)
            }
        }
    }

    fn dpi_diagnostics(&self, request: DpiStopRequest) -> Result<obsession_runtime_protocol::DpiDiagnostics, BackendError> {
        let mut diagnostics = self.executor.diagnostics(request.generation).map_err(map_executor_error)?;
        diagnostics.packet_capture = self.packet_observer.snapshot(request.generation);
        Ok(diagnostics)
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
        let stop_result = self.executor.stop(request.generation);
        self.packet_observer.stop(request.generation);
        stop_result.map_err(map_executor_error)?;
        self.active_request = None;
        if let Some(result) = observer_result {
            result.map_err(map_observer_error)?;
        }
        Ok(())
    }

    fn restore_system_dns(&mut self) -> Result<obsession_runtime_protocol::SystemDnsStatus, BackendError> {
        self.dns_recovery.as_mut().ok_or(BackendError::ServiceUnavailable)?.restore()
    }

    fn system_dns_status(&self) -> Result<obsession_runtime_protocol::SystemDnsStatus, BackendError> {
        self.dns_recovery.as_ref().ok_or(BackendError::ServiceUnavailable)?.status()
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
        let engine_exited = self.executor.snapshot().is_none_or(|runtime|
            self.executor.diagnostics(runtime.generation).is_ok_and(|diagnostics|
                diagnostics.processes.iter().any(|process| process.exit_code.is_some())));
        self.packet_observer.poll(engine_exited);
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
        ExecutorError::Materialization(error) => BackendError::ProtectedMaterializationFailed(
            materialization_failure_detail(&error)),
        ExecutorError::InvalidOwnership => BackendError::RuntimeProcessFailed(
            obsession_runtime_protocol::RuntimeFailureDetail::OwnershipMismatch),
        ExecutorError::Process(error) => {
            use crate::dpi_executor::ProcessError;
            use obsession_runtime_protocol::RuntimeFailureDetail as Detail;
            BackendError::RuntimeProcessFailed(match error {
                ProcessError::ExitedBeforeReady { exit_code, .. } => Detail::ExitedBeforeReady { exit_code },
                ProcessError::StartupFailed { exit_code, diagnostics, .. } => Detail::StartupFailed { exit_code, diagnostics },
                ProcessError::Windows(error) => Detail::Windows { hresult: error.code().0 },
                ProcessError::InvalidLaunchPath => Detail::InvalidLaunchPath,
                ProcessError::NoLaunches => Detail::NoLaunches,
                ProcessError::ResumeFailed => Detail::ResumeFailed,
                ProcessError::StopTimedOut => Detail::StopTimedOut,
                ProcessError::UnexpectedWaitResult(result) => Detail::UnexpectedWait { result },
            })
        },
    }
}

fn materialization_failure_detail(error: &MaterializationError) -> obsession_runtime_protocol::RuntimeFailureDetail {
    use obsession_runtime_protocol::{RuntimeFailureDetail, RuntimeMaterializationStage as Stage};
    let materialization_stage = match error {
        MaterializationError::InvalidGeneration
        | MaterializationError::GenerationAlreadyExists(_)
        | MaterializationError::GenerationExhausted
        | MaterializationError::InvalidGenerationState => Stage::GenerationReservation,
        MaterializationError::Layout(_) => Stage::ResourceVerification,
        MaterializationError::UndeclaredConfigPath(_) => Stage::ConfigCompilation,
        MaterializationError::InvalidAutoHostlist => Stage::ConfigVerification,
        MaterializationError::InvalidConfig(reason) if reason.starts_with("materialized") => Stage::ConfigVerification,
        MaterializationError::InvalidConfig(_) => Stage::ConfigCompilation,
        MaterializationError::Io { operation, .. } if operation.contains("generation sequence")
            || operation.contains("generation number") || *operation == "create generation directory" => Stage::GenerationReservation,
        MaterializationError::Io { operation: "create service-owned file" | "write service-owned file", .. } => Stage::ConfigWrite,
        MaterializationError::Io { operation: "remove exact materialized generation", .. } => Stage::Cleanup,
        _ => Stage::StateLayout,
    };
    let windows_error = match error {
        MaterializationError::AclQueryFailed { code, .. } => Some(*code),
        MaterializationError::Canonicalize { source, .. } | MaterializationError::Io { source, .. } => source.raw_os_error().map(|code| code as u32),
        MaterializationError::Layout(LayoutError::Canonicalize { source, .. } | LayoutError::Read { source, .. }) => source.raw_os_error().map(|code| code as u32),
        _ => None,
    };
    RuntimeFailureDetail::Materialization { materialization_stage, windows_error }
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
        DpiCategory, DpiRuntimeOptions, DpiSelection, HostsProvider,
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
            let lua_diagnostic = "strategy-packs/builtin/lua/obsession-diagnostic.lua";
            let lua_clienthello = "strategy-packs/builtin/lua/obsession-clienthello.lua";
            let lua_lib_bytes = b"-- protected lua runtime\n";
            let lua_antidpi_bytes = b"-- protected adaptive functions\n";
            let lua_diagnostic_bytes = b"-- protected diagnostic observer\n";
            let lua_clienthello_bytes = b"-- protected ClientHello experiment\n";
            let pack = serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "pack_id": "builtin",
                "pack_version": "1.0.0",
                "engine": { "winws2_min": ">=1.0.2", "lua_api": 6 },
                "categories": ["discord"],
                "protocols": ["tcp", "tls", "udp", "quic"],
                "files": [
                    { "path": "lua/zapret-lib.lua", "sha256": sha256(lua_lib_bytes) },
                    { "path": "lua/zapret-antidpi.lua", "sha256": sha256(lua_antidpi_bytes) },
                    { "path": "lua/obsession-diagnostic.lua", "sha256": sha256(lua_diagnostic_bytes) },
                    { "path": "lua/obsession-clienthello.lua", "sha256": sha256(lua_clienthello_bytes) }
                ],
                "experiments": [{
                    "id": "discord_tls_clienthello_clone", "category": "discord", "aggressiveness": 1,
                    "lua": "lua/obsession-clienthello.lua", "desync": ["obsession_clienthello"],
                    "transports": ["tcp", "tls"], "filter_tcp": "443,2053,2083,2087,2096,8443",
                    "filter_l7": ["tls"], "payload": ["tls_client_hello"], "out_range": "-d10"
                }],
                "presets": [{
                    "id": "classic_discord_tls_split_sni", "category": "discord", "aggressiveness": 1,
                    "lua": "lua/zapret-antidpi.lua", "desync": ["multisplit:pos=midsld"],
                    "transports": ["tcp", "tls"], "filter_l7": ["tls"], "payload": ["tls_client_hello"], "out_range": "-d10"
                }],
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
                }, {
                    "id": "discord_quic_level_1", "category": "discord", "aggressiveness": 1,
                    "lua": "lua/zapret-antidpi.lua", "desync": ["fake:blob=fake_default_quic:repeats=1"],
                    "transports": ["udp", "quic"], "filter_udp": "443", "filter_l7": ["quic"], "payload": ["quic_initial"]
                }]
            }))
            .unwrap();
            let zapret2_files = vec![
                resource(&install_root, zapret2_executable, b"protected-zapret2"),
                resource(&install_root, lua_lib, lua_lib_bytes),
                resource(&install_root, lua_antidpi, lua_antidpi_bytes),
                resource(&install_root, lua_diagnostic, lua_diagnostic_bytes),
                resource(&install_root, lua_clienthello, lua_clienthello_bytes),
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
                    "dependencies": [lua_lib, lua_antidpi, lua_diagnostic, lua_clienthello, hostlist_relative]
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
        fail_on_launch: Option<usize>,
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
            if state.fail_on_launch == Some(state.launches) {
                return Err(ProcessError::ExitedBeforeReady { pid: 100, exit_code: 87 });
            }
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
                zapret2_level: 0, zapret2_clienthello: false, zapret2_profiles: Vec::new(),
                legacy_reliability: false,
                zapret2_overrides: Vec::new(),
                zapret2_diagnostics: false,
            },
        }
    }

    fn backend(
        fixture: &Fixture,
        state: Arc<Mutex<FakeState>>,
    ) -> ProtectedDpiBackend<FakeLauncher> {
        let mut backend = ProtectedDpiBackend::from_preflight(
            &fixture.install_layout,
            fixture.state_layout.clone(),
            FakeLauncher(state),
        )
        .unwrap();
        // These tests exercise DPI/reliability only. Never dispatch native
        // machine DNS operations from a fixture.
        backend.dns_recovery = None;
        backend
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
    fn automatic_dns_mutations_are_disabled_for_every_dpi_generation() {
        let fixture = setup_fixture();
        let state = Arc::new(Mutex::new(FakeState::default()));
        let mut backend = backend(&fixture, state);
        let dns_request = |generation| obsession_runtime_protocol::SystemDnsRequest { generation };
        assert_eq!(backend.repair_system_dns(dns_request(1)), Err(BackendError::ServiceUnavailable));
        assert_eq!(backend.flush_system_dns(dns_request(1)), Err(BackendError::ServiceUnavailable));
        let started = backend.dpi_start(request(DpiEngine::Legacy)).unwrap();
        assert_eq!(backend.repair_system_dns(dns_request(started.generation + 1)), Err(BackendError::ServiceUnavailable));
        assert_eq!(backend.flush_system_dns(dns_request(started.generation + 1)), Err(BackendError::ServiceUnavailable));
        assert_eq!(backend.repair_system_dns(dns_request(started.generation)), Err(BackendError::ServiceUnavailable));
        assert_eq!(backend.flush_system_dns(dns_request(started.generation)), Err(BackendError::ServiceUnavailable));
        assert!(!backend.capabilities().unwrap().features.contains(&Feature::SystemDnsRecovery));
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
    fn diagnostic_sensor_is_explicit_and_does_not_start_legacy_recovery() {
        use obsession_runtime_protocol::DpiPacketCaptureState;
        let fixture = setup_fixture_with_zapret2(true);
        let state = Arc::new(Mutex::new(FakeState::default()));
        let mut backend = backend(&fixture, state);
        let diagnostic = DpiStartRequest {
            engine: DpiEngine::Zapret2,
            selections: vec![DpiSelection { category: DpiCategory::Discord, strategy_id: "builtin-discord".into() }],
            options: DpiRuntimeOptions { zapret2_level: 1, zapret2_clienthello: false, zapret2_profiles: Vec::new(), legacy_reliability: false,
                zapret2_overrides: vec![], zapret2_diagnostics: true },
        };
        let started = backend.dpi_start(diagnostic.clone()).unwrap();
        let capture = backend.dpi_diagnostics(DpiStopRequest { generation: started.generation }).unwrap().packet_capture.unwrap();
        assert_eq!(capture.generation, started.generation);
        // The fixture deliberately has no real driver DLL. Missing observation
        // must be explicit, and must not turn a fake-launcher test into capture.
        assert_eq!(capture.state, DpiPacketCaptureState::Unavailable);
        assert!(backend.runtime_snapshot().unwrap().legacy_reliability.is_none());
        let replaced = backend.dpi_replace(DpiReplaceRequest { expected_generation: started.generation, runtime: diagnostic.clone() }).unwrap();
        assert_eq!(backend.dpi_diagnostics(DpiStopRequest { generation: started.generation }), Err(BackendError::Conflict));
        assert_eq!(backend.dpi_diagnostics(DpiStopRequest { generation: replaced.generation }).unwrap().packet_capture.unwrap().generation, replaced.generation);
        let normal = DpiStartRequest { options: DpiRuntimeOptions { zapret2_diagnostics: false, ..diagnostic.options }, ..diagnostic };
        let normal = backend.dpi_replace(DpiReplaceRequest { expected_generation: replaced.generation, runtime: normal }).unwrap();
        assert!(backend.dpi_diagnostics(DpiStopRequest { generation: normal.generation }).unwrap().packet_capture.is_none());
        backend.dpi_stop(DpiStopRequest { generation: normal.generation }).unwrap();
    }

    #[test]
    fn diagnostic_observer_preserves_strategy_arguments_and_requires_verified_resource() {
        let fixture = setup_fixture_with_zapret2(true);
        let catalog = fixture.install_layout.load_verified_catalog().unwrap();
        let mut request = DpiStartRequest { engine: DpiEngine::Zapret2,
            selections: vec![DpiSelection { category: DpiCategory::Discord, strategy_id: "builtin-discord".into() }],
            options: DpiRuntimeOptions { zapret2_level: 1, zapret2_clienthello: false, zapret2_profiles: Vec::new(), legacy_reliability: false,
                zapret2_overrides: vec![], zapret2_diagnostics: false } };
        let normal_plan = catalog.resolve_dpi_plan(&request).unwrap();
        let normal = fixture.state_layout.materialize(&normal_plan, 10).unwrap();
        request.options.zapret2_diagnostics = true;
        let observed_plan = catalog.resolve_dpi_plan(&request).unwrap();
        assert_ne!(normal_plan.fingerprint(), observed_plan.fingerprint());
        let observed = fixture.state_layout.materialize(&observed_plan, 11).unwrap();
        let arguments = observed.launches()[0].arguments();
        assert_eq!(&arguments[arguments.len()-3..], ["--new", "--name=obsession_unmatched", "--lua-desync=obsession_trace:profile=unmatched:routing"]);
        let original_arguments = arguments[..arguments.len()-3].iter().filter(|arg|
            !arg.ends_with("obsession-diagnostic.lua")
                && !arg.starts_with("--lua-desync=obsession_trace:"))
            .cloned().collect::<Vec<_>>();
        assert_eq!(original_arguments, normal.launches()[0].arguments());
        // Loading diagnostic helpers must not take over ordinary builtin profiles.
        assert!(!arguments.iter().any(|arg| arg.starts_with("--lua-desync=obsession_execute:")));
        let lua = fixture.install_layout.root().join("strategy-packs/builtin/lua/obsession-diagnostic.lua");
        fs::write(lua, b"modified observer").unwrap();
        assert!(observed_plan.reverify().is_err());
    }


    #[test]
    fn failed_replace_attests_the_restarted_previous_request_and_native_error() {
        let fixture = setup_fixture_with_zapret2(true);
        let state = Arc::new(Mutex::new(FakeState {
            fail_on_launch: Some(2), ..Default::default()
        }));
        let mut backend = backend(&fixture, state.clone());
        let previous = DpiStartRequest {
            engine: DpiEngine::Zapret2,
            selections: vec![DpiSelection {
                category: DpiCategory::Discord, strategy_id: "builtin-discord".into(),
            }],
            options: DpiRuntimeOptions {
                zapret2_level: 1, zapret2_clienthello: false, zapret2_profiles: Vec::new(), legacy_reliability: false, zapret2_overrides: vec![],
                zapret2_diagnostics: false,
            },
        };
        let started = backend.dpi_start(previous.clone()).unwrap();
        let replacement = DpiStartRequest {
            options: DpiRuntimeOptions { zapret2_level: 2, ..previous.options.clone() },
            ..previous.clone()
        };
        let error = backend.dpi_replace(DpiReplaceRequest {
            expected_generation: started.generation, runtime: replacement,
        }).unwrap_err();
        let restored = backend.runtime_snapshot().unwrap().dpi.unwrap();
        assert!(restored.generation > started.generation);
        assert_eq!(error, BackendError::ReplacementRolledBack {
            code: obsession_runtime_protocol::ServiceErrorCode::RuntimeFailed,
            detail: Some(obsession_runtime_protocol::RuntimeFailureDetail::ExitedBeforeReady { exit_code: 87 }),
            generation: restored.generation,
        });
        assert_eq!(backend.active_request.as_ref(), Some(&previous));
        let state = state.lock().unwrap();
        assert_eq!(state.launches, 3);
        assert_eq!(state.stops, 1);
    }

    #[test]
    fn retired_override_cannot_replace_a_running_manual_configuration() {
        let fixture = setup_fixture_with_zapret2(true);
        let state = Arc::new(Mutex::new(FakeState::default()));
        let mut backend = backend(&fixture, state.clone());
        assert!(!backend.capabilities().unwrap().features.contains(&Feature::DpiZapret2Adaptive));
        let original = DpiStartRequest {
            engine: DpiEngine::Zapret2,
            selections: vec![DpiSelection {
                category: DpiCategory::Discord,
                strategy_id: "builtin-discord".into(),
            }],
            options: DpiRuntimeOptions {
                zapret2_level: 1, zapret2_clienthello: false, zapret2_profiles: Vec::new(),
                legacy_reliability: false,
                zapret2_overrides: Vec::new(),
                zapret2_diagnostics: false,
            },
        };
        let started = backend.dpi_start(original.clone()).unwrap();
        let mut retired = original.clone();
        retired.options.zapret2_overrides = vec![serde_json::json!({
            "category": "discord", "steps": []
        })];
        assert!(backend.dpi_replace(DpiReplaceRequest {
            expected_generation: started.generation,
            runtime: retired,
        }).is_err());
        assert_eq!(backend.runtime_snapshot().unwrap().dpi.unwrap().generation, started.generation);
        assert_eq!(backend.active_request.as_ref(), Some(&original));
        let state = state.lock().unwrap();
        assert_eq!(state.launches, 1);
        assert_eq!(state.stops, 0);
    }

    #[test]
    fn invalid_named_profile_is_rejected_before_stopping_active_generation() {
        use obsession_runtime_protocol::Zapret2ProfileSelection;
        let fixture = setup_fixture_with_zapret2(true);
        let state = Arc::new(Mutex::new(FakeState::default()));
        let mut backend = backend(&fixture, state.clone());
        let mut original = request(DpiEngine::Zapret2);
        original.selections[0].strategy_id = "builtin-discord".into();
        let started = backend.dpi_start(original.clone()).unwrap();
        for (transport, id) in [("tls", "unknown_profile"), ("tls", "discord_quic_level_1"), ("quic", "discord_tls_text")] {
            let mut replacement = original.clone();
            replacement.options.zapret2_profiles = vec![Zapret2ProfileSelection { category: "discord".into(), transport: transport.into(), profile_id: id.into() }];
            assert!(backend.dpi_replace(DpiReplaceRequest { expected_generation: started.generation, runtime: replacement }).is_err());
            assert_eq!(backend.runtime_snapshot().unwrap().dpi.unwrap().generation, started.generation);
            assert_eq!(backend.active_request.as_ref(), Some(&original));
        }
        let state = state.lock().unwrap();
        assert_eq!(state.launches, 1);
        assert_eq!(state.stops, 0);
    }

    #[test]
    fn named_profiles_use_canonical_plan_identity_and_verified_native_resources() {
        use obsession_runtime_protocol::Zapret2ProfileSelection;
        let fixture = setup_fixture_with_zapret2(true);
        let catalog = fixture.install_layout.load_verified_catalog().unwrap();
        let mut request = request(DpiEngine::Zapret2);
        request.selections[0].strategy_id = "builtin-discord".into();
        let original = catalog.resolve_dpi_plan(&request).unwrap();
        request.options.zapret2_profiles = vec![
            Zapret2ProfileSelection { category: "discord".into(), transport: "tls".into(), profile_id: "classic_discord_tls_split_sni".into() },
            Zapret2ProfileSelection { category: "discord".into(), transport: "quic".into(), profile_id: "discord_quic_level_1".into() },
        ];
        let explicit = catalog.resolve_dpi_plan(&request).unwrap();
        assert_ne!(explicit.fingerprint(), original.fingerprint());
        request.options.zapret2_profiles.reverse();
        let reversed = catalog.resolve_dpi_plan(&request).unwrap();
        assert_eq!(explicit, reversed);
        let materialized = fixture.state_layout.materialize(&explicit, 200).unwrap();
        let arguments = materialized.launches()[0].arguments();
        assert!(arguments.iter().any(|argument| argument == "--name=classic_discord_tls_split_sni"));
        assert!(arguments.iter().any(|argument| argument == "--lua-desync=multisplit:pos=midsld"));
        assert!(arguments.iter().any(|argument| argument == "--name=discord_quic_level_1"));
        assert!(!arguments.iter().any(|argument| argument.contains("obsession_execute")));
        fs::write(fixture.install_layout.root().join("strategy-packs/builtin/lua/zapret-antidpi.lua"), b"tampered").unwrap();
        assert!(catalog.resolve_dpi_plan(&request).is_err());
    }

    #[test]
    fn clienthello_uses_only_verified_experiment_and_changes_plan_identity() {
        let fixture = setup_fixture_with_zapret2(true);
        let catalog = fixture.install_layout.load_verified_catalog().unwrap();
        let mut runtime = request(DpiEngine::Zapret2);
        runtime.selections[0].strategy_id = "builtin-discord".into();
        let normal = catalog.resolve_dpi_plan(&runtime).unwrap();
        let original = fixture.state_layout.materialize(&normal, 1).unwrap();
        runtime.options.zapret2_clienthello = true;
        let experiment = catalog.resolve_dpi_plan(&runtime).unwrap();
        assert_ne!(normal.fingerprint(), experiment.fingerprint());
        let experimental = fixture.state_layout.materialize(&experiment, 2).unwrap();
        let arguments = experimental.launches()[0].arguments();
        assert!(arguments.iter().any(|arg| arg == "--lua-desync=obsession_clienthello"));
        assert!(arguments.iter().any(|arg| arg.ends_with("obsession-clienthello.lua")));
        assert!(!arguments.iter().any(|arg| arg.starts_with("--lua-desync=multidisorder_legacy:")));
        assert!(original.launches()[0].arguments().iter().any(|arg| arg.starts_with("--lua-desync=multidisorder_legacy:")));
        fs::write(fixture.install_layout.root().join("strategy-packs/builtin/lua/obsession-clienthello.lua"), b"tampered experiment").unwrap();
        assert!(experiment.reverify().is_err());
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
            Err(BackendError::RuntimeProcessFailed(obsession_runtime_protocol::RuntimeFailureDetail::InvalidLaunchPath))
        );

        let fixture = setup_fixture();
        let integrity_state = Arc::new(Mutex::new(FakeState::default()));
        let mut integrity_backend = backend(&fixture, integrity_state.clone());
        fs::write(&fixture.executable, b"tampered-after-startup").unwrap();
        assert_eq!(
            integrity_backend.dpi_start(request(DpiEngine::Legacy)),
            Err(BackendError::ProtectedMaterializationFailed(
                obsession_runtime_protocol::RuntimeFailureDetail::Materialization {
                    materialization_stage: obsession_runtime_protocol::RuntimeMaterializationStage::ResourceVerification,
                    windows_error: None,
                }
            ))
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
            Err(BackendError::RuntimeProcessFailed(obsession_runtime_protocol::RuntimeFailureDetail::OwnershipMismatch))
        );
        let state = state.lock().unwrap();
        assert_eq!(state.launches, 1);
        assert_eq!(state.stops, 1);
    }
    #[test]
    fn process_exit_detail_survives_backend_mapping() {
        let mapped = map_executor_error(ExecutorError::Process(
            crate::dpi_executor::ProcessError::ExitedBeforeReady { pid: 123, exit_code: 87 }));
        assert_eq!(mapped, BackendError::RuntimeProcessFailed(
            obsession_runtime_protocol::RuntimeFailureDetail::ExitedBeforeReady { exit_code: 87 }));
        assert_eq!(mapped.wire_code(), obsession_runtime_protocol::ServiceErrorCode::RuntimeFailed);
    }

    #[test]
    fn generation_reservation_detail_keeps_native_error_and_resource_wire_code() {
        let mapped = map_executor_error(ExecutorError::Materialization(MaterializationError::Io {
            operation: "create generation directory",
            path: PathBuf::from("never-sent-private-path"),
            source: std::io::Error::from_raw_os_error(183),
        }));
        assert_eq!(mapped, BackendError::ProtectedMaterializationFailed(
            obsession_runtime_protocol::RuntimeFailureDetail::Materialization {
                materialization_stage: obsession_runtime_protocol::RuntimeMaterializationStage::GenerationReservation,
                windows_error: Some(183),
            }
        ));
        assert_eq!(mapped.wire_code(), obsession_runtime_protocol::ServiceErrorCode::ProtectedResourceInvalid);
    }

}
