use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Manager};

use super::dsl::{AdaptiveCategory, StrategyTransport};
use super::evidence::FailureStage;
use super::probe::{self, ProbeTarget, SessionDnsCache};
use super::runtime::{log_probe_series, EvidenceWindow};
use crate::state::{AppState, DpiRuntimeSnapshot};

#[derive(Clone, Copy, Debug)]
pub(super) struct RollbackConfig {
    pub(super) probe_timeout: Duration,
    pub(super) probe_interval: Duration,
    pub(super) stabilization_delay: Duration,
    pub(super) base_recheck_rounds: u8,
    pub(super) eyes_quiet_window: Duration,
    pub(super) eyes_quiet_deadline: Duration,
}

#[derive(Clone)]
pub(super) struct RollbackRequest {
    pub(super) session_id: u64,
    pub(super) attempt_id: u64,
    pub(super) original: Option<DpiRuntimeSnapshot>,
    pub(super) expected_generation: u64,
    pub(super) category: Option<AdaptiveCategory>,
    pub(super) transport: Option<StrategyTransport>,
    pub(super) targets: Vec<ProbeTarget>,
    pub(super) dns_cache: SessionDnsCache,
    pub(super) recovery_mode: bool,
    pub(super) config: RollbackConfig,
    pub(super) evidence: Arc<EvidenceWindow>,
}

#[derive(Debug)]
pub(super) struct RollbackOutcome {
    pub(super) generation_after: u64,
    pub(super) restored: bool,
    pub(super) base_healthy: bool,
    pub(super) base_probe_reliable: bool,
}

pub(super) async fn run(app: AppHandle, request: RollbackRequest) -> RollbackOutcome {
    let restored = if let Some(original) = request.original.as_ref() {
        let state = app.state::<AppState>();
        let _gate = state.dpi_gate.lock().await;
        crate::protected_runtime::restore_adaptive_snapshot(
            &app,
            original,
            request.expected_generation,
        )
        .await
    } else {
        Err("Adaptive rollback snapshot отсутствует".to_string())
    };

    let mut base_healthy = request.recovery_mode && restored.is_ok();
    let mut base_probe_reliable = request.recovery_mode;
    if let Ok(generation) = restored.as_ref() {
        if !request.recovery_mode {
            if let (Some(category), Some(transport)) = (request.category, request.transport) {
                request.evidence.begin(*generation);
                tokio::time::sleep(request.config.stabilization_delay).await;
                let mut series = probe::run_probe_series_for_targets_with_cache(
                    category,
                    transport,
                    &request.targets,
                    request.config.probe_timeout,
                    request.config.base_recheck_rounds,
                    1,
                    request.config.probe_interval,
                    &request.dns_cache,
                )
                .await;
                let mut eyes = request.evidence.snapshot();
                let mut result = series.evaluate_base(&eyes);
                if result.is_success() {
                    request
                        .evidence
                        .wait_for_quiet(
                            *generation,
                            request.config.eyes_quiet_window,
                            request.config.eyes_quiet_deadline,
                        )
                        .await;
                    eyes = request.evidence.snapshot();
                    result = series.evaluate_base(&eyes);
                }
                log_probe_series(&app, "base_recheck", &series, &result);

                if result.failure_stage == FailureStage::Dns && !result.dns_ok {
                    crate::util::emit_log(
                        &app,
                        "warn",
                        "adaptive",
                        "base_recheck DNS failure: retrying once with session cache",
                    );
                    series = probe::run_probe_series_for_targets_with_cache(
                        category,
                        transport,
                        &request.targets,
                        request.config.probe_timeout,
                        request.config.base_recheck_rounds,
                        1,
                        request.config.probe_interval,
                        &request.dns_cache,
                    )
                    .await;
                    eyes = request.evidence.snapshot();
                    result = series.evaluate_base(&eyes);
                    if result.is_success() {
                        request
                            .evidence
                            .wait_for_quiet(
                                *generation,
                                request.config.eyes_quiet_window,
                                request.config.eyes_quiet_deadline,
                            )
                            .await;
                        eyes = request.evidence.snapshot();
                        result = series.evaluate_base(&eyes);
                    }
                    log_probe_series(&app, "base_recheck_dns_retry", &series, &result);
                }

                base_probe_reliable = result.failure_stage != FailureStage::Dns || result.dns_ok;
                base_healthy = result.is_success();
            }
        }
    }

    let generation_after = crate::protected_runtime::adaptive_runtime_snapshot().generation;
    let restored = restored.is_ok_and(|generation| generation == generation_after);
    crate::util::emit_log(
        &app,
        if restored && base_healthy {
            "info"
        } else if restored && !base_probe_reliable {
            "warn"
        } else {
            "error"
        },
        "adaptive",
        &format!(
            "rollback_finished session={} attempt={} restored={restored} base_healthy={base_healthy} base_probe_reliable={base_probe_reliable}",
            request.session_id, request.attempt_id
        ),
    );

    RollbackOutcome {
        generation_after,
        restored,
        base_healthy,
        base_probe_reliable,
    }
}
