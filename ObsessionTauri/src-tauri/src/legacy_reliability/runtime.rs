//! Async shell for observe-only assessment, bounded Gate probes, and local log.
//!
//! Network probes and privacy-safe diagnostics are allowed in Phase 2; process,
//! selection, config, and cache mutations remain intentionally absent.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use tauri::AppHandle;
use tokio::sync::{watch, Mutex, Notify};
use tokio::task::JoinHandle;

use super::assessment::AssessmentClassification;
use super::contracts::{LaneGeneration, LegacySessionContext, SensorGeneration};
use super::environment_gate::{
    EnvironmentGate, GateReport, GateRequest, GateRequestError, LocalNetworkSnapshot,
};
use super::health::AtomicHealthCounters;
use super::ingress::{channel, LegacyIngress};
use super::manager::{
    ObserveOnlyManager, ObserveOnlySnapshot, PreparedEnvironmentGate, ReceiveOutcome,
};
use super::reliability_log::{
    AssessmentKind as LogAssessmentKind, ClassificationKind as LogClassificationKind,
    GateSummary as LogGateSummary, IntentKind as LogIntentKind, ReliabilityCounters,
    ReliabilityEventKind, ReliabilityLane, ReliabilityLog, ReliabilityRecord,
};
use super::target_registry::TargetRegistry;

const HEALTH_POLL_INTERVAL: Duration = Duration::from_millis(500);
const BASELINE_RETRY_INTERVAL: Duration = Duration::from_secs(60);
const BASELINE_REFRESH_INTERVAL: Duration = Duration::from_secs(12 * 60 * 60);

/// Handle kept by AppState for one Legacy observe-only session.
pub struct LegacyReliabilityHandle {
    pub ingress: LegacyIngress,
    status: watch::Receiver<ObserveOnlySnapshot>,
    registry: Arc<TargetRegistry>,
    gate: Option<Arc<Mutex<EnvironmentGate>>>,
    stop: watch::Sender<bool>,
    wake: Arc<Notify>,
    join: JoinHandle<()>,
}

impl LegacyReliabilityHandle {
    pub fn snapshot(&self) -> ObserveOnlySnapshot {
        read_coherent_snapshot_with_hook(&self.status, &self.ingress, || {})
    }

    /// Immutable registry used by both Eyes and this exact Manager generation.
    /// Assisted preflight may inspect it, but cannot mutate active ownership.
    pub fn registry(&self) -> Arc<TargetRegistry> {
        Arc::clone(&self.registry)
    }

    /// Shared Environment Gate preserves the process-local latency baseline
    /// learned by the observe-only runtime. The scoped executor must re-run
    /// this exact gate before its first process side effect.
    pub fn environment_gate(&self) -> Option<Arc<Mutex<EnvironmentGate>>> {
        self.gate.as_ref().map(Arc::clone)
    }

    /// Forwards health/lifecycle snapshots into the app-owned public status.
    /// The publisher performs exact session+sensor fencing and deduplicates the
    /// public projection, so frequent manager polls cannot create an event storm.
    pub fn forward_public_status(&self, app: AppHandle) {
        let mut status = self.status.clone();
        let owner = self.snapshot();
        let session_id = owner.session.session_id;
        let sensor_generation = owner.session.sensor_generation;
        let active_categories = owner.session.active_categories;
        tauri::async_runtime::spawn(async move {
            loop {
                match status.changed().await {
                    Ok(()) => {
                        let snapshot = status.borrow_and_update().clone();
                        super::status::publish_snapshot_if_owned(&app, &snapshot);
                    }
                    Err(_) => {
                        // A panic/abort can close the watch sender without a
                        // terminal snapshot. Normal stop already published
                        // inactive, so its old owner is rejected here.
                        super::status::publish_if_owned(
                            &app,
                            session_id,
                            sensor_generation,
                            super::status::LegacyReliabilityStatus::blind(
                                active_categories,
                                session_id,
                                sensor_generation,
                            ),
                        );
                        break;
                    }
                }
            }
        });
    }

    pub async fn shutdown(self) {
        let _ = self.stop.send(true);
        self.wake.notify_one();
        let mut join = self.join;
        if tokio::time::timeout(Duration::from_secs(1), &mut join)
            .await
            .is_err()
        {
            join.abort();
            let _ = join.await;
        }
    }
}

fn read_coherent_snapshot_with_hook(
    status: &watch::Receiver<ObserveOnlySnapshot>,
    ingress: &LegacyIngress,
    mut after_borrow: impl FnMut(),
) -> ObserveOnlySnapshot {
    let mut status = status.clone();
    loop {
        let mut snapshot = status.borrow_and_update().clone();
        after_borrow();
        let pending_ingress_control_events = ingress.pending_control_event_count();
        let pending_ingress_flow_events = ingress.pending_flow_event_counts();
        match status.has_changed() {
            Ok(true) => continue,
            Ok(false) => {
                snapshot.pending_ingress_control_events = pending_ingress_control_events;
                snapshot.pending_ingress_flow_events = pending_ingress_flow_events;
                return snapshot;
            }
            Err(_) => {
                snapshot.pending_ingress_control_events = pending_ingress_control_events;
                snapshot.pending_ingress_flow_events = pending_ingress_flow_events;
                snapshot.session.closed = true;
                snapshot.health.state = super::contracts::EyeHealthState::Blind;
                snapshot.health.receiver_failed = true;
                return snapshot;
            }
        }
    }
}

/// Creates an observe-only manager and its capture-side sender. No process,
/// cache, UI, or Brain action is performed by this function or its task.
pub fn spawn(
    context: LegacySessionContext,
    sensor_generation: SensorGeneration,
    registry: Arc<TargetRegistry>,
    lane_generations: BTreeMap<String, LaneGeneration>,
    log_root: PathBuf,
    started_at_monotonic_ms: u64,
) -> Result<LegacyReliabilityHandle, super::ingress::FenceBuildError> {
    spawn_with_environment_gate(
        context,
        sensor_generation,
        registry,
        lane_generations,
        log_root,
        None,
        started_at_monotonic_ms,
    )
}

/// Rebuilds a Manager/Eyes generation while retaining the already learned
/// process-local Environment Gate baseline. This is used only by the scoped
/// assisted executor when it installs a tentative candidate sensor plan.
pub fn spawn_with_environment_gate(
    context: LegacySessionContext,
    sensor_generation: SensorGeneration,
    registry: Arc<TargetRegistry>,
    lane_generations: BTreeMap<String, LaneGeneration>,
    log_root: PathBuf,
    existing_gate: Option<Arc<Mutex<EnvironmentGate>>>,
    started_at_monotonic_ms: u64,
) -> Result<LegacyReliabilityHandle, super::ingress::FenceBuildError> {
    let (ingress, receiver) = channel();
    let counters: Arc<AtomicHealthCounters> = ingress.counters();
    let manager = ObserveOnlyManager::new_with_registry(
        context,
        sensor_generation,
        Arc::clone(&registry),
        lane_generations,
        counters,
        receiver,
    )?;
    let initial = manager.snapshot();
    let (status_tx, status_rx) = watch::channel(initial);
    let (stop, mut stop_rx) = watch::channel(false);
    let wake = Arc::new(Notify::new());
    let wake_task = Arc::clone(&wake);
    let gate = existing_gate.or_else(|| {
        EnvironmentGate::production()
            .ok()
            .map(|gate| Arc::new(Mutex::new(gate)))
    });
    let gate_task = gate.as_ref().map(Arc::clone);
    let handle_registry = registry;
    let join = tokio::spawn(async move {
        run_manager(
            manager,
            status_tx,
            &mut stop_rx,
            wake_task,
            log_root,
            gate_task,
            started_at_monotonic_ms,
        )
        .await;
    });

    Ok(LegacyReliabilityHandle {
        ingress,
        status: status_rx,
        registry: handle_registry,
        gate,
        stop,
        wake,
        join,
    })
}

async fn run_manager(
    mut manager: ObserveOnlyManager,
    status_tx: watch::Sender<ObserveOnlySnapshot>,
    stop_rx: &mut watch::Receiver<bool>,
    wake: Arc<Notify>,
    log_root: PathBuf,
    gate: Option<Arc<Mutex<EnvironmentGate>>>,
    started_at_monotonic_ms: u64,
) {
    let started = Instant::now();
    let mut reliability_log = ReliabilityLog::open(log_root, Utc::now()).ok();
    let mut previous_snapshot = manager.snapshot();
    append_session_log(
        reliability_log.as_mut(),
        &previous_snapshot,
        ReliabilityEventKind::SessionStarted,
    );

    let baseline_request = gate
        .as_ref()
        .and_then(|_| manager.baseline_gate_request(placeholder_local_network(&manager)));
    let mut gate_task = match (gate.as_ref(), baseline_request) {
        (Some(gate), Some(request)) => Some(spawn_gate_task(
            Arc::clone(gate),
            GatePurpose::Baseline(request),
            process_monotonic_ms(started_at_monotonic_ms, started),
        )),
        _ => None,
    };
    let mut next_baseline_at = Instant::now()
        + if gate_task.is_some() {
            BASELINE_REFRESH_INTERVAL
        } else {
            BASELINE_RETRY_INTERVAL
        };

    loop {
        let now_ms = manager_monotonic_ms(started);
        tokio::select! {
            biased;
            changed = stop_rx.changed() => {
                if changed.is_err() || *stop_rx.borrow() {
                    if let Some(task) = gate_task.take() {
                        task.join.abort();
                    }
                    let snapshot = manager.shutdown(manager_monotonic_ms(started));
                    append_snapshot_logs(
                        reliability_log.as_mut(),
                        &previous_snapshot,
                        &snapshot,
                    );
                    append_session_log(
                        reliability_log.as_mut(),
                        &snapshot,
                        ReliabilityEventKind::SessionClosed,
                    );
                    let _ = status_tx.send(snapshot);
                    break;
                }
            }
            completion = wait_gate_task(&mut gate_task), if gate_task.is_some() => {
                if let Some(active) = gate_task.take() {
                    match active.purpose {
                        GatePurpose::Baseline(_) => {
                            let controls_healthy = completion
                                .as_ref()
                                .and_then(|joined| joined.as_ref().ok())
                                .and_then(|result| result.as_ref().ok())
                                .is_some_and(|report| {
                                    report
                                        .controls
                                        .iter()
                                        .filter(|outcome| outcome.has_http_response())
                                        .count()
                                        >= super::environment_gate::CONTROL_QUORUM
                                });
                            next_baseline_at = Instant::now()
                                + if controls_healthy {
                                    BASELINE_REFRESH_INTERVAL
                                } else {
                                    BASELINE_RETRY_INTERVAL
                                };
                        }
                        GatePurpose::Assessment(prepared) => {
                            match completion {
                                Some(Ok(Ok(report))) => {
                                    let _ = manager.apply_environment_gate_report(
                                        &prepared,
                                        &report,
                                        manager_monotonic_ms(started),
                                    );
                                    append_gate_log(
                                        reliability_log.as_mut(),
                                        &manager.snapshot(),
                                        &prepared,
                                        &report,
                                    );
                                }
                                Some(Ok(Err(GateRequestError::NetworkFingerprintMismatch))) => {
                                    let _ = manager.apply_environment_gate_failure(
                                        &prepared,
                                        AssessmentClassification::SensorUnreliable,
                                        manager_monotonic_ms(started),
                                    );
                                }
                                Some(Ok(Err(_))) | Some(Err(_)) | None => {
                                    let _ = manager.apply_environment_gate_failure(
                                        &prepared,
                                        AssessmentClassification::UpstreamDegraded,
                                        manager_monotonic_ms(started),
                                    );
                                }
                            }
                            let snapshot = manager.snapshot();
                            append_snapshot_logs(
                                reliability_log.as_mut(),
                                &previous_snapshot,
                                &snapshot,
                            );
                            previous_snapshot = snapshot.clone();
                            let _ = status_tx.send(snapshot);
                        }
                    }
                }
            }
            outcome = tokio::time::timeout(HEALTH_POLL_INTERVAL, manager.recv_next(now_ms)) => {
                match outcome {
                    Ok(ReceiveOutcome::Event { pending_scope, .. }) => {
                        let snapshot = manager.snapshot();
                        append_snapshot_logs(
                            reliability_log.as_mut(),
                            &previous_snapshot,
                            &snapshot,
                        );
                        previous_snapshot = snapshot.clone();
                        let _ = status_tx.send(snapshot);
                        manager.acknowledge_published_ingress_event(&pending_scope);
                    }
                    Ok(ReceiveOutcome::ReceiverClosed | ReceiveOutcome::ManagerClosed) => {
                        if let Some(task) = gate_task.take() {
                            task.join.abort();
                        }
                        let snapshot = manager.snapshot();
                        append_snapshot_logs(
                            reliability_log.as_mut(),
                            &previous_snapshot,
                            &snapshot,
                        );
                        append_session_log(
                            reliability_log.as_mut(),
                            &snapshot,
                            ReliabilityEventKind::SessionClosed,
                        );
                        let _ = status_tx.send(snapshot);
                        break;
                    }
                    Err(_) => {
                        let now_ms = manager_monotonic_ms(started);
                        let snapshot = manager.poll(now_ms);
                        append_snapshot_logs(
                            reliability_log.as_mut(),
                            &previous_snapshot,
                            &snapshot,
                        );
                        previous_snapshot = snapshot.clone();
                        let _ = status_tx.send(snapshot);
                    }
                }
            }
            _ = wake.notified() => {
                let now_ms = manager_monotonic_ms(started);
                let snapshot = manager.poll(now_ms);
                append_snapshot_logs(
                    reliability_log.as_mut(),
                    &previous_snapshot,
                    &snapshot,
                );
                previous_snapshot = snapshot.clone();
                let _ = status_tx.send(snapshot);
            }
        }

        if gate_task.is_none() {
            if let Some(gate) = gate.as_ref() {
                let local = placeholder_local_network(&manager);
                if let Some(prepared) = manager.take_environment_gate_request(local) {
                    gate_task = Some(spawn_gate_task(
                        Arc::clone(gate),
                        GatePurpose::Assessment(prepared),
                        process_monotonic_ms(started_at_monotonic_ms, started),
                    ));
                    let snapshot = manager.snapshot();
                    append_snapshot_logs(reliability_log.as_mut(), &previous_snapshot, &snapshot);
                    previous_snapshot = snapshot.clone();
                    let _ = status_tx.send(snapshot);
                } else if Instant::now() >= next_baseline_at {
                    let local = placeholder_local_network(&manager);
                    if let Some(request) = manager.baseline_gate_request(local) {
                        gate_task = Some(spawn_gate_task(
                            Arc::clone(gate),
                            GatePurpose::Baseline(request),
                            process_monotonic_ms(started_at_monotonic_ms, started),
                        ));
                        next_baseline_at = Instant::now() + BASELINE_REFRESH_INTERVAL;
                    } else {
                        next_baseline_at = Instant::now() + BASELINE_RETRY_INTERVAL;
                    }
                }
            } else {
                let local = placeholder_local_network(&manager);
                if let Some(prepared) = manager.take_environment_gate_request(local) {
                    let applied = manager.apply_environment_gate_failure(
                        &prepared,
                        AssessmentClassification::UpstreamDegraded,
                        manager_monotonic_ms(started),
                    );
                    if applied {
                        let snapshot = manager.snapshot();
                        append_snapshot_logs(
                            reliability_log.as_mut(),
                            &previous_snapshot,
                            &snapshot,
                        );
                        previous_snapshot = snapshot.clone();
                        let _ = status_tx.send(snapshot);
                    }
                }
            }
        }
    }
}

fn manager_monotonic_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

fn process_monotonic_ms(base_ms: u64, started: Instant) -> u64 {
    base_ms.saturating_add(started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64)
}

enum GatePurpose {
    Baseline(GateRequest),
    Assessment(PreparedEnvironmentGate),
}

struct ActiveGateTask {
    purpose: GatePurpose,
    join: JoinHandle<Result<GateReport, GateRequestError>>,
}

fn spawn_gate_task(
    gate: Arc<Mutex<EnvironmentGate>>,
    purpose: GatePurpose,
    baseline_clock_ms: u64,
) -> ActiveGateTask {
    let mut request = match &purpose {
        GatePurpose::Baseline(request) => request.clone(),
        GatePurpose::Assessment(prepared) => prepared.request.clone(),
    };
    let join = tokio::spawn(async move {
        // The resolver owns a shorter shared deadline and kills/reaps each
        // route/ARP/ping child. Await it directly so a generic async timeout
        // cannot detach an unbounded spawn_blocking task.
        let local = crate::netid::resolve_local_read_only().await;
        request.local_network = LocalNetworkSnapshot {
            online: local.online,
            interface_up: local.interface_up,
            default_route_available: local.default_route_available,
            gateway_reachable: local.gateway_reachable,
            // Preserve the expected identity while offline so Gate can report
            // Offline. An online identity change remains a stale fence.
            network_fingerprint: if local.online {
                local.fingerprint
            } else {
                request.fence.network_fingerprint.clone()
            },
        };
        gate.lock()
            .await
            .evaluate_with_baseline_clock(request, baseline_clock_ms)
            .await
    });
    ActiveGateTask { purpose, join }
}

async fn wait_gate_task(
    task: &mut Option<ActiveGateTask>,
) -> Option<Result<Result<GateReport, GateRequestError>, tokio::task::JoinError>> {
    Some((&mut task.as_mut()?.join).await)
}

fn placeholder_local_network(manager: &ObserveOnlyManager) -> LocalNetworkSnapshot {
    let snapshot = manager.snapshot();
    LocalNetworkSnapshot {
        online: true,
        interface_up: true,
        default_route_available: true,
        gateway_reachable: true,
        network_fingerprint: snapshot.session.network_fingerprint_at_start,
    }
}

fn append_session_log(
    log: Option<&mut ReliabilityLog>,
    snapshot: &ObserveOnlySnapshot,
    event: ReliabilityEventKind,
) {
    let Some(log) = log else {
        return;
    };
    let record = ReliabilityRecord::new(event, snapshot_envelope(snapshot));
    let _ = log.append(Utc::now(), &record);
}

fn append_snapshot_logs(
    mut log: Option<&mut ReliabilityLog>,
    previous: &ObserveOnlySnapshot,
    current: &ObserveOnlySnapshot,
) {
    let Some(log) = log.as_mut() else {
        return;
    };
    if previous.health.state != current.health.state
        || previous.gaps.total_gaps != current.gaps.total_gaps
    {
        let mut record = ReliabilityRecord::new(
            ReliabilityEventKind::SensorHealthChanged,
            snapshot_envelope(current),
        );
        record.counters = Some(log_counters(current, None));
        let _ = log.append(Utc::now(), &record);
    }

    for lane in &current.lanes {
        let previous_lane = previous
            .lanes
            .iter()
            .find(|candidate| candidate.category == lane.category);
        if previous_lane.is_some_and(|previous| log_lane_equal(previous, lane)) {
            continue;
        }
        let mut record = ReliabilityRecord::new(
            ReliabilityEventKind::AssessmentUpdated,
            snapshot_envelope(current),
        );
        record.lane = Some(ReliabilityLane::new(
            lane.category.clone(),
            lane.lane_generation,
        ));
        record.assessment = Some(log_assessment(lane));
        record.classification = log_classification(lane.classification);
        record.counters = Some(log_counters(current, Some(lane)));
        let _ = log.append(Utc::now(), &record);
    }

    if previous.presumed_intent != current.presumed_intent {
        let mut record = ReliabilityRecord::new(
            ReliabilityEventKind::IntentProposed,
            snapshot_envelope(current),
        );
        record.intent = Some(match &current.presumed_intent {
            super::policy::PresumedIntent::Wait { .. } => LogIntentKind::Wait,
            super::policy::PresumedIntent::SwitchLane { .. } => LogIntentKind::SwitchLane,
            super::policy::PresumedIntent::FreezeLane { .. } => LogIntentKind::FreezeLane,
        });
        let _ = log.append(Utc::now(), &record);
    }
}

fn append_gate_log(
    log: Option<&mut ReliabilityLog>,
    snapshot: &ObserveOnlySnapshot,
    prepared: &PreparedEnvironmentGate,
    report: &GateReport,
) {
    let Some(log) = log else {
        return;
    };
    let mut record = ReliabilityRecord::new(
        ReliabilityEventKind::EnvironmentGateCompleted,
        snapshot_envelope(snapshot),
    );
    record.lane = Some(ReliabilityLane::new(
        prepared.pending.category.clone(),
        prepared.pending.lane_generation,
    ));
    record.classification = match report.classification {
        super::environment_gate::GateClassification::Stable => None,
        super::environment_gate::GateClassification::Offline => {
            Some(LogClassificationKind::Offline)
        }
        super::environment_gate::GateClassification::DnsFailure => {
            Some(LogClassificationKind::DnsFailure)
        }
        super::environment_gate::GateClassification::UpstreamDegraded => {
            Some(LogClassificationKind::UpstreamDegraded)
        }
        super::environment_gate::GateClassification::TargetUnavailable => {
            Some(LogClassificationKind::TargetUnavailable)
        }
        super::environment_gate::GateClassification::ServiceSlow => {
            Some(LogClassificationKind::ServiceSlow)
        }
        super::environment_gate::GateClassification::DpiSuspected => {
            Some(LogClassificationKind::DpiSuspected)
        }
        super::environment_gate::GateClassification::DpiBlocked => {
            Some(LogClassificationKind::DpiBlocked)
        }
        super::environment_gate::GateClassification::SensorUnreliable => {
            Some(LogClassificationKind::SensorUnreliable)
        }
    };
    record.gate = Some(LogGateSummary {
        attempted_controls: report.controls.len().min(usize::from(u8::MAX)) as u8,
        successful_controls: report
            .controls
            .iter()
            .filter(|outcome| outcome.has_http_response())
            .count()
            .min(usize::from(u8::MAX)) as u8,
        attempted_targets: report.category_targets.len().min(usize::from(u16::MAX)) as u16,
        successful_targets: report
            .category_targets
            .iter()
            .filter(|outcome| outcome.category_target_reachable())
            .count()
            .min(usize::from(u16::MAX)) as u16,
        duration_ms: report
            .generated_at_monotonic_ms
            .saturating_sub(prepared.request.requested_at_monotonic_ms),
    });
    let _ = log.append(Utc::now(), &record);
}

fn snapshot_envelope(snapshot: &ObserveOnlySnapshot) -> super::contracts::EventEnvelope {
    super::contracts::EventEnvelope::new(
        snapshot.session.session_id,
        snapshot.session.sensor_generation,
        snapshot.session.target_registry_version,
    )
}

fn log_lane_equal(
    left: &super::assessment::LaneAssessment,
    right: &super::assessment::LaneAssessment,
) -> bool {
    left.category == right.category
        && left.lane_generation == right.lane_generation
        && left.phase == right.phase
        && left.classification == right.classification
        && left.confidence == right.confidence
        && left.cooldown_until_ms == right.cooldown_until_ms
        && clamped_log_evidence(left.evidence) == clamped_log_evidence(right.evidence)
}

fn clamped_log_evidence(
    evidence: super::assessment::EvidenceSummary,
) -> super::assessment::EvidenceSummary {
    super::assessment::EvidenceSummary {
        working_flows: evidence.working_flows.min(2),
        working_targets: evidence.working_targets.min(2),
        reset_flows: evidence.reset_flows.min(3),
        reset_targets: evidence.reset_targets.min(2),
        blackhole_flows: evidence.blackhole_flows.min(2),
        blackhole_targets: evidence.blackhole_targets.min(2),
    }
}

fn log_assessment(lane: &super::assessment::LaneAssessment) -> LogAssessmentKind {
    use super::assessment::LanePhase;
    match lane.phase {
        LanePhase::Healthy => LogAssessmentKind::Healthy,
        LanePhase::SensorUnreliable => LogAssessmentKind::SensorUnreliable,
        LanePhase::BlockedCooldown => LogAssessmentKind::BlackholeQuorum,
        LanePhase::GatePending if lane.evidence.blackhole_quorum() => {
            LogAssessmentKind::BlackholeQuorum
        }
        LanePhase::GatePending if lane.evidence.reset_quorum() => LogAssessmentKind::ResetQuorum,
        LanePhase::Suspect if lane.evidence.blackhole_flows > 0 => {
            LogAssessmentKind::InsufficientEvidence
        }
        LanePhase::Suspect => LogAssessmentKind::ResetSuspected,
        LanePhase::Observing | LanePhase::GatePending => LogAssessmentKind::InsufficientEvidence,
    }
}

fn log_classification(classification: AssessmentClassification) -> Option<LogClassificationKind> {
    match classification {
        AssessmentClassification::AwaitingEvidence | AssessmentClassification::Working => None,
        AssessmentClassification::Offline => Some(LogClassificationKind::Offline),
        AssessmentClassification::DnsFailure => Some(LogClassificationKind::DnsFailure),
        AssessmentClassification::UpstreamDegraded => Some(LogClassificationKind::UpstreamDegraded),
        AssessmentClassification::TargetUnavailable => {
            Some(LogClassificationKind::TargetUnavailable)
        }
        AssessmentClassification::ServiceSlow => Some(LogClassificationKind::ServiceSlow),
        AssessmentClassification::DpiSuspected => Some(LogClassificationKind::DpiSuspected),
        AssessmentClassification::DpiBlocked => Some(LogClassificationKind::DpiBlocked),
        AssessmentClassification::SensorUnreliable => Some(LogClassificationKind::SensorUnreliable),
    }
}

fn log_counters(
    snapshot: &ObserveOnlySnapshot,
    lane: Option<&super::assessment::LaneAssessment>,
) -> ReliabilityCounters {
    let evidence = lane.map_or_else(Default::default, |lane| clamped_log_evidence(lane.evidence));
    ReliabilityCounters {
        accepted_flows: snapshot.accepted.attributed_flows,
        rejected_flows: snapshot.rejected.total_events,
        reset_events: u32::from(evidence.reset_flows),
        distinct_reset_targets: evidence.reset_targets,
        blackhole_flows: u32::from(evidence.blackhole_flows),
        distinct_blackhole_targets: evidence.blackhole_targets,
        gaps: snapshot.gaps.total_gaps,
        queue_drops: snapshot.health.counters.queue_drops,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::legacy_reliability::contracts::{
        EventEnvelope, EyeEvent, EyeHealthCounters, EyeHealthState, HealthEvent, LaneGeneration,
        LegacySessionContext, NetworkFingerprint, RegistryVersion, SensorGeneration, SessionId,
    };
    use crate::legacy_reliability::ingress::{channel, ControlIngressResult, PendingIngressScope};

    fn snapshot_fixture() -> ObserveOnlySnapshot {
        let (ingress, receiver) = channel();
        ObserveOnlyManager::new(
            LegacySessionContext::new(
                SessionId::new(1),
                vec!["discord".to_owned()],
                NetworkFingerprint::Unknown,
            ),
            SensorGeneration::new(2),
            RegistryVersion::new(3),
            BTreeMap::from([("discord".to_owned(), LaneGeneration::new(4))]),
            ingress.counters(),
            receiver,
        )
        .unwrap()
        .snapshot()
    }

    #[test]
    fn snapshot_reader_retries_when_publication_overtakes_pending_sample() {
        let initial = snapshot_fixture();
        let mut published = initial.clone();
        published.accepted.health_events = 1;
        published.accepted.total_events = 1;
        let (status_tx, status_rx) = watch::channel(initial);
        let (ingress, mut receiver) = channel();
        let mut inject_once = true;

        let snapshot = read_coherent_snapshot_with_hook(&status_rx, &ingress, || {
            if !inject_once {
                return;
            }
            inject_once = false;
            let event = HealthEvent {
                envelope: EventEnvelope::new(
                    SessionId::new(1),
                    SensorGeneration::new(2),
                    RegistryVersion::new(3),
                ),
                state: EyeHealthState::Ready,
                counters: EyeHealthCounters::default(),
            };
            assert_eq!(
                ingress.try_health(event.clone()),
                ControlIngressResult::Accepted
            );
            status_tx.send(published.clone()).unwrap();
            let received = receiver.try_recv_control().unwrap();
            assert_eq!(received, EyeEvent::Health(event));
            receiver.acknowledge_published_event(&PendingIngressScope::for_event(&received));
        });

        assert_eq!(snapshot.accepted.health_events, 1);
        assert_eq!(snapshot.pending_ingress_control_events, 0);
    }

    #[test]
    fn closed_manager_watch_is_returned_fail_closed() {
        let (status_tx, status_rx) = watch::channel(snapshot_fixture());
        let (ingress, _receiver) = channel();
        drop(status_tx);

        let snapshot = read_coherent_snapshot_with_hook(&status_rx, &ingress, || {});

        assert!(snapshot.session.closed);
        assert_eq!(snapshot.health.state, EyeHealthState::Blind);
        assert!(snapshot.health.receiver_failed);
    }
}
